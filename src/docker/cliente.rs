//! El cliente del motor de contenedores: la conexión y el `exec` con su salida.

use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
// El modelo de la inspección se importa de `bollard::models` —el reexport de la biblioteca—
// y no de `bollard::exec`: en `exec` el nombre está **privado**, así que la ruta corta no
// compila.
use bollard::models::ExecInspectResponse;
use futures_util::StreamExt;

use super::{Canal, Error, Orden, Trozo};

/// El cliente del motor de contenedores.
///
/// Se construye **una vez** por proceso y se comparte: por dentro hay un cliente HTTP con
/// su propio agrupador de conexiones, así que construir uno por ejecución tiraría el
/// agrupador y multiplicaría los sockets abiertos sin ganar nada a cambio.
pub struct Cliente {
    /// El motor, tal y como lo construye la biblioteca.
    docker: Docker,
    /// El socket con el que se construyó: es lo que los errores de conexión nombran.
    socket: String,
    /// Tiempo máximo por petición, ya redondeado a segundos como lo guarda la biblioteca.
    timeout: Duration,
}

impl Cliente {
    /// Conecta con el socket indicado.
    ///
    /// `socket` acepta las dos formas: `unix://<ruta>` y la ruta pelada —la biblioteca
    /// recorta el prefijo por su cuenta—.
    ///
    /// **No se lee el entorno del proceso, y aquí conviene explicar por qué**, porque la
    /// tentación es fuerte: la biblioteca tiene un `Docker::connect_with_defaults()` que
    /// lee `DOCKER_HOST` y cae a la ruta por defecto de la plataforma, y bastaría con no
    /// recibir un parámetro. No se usa porque el contrato de configuración de este
    /// proyecto exige que `DOCKER_HOST` se lea por el **mecanismo inyectado** de la
    /// configuración, y que una prueba pueda apuntar el motor a otro sitio sin tocar el
    /// entorno del proceso. Con esta firma, dos pruebas construyen dos clientes con dos
    /// sockets distintos a la vez y ninguno ve el del otro; con `connect_with_defaults()`
    /// eso es imposible, porque la variable es una sola para todo el proceso y mutarla
    /// deja de ser reproducible en cuanto la suite corre en paralelo.
    pub fn conectar(socket: &str, timeout: Duration) -> Result<Self, Error> {
        // El tiempo de la biblioteca son segundos enteros, y cero no significa «sin tope»: se
        // garantiza un **mínimo de un segundo** para que un `Duration` de milisegundos no se
        // convierta en un tiempo máximo nulo sin que nadie lo haya pedido. `as_secs()`
        // **trunca** —no redondea—, así que lo que sale de aquí es «al menos un segundo», no
        // «el múltiplo de arriba».
        let segundos = timeout.as_secs().max(1);

        // La versión de API que se pasa aquí **hoy no llega al cable**, y conviene dejarlo
        // escrito para que nadie lea esta línea creyendo que la versión está negociada.
        // Medido en `bollard-0.18.1`: su `Uri::parse` compone la cadena con el prefijo
        // —`{esquema}://{host}/v{major}.{minor}{path}`— y acto seguido hace `url.join(path)`
        // con `path` absoluto, que **sustituye la ruta entera y se lleva el prefijo por
        // delante**; el `Podman` de esta máquina, además, contesta `200` a cualquier prefijo,
        // así que la elección es inerte y no se puede notar. Se deja como está porque
        // `API_DEFAULT_VERSION` es también el defecto de la biblioteca, de modo que no se
        // está eligiendo nada que ella no eligiera.
        //
        // El día que un parche de `bollard` restaure el prefijo, la salida **no** es fijar
        // una constante más alta a mano —eso rompería contra un motor antiguo, que es lo que
        // el propio aviso de la biblioteca advierte—, sino **negociar**: `Docker::negotiate_version`
        // consulta `/version` y baja la versión del cliente a la que soporte el servidor.
        let docker = Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)
            .map_err(|causa| Error::del_socket(socket, causa))?;

