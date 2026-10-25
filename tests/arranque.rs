//! Prueba de integración del **arranque real** del binario de Kesto.
//!
//! El `SHALL` «la aplicación SHALL intentar el descubrimiento del proveedor al
//! arrancar … SHALL arrancar igualmente con un aviso» vive en `src/main.rs`, que
//! ningún test unitario toca: el escenario 1.5 prueba `descubrimiento_al_arrancar`
//! **como función**, pero no que el arranque la llame ni que emita el `WARN`. Si
//! alguien borrase ese bloque de `main`, la suite seguiría verde y la aplicación
//! dejaría de cumplir el `SHALL` sin que nada lo delate.
//!
//! Aquí se lanza el binario de verdad con un emisor al que nada contesta y se
//! comprueba, leyendo su salida de registro, que **avisa** y que **arranca**. Es
//! lo más robusto que se puede hacer sin deformar `main`: se verifica el camino
//! real —lectura del entorno, inicialización del registro, llamada al
//! descubrimiento y arranque del servidor—, no una función extraída que podría
//! ejercitarse sin estar cableada en el arranque.
//!
//! Y no solo mira el registro: **también mira lo que el arranque dejó en disco**.
//! Después de ver la línea `escuchando`, comprueba que el binario creó
//! `<directorio>/kesto.db` y que dentro están las cuatro tablas del §3. Es lo que
//! ata `main` a la base: sin estas dos comprobaciones, borrar el bloque
//! `let _database = …` de `src/main.rs` dejaría la suite verde y el arranque sin
//! abrir nada.
//!
//! **No sale a la red**: el emisor es un puerto de `127.0.0.1` que se reserva y
//! no se suelta (ver [`emisor_caido`]), y no se resuelve ningún DNS.
//!
//! # La tarea 1.34 de `backup-engine`
//!
//! El fichero lleva además la prueba del **arranque y la parada** con el motor de copias dentro:
//! un socket de Docker inalcanzable no impide arrancar, el motor está montado antes de escuchar y
//! un `SIGTERM` acaba con el proceso dentro de un plazo acotado. Es la misma maquinaria —el
//! binario de verdad, su registro leído por líneas y su salud preguntada por HTTP—, con dos
//! pruebas más al final del fichero; el encabezado de cada una dice qué afirma y qué no.
//!
//! Las tres cosas que afirma son desenlaces **negativos** —que arranque igual, que el motor esté
//! montado *antes* de otra cosa, que no se cuelgue—, así que ninguna de estas pruebas puede estar
//! roja hoy: pasan y seguirán pasando mientras nadie reordene `main`. Son **guardas de
//! regresión**, y el requisito que protegen es real; lo que no se puede es fingir que son una roja
//! medida, y por eso queda dicho aquí y en el informe de la tarea.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use uuid::Uuid;

/// Clave maestra larga: que no se cuele ningún aviso por credencial débil, y el
/// único `WARN` de la salida sea el del descubrimiento.
const MASTER_KEY: &str = "clave-maestra-distinguible-000000000000000000000000000000000000";
/// Clave de firma de la sesión, con la longitud de sobra, por el mismo motivo.
const SESSION_SECRET: &str = "clave-de-firma-distinguible-000000000000000000000000000000000000";

/// Marca del aviso del arranque: es la señal que se espera en la salida.
const AVISO_DESCUBRIMIENTO: &str = "descubrir el proveedor de identidad al arrancar";
/// Línea que prueba que el arranque llegó a servir.
const ARRANCADO: &str = "escuchando";
/// Tope de espera del arranque: el descubrimiento falla al instante, así que es
/// holgado; no es un `sleep` a ciegas, es el límite de un `recv_timeout`.
const ESPERA: Duration = Duration::from_secs(30);

/// Proceso hijo que se mata al soltarse, pase lo que pase.
///
/// Es lo que garantiza que el proceso muere **también si el test entra en
/// pánico**: el `Drop` corre al desenrollar la pila y no deja el servidor vivo.
struct Hijo(Child);

impl Drop for Hijo {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Directorio temporal que se borra al soltarse.
///
/// La guarda se declara **antes** que [`Hijo`] a propósito: las variables se
/// sueltan en orden inverso al de declaración, así que el proceso muere —y suelta
/// la base— antes de que el directorio desaparezca.
struct DirectorioTemporal(PathBuf);

impl Drop for DirectorioTemporal {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Emisor al que nada contesta, con el puerto **retenido**.
///
/// Igual que el `emisor_caido` del arnés unitario: un `TcpListener` que se
/// conserva en un hilo que acepta cada conexión y la cierra sin contestar. Así el
/// puerto no vuelve a quedar libre —ningún otro test en paralelo puede ocuparlo—
/// y `reqwest` falla al instante en vez de esperar al timeout.
fn emisor_caido() -> String {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("la reserva de un puerto no puede fallar");
    let puerto = listener
        .local_addr()
        .expect("el puerto reservado debe conocerse")
        .port();

    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            drop(stream);
        }
    });

    format!("http://127.0.0.1:{puerto}")
}

