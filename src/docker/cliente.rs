//! El cliente del motor de contenedores: la conexión y el `exec` con su salida.

use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
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
        // El tiempo de la biblioteca son segundos enteros, y cero no significa «sin tope»:
        // se redondea hacia arriba a un segundo para que un `Duration` de milisegundos no
        // se convierta en un tiempo máximo nulo sin que nadie lo haya pedido.
        let segundos = timeout.as_secs().max(1);

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
        while let Some(trozo) = output.next().await {
            let trozo = trozo.map_err(|causa| Error::del_contenedor(contenedor, causa))?;

            let (canal, bytes) = match trozo {
                LogOutput::StdOut { message } => (Canal::Salida, message),
                LogOutput::StdErr { message } => (Canal::Error, message),
                // Con tty —que aquí no se pide— no hay separación posible: todo sale como
                // `Console`, y se entrega como salida para que activarlo no dejara la
                // lectura sin nada. `StdIn` solo llegaría si se pidiera `attach_stdin`.
                LogOutput::Console { message } => (Canal::Salida, message),
                LogOutput::StdIn { message } => (Canal::Salida, message),
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

        // Un `exit_code` ausente significa que el motor no lo informó, y darlo por cero
        // convertiría un fallo en un éxito: para una copia, eso es la peor de las salidas
        // —existe, ocupa sitio y no se puede restaurar—. Un valor que no cabe en un `i32`
        // tampoco es un código de salida, y por eso los dos desenlaces van a la misma
        // variante: en ninguno de los dos se puede decir cómo acabó el proceso.
        estado
            .exit_code
            .and_then(|codigo| i32::try_from(codigo).ok())
            .ok_or_else(|| Error::SinCodigoDeSalida {
                contenedor: contenedor.to_owned(),
            })
    }
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

    use super::Cliente;
    use crate::docker::{Canal, Error, Orden, Trozo};
    use crate::test_support::{
        ContenedorDePrueba, TIMEOUT_DEL_MOTOR, cliente_del_motor, directorio_de_prueba,
        socket_del_motor,
    };

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

    /// Ejecuta una orden y devuelve su código de salida y todos sus trozos, en orden.
    ///
    /// El cierre va **prestado** —un `&mut` de un `FnMut` es `FnMut`— y vive en su propio
    /// bloque para que el préstamo de `trozos` acabe con la llamada y se pueda leer después.
    async fn ejecutar_recogiendo(
        cliente: &Cliente,
        contenedor: &str,
        orden: &Orden,
    ) -> Result<(i32, Vec<Trozo>), Error> {
        let mut trozos: Vec<Trozo> = Vec::new();

        let codigo = {
            let mut guardar = |trozo: Trozo| trozos.push(trozo);

            cliente.ejecutar(contenedor, orden, &mut guardar).await?
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn un_contenedor_que_no_existe_falla_y_el_motivo_lo_nombra() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let nombre = "kesto-prueba-contenedor-que-no-existe";

        let orden = orden_de_shell("echo nunca-llega");

        let error = cliente
            .ejecutar(nombre, &orden, |_| {})
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
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

    /// El tiempo máximo por petición se guarda redondeado a segundos, nunca en cero.
    ///
    /// Un `Duration` de milisegundos no puede convertirse en «sin tiempo máximo» sin que
    /// nadie lo haya pedido: la biblioteca cuenta en segundos enteros y su cero no
    /// significa «sin tope».
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
}