        Ok(Self {
            docker,
            socket: socket.to_owned(),
            timeout: Duration::from_secs(segundos),
        })
    }

    /// Comprueba que el motor contesta.
    ///
    /// Se usa al arrancar para **avisar, no para fallar**: un socket que no responde no
    /// impide arrancar, porque el panel tiene que poder levantarse precisamente para
    /// enseñar que el socket no responde. Devolver el fallo es lo que permite que el
    /// arranque decida qué hacer con él —avisar— en vez de encontrárselo como un pánico.
    pub async fn comprobar(&self) -> Result<(), Error> {
        self.docker
            .ping()
            .await
            .map(|_| ())
            .map_err(|causa| Error::del_socket(&self.socket, causa))
    }

    /// Ejecuta `orden` dentro de `contenedor`, entrega cada trozo a `al_llegar` **en cuanto
    /// llega**, y devuelve el código de salida del proceso.
    ///
    /// # Seguridad
    ///
    /// Esta llamada **publica el valor del entorno** si el target `bollard` está registrado a
    /// `debug`: la biblioteca registra el cuerpo serializado de la petición —medido:
    /// `serialize_payload` en `bollard-0.18.1/src/docker.rs:1227`—, y el cuerpo de un
    /// `create_exec` lleva el `env` con la contraseña de la base dentro. El `Debug` de
    /// [`Orden`] no protege de esto, porque el valor no pasa por él: pasa por la biblioteca.
    /// La exposición se corta **por filtro**, y de eso se encarga el **proceso**: el arranque
    /// añade `bollard=warn` detrás del filtro que pida quien lance el binario —ver
    /// [`crate::DIRECTIVA_DEL_MOTOR`]—, así que no depende de la receta con la que se
    /// arranque. Queda un hueco, y el arranque lo avisa: un sub-objetivo **más específico**
    /// (`bollard::docker=debug`) va por delante de esa directiva y sí publicaría el valor.
    /// Cualquier código que suba el nivel de `bollard` a `debug` está publicando credenciales.
    ///
    /// ## El runtime que conduce esto tiene que seguir vivo
    ///
    /// Es una restricción **medida**, no una precaución: si el `exec` se lanza en un
    /// runtime y su flujo se drena desde otro hilo —con otro runtime— después de que el
    /// primero haya muerto, el drenaje no se atasca, **falla** en once microsegundos con
    /// `A Tokio 1.x context was found, but it is being shutdown`. La forma que funciona,
    /// también medida, es un `std::thread::spawn` que crea **dentro del hilo** su propio
    /// runtime multi-hilo y hace allí el `create_exec`, el `start_exec` y el drenaje. Hay
    /// una prueba marcada `#[ignore]` que lo fija, y está ahí para que nadie «simplifique»
    /// esto a un `tokio::spawn`.
    ///
    /// ## El `timeout` del cliente acota el apretón de manos, **no** el drenaje
    ///
    /// Es la trampa que hace falta conocer antes de elegir un tiempo máximo: el
    /// `timeout` que la biblioteca le pone a `client.request(req)` se resuelve **cuando
    /// llegan las cabeceras**, y el cuerpo lo drena después este bucle, **fuera** de ese
    /// plazo. Un `mariadb-dump` colgado no lo caza ningún `timeout` del cliente: se queda
    /// esperando en `output.next()` para siempre. Lo que acota el trabajo es
    /// `inactividad_maxima`, y es lo único que lo acota.
    ///
    /// Por eso el parámetro es **inactividad** y no tiempo máximo: se mide entre trozos, así
    /// que un volcado lento pero que avanza no puede fallar por esto —tarda lo que tarde— y
    /// uno que se calla, sí: vence el plazo y sale [`Error::ExtraccionEstancada`]. El valor no
    /// se elige aquí: lo pasa el motor, que es quien sabe cuánto puede tardar un volcado de
    /// verdad en decir su primera palabra.
    ///
    /// ## Lo que se entrega y lo que no
    ///
    /// El `stderr` llega por el **mismo** flujo que el `stdout`, solo que marcado con su
    /// canal: separarlos es lo que permite distinguir un fallo de extracción de una base
    /// vacía. Los bytes se entregan tal cual, con sus saltos de línea y sin recortar, y el
    /// código de salida se lee **después** de que el flujo cierre.
    pub async fn ejecutar(
        &self,
        contenedor: &str,
        orden: &Orden,
        inactividad_maxima: Duration,
        mut al_llegar: impl FnMut(Trozo),
    ) -> Result<i32, Error> {
        // `env` va **explícito** aunque la orden no traiga variables: es el campo por el que
        // viajan las credenciales de la base, y dejarlo sin poner convertiría un cambio de
        // arriba en credenciales que no llegan sin que nada lo diga.
        //
        // `attach_stdout` y `attach_stderr` en `true` y `tty` en `false` son la combinación
        // medida que entrega la salida: sin ningún `attach` el arranque falla con un `500`
        // (`must provide at least one stream to attach to`), y con tty todo llega como
        // `Console` y se pierde la separación entre `stdout` y `stderr`.
        //
        // El comando y el entorno se pasan **prestados**: la biblioteca solo los serializa
        // en la petición, así que clonarlos para construirla sería copiar dos veces lo que
        // ya está en la orden.
        let opciones = CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(false),
            env: Some(orden.env.iter().map(String::as_str).collect()),
            cmd: Some(orden.cmd.iter().map(String::as_str).collect()),
            ..Default::default()
        };

        let creado = self
            .docker
            .create_exec(contenedor, opciones)
            .await
            .map_err(|causa| Error::del_contenedor(contenedor, causa))?;

        let iniciado = self
            .docker
            .start_exec(
                &creado.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: None,
                }),
            )
            .await
            .map_err(|causa| Error::del_contenedor(contenedor, causa))?;

        // El `input` no se saca del patrón a propósito: se deja caer sin escribirle un solo
        // byte. Esta orden no recibe entrada —no se pide `attach_stdin`—, y escribir en un
        // `exec` que no la ha pedido degrada el cierre limpio del flujo a un `Connection
        // reset by peer`. Tampoco hay que cerrarlo: el proceso termina cuando el comando
        // acaba y el flujo se cierra solo, que es lo que hace que el bucle de abajo acabe.
        let StartExecResults::Attached { mut output, .. } = iniciado else {
            // No puede pasar —`detach: false` está pedido unas líneas más arriba—, pero
            // devolver un cero aquí sería dar por buena una orden que nadie leyó.
            return Err(Error::SinSalida {
                contenedor: contenedor.to_owned(),
            });
        };

        // El drenaje, con `StreamExt::next()` hasta el `None` del final y **en este mismo
        // runtime**: ver la nota de la firma. Un fallo del flujo es del motor, no del
        // comando: el comando ya dijo lo suyo por `stderr` y por su código de salida.
        //
        // Cada lectura va envuelta en el plazo de inactividad, y esa es la única acotación
        // del trabajo: el `timeout` del cliente ya se resolvió cuando llegaron las
        // cabeceras. Y son **tres** desenlaces, no dos: `Ok(Some(..))` es un trozo, `Ok(None)`
        // es que el flujo cerró —el proceso terminó— y `Err(Elapsed)` es que venció el plazo.
        // Confundir los dos últimos convertiría un cierre limpio en un fallo —o al revés, que
        // es peor: un colgado se leería como «terminó sin escribir nada» y acabaría en un
        // snapshot vacío dado por bueno—.
        loop {
            let trozo = match tokio::time::timeout(inactividad_maxima, output.next()).await {
                Ok(Some(trozo)) => {
                    trozo.map_err(|causa| Error::del_contenedor(contenedor, causa))?
                }
                // El flujo cerró: el proceso acabó y no queda nada por leer. Se sale a leer
                // el código de salida, que es lo que dice cómo acabó.
                Ok(None) => break,
                Err(_) => {
                    return Err(Error::ExtraccionEstancada {
                        contenedor: contenedor.to_owned(),
                        inactividad: inactividad_maxima,
                    });
                }
            };

            let (canal, bytes) = match trozo {
                LogOutput::StdOut { message } => (Canal::Salida, message),
                LogOutput::StdErr { message } => (Canal::Error, message),
                // Con tty —que aquí **no** se pide, y está pedido unas líneas más arriba—
                // no hay separación posible: todo sale como `Console`, y `StdIn` solo
                // llegaría si se pidiera `attach_stdin`. Ninguna de las dos debería
                // aparecer, así que el caso se canda: si alguien enciende el tty, **todo**
                // lo que salga se entregaría como volcado —el motivo de un fallo viajaría
                // como bytes de copia, en silencio—, que es justo lo que el requisito de
                // distinguir un fallo de una base vacía prohíbe. En depuración esto salta;
                // en publicación se conserva el reparto de abajo, que es el menos malo de
                // los dos.
                LogOutput::Console { message } => {
                    debug_assert!(
                        false,
                        "la biblioteca entregó un `Console`: alguien encendió el tty, y con \
                         tty el motivo de un fallo viaja como bytes de copia"
                    );
                    (Canal::Salida, message)
                }
                LogOutput::StdIn { message } => {
                    debug_assert!(
                        false,
                        "la biblioteca entregó un `StdIn`: alguien pidió `attach_stdin`, que \
                         esta orden no necesita"
                    );
                    (Canal::Salida, message)
                }
            };

            al_llegar(Trozo {
                canal,
                bytes: bytes.to_vec(),
            });
        }

        // El código de salida se pregunta **después** de que el flujo cierre, que es cuando
        // el motor ya sabe que el proceso terminó: `pid` no sirve para esto —queda en cero
        // incluso con el proceso acabado— y el campo de «sigue corriendo» es `running`.
        let estado = self
            .docker
            .inspect_exec(&creado.id)
            .await
            .map_err(|causa| Error::del_contenedor(contenedor, causa))?;

        // La decisión de si lo que informa el motor es un código de salida o no vive en
        // [`codigo_de_salida`], que es donde están los tres desenlaces que no lo son.
        codigo_de_salida(&estado).ok_or_else(|| Error::SinCodigoDeSalida {
            contenedor: contenedor.to_owned(),
        })
    }
}

