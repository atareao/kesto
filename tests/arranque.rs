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

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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