#[test]
fn el_arranque_avisa_si_el_proveedor_no_contesta_y_arranca_igualmente() {
    let emisor = emisor_caido();

    // La base de datos, en un directorio temporal propio.
    //
    // `env_clear` deja la prueba hermética, pero el `DATABASE_URL` por defecto del
    // SRS es `sqlite:///app/data/kesto.db`, y ese directorio no existe en una
    // máquina de desarrollo: el arranque **fallaría**, que es lo correcto según el
    // escenario 1.2 —no se crea el directorio— pero no es lo que esta prueba mide,
    // que es el aviso del descubrimiento y que el proceso siga vivo y escuchando.
    // Por eso la base va a un directorio temporal, y sus dos aserciones no cambian.
    let base = std::env::temp_dir().join(format!("kesto-arranque-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&base).expect("el directorio temporal de la prueba debe crearse");

    // La guarda va **antes** que `Hijo`: al soltarse las variables en orden inverso,
    // el proceso muere —y suelta la base— antes de que el directorio desaparezca.
    let directorio = DirectorioTemporal(base);
    let dsn = format!("sqlite://{}/kesto.db", directorio.0.display());

    let child = Command::new(env!("CARGO_BIN_EXE_kesto"))
        // `env_clear` hace la prueba hermética: la salida solo depende de lo que
        // se fija aquí, no del entorno de quien corre la suite.
        .env_clear()
        .env("KESTO_MASTER_KEY", MASTER_KEY)
        .env("KESTO_SECRET", SESSION_SECRET)
        .env("OIDC_ISSUER_URL", &emisor)
        .env("OIDC_CLIENT_ID", "kesto-panel")
        .env("OIDC_CLIENT_SECRET", "secreto-de-cliente-distinguible")
        .env("DATABASE_URL", &dsn)
        // Un puerto efímero: lo elige el sistema y ningún test choca con el 8080
        // del Kesto en desarrollo.
        .env("BIND_ADDRESS", "127.0.0.1:0")
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        // El `stderr` se descarta: la configuración es válida, así que el camino
        // de error —que escribe ahí— no se recorre, y no leerlo evitaría un
        // `eprintln` que sature la tubería.
        .stderr(Stdio::null())
        .spawn()
        .expect("el binario de Kesto debe poder lanzarse");

    let mut hijo = Hijo(child);
    let stdout = hijo.0.stdout.take().expect("stdout capturado");

    // Un hilo lee la salida y la reenvía: así el test principal puede esperar con
    // un tope sin bloquearse para siempre en un `read_line`.
    let (emisor_lineas, receptor) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for linea in BufReader::new(stdout).lines() {
            match linea {
                Ok(linea) => {
                    if emisor_lineas.send(linea).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let inicio = Instant::now();
    let mut salida = String::new();
    let mut aviso_visto = false;
    let mut arrancado_visto = false;

    while !(aviso_visto && arrancado_visto) {
        let Some(restante) = ESPERA.checked_sub(inicio.elapsed()) else {
            break;
        };
        if restante.is_zero() {
            break;
        }

        match receptor.recv_timeout(restante) {
            Ok(linea) => {
                if linea.contains(AVISO_DESCUBRIMIENTO) {
                    aviso_visto = true;
                }
                if linea.contains(ARRANCADO) {
                    arrancado_visto = true;
                }
                salida.push_str(&linea);
                salida.push('\n');
            }
            Err(_) => break,
        }
    }

    assert!(
        aviso_visto,
        "el arranque no emitió el aviso del descubrimiento fallido. Salida:\n{salida}"
    );
    assert!(
        arrancado_visto,
        "el arranque no llegó a escuchar: no arrancó pese al proveedor caído. Salida:\n{salida}"
    );
    assert!(
        hijo.0
            .try_wait()
            .expect("try_wait no puede fallar")
            .is_none(),
        "el proceso terminó en vez de seguir arrancado. Salida:\n{salida}"
    );

    // Lo que el arranque dejó en disco, que es lo que ata `main` a la base: el
    // fichero de la base y las cuatro tablas del §3.
    let ruta_de_la_base = directorio.0.join("kesto.db");
    assert!(
        ruta_de_la_base.exists(),
        "el arranque no creó el fichero de la base en {}: si alguien borra el bloque \
         `let _database = …` de main, esto lo delata. Salida:\n{salida}",
        ruta_de_la_base.display()
    );

    // El runtime se construye a mano en vez de volver el test `#[tokio::test]`: un
    // `#[tokio::test]` dejaría el `recv_timeout` de arriba bloqueando el ejecutor.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("el runtime de la prueba debe construirse");

    let tablas: Vec<String> = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&dsn)
            .await
            .expect("la base que dejó el arranque debe poder abrirse");

        let tablas =
            sqlx::query_scalar::<_, String>("SELECT name FROM sqlite_master WHERE type = 'table'")
                .fetch_all(&pool)
                .await
                .expect("la consulta de tablas no puede fallar");

        pool.close().await;

        tablas
    });

    for esperada in [
        "repositories",
        "backup_targets",
        "backup_jobs",
        "job_executions",
    ] {
        assert!(
            tablas.iter().any(|tabla| tabla == esperada),
            "el arranque no dejó la tabla {esperada}: {tablas:?}"
        );
    }
}

// ── La tarea 1.34: el arranque con el motor de copias dentro y la parada ─────────────
//
// Lo que se mide aquí es el **orden** y los **desenlaces de no-fallo** del requisito 9 de
// `engine` («el arranque monta el motor y el cierre ordenado lo para»), que es lo único de ese
// requisito que no vive en una función con prueba propia: el orden está en la secuencia de
// `main` y la parada, en su encadenado. Es lo mismo que ya justifica la prueba del descubrimiento
// de arriba —`main` no tiene prueba unitaria, así que se le lanza de verdad— aplicado al motor.

/// Marca del aviso con el que el arranque deja constancia de que el socket del motor de
/// contenedores no responde.
///
/// Se espera esta marca **en la salida del proceso** por una razón de orden y no de contenido:
/// `main` la emite **después** de montar el motor y **antes** de la línea de escucha, así que verla
/// antes de `ARRANCADO` es la prueba —sin deformar `main`— de que el motor se montó y de dónde se
/// montó respecto al listener.
const AVISO_DEL_MOTOR_MUDO: &str = "no puede hablar con el motor de contenedores";
/// Marca de la señal de cierre atendida: solo la emite el apagado ordenado.
const SENAL_DE_CIERRE: &str = "señal de cierre recibida";
/// Marca de la parada del motor, que solo se emite al pararlo.
const MOTOR_PARADO: &str = "motor parado";

/// Plazo para que el proceso **acabe** tras el `SIGTERM`.
///
/// Generoso a propósito: lo que se mide es que acaba, no cuánto tarda, y aquí no hay ninguna
/// ejecución en vuelo —el proceso de esta prueba no tiene contra qué copiar—, así que su parada no
/// espera a nadie y el proceso tendría que acabar en milisegundos. El número es mayor que la
/// espera de parada del motor (cinco segundos) para que un apagado que sí espera se vea como lo que
/// es —el proceso **termina**, solo que tarde— y no como un plazo demasiado corto.
const PLAZO_DE_LA_PARADA: Duration = Duration::from_secs(10);

/// Plazo de una petición de salud suelta, con el puerto ya contestando.
///
/// Corto porque la respuesta es local y diminuta: lo que acota esto es que el proceso no conteste
/// **nunca**, para que el bucle de [`esperar_la_salud`] reintente en vez de quedarse colgado en una
/// lectura que no llega.
const PLAZO_DE_LA_PETICION: Duration = Duration::from_secs(5);

/// Cuántos arranques se intentan antes de dar por bueno el desenlace.
///
/// Ver [`arrancar`]: es la red que tapa la carrera del puerto. Tres es holgado para un suceso que
/// casi no ocurre, y no esconde un fallo de verdad, porque un fallo de verdad no deja el proceso
/// muerto: deja un proceso vivo al que le faltan marcas, que es el caso que se devuelve sin
/// reintentar.
const INTENTOS_DE_ARRANQUE: usize = 3;

/// Un proceso de Kesto en marcha, con su base temporal y su registro recogido por líneas.
///
/// Los campos se sueltan **en el orden en que se declaran**, y por eso [`Hijo`] va antes que
/// [`DirectorioTemporal`]: al soltarse la estructura, el proceso muere —y suelta la base— antes de
/// que el directorio desaparezca. Es la misma precaución que en la prueba de arriba, donde se
/// consigue con el orden de las variables locales.
struct EnMarcha {
    /// El proceso. Se mata al soltarse, pase lo que pase, incluso si la prueba entra en pánico.
    hijo: Hijo,
    /// El directorio con la base, que se borra al soltarse.
    directorio: DirectorioTemporal,
    /// El puerto en el que se le pidió escuchar.
    puerto: u16,
    /// Las líneas que el hilo lector va entregando, en el orden en que salieron.
    lineas: mpsc::Receiver<String>,
    /// Lo ya leído de esas líneas, también en orden.
    salida: Vec<String>,
}

/// Lo que contestó la salud: el código de la línea de estado y el cuerpo.
struct Salud {
    codigo: u16,
    cuerpo: String,
}

impl EnMarcha {
    /// `true` si alguna de las líneas leídas contiene `marca`.
    fn contiene(&self, marca: &str) -> bool {
        self.salida.iter().any(|linea| linea.contains(marca))
    }

    /// La posición de `marca` en el registro: el índice de la primera línea que la contiene.
    fn posicion(&self, marca: &str) -> Option<usize> {
        self.salida.iter().position(|linea| linea.contains(marca))
    }

    /// Todo lo leído, para poder enseñarlo cuando una aserción falle.
    fn registro(&self) -> String {
        self.salida.join("\n")
    }

    /// `true` si el proceso ya ha terminado —y ha quedado recogido—.
    fn ha_terminado(&mut self) -> bool {
        self.hijo
            .0
            .try_wait()
            .expect("try_wait no puede fallar")
            .is_some()
    }

    /// El fichero de la base que el arranque tiene que haber dejado.
    fn ruta_de_la_base(&self) -> PathBuf {
        self.directorio.0.join("kesto.db")
    }

    /// Lee líneas hasta que el registro contenga **todas** las `marcas`, o se agote `plazo`.
    ///
    /// No devuelve nada: deja lo leído en [`EnMarcha::salida`] y son las aserciones de quien llama
    /// las que dicen qué faltaba y dentro de qué plazo se esperaba. Se para antes si el proceso
    /// muere, que para el caso es lo mismo: lo que hay leído es todo lo que habrá.
    fn esperar_las_marcas(&mut self, plazo: Duration, marcas: &[&str]) {
        let inicio = Instant::now();

        while !marcas.iter().all(|marca| self.contiene(marca)) {
            let Some(restante) = plazo.checked_sub(inicio.elapsed()) else {
                break;
            };
            if restante.is_zero() {
                break;
            }

            match self.lineas.recv_timeout(restante) {
                Ok(linea) => self.salida.push(linea),
                Err(_) => break,
            }
        }
    }

    /// Vacía lo que quede en la tubería una vez que el proceso ha terminado.
    ///
    /// Hace falta porque el apagado del proceso y el drenaje del hilo lector van a la par: cuando
    /// `try_wait` ve el proceso muerto, sus últimas líneas pueden seguir en el tubo. El fin de la
    /// tubería —el `Disconnected` del canal, cuando el hilo lector termina— es la señal de que no
    /// queda nada, así que se espera a él con un plazo en vez de mirar solo lo leído hasta ahora.
    fn drenar(&mut self) {
        while let Ok(linea) = self.lineas.recv_timeout(Duration::from_secs(1)) {
            self.salida.push(linea);
        }
    }

    /// Espera a que el proceso termine y devuelve cómo terminó, o `None` si se agota `plazo`.
    fn esperar_el_final(&mut self, plazo: Duration) -> Option<ExitStatus> {
        let inicio = Instant::now();

        loop {
            if let Some(estado) = self.hijo.0.try_wait().expect("try_wait no puede fallar") {
                return Some(estado);
            }

            match plazo.checked_sub(inicio.elapsed()) {
                Some(restante) if !restante.is_zero() => {
                    std::thread::sleep(restante.min(Duration::from_millis(50)));
                }
                _ => return None,
            }
        }
    }

    /// Manda `SIGTERM` a **este** proceso, con el `kill` del sistema.
    ///
    /// No vale `Child::kill`, que manda `SIGKILL` y no deja recorrer el cierre ordenado —y el
    /// requisito es sobre `SIGTERM`—, y no se añade `libc` ni `nix` al manifiesto por esto: la
    /// suite corre con `--locked` y sin red, así que un crate nuevo aquí rompería el CI. El binario
    /// `kill` está en cualquier imagen de Linux capaz de compilar esto. Y se llama **con el
    /// identificador del hijo**, nunca con `pkill -x kesto`, que mataría también al proceso de la
    /// otra prueba de este fichero, que corre en paralelo.
    fn sigterm(&self) -> Result<(), String> {
        let pid = self.hijo.0.id().to_string();

        let estado = Command::new("kill")
            .args(["-s", "TERM", &pid])
            .status()
            .map_err(|error| {
                format!(
                    "no se pudo ejecutar `kill` para el proceso {pid}: {error} \
                         (¿está en el `PATH`?)"
                )
            })?;

        if !estado.success() {
            return Err(format!("`kill -s TERM {pid}` no salió bien: {estado}"));
        }

        Ok(())
    }
}

/// Un puerto libre de `127.0.0.1`, **ya soltado**.
///
/// El binario necesita un puerto conocido: con `BIND_ADDRESS=127.0.0.1:0` el sistema elige uno,
/// pero el proceso solo publica **lo que pidió** —la línea de escucha lleva la dirección de la
/// configuración, no la que quedó atada—, así que la prueba no tendría forma de saber a qué puerto
/// preguntar. De ahí reservar uno efímero y soltarlo acto seguido para que el binario pueda atarlo.
/// La carrera que eso abre la tapa [`arrancar`], y se paga a gusto: la alternativa sería buscar el
/// puerto del hijo en `/proc`, que ata la prueba a Linux y a cómo el núcleo numera los sockets.
fn puerto_libre() -> u16 {
    let reserva =
        TcpListener::bind("127.0.0.1:0").expect("la reserva de un puerto no puede fallar");

    reserva
        .local_addr()
        .expect("el puerto reservado debe conocerse")
        .port()
}

/// La ruta de un socket de Docker que **no existe**.
///
/// Es la forma que tiene esta prueba de tener un socket inalcanzable sin montar nada: `bollard`
/// comprueba la existencia del fichero al construir el cliente —medido en su `connect_with_socket`,
/// que devuelve `SocketNotFoundError`—, así que el aviso sale al instante y no hay ni un contenedor
/// ni una espera de red. La ruta **no se crea** en ningún momento: crear el fichero sería dejar de
/// medir lo que esto dice medir. Y va bajo el directorio temporal, que no existe: no se toca nada
/// del sistema.
fn socket_muerto() -> String {
    let ruta = std::env::temp_dir().join(format!("kesto-socket-muerto-{}", Uuid::new_v4()));

    format!("unix://{}/podman.sock", ruta.display())
}

/// Lanza el binario de Kesto con una base temporal y el entorno fijado aquí.
///
/// El entorno va con `env_clear` y **solo** con lo que estas pruebas necesitan —la clave maestra,
/// la base, la dirección de escucha y el socket del motor—, así que la salida depende de lo escrito
/// aquí y no del entorno de quien corre la suite. Las tres `OIDC_*` se dejan **fuera** a propósito:
/// lo que estas pruebas miden no es la autenticación, y sin ellas el modo es `Disabled` y la salud
/// —que es pública en los dos modos— contesta sin sesión, que es lo que hace falta.
fn lanzar(puerto: u16, socket_del_motor: &str) -> EnMarcha {
    let base = std::env::temp_dir().join(format!("kesto-arranque-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&base).expect("el directorio temporal de la prueba debe crearse");

    let directorio = DirectorioTemporal(base);
    let dsn = format!("sqlite://{}/kesto.db", directorio.0.display());

    let child = Command::new(env!("CARGO_BIN_EXE_kesto"))
        .env_clear()
        .env("KESTO_MASTER_KEY", MASTER_KEY)
        .env("DATABASE_URL", &dsn)
        .env("BIND_ADDRESS", format!("127.0.0.1:{puerto}"))
        .env("DOCKER_HOST", socket_del_motor)
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        // El `stderr` se descarta, como en la prueba de arriba: la configuración es válida y el
        // camino de error no se recorre. Un fallo de arranque sale por ahí —y entonces el registro
        // del proceso estará vacío—, que es lo que hay que leer en el mensaje de la aserción.
        .stderr(Stdio::null())
        .spawn()
        .expect("el binario de Kesto debe poder lanzarse");

    let mut hijo = Hijo(child);
    let stdout = hijo.0.stdout.take().expect("stdout capturado");

    // El hilo lector, igual que en la prueba de arriba: entrega las líneas por un canal para que la
    // prueba pueda esperarlas con un tope en vez de bloquearse para siempre en un `read_line`.
    let (emisor_lineas, lineas) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for linea in BufReader::new(stdout).lines() {
            match linea {
                Ok(linea) => {
                    if emisor_lineas.send(linea).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    EnMarcha {
        hijo,
        directorio,
        puerto,
        lineas,
        salida: Vec::new(),
    }
}

/// Arranca el binario y espera a sus `marcas`, reintentando si el puerto le jugó una mala pasada.
///
/// El puerto se reserva y se suelta —ver [`puerto_libre`]—, así que entre soltarlo y que el binario
/// lo ate cabe una carrera con la otra prueba de este fichero, que corre en paralelo: si el proceso
/// **muere** sin llegar a las marcas, el sospechoso es el puerto y se reintenta con otro hasta
/// [`INTENTOS_DE_ARRANQUE`]. Si sigue **vivo**, no se reintenta nada: ese es el caso interesante
/// —un aviso que no llega, un arranque que se queda a medias— y quien llama tiene que ver su
/// registro, que es lo que devuelve el último intento.
fn arrancar(socket_del_motor: &str, marcas: &[&str]) -> EnMarcha {
    let mut intento = 0_usize;

    loop {
        intento += 1;

        let mut en_marcha = lanzar(puerto_libre(), socket_del_motor);
        en_marcha.esperar_las_marcas(ESPERA, marcas);

        let lo_tiene_todo = marcas.iter().all(|marca| en_marcha.contiene(marca));
        if lo_tiene_todo || !en_marcha.ha_terminado() || intento >= INTENTOS_DE_ARRANQUE {
            return en_marcha;
        }
    }
}

/// Pregunta por la salud hasta que conteste, y devuelve lo que contestó.
///
/// La marca `ARRANCADO` **no** prueba que el proceso atienda: `main` la emite una línea antes de que
/// `serve` ate el puerto, así que verla solo dice que el proceso llegó hasta ahí. Por eso aquí se
/// pregunta de verdad, en bucle y con plazo, y el motivo del último intento vuelve para poder
/// decirlo en la aserción cuando el plazo se agote.
fn esperar_la_salud(puerto: u16, plazo: Duration) -> Result<Salud, String> {
    let inicio = Instant::now();

    loop {
        let motivo = match pedir_la_salud(puerto) {
            Ok(salud) => return Ok(salud),
            Err(motivo) => motivo,
        };

        match plazo.checked_sub(inicio.elapsed()) {
            Some(restante) if !restante.is_zero() => {
                std::thread::sleep(restante.min(Duration::from_millis(50)));
            }
            _ => return Err(motivo),
        }
    }
}

/// Una petición `GET /api/v1/health` escrita a mano.
///
/// A mano y no con un cliente HTTP porque la suite corre con `--locked` y sin red: añadir un cliente
/// al manifiesto por esto rompería el CI. La petición va con `Connection: close`, se lee hasta el
/// cierre y el cuerpo se separa por la primera línea vacía; el cuerpo es un `Json` de axum, que
/// lleva su longitud en las cabeceras, así que no hay troceado que deshacer.
fn pedir_la_salud(puerto: u16) -> Result<Salud, String> {
    let direccion = SocketAddr::from(([127, 0, 0, 1], puerto));

    let mut conexion = TcpStream::connect_timeout(&direccion, PLAZO_DE_LA_PETICION)
        .map_err(|error| format!("no se pudo abrir la conexión con {direccion}: {error}"))?;

    conexion
        .set_read_timeout(Some(PLAZO_DE_LA_PETICION))
        .map_err(|error| format!("no se pudo acotar la lectura: {error}"))?;

    conexion
        .write_all(
            b"GET /api/v1/health HTTP/1.1\r\n\
              Host: 127.0.0.1\r\n\
              Connection: close\r\n\r\n",
        )
        .map_err(|error| format!("no se pudo escribir la petición: {error}"))?;

    let mut respuesta = Vec::new();
    conexion
        .read_to_end(&mut respuesta)
        .map_err(|error| format!("no se pudo leer la respuesta: {error}"))?;

    let texto = String::from_utf8_lossy(&respuesta).into_owned();

    let (cabeceras, cuerpo) = texto
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("la respuesta llegó sin cabeceras: {texto}"))?;

    let codigo = cabeceras
        .split("\r\n")
        .next()
        .and_then(|linea| linea.split(' ').nth(1))
        .and_then(|codigo| codigo.parse::<u16>().ok())
        .ok_or_else(|| format!("la respuesta llegó sin línea de estado: {texto}"))?;

    Ok(Salud {
        codigo,
        cuerpo: cuerpo.to_owned(),
    })
}

/// 1.34 (escenarios 1 y 3) — **Un socket de Docker inalcanzable no impide arrancar, y el motor
/// está montado antes de escuchar.**
///
/// Los tres desenlaces que afirma, cada uno con su aserción y su mensaje:
///
/// 1. el aviso del socket inalcanzable está en el registro del proceso —lo que prueba que el
///    arranque pasó por `Motor::comprobar`, que solo se puede llamar con el motor montado—;
/// 2. esa línea va **antes** de la de escucha, que es el orden del requisito: la base, el motor y
///    el listener, en ese orden;
/// 3. el proceso siguió vivo y **atendió** `GET /api/v1/health` con un `200` y la envoltura del
///    contrato. Esto es la mitad que no se queda en la narración del proceso: si `Motor::arrancar`
///    hubiera fallado, `main` terminaría antes de escuchar —no hay modo degradado— y la salud no
///    contestaría.
///
/// **No es una prueba roja, y se dice**: los tres desenlaces son negativos, así que pasa hoy y
/// seguirá pasando mientras nadie toque `main`. Es una guarda de regresión: la rompe quien mueva el
/// montaje del motor detrás del listener —la marca 2 deja de cumplirse—, quien lo borre —el proceso
/// no arranca y las tres caen— y quien deje que un socket caído impida arrancar —caen 1 y 3—.
#[test]
fn un_socket_de_docker_inalcanzable_no_impide_arrancar_y_el_motor_va_antes_del_listener() {
    let mut en_marcha = arrancar(&socket_muerto(), &[AVISO_DEL_MOTOR_MUDO, ARRANCADO]);

    // (3) El aviso: el arranque deja constancia de que el socket no responde.
    assert!(
        en_marcha.contiene(AVISO_DEL_MOTOR_MUDO),
        "el arranque no dejó constancia de que el socket del motor de contenedores no responde \
         dentro de {ESPERA:?}: se esperaba el aviso del socket inalcanzable en el registro. Salida:\n{}",
        en_marcha.registro()
    );

    assert!(
        en_marcha.contiene(ARRANCADO),
        "el arranque no llegó a escuchar dentro de {ESPERA:?}: se esperaba que un socket de Docker \
         inalcanzable no impidiera arrancar. Salida:\n{}",
        en_marcha.registro()
    );

    // (1) El orden: el aviso del motor va antes de la línea de escucha, que es lo que dice que el
    // motor se montó **antes** de escuchar y no después.
    let (del_motor, de_escuchar) = (
        en_marcha.posicion(AVISO_DEL_MOTOR_MUDO),
        en_marcha.posicion(ARRANCADO),
    );
    assert!(
        matches!((del_motor, de_escuchar), (Some(del_motor), Some(de_escuchar)) if del_motor < de_escuchar),
        "el motor no estaba montado antes de escuchar: se esperaba el aviso del socket inalcanzable \
         antes de la línea de escucha y salió al revés ({del_motor:?} y {de_escuchar:?}). Salida:\n{}",
        en_marcha.registro()
    );

    // Y el proceso sigue en pie: no arrancó para morir.
    assert!(
        !en_marcha.ha_terminado(),
        "el proceso terminó en vez de seguir arrancado pese al socket muerto. Salida:\n{}",
        en_marcha.registro()
    );

    // (3) Lo que de verdad se le pregunta: la salud, por HTTP y contra el puerto de verdad.
    let salud = match esperar_la_salud(en_marcha.puerto, ESPERA) {
        Ok(salud) => salud,
        Err(motivo) => panic!(
            "la salud no contestó dentro de {ESPERA:?} ({motivo}): se esperaba `GET /api/v1/health` \
             con un `200`, porque un arranque con el motor montado atiende aunque su socket no \
             responda. Salida:\n{}",
            en_marcha.registro()
        ),
    };

    assert_eq!(
        salud.codigo, 200,
        "la salud contestó {} en vez de `200` con el socket del motor inalcanzable: se esperaba que \
         el socket muerto no afectara a la salud. Cuerpo:\n{}",
        salud.codigo, salud.cuerpo
    );

    let cuerpo: serde_json::Value = serde_json::from_str(&salud.cuerpo).unwrap_or_else(|error| {
        panic!(
            "la salud contestó `200` pero su cuerpo no es JSON ({error}): se esperaba la envoltura \
             del contrato. Cuerpo:\n{}",
            salud.cuerpo
        )
    });

    assert_eq!(
        cuerpo["data"]["auth_mode"], "disabled",
        "la salud contestó `200` pero no declaró el modo de autenticación de esta prueba: se esperaba \
         el modo `Disabled`, que es el de un arranque sin las tres `OIDC_*`. Cuerpo:\n{}",
        salud.cuerpo
    );

    // Y el arranque pasó por la base: el fichero está donde se le pidió. Es la mitad de arriba del
    // orden del requisito, y sin ella un «arranca igual» no diría que arranca **entero**.
    assert!(
        en_marcha.ruta_de_la_base().exists(),
        "el arranque no creó el fichero de la base en {}: se esperaba que abriera y migrara la base \
         antes de escuchar. Salida:\n{}",
        en_marcha.ruta_de_la_base().display(),
        en_marcha.registro()
    );
}

/// 1.34 (escenario 2) — **`SIGTERM` acaba con el proceso dentro de un plazo acotado.**
///
/// La mitad que se mide: que la señal **se atiende**, que el cierre ordenado **recorre** sus pasos
/// —el registro trae la señal y la parada del motor— y que el proceso **termina** sin quedarse
/// colgado. Lo último tiene dos aserciones distintas y cada una dice lo suyo: que terminó dentro
/// del plazo y que acabó con código `0` en vez de morir por la señal —un código de señal sería un
/// `SIGTERM` que atendió el sistema, no Kesto, y la prueba no habría medido nada—.
///
/// La mitad que **no** se puede medir aquí, y se declara en vez de fingirse: «con una ejecución en
/// curso». Reproducirla exigiría una copia de verdad dentro del motor, y el binario **no** tiene el
/// abridor local de las pruebas unitarias —en el proceso el abridor es el de S3—, así que haría
/// falta un destino S3 y, para esa copia, un contenedor de base de datos: ni red ni contenedores en
/// el CI. Lo que cubre esa mitad: `parar_deja_de_aceptar_trabajo_y_espera_lo_acotado` en
/// `src/engine/mod.rs`, que para el motor **con una ejecución en vuelo**, comprueba que la parada
/// vuelve dentro de su plazo sin esperarla, deja constancia de lo que no terminó y rechaza el
/// trabajo nuevo; `una_cola_se_para_una_sola_vez` en `src/engine/cola.rs`, que fija que la parada es
/// de una sola vez; y la tarea 4.7 de la verificación del operador, que mata el proceso con
/// `pkill -x kesto` y comprueba que no quedan contenedores huérfanos.
#[test]
fn un_sigterm_acaba_con_el_proceso_dentro_del_plazo() {
    let mut en_marcha = arrancar(&socket_muerto(), &[ARRANCADO]);

    // Antes de la señal, el proceso tiene que estar **atendiendo**, y no basta con la línea de
    // escucha: `main` la emite antes de atar el puerto y el controlador de `SIGTERM` se instala al
    // sondear el servidor, así que una señal mandada antes de esto la atendería el sistema por su
    // cuenta. Una respuesta HTTP prueba que el servidor está sondeando.
    let salud = match esperar_la_salud(en_marcha.puerto, ESPERA) {
        Ok(salud) => salud,
        Err(motivo) => panic!(
            "el proceso no llegó a atender `GET /api/v1/health` antes del `SIGTERM` ({motivo}): se \
             esperaba que ya estuviera sirviendo, porque una señal que llegue antes la atiende el \
             sistema y esta prueba dejaría de medir el cierre ordenado. Salida:\n{}",
            en_marcha.registro()
        ),
    };

    assert_eq!(
        salud.codigo, 200,
        "la salud no contestó `200` antes del `SIGTERM`, así que el proceso no estaba sirviendo: se \
         esperaba un proceso atendiendo para poder pararlo. Cuerpo:\n{}",
        salud.cuerpo
    );

    let empezado = Instant::now();

    if let Err(motivo) = en_marcha.sigterm() {
        panic!("no se pudo mandar `SIGTERM` al proceso: {motivo}");
    }

    // (2) Lo que mide esta prueba: el proceso acaba, y acaba dentro del plazo.
    let estado = match en_marcha.esperar_el_final(PLAZO_DE_LA_PARADA) {
        Some(estado) => estado,
        None => panic!(
            "el proceso no acabó dentro de {PLAZO_DE_LA_PARADA:?} tras el `SIGTERM`: se esperaba que \
             el cierre ordenado lo acabara —no hay ninguna ejecución en vuelo que esperar— y que el \
             proceso terminara; sigue vivo, y el `Drop` de la guarda lo matará. Salida:\n{}",
            en_marcha.registro()
        ),
    };
    let tardado = empezado.elapsed();

    assert_eq!(
        estado.code(),
        Some(0),
        "el proceso no acabó por su cierre ordenado: {estado} tras {tardado:?}. Se esperaba el código \
         `0` de un apagado que llega al final; un código de señal quiere decir que el `SIGTERM` lo \
         atendió el sistema y no Kesto. Salida:\n{}",
        en_marcha.registro()
    );

    // El registro, ya completo: el hilo lector puede no haber entregado las últimas líneas cuando
    // `try_wait` vio el proceso muerto.
    en_marcha.drenar();

    assert!(
        en_marcha.contiene(SENAL_DE_CIERRE),
        "el proceso acabó sin dejar constancia de la señal de cierre: se esperaba que el `SIGTERM` \
         lo hubiera atendido el apagado ordenado. Salida:\n{}",
        en_marcha.registro()
    );

    assert!(
        en_marcha.contiene(MOTOR_PARADO),
        "el apagado no paró el motor: se esperaba la constancia de la parada del motor detrás de la \
         señal, porque el cierre ordenado lo para **después** de que el servidor deje de aceptar \
         conexiones. Salida:\n{}",
        en_marcha.registro()
    );
}

/// Abre `GET /api/v1/events` en un socket crudo y **deja al cliente ahí**.
///
/// Lo que la prueba de abajo necesita de un cliente del stream no es leer eventos: es que haya una
/// conexión **viva** con un cuerpo abierto que no termina. Así que se pide el stream, se comprueba
/// que se abrió de verdad —`200` y `text/event-stream`, que es lo que lo distingue de una respuesta
/// normal, que ya habría terminado— y no se vuelve a leer: el cuerpo de un `text/event-stream` no
/// tiene final, así que leerlo entero sería colgarse. El socket se queda abierto hasta el final de
/// la prueba, y su `Drop` —el del llamante, al salir de ámbito— es lo último que lo cierra.
///
/// Se lee **solo la cabecera**, hasta la primera línea vacía: sin leer nada no se sabría si el
/// stream llegó a abrirse —una petición rechazada también dejaría el socket abierto, y entonces la
/// prueba no mediría lo que dice medir— y leer más sería meterse en el cuerpo, que es justo lo que
/// aquí se quiere dejar a medias. El `Connection: close` no se manda: un cliente del stream lo
/// mantiene abierto, y esa es la conexión que tiene que retener el apagado.
fn abrir_el_stream(puerto: u16) -> TcpStream {
    let direccion = SocketAddr::from(([127, 0, 0, 1], puerto));

    let mut conexion = TcpStream::connect_timeout(&direccion, PLAZO_DE_LA_PETICION)
        .unwrap_or_else(|error| panic!("no se pudo abrir la conexión con {direccion}: {error}"));

    conexion
        .set_read_timeout(Some(PLAZO_DE_LA_PETICION))
        .unwrap_or_else(|error| panic!("no se pudo acotar la lectura: {error}"));

    conexion
        .write_all(
            b"GET /api/v1/events HTTP/1.1\r\n\
              Host: 127.0.0.1\r\n\
              Accept: text/event-stream\r\n\r\n",
        )
        .unwrap_or_else(|error| panic!("no se pudo escribir la petición del stream: {error}"));

    let mut recibido = Vec::new();
    let mut tramo = [0_u8; 1024];

    while !String::from_utf8_lossy(&recibido).contains("\r\n\r\n") {
        match conexion.read(&mut tramo) {
            Ok(0) => break,
            Ok(leidos) => recibido.extend_from_slice(&tramo[..leidos]),
            Err(error) => panic!("la cabecera del stream no llegó entera: {error}"),
        }
    }

    let cabecera = String::from_utf8_lossy(&recibido).into_owned();

    assert!(
        cabecera.starts_with("HTTP/1.1 200"),
        "el stream no se abrió: se esperaba un `200` en la línea de estado para poder decir que hay \
         un cliente conectado, y llegó:\n{cabecera}"
    );

    assert!(
        cabecera.contains("text/event-stream"),
        "la respuesta del stream no es un `text/event-stream`, así que no es el cliente que esta \
         prueba necesita: el cuerpo de esa respuesta habría terminado. Cabecera:\n{cabecera}"
    );

    conexion
}

/// 1.34 (escenario 2, con un cliente conectado) — **Un `SIGTERM` acaba con el proceso aunque haya
/// un cliente en el stream.**
///
/// Es la mitad del requisito que faltaba, y faltaba de verdad: el cierre ordenado de `axum` **no
/// resuelve hasta que todas las conexiones terminan**, y un `text/event-stream` no termina nunca
/// —el cuerpo se acaba cuando el emisor del hub se suelta, y durante el apagado nadie lo soltaba—,
/// así que el proceso se quedaba vivo y lo mataba el `SIGKILL` del período de gracia **con el hilo
/// de trabajo a mitad de archivar**. Es exactamente lo que la parada acotada existía para evitar, y
/// el caso que lo rompía no era una esquina: es la única configuración en la que el stream sirve
/// para algo.
///
/// Las cuatro aserciones, y por qué ninguna sobra:
///
/// 1. el stream **se abrió** —lo comprueba [`abrir_el_stream`]—: sin un cliente conectado esto
///    sería la prueba de arriba, que ya pasaba;
/// 2. el proceso **acaba dentro del plazo**, que es la mitad que estaba roja;
/// 3. acaba con código `0`, y no por la señal: un `SIGKILL` del período de gracia da un código de
///    señal, que es justo el desenlace que no se quiere;
/// 4. y el cierre **recorrió sus pasos**, con la señal atendida y el motor parado en el registro.
///    Sin las dos marcas, un proceso que muriera de cualquier otra forma colaría.
///
/// **Lo que no se mide aquí, y se declara**: un cliente que además **deje de leer** —un socket
/// lleno, un portátil cerrado— puede seguir retrasando el cierre, porque el cuerpo no se puede
/// escribir. Eso no está en manos del motor: lo que el arreglo cierra es el canal, que es lo que sí
/// lo está. El límite está escrito en el rustdoc de [`Hub::cerrar`](crate::engine::Hub::cerrar).
#[test]
fn un_sigterm_acaba_con_el_proceso_aunque_haya_un_cliente_en_el_stream() {
    let mut en_marcha = arrancar(&socket_muerto(), &[ARRANCADO]);

    // (1) El cliente, conectado **antes** de la señal. Que la respuesta sea un `200` con
    // `text/event-stream` prueba de paso que el servidor está atendiendo, así que no hace falta
    // preguntar por la salud como en la prueba de arriba.
    let cliente = abrir_el_stream(en_marcha.puerto);

    if let Err(motivo) = en_marcha.sigterm() {
        panic!("no se pudo mandar `SIGTERM` al proceso: {motivo}");
    }

    // (2) La mitad que estaba roja: con un cliente en el stream, el proceso tiene que acabar
    // igualmente, y dentro del plazo.
    let estado = match en_marcha.esperar_el_final(PLAZO_DE_LA_PARADA) {
        Some(estado) => estado,
        None => panic!(
            "el proceso no acabó dentro de {PLAZO_DE_LA_PARADA:?} tras el `SIGTERM` con un cliente \
             conectado al stream: el cierre ordenado espera a que las conexiones terminen, y el \
             cuerpo de un `text/event-stream` no termina mientras el hub siga teniendo su emisor. \
             El proceso sigue vivo y el `Drop` de la guarda lo matará. Salida:\n{}",
            en_marcha.registro()
        ),
    };

    // (3) Y acabó por su propio cierre, no por la señal.
    assert_eq!(
        estado.code(),
        Some(0),
        "el proceso acabó de otra forma que por su cierre ordenado: {estado}. Se esperaba el código \
         `0`; un código de señal quiere decir que lo mató el período de gracia, que es el desenlace \
         que la parada acotada existe para evitar. Salida:\n{}",
        en_marcha.registro()
    );

    en_marcha.drenar();

    // (4) El cierre recorrió sus pasos: la señal atendida y el motor parado.
    assert!(
        en_marcha.contiene(SENAL_DE_CIERRE),
        "el proceso acabó sin dejar constancia de la señal de cierre: se esperaba que el `SIGTERM` \
         lo hubiera atendido el apagado ordenado. Salida:\n{}",
        en_marcha.registro()
    );

    assert!(
        en_marcha.contiene(MOTOR_PARADO),
        "el apagado no paró el motor con un cliente en el stream: se esperaba la constancia de la \
         parada del motor detrás de la señal, porque el cierre ordenado lo para **después** de que \
         el servidor deje de aceptar conexiones. Salida:\n{}",
        en_marcha.registro()
    );

    // El cliente se suelta aquí, con el proceso ya muerto y sus aserciones ya hechas: el socket
    // tiene que haber estado abierto todo el tiempo para que la prueba midiera lo que mide.
    drop(cliente);
}