/// El código de salida de un `exec` ya inspeccionado, o `None` si no se puede decir cómo acabó.
///
/// Son **tres** los desenlaces que no dan código, y los tres tienen que caer del mismo lado
/// —`None`—, porque darlos por cero convertiría un fallo en un éxito: para una copia, eso es
/// la peor de las salidas —existe, ocupa sitio y no se puede restaurar—.
///
/// Uno: el motor no informó el código. Dos: el código no cabe en un `i32`, que tampoco es un
/// código de salida.
///
/// Y tres, que es el que faltaba: **el proceso seguía en marcha** cuando se le preguntó. El
/// campo `running` es opcional y nada obliga al motor a informar el código solo cuando el
/// proceso ha muerto: un `exec` que sigue corriendo llega con `exit_code: 0` —lo señala la
/// auditoría de esta capa, y el modelo lo permite, porque `exit_code` es un `Option`—, así que
/// un flujo que cerrara **antes** del fin del proceso devolvería `0`, y con un `0` el motor
/// declara el final del volcado: entrega **completa** con el volcado truncado. El camino
/// normal no lo alcanza —el flujo cierra cuando el proceso muere—, de modo que esto es un
/// hueco de defensa y no un fallo reproducido: lo que se cierra es la pregunta, no el caso.
/// Un `running` que el motor **no** informe (`None`) no se lee como «sigue en marcha»: no
/// saberlo no es saber que sí, y ese desenlace ya está cubierto por el `exit_code` ausente.
fn codigo_de_salida(estado: &ExecInspectResponse) -> Option<i32> {
    if estado.running == Some(true) {
        return None;
    }

    estado
        .exit_code
        .and_then(|codigo| i32::try_from(codigo).ok())
}

impl std::fmt::Debug for Cliente {
    /// Publica el socket y el tiempo máximo, y **nada del interior de la biblioteca**.
    ///
    /// No se deriva: `bollard::Docker` sí implementa `Debug` y volcaría su transporte
    /// entero —el cliente HTTP con su configuración— en cada línea de registro que lo
    /// imprima. Lo que hace falta para diagnosticar es a qué socket se está hablando y con
    /// qué tiempo máximo, y eso es lo que se publica.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cliente")
            .field("socket", &self.socket)
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bollard::models::ExecInspectResponse;

    use super::{Cliente, codigo_de_salida};
    use crate::docker::{Canal, Error, Orden, Trozo};
    use crate::test_support::{
        ContenedorDePrueba, TIMEOUT_DEL_MOTOR, cliente_del_motor, directorio_de_prueba,
        socket_del_motor,
    };

    /// Plazo de inactividad de las pruebas con motor.
    ///
    /// Holgado a propósito: lo que estas pruebas fijan es que la acotación **no** estorba a
    /// una orden que avanza —incluido el volumen de cuatro mebibytes, que tarda lo suyo en
    /// llegar—. El único test que usa un plazo corto es el del cuelgue, y ahí el plazo corto
    /// **es** lo que se prueba.
    const INACTIVIDAD_DE_PRUEBA: Duration = Duration::from_secs(60);

    /// Plazo de inactividad del test del cuelgue: tiene que vencer, así que es corto.
    const INACTIVIDAD_CORTA: Duration = Duration::from_millis(500);

    /// `conectar` con un socket que no existe falla, y el error **nombra el socket**.
    ///
    /// Es la mitad del contrato de configuración que se puede probar sin motor: el socket
    /// entra por parámetro, así que una ruta muerta se ejercita sin tocar nada del sistema.
    #[test]
    fn conectar_con_un_socket_que_no_existe_falla_y_lo_nombra() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta().join("socket-que-no-existe.sock");
        let ruta = ruta.to_string_lossy().into_owned();

        let error = Cliente::conectar(&ruta, TIMEOUT_DEL_MOTOR)
            .expect_err("un socket que no existe no puede dar un cliente");
        let motivo = error.to_string();

        assert!(
            matches!(&error, Error::SocketInexistente { socket } if socket == &ruta),
            "el error debe reconocer el socket que se pidió: {error:?}"
        );
        assert!(
            motivo.contains(&ruta),
            "el motivo no nombra el socket: {motivo}"
        );
    }

    /// `conectar` acepta las dos formas del socket: `unix://<ruta>` y la ruta pelada.
    ///
    /// Se conecta a un **fichero** temporal y no a un socket: la biblioteca comprueba que
    /// la ruta existe y construye el cliente, y no habla por ella hasta la primera
    /// petición. Eso es lo que permite fijar la aceptación de las dos formas sin depender
    /// de que haya un motor de contenedores en la máquina —y por tanto en el CI, que no lo
    /// tiene—.
    #[test]
    fn conectar_acepta_las_dos_formas_del_socket() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta().join("socket-de-prueba");
        std::fs::File::create(&ruta).expect("la ruta de la prueba debe poder crearse");
        let ruta = ruta.to_string_lossy().into_owned();

        let con_esquema = Cliente::conectar(&format!("unix://{ruta}"), TIMEOUT_DEL_MOTOR)
            .expect("la forma con `unix://` es válida");
        let pelada = Cliente::conectar(&ruta, TIMEOUT_DEL_MOTOR).expect("la ruta pelada es válida");

        // Las dos hablan con el mismo sitio: la ruta que el cliente nombra es la misma.
        assert!(
            format!("{con_esquema:?}").contains(&ruta),
            "la forma con esquema no llegó a la misma ruta: {con_esquema:?}"
        );
        assert!(
            format!("{pelada:?}").contains(&ruta),
            "la forma pelada no llegó a la misma ruta: {pelada:?}"
        );
    }

    /// El `Debug` del cliente no vuelca el interior de la biblioteca.
    ///
    /// Se compara contra el `Debug` del propio motor en vez de contra un umbral inventado:
    /// un `#[derive(Debug)]` habría publicado eso —el cliente HTTP entero con su
    /// configuración— y lo que se publica es solo lo que sirve para diagnosticar.
    #[test]
    fn el_debug_del_cliente_no_vuelca_el_interior_de_la_biblioteca() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta().join("socket-de-prueba");
        std::fs::File::create(&ruta).expect("la ruta de la prueba debe poder crearse");
        let ruta = ruta.to_string_lossy().into_owned();

        let cliente = Cliente::conectar(&ruta, TIMEOUT_DEL_MOTOR).expect("la ruta existe");

        let nuestro = format!("{cliente:?}");
        let del_motor = format!("{:?}", cliente.docker);

        assert!(
            nuestro.contains(&ruta),
            "el Debug no nombra el socket: {nuestro}"
        );
        assert!(
            nuestro.contains("timeout"),
            "el Debug no publica el tiempo máximo: {nuestro}"
        );
        assert!(
            nuestro.len() < del_motor.len(),
            "el Debug del cliente publica el interior de la biblioteca ({nuestro}) en vez de \
             quedarse con el socket y el tiempo máximo: el del motor mide {}",
            del_motor.len()
        );
    }

    /// Un `exec` que **seguía en marcha** no da un código de salida que valga.
    ///
    /// Es el hueco que la auditoría de esta capa señaló, y aquí se fija sobre el modelo de la
    /// biblioteca porque con un `exec` real **no se puede provocar**: el flujo cierra cuando
    /// el proceso muere, así que la inspección del código llega siempre con el proceso
    /// acabado. Lo que sí se puede fijar es lo que decide la función, que es donde está la
    /// mentira del motor: un `exec` vivo llega con `exit_code: 0`, y con un `0` el motor
    /// declararía el final del volcado —entrega completa con el volcado truncado—.
    #[test]
    fn un_proceso_que_sigue_en_marcha_no_da_codigo_de_salida() {
        let en_marcha = ExecInspectResponse {
            running: Some(true),
            exit_code: Some(0),
            ..Default::default()
        };

        assert_eq!(
            codigo_de_salida(&en_marcha),
            None,
            "un proceso que sigue en marcha no puede dar un código de salida: el `0` que \
             informa el motor no significa «salió bien», significa «todavía corre»"
        );

        // Y la otra mitad: un proceso terminado sí da su código, que es lo que hace que esta
        // guarda no se lleve por delante el camino bueno.
        let terminado_mal = ExecInspectResponse {
            running: Some(false),
            exit_code: Some(3),
            ..Default::default()
        };
        assert_eq!(codigo_de_salida(&terminado_mal), Some(3));

        // Un `running` que el motor no informa no es «sigue en marcha»: no saberlo no es
        // saber que sí, y ahí sigue mandando el `exit_code`.
        let sin_running = ExecInspectResponse {
            running: None,
            exit_code: Some(0),
            ..Default::default()
        };
        assert_eq!(codigo_de_salida(&sin_running), Some(0));

        // Y los dos desenlaces de siempre siguen sin dar código: no lo informó, o no cabe.
        for sin_codigo in [
            ExecInspectResponse {
                running: Some(false),
                exit_code: None,
                ..Default::default()
            },
            ExecInspectResponse {
                running: Some(false),
                exit_code: Some(i64::from(i32::MAX) + 1),
                ..Default::default()
            },
        ] {
            assert_eq!(
                codigo_de_salida(&sin_codigo),
                None,
                "un código que no se puede decir es un código que no se puede decir: \
                 {sin_codigo:?}"
            );
        }
    }

    /// Ejecuta una orden y devuelve su código de salida y todos sus trozos, en orden.
    ///
    /// El cierre va **prestado** —un `&mut` de un `FnMut` es `FnMut`— y vive en su propio
    /// bloque para que el préstamo de `trozos` acabe con la llamada y se pueda leer después.
    /// El plazo de inactividad es el de las pruebas —[`INACTIVIDAD_DE_PRUEBA`]—: ninguna de
    /// estas órdenes calla tanto, así que lo que se está fijando es que la acotación no
    /// estorba a una extracción que avanza.
    async fn ejecutar_recogiendo(
        cliente: &Cliente,
        contenedor: &str,
        orden: &Orden,
    ) -> Result<(i32, Vec<Trozo>), Error> {
        let mut trozos: Vec<Trozo> = Vec::new();

        let codigo = {
            let mut guardar = |trozo: Trozo| trozos.push(trozo);

            cliente
                .ejecutar(contenedor, orden, INACTIVIDAD_DE_PRUEBA, &mut guardar)
                .await?
        };

        Ok((codigo, trozos))
    }

    /// Texto de los trozos de un canal, concatenado y tolerando bytes no imprimibles.
    fn texto_de(trozos: &[Trozo], canal: Canal) -> String {
        let bytes: Vec<u8> = trozos
            .iter()
            .filter(|trozo| trozo.canal == canal)
            .flat_map(|trozo| trozo.bytes.iter().copied())
            .collect();

        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Una orden de shell: el contenedor de prueba trae `sh` de busybox.
    fn orden_de_shell(guion: &str) -> Orden {
        Orden {
            cmd: vec!["sh".to_owned(), "-c".to_owned(), guion.to_owned()],
            env: Vec::new(),
        }
    }

    /// Un `echo` entrega su salida por el callback y el código de salida es `0`.
    ///
    /// Es la prueba mínima de que el camino entero está en pie: crear el `exec`, arrancarlo,
    /// drenar el flujo y leer el código.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_echo_entrega_su_salida_y_el_codigo_es_cero() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("un_echo_entrega_su_salida_y_el_codigo_es_cero").await;

        let orden = Orden {
            cmd: vec!["echo".to_owned(), "hola".to_owned()],
            env: Vec::new(),
        };

        let (codigo, trozos) = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden)
            .await
            .expect("el `echo` debe poder ejecutarse");

        assert_eq!(codigo, 0, "el `echo` debe salir con cero");
        assert_eq!(
            texto_de(&trozos, Canal::Salida),
            "hola\n",
            "la salida no llegó entera: {trozos:?}"
        );
        assert_eq!(
            texto_de(&trozos, Canal::Error),
            "",
            "no puede haber nada por el canal de error: {trozos:?}"
        );

        contenedor.retirar().await;
    }

    /// Un comando que sale con `3` y escribe en `stderr`: el trozo llega por
    /// [`Canal::Error`], con el texto que escribió, y el código devuelto es `3`.
    ///
    /// Es lo que hace que un fallo de volcado se pueda contar: sin el canal de error, el
    /// motivo del fallo se perdería y solo quedaría un número.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_comando_que_falla_entrega_su_motivo_por_el_canal_de_error() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "un_comando_que_falla_entrega_su_motivo_por_el_canal_de_error",
        )
        .await;

        let orden = orden_de_shell("echo el-motivo-del-fallo >&2; exit 3");

        let (codigo, trozos) = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden)
            .await
            .expect("un comando que sale con 3 no es un fallo del transporte");

        assert_eq!(codigo, 3, "el código de salida no es el del comando");
        assert!(
            texto_de(&trozos, Canal::Error).contains("el-motivo-del-fallo"),
            "el motivo no llegó por el canal de error: {trozos:?}"
        );
        assert_eq!(
            texto_de(&trozos, Canal::Salida),
            "",
            "el canal de salida no puede traer el motivo: {trozos:?}"
        );

        contenedor.retirar().await;
    }

    /// Un binario que **no existe** da `127` y el motivo llega por [`Canal::Error`].
    ///
    /// Es la prueba que demuestra que un fallo de extracción se distingue de una base
    /// vacía: `mariadb-dump` no está en `alpine`, así que esto es exactamente lo que pasa
    /// cuando el contenedor de la base no trae el cliente de volcado. Una base vacía sería
    /// un `0` con la salida vacía, que es otra cosa.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_binario_que_no_existe_da_127_y_el_motivo_por_el_canal_de_error() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "un_binario_que_no_existe_da_127_y_el_motivo_por_el_canal_de_error",
        )
        .await;

        let orden = Orden {
            cmd: vec!["mariadb-dump".to_owned(), "--version".to_owned()],
            env: Vec::new(),
        };

        let (codigo, trozos) = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden)
            .await
            .expect("que el binario no exista no rompe el `exec`");

        assert_eq!(codigo, 127, "un binario ausente sale con 127");
        assert_eq!(
            texto_de(&trozos, Canal::Salida),
            "",
            "un binario ausente no escribe nada en la salida: {trozos:?}"
        );
        assert!(
            !texto_de(&trozos, Canal::Error).is_empty(),
            "el motivo del contenedor se perdió: {trozos:?}"
        );

        contenedor.retirar().await;
    }

    /// Un contenedor que no existe: falla, y el motivo **nombra el contenedor**.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_contenedor_que_no_existe_falla_y_el_motivo_lo_nombra() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let nombre = "kesto-prueba-contenedor-que-no-existe";

        let orden = orden_de_shell("echo nunca-llega");

        let error = cliente
            .ejecutar(nombre, &orden, INACTIVIDAD_DE_PRUEBA, |_| {})
            .await
            .expect_err("un contenedor que no existe no puede ejecutar nada");
        let motivo = error.to_string();

        assert!(
            motivo.contains(nombre),
            "el motivo no nombra el contenedor: {motivo}"
        );
        assert!(
            matches!(
                &error,
                Error::MotorContesta {
                    status_code: 404,
                    ..
                }
            ),
            "el contenedor ausente debe llegar como respuesta del motor: {error:?}"
        );
    }

    /// Las variables de entorno de la orden **llegan** al proceso del contenedor.
    ///
    /// Es el camino por el que viajan las credenciales de la base, así que si no llegaran,
    /// el volcado fallaría con un error de autenticación que no diría nada de esto.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn las_variables_de_entorno_de_la_orden_llegan_al_proceso() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("las_variables_de_entorno_de_la_orden_llegan_al_proceso")
                .await;

        let orden = Orden {
            cmd: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "echo PWD=[$MYSQL_PWD]".to_owned(),
            ],
            env: vec!["MYSQL_PWD=secreto-de-prueba".to_owned()],
        };

        let (codigo, trozos) = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden)
            .await
            .expect("el `sh` debe poder ejecutarse");

        assert_eq!(codigo, 0, "el `sh` debe salir con cero");
        assert!(
            texto_de(&trozos, Canal::Salida).contains("PWD=[secreto-de-prueba]"),
            "la variable de entorno no llegó al proceso: {trozos:?}"
        );

        contenedor.retirar().await;
    }

    /// Un volumen grande llega **entero**: los bytes recibidos suman lo que el comando
    /// escribió, sin truncar y sin perder trozos por el camino.
    ///
    /// Es la prueba que cubre el caso del §4.A —un volcado de verdad no es pequeño— sin
    /// necesitar una base de datos: `yes a | head -c` escribe exactamente esa cantidad de
    /// bytes, así que la suma de los trozos tiene que dar ese número exacto.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_volumen_grande_llega_entero() {
        /// Bytes que escribe el comando de la prueba: cuatro mebibytes.
        const BYTES_ESCRITOS: usize = 4 * 1024 * 1024;

        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo("un_volumen_grande_llega_entero").await;

        let orden = orden_de_shell(&format!("yes a | head -c {BYTES_ESCRITOS}"));

        let (codigo, trozos) = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden)
            .await
            .expect("un `stdout` grande no es un fallo del transporte");

        let recibidos: usize = trozos
            .iter()
            .filter(|trozo| trozo.canal == Canal::Salida)
            .map(|trozo| trozo.bytes.len())
            .sum();

        assert_eq!(codigo, 0, "el comando debe salir con cero");
        assert_eq!(
            recibidos, BYTES_ESCRITOS,
            "los bytes no cuadran: se perdieron o se duplicaron trozos"
        );
        assert!(
            trozos
                .iter()
                .filter(|trozo| trozo.canal == Canal::Salida)
                .all(|trozo| trozo
                    .bytes
                    .iter()
                    .all(|byte| *byte == b'a' || *byte == b'\n')),
            "el contenido llegó alterado"
        );

        contenedor.retirar().await;
    }

    /// **LA PRUEBA CLAVE — la forma que usa el motor**: el `exec` se conduce en el runtime
    /// del hilo que lo lanza, no en el de la aplicación.
    ///
    /// Es el candado de la restricción crítica medida: si el `exec` se lanza en un runtime y
    /// su flujo se drena desde otro hilo —con otro runtime— después de que el primero haya
    /// muerto, el drenaje no se atasca, falla en once microsegundos con «A Tokio 1.x context
    /// was found, but it is being shutdown». Esta prueba existe para que nadie
    /// «simplifique» la forma del motor a un `tokio::spawn` o a drenar desde otro runtime:
    /// quien lo haga, cae aquí.
    ///
    /// No es una prueba `#[tokio::test]` a propósito: el runtime que la conduce es el que el
    /// hilo construye **dentro**, que es exactamente lo que hay que fijar.
    #[test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    fn el_exec_se_conduce_en_el_runtime_del_hilo_que_lo_lanza() {
        const PRUEBA: &str = "el_exec_se_conduce_en_el_runtime_del_hilo_que_lo_lanza";

        let socket = socket_del_motor().expect("hace falta un socket de Docker o Podman");

        let (resultado, nombre) = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("el runtime del hilo debe poder construirse");

            runtime.block_on(async move {
                let contenedor = ContenedorDePrueba::nuevo(PRUEBA).await;
                let nombre = contenedor.nombre().to_owned();

                let cliente =
                    Cliente::conectar(&socket, TIMEOUT_DEL_MOTOR).expect("el socket existe");

                let orden = Orden {
                    cmd: vec!["echo".to_owned(), "desde-el-hilo".to_owned()],
                    env: Vec::new(),
                };

                let resultado = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden).await;

                // El borrado explícito, aquí dentro, para que ocurra antes de que el runtime
                // se suelte: el `Drop` del guardián es la red de seguridad, no el camino.
                contenedor.retirar().await;

                (resultado, nombre)
            })
        })
        .join()
        .expect("el hilo que conduce el `exec` no puede entrar en pánico");

        let (codigo, trozos) =
            resultado.expect("el `exec` debe completarse con el runtime del hilo vivo");

        assert_eq!(codigo, 0, "el `echo` de {nombre} debe salir con cero");
        assert_eq!(
            texto_de(&trozos, Canal::Salida),
            "desde-el-hilo\n",
            "la salida no llegó entera desde el runtime del hilo: {trozos:?}"
        );
    }

    /// El tiempo máximo por petición se guarda con **un mínimo de un segundo**, nunca en cero.
    ///
    /// Un `Duration` de milisegundos no puede convertirse en «sin tiempo máximo» sin que
    /// nadie lo haya pedido: la biblioteca cuenta en segundos enteros, su cero no significa
    /// «sin tope», y `as_secs()` **trunca**, así que lo que hay que garantizar es el mínimo.
    #[test]
    fn un_tiempo_maximo_menor_de_un_segundo_no_queda_en_cero() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta().join("socket-de-prueba");
        std::fs::File::create(&ruta).expect("la ruta de la prueba debe poder crearse");
        let ruta = ruta.to_string_lossy().into_owned();

        let cliente = Cliente::conectar(&ruta, Duration::from_millis(500)).expect("la ruta existe");

        assert!(
            format!("{cliente:?}").contains("timeout: 1s"),
            "el tiempo máximo quedó por debajo de un segundo: {cliente:?}"
        );
    }

    /// Un guion que **alterna los dos canales** en la misma ejecución: cada canal llega
    /// completo y en el orden en que se escribió.
    ///
    /// Es el caso que las otras pruebas no ejercitan: una escribe solo en `stdout` y otra solo
    /// en `stderr`, así que el camino mixto —donde el motor multiplexa los dos flujos en uno
    /// y solo el tipo del trozo dice de cuál viene— no estaba cubierto. Y es el camino real de
    /// un volcado: `mariadb-dump` escribe el volcado por `stdout` y sus avisos por `stderr`, a
    /// la vez. Si los bytes se repartieran mal, el volcado llegaría incompleto al repositorio
    /// —o el motivo de un fallo entraría como bytes de copia, que es la confusión que el
    /// requisito prohíbe—.
    ///
    /// # Qué se mide y qué no
    ///
    /// El guion escribe `uno` (S), `dos` (E), `tres` (S) y `cuatro` (E), y el motor entrega
    /// **dos** trozos: `uno\ntres\n` por la salida y `dos\ncuatro\n` por el error. Es decir,
    /// **agrupa por canal**, y el entrelazado fino de cuatro trozos no existe. Está medido
    /// cinco veces con el Podman de esta máquina, siempre `SE`.
    ///
    /// De ahí lo que la prueba afirma y lo que no: el orden **dentro** de cada canal es
    /// contrato de este módulo —los bytes salen como llegaron—, y el orden **entre** canales
    /// es del motor, que aquí agrupa; fijarlo sería fijar su troceado, que puede cambiar en
    /// una versión suya sin que este módulo tenga nada que decir. Lo que sí se fija es que los
    /// dos canales llegan, cada uno con su contenido entero y sin que un trozo traiga bytes
    /// del otro: si se cruzaran, el texto de abajo no cuadraría.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn los_dos_canales_de_una_misma_ejecucion_llegan_completos_y_en_su_orden() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "los_dos_canales_de_una_misma_ejecucion_llegan_completos_y_en_su_orden",
        )
        .await;

        let orden = orden_de_shell("echo uno; echo dos >&2; echo tres; echo cuatro >&2");

        let (codigo, trozos) = ejecutar_recogiendo(&cliente, contenedor.nombre(), &orden)
            .await
            .expect("alternar los dos canales no es un fallo del transporte");

        assert_eq!(codigo, 0, "el guion debe salir con cero");
        assert!(
            !trozos.is_empty(),
            "una ejecución que escribe por los dos canales no puede no entregar nada"
        );

        // Cada canal con sus bytes y en su orden: si un trozo de un canal hubiera viajado con
        // el otro —o si se hubiera perdido uno— este par de aserciones no cuadra.
        assert_eq!(
            texto_de(&trozos, Canal::Salida),
            "uno\ntres\n",
            "la salida no llegó completa y en orden por su canal: {trozos:?}"
        );
        assert_eq!(
            texto_de(&trozos, Canal::Error),
            "dos\ncuatro\n",
            "el error no llegó completo y en orden por su canal: {trozos:?}"
        );

        // Y los dos canales están presentes: el caso mixto es el que se quería ejercitar, así
        // que una ejecución que solo hubiera entregado uno no vale como esta prueba.
        assert!(
            trozos.iter().any(|trozo| trozo.canal == Canal::Salida)
                && trozos.iter().any(|trozo| trozo.canal == Canal::Error),
            "faltó uno de los dos canales: {trozos:?}"
        );

        contenedor.retirar().await;
    }

    /// Una extracción **legítimamente vacía**: código `0`, ninguna llamada al callback y
    /// ningún byte.
    ///
    /// Es el espejo del `127` que ya se prueba. Aquél fija que un fallo no se lea como base
    /// vacía; éste fija lo contrario, que es la mitad que faltaba: que una base vacía no se
    /// lea como fallo. La distinción no se puede hacer por el `stdout` —los dos son cero
    /// bytes—, así que se hace por el código de salida y por el canal: `true` sale con `0` y
    /// no escribe **nada**, ni por la salida ni por el error.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_base_vacia_no_se_lee_como_fallo_ni_llama_al_callback() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("una_base_vacia_no_se_lee_como_fallo_ni_llama_al_callback")
                .await;

        let orden = orden_de_shell("true");

        let mut llamadas = 0usize;
        let mut del_canal_de_error = 0usize;

        let codigo = {
            let mut contar = |trozo: Trozo| {
                llamadas += 1;
                if trozo.canal == Canal::Error {
                    del_canal_de_error += 1;
                }
            };

            cliente
                .ejecutar(
                    contenedor.nombre(),
                    &orden,
                    INACTIVIDAD_DE_PRUEBA,
                    &mut contar,
                )
                .await
                .expect("una base vacía no es un fallo de extracción")
        };

        assert_eq!(codigo, 0, "una extracción vacía y correcta sale con cero");
        assert_eq!(
            llamadas, 0,
            "el callback se llamó {llamadas} veces con una extracción que no escribió nada"
        );
        assert_eq!(
            del_canal_de_error, 0,
            "no puede haber nada por el canal de error: el comando salió con cero"
        );

        contenedor.retirar().await;
    }

    /// Una orden que **se calla más que el plazo de inactividad** acaba en
    /// [`Error::ExtraccionEstancada`], y el motivo nombra el contenedor y el plazo.
    ///
    /// Es el candado del requisito del motor que el `timeout` del cliente **no** cubre: un
    /// `mariadb-dump` colgado deja la ejecución en `running` para siempre, y con ella el
    /// worker de su repositorio, con todo el trabajo posterior de ese repositorio haciendo
    /// cola detrás. `sleep 600` no escribe una sola línea: es exactamente ese cuelgue, sin
    /// necesitar una base de datos.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_orden_que_se_calla_mas_que_el_plazo_acaba_en_extraccion_estancada() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "una_orden_que_se_calla_mas_que_el_plazo_acaba_en_extraccion_estancada",
        )
        .await;

        let orden = orden_de_shell("sleep 600");

        let error = cliente
            .ejecutar(contenedor.nombre(), &orden, INACTIVIDAD_CORTA, |_| {})
            .await
            .expect_err("una orden que se calla más que el plazo no puede darse por buena");
        let motivo = error.to_string();

        assert!(
            matches!(
                &error,
                Error::ExtraccionEstancada { contenedor: cual, inactividad }
                    if cual == contenedor.nombre() && *inactividad == INACTIVIDAD_CORTA
            ),
            "el vencimiento no llegó como extracción estancada con su plazo: {error:?}"
        );
        assert!(
            motivo.contains(contenedor.nombre()),
            "el motivo no nombra el contenedor: {motivo}"
        );
        assert!(
            motivo.contains("colgada"),
            "el motivo no dice qué puede estar pasando: {motivo}"
        );

        contenedor.retirar().await;
    }

    /// `comprobar` falla si al otro lado del socket no hay un motor, y lo nombra.
    ///
    /// Es el camino del aviso de arranque, y se ejercita **sin motor de contenedores**: basta
    /// con apuntar el cliente a un fichero normal —como hace la prueba de las dos formas del
    /// socket— porque el `ping` habla por la ruta y un fichero no contesta. Es el candado de
    /// la única función del módulo que no lo tenía, y la que decide si el arranque avisa.
    #[tokio::test]
    async fn comprobar_falla_si_al_otro_lado_no_hay_motor() {
        let directorio = directorio_de_prueba();
        let ruta = directorio.ruta().join("fichero-que-no-es-un-socket");
        std::fs::File::create(&ruta).expect("la ruta de la prueba debe poder crearse");
        let ruta = ruta.to_string_lossy().into_owned();

        let cliente = Cliente::conectar(&ruta, TIMEOUT_DEL_MOTOR)
            .expect("la ruta existe, así que el cliente se construye");

        let error = cliente
            .comprobar()
            .await
            .expect_err("un fichero normal no contesta a un `ping`");
        let motivo = error.to_string();

        assert!(
            matches!(&error, Error::MotorNoDisponible { socket, .. } if socket == &ruta),
            "el fallo del `ping` no se reconoció como motor no disponible: {error:?}"
        );
        assert!(
            motivo.contains(&ruta),
            "el motivo no nombra el socket: {motivo}"
        );
    }

    /// `comprobar` **dice que sí** cuando el motor contesta.
    ///
    /// La otra dirección de la prueba de arriba, y la que hace falta para que `comprobar` sirva
    /// de algo: una función que siempre fallara pasaría el candado del fallo y dejaría el
    /// arranque avisando de un socket que está perfectamente vivo.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn comprobar_dice_que_si_cuando_el_motor_contesta() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");

        cliente
            .comprobar()
            .await
            .expect("un motor vivo contesta al `ping`");
    }
}
