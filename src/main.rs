//! Kesto: orquestador ligero de copias de seguridad dentro de un contenedor.

// Contrato público del crate: el módulo raíz lo declara como su superficie.
//
// Son módulos públicos —y no privados— porque `AppError`, `ApiResponse`,
// `Config` y `SecretString` fijan el contrato de configuración y la forma del
// JSON, que es la parte cara de cambiar después. Un binario no tiene consumidor
// externo, así que lo que no sea alcanzable desde esta superficie se considera
// código muerto; declararla es lo que hace que el contrato no se lea como
// código muerto y siga delatando, en cambio, los ayudantes privados sin uso.
pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod docker;
pub mod engine;
pub mod error;
pub mod rustic;
pub mod secret;
pub mod state;

#[cfg(test)]
mod test_support;

use std::future::Future;
use std::sync::Arc;

use crate::db::crypto::Clave;
use crate::engine::{Ajustes, Dependencias, Motor};
use crate::state::{AppState, EstadoDeBase};

/// Arranca el servidor sobre `state.config.bind_address` y devuelve cuando
/// `shutdown` se resuelve, dando salida antes a las peticiones en curso.
pub async fn serve(
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(state.config.bind_address).await?;

    axum::serve(listener, api::api_router(state))
        .with_graceful_shutdown(shutdown)
        .await
}

/// Señal de cierre del proceso: `SIGTERM` o `SIGINT`.
///
/// Si una de las dos no se puede registrar, se deja de esperar por ella en vez
/// de apagar el servidor: un fallo al escuchar una señal no es una señal.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(%error, "no se pudo atender SIGINT");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "no se pudo atender SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }

    tracing::info!("señal de cierre recibida: se deja de aceptar conexiones");
}

/// El objetivo de registro del motor de contenedores.
///
/// `bollard` registra con el `log` de siempre, así que el destino de sus llamadas es su
/// `module_path!()` —`bollard`, `bollard::docker`, …—. Lo que hay que silenciar, por tanto,
/// es el prefijo `bollard` entero, no un módulo suyo.
const OBJETIVO_DEL_MOTOR: &str = "bollard";

/// La directiva de registro que silencia al motor de contenedores, y **por qué** la pone el
/// proceso.
///
/// `bollard` registra a `DEBUG` el cuerpo serializado de cada petición —medido:
/// `serialize_payload` en `bollard-0.18.1/src/docker.rs:1227`—, y el cuerpo de un
/// `create_exec` lleva el `env` **entero**, con `MYSQL_PWD` o `PGPASSWORD` dentro. El valor
/// no pasa por ningún tipo de este crate, así que no hay `Debug` propio que lo proteja: lo
/// único que corta esa exposición es el filtro de registro. Y por eso la directiva la pone
/// el **proceso** y no la receta con la que se le lance: con `RUST_LOG=debug` a secas —una
/// forma perfectamente normal de arrancar— el registro imprime la contraseña de la base.
///
/// Es el requisito «la credencial no aparece en el registro del proceso» de
/// `openspec/changes/backup-engine/specs/extraction/`, que exige que la directiva esté
/// declarada donde arranca el proceso y no dejada a la memoria de quien lo ejecute.
pub const DIRECTIVA_DEL_MOTOR: &str = "bollard=warn";

/// Los niveles que ya dejan el motor de contenedores en silencio.
///
/// Es lo que distingue «el operador pidió el objetivo del motor y el proyecto lo tapa» —que
/// se avisa— de «el operador pidió lo mismo que el proyecto» —que no se avisa, porque no hay
/// nada que tapar—. Sin esta distinción, `RUST_LOG=debug,bollard=warn`, que es lo que
/// recetan `just dev` y `just run`, daría un aviso en cada arranque y convertiría un aviso
/// útil en ruido que se aprende a ignorar.
const NIVELES_QUE_YA_LO_SILENCIAN: [&str; 3] = ["warn", "error", "off"];

/// El filtro que se aplica de verdad, con la directiva del proyecto **detrás**, y el aviso
/// que merezca el filtro que llegó.
///
/// El orden no es estilo: medido en `tracing-subscriber-0.3.23`, su `DirectiveSet::add`
/// sustituye la directiva anterior cuando la nueva es igual de específica —mismo destino y
/// sin campos— (`src/filter/directive.rs:88`), y para un mismo evento gana la más específica
/// primero (`enabled` toma la primera que casa). La del proyecto va la última, así que es la
/// que manda sobre cualquier otra que nombre el mismo objetivo. La contrapartida, que el
/// aviso de [`aviso_del_filtro`] deja por escrito: un sub-objetivo más largo
/// (`bollard::docker=debug`) es más específico y sigue por delante.
fn componer_el_filtro(lo_que_puso: &str) -> (String, Option<String>) {
    (
        format!("{lo_que_puso},{DIRECTIVA_DEL_MOTOR}"),
        aviso_del_filtro(lo_que_puso),
    )
}

/// El aviso que merece el filtro que llegó, si merece alguno.
///
/// Se avisa cuando el filtro pide el motor —él o un sub-objetivo suyo— **más hablador** que
/// lo que el proyecto deja: eso no es una preferencia de estilo que se pueda pisar en
/// silencio, porque lo que ese objetivo registra es el `env` del `exec`, que lleva la
/// contraseña de la base. Y hay dos desenlaces distintos, con avisos distintos: el objetivo
/// exacto lo pisa la directiva del proyecto —que va detrás y gana—, mientras que un
/// sub-objetivo más específico **no** lo tapa nadie y es el caso que de verdad hay que mirar.
///
/// El `[span{campo}]=nivel` se analiza hasta los corchetes: lo que importa es el destino. Lo
/// que no se interpreta aquí —y da igual, porque el aviso es una constancia y no la
/// protección— es el `span` y el campo de dentro.
fn aviso_del_filtro(lo_que_puso: &str) -> Option<String> {
    let mut pidio_el_objetivo = false;
    let mut pidio_un_sub_objetivo = false;

    for directiva in lo_que_puso.split(',') {
        let (destino, nivel) = match directiva.split_once('=') {
            Some((destino, nivel)) => (destino, Some(nivel.trim())),
            // Un destino sin `=` es el nivel más hablador que hay, así que pasa el filtro de
            // abajo como si hubiera pedido `trace`.
            None => (directiva, None),
        };
        let destino = destino.split('[').next().unwrap_or(destino).trim();

        let hablador = nivel.is_none_or(|nivel| !NIVELES_QUE_YA_LO_SILENCIAN.contains(&nivel));
        if !hablador {
            continue;
        }

        if destino == OBJETIVO_DEL_MOTOR {
            pidio_el_objetivo = true;
        } else if destino
            .strip_prefix(OBJETIVO_DEL_MOTOR)
            .is_some_and(|resto| resto.starts_with("::"))
        {
            pidio_un_sub_objetivo = true;
        }
    }

    if pidio_un_sub_objetivo {
        return Some(format!(
            "el filtro de registro pide un sub-objetivo de `{OBJETIVO_DEL_MOTOR}` más hablador \
             que `warn`: esa directiva es más específica que `{DIRECTIVA_DEL_MOTOR}` y sigue por \
             delante, así que ese objetivo puede escribir en el registro el `env` del `exec` de \
             la extracción, donde viaja la contraseña de la base"
        ));
    }

    if pidio_el_objetivo {
        return Some(format!(
            "el filtro de registro pide `{OBJETIVO_DEL_MOTOR}` más hablador que `warn`, y el \
             proceso lo pisa con `{DIRECTIVA_DEL_MOTOR}`: ese objetivo registra el `env` del \
             `exec` de la extracción, donde viaja la contraseña de la base, así que la directiva \
             del proyecto no se toca"
        ));
    }

    None
}

/// Inicializa el registro con el filtro ya resuelto por la configuración.
///
/// El filtro entra por parámetro en vez de volver a leer el entorno: el `EnvFilter` que se
/// aplica y el que se vuelca en el arranque tienen que ser el mismo, o el volcado miente.
///
/// Lo que se aplica **no** es el filtro tal cual: es el de quien arranca con la directiva del
/// proyecto detrás —ver [`componer_el_filtro`]—, y el que se publica en el registro es el
/// compuesto, que es el que de verdad manda. Así el volcado de arranque, que lleva el
/// `log_filter` de la configuración —lo que puso el operador— se puede comparar con el
/// efectivo sin adivinar nada.
///
/// Si la cadena no es un filtro válido, se cae a
/// [`crate::config::DEFAULT_LOG_FILTER`] avisando por `stderr`: un `RUST_LOG` mal escrito no
/// puede dejar el proceso sin registro —ni reabrir la puerta que el proyecto cierra, así que
/// el filtro de reserva también lleva la directiva—.
fn init_tracing(filtro: &str) {
    let (aplicado, aviso) = componer_el_filtro(filtro);

    let filter = match tracing_subscriber::EnvFilter::try_new(&aplicado) {
        Ok(filter) => filter,
        Err(error) => {
            eprintln!(
                "el filtro de registro {aplicado:?} no es válido: {error}; se usa {}",
                crate::config::DEFAULT_LOG_FILTER
            );

            tracing_subscriber::EnvFilter::new(
                componer_el_filtro(crate::config::DEFAULT_LOG_FILTER).0,
            )
        }
    };

    // Un fallo de instalación no impide dejar constancia por el registro —que en ese caso no
    // recoge nada— ni por `stderr`, que es donde va el propio mensaje del fallo.
    if let Err(error) = tracing_subscriber::fmt().with_env_filter(filter).try_init() {
        eprintln!("no se pudo inicializar el registro: {error}");
    }

    // El filtro **efectivo**, que no es el que puso quien arranca: es el de arriba. Se publica
    // aquí porque el volcado de arranque publica el `log_filter` de la configuración —lo que
    // pidió el operador— y con las dos líneas a la vista se ve qué se está aplicando de
    // verdad; con una sola, el volcado mentiría.
    tracing::info!(filtro = %aplicado, "filtro de registro efectivo");

    if let Some(aviso) = aviso {
        tracing::warn!("{aviso}");
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // 1. Configuración. Un fallo no arranca el proceso, y el mensaje nombra la
    //    variable ausente y nunca su valor. Sale por `stderr` porque el registro
    //    todavía no está en pie.
    let config = match config::Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("kesto no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 2. Registro, con el filtro de la configuración efectiva.
    init_tracing(&config.log_filter);

    // 3. Volcado de arranque y avisos.
    tracing::info!(config = %config.describe(), "configuración efectiva al arrancar");

    for aviso in config.warnings() {
        tracing::warn!("{aviso}");
    }

    // 4. Base de datos: se abre y se deja el esquema aplicado **antes** de
    //    escuchar. Un fallo aquí no arranca el proceso, y no es un modo
    //    degradado: un esquema a medias es peor que un arranque fallido, porque
    //    el motor escribiría contra columnas que no existen y el síntoma sería
    //    una copia perdida en vez de un arranque roto.
    //
    //    El pool **no** se queda suelto como variable local: con él se construye el
    //    estado de base —la clave de cifrado en reposo y las cuatro capas de acceso—,
    //    que viaja dentro del `AppState` y llega así a los manejadores. Sigue habiendo
    //    **una sola** base por proceso: la abre esta línea y ninguna otra.
    let pool = match db::preparar(&config).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("kesto no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // La clave del cifrado en reposo se deriva de `KESTO_MASTER_KEY`, que la
    // configuración exige siempre. Se construye **una vez** y se comparte: las capas de
    // `repositories` y `backup_targets` la reciben al construirse.
    let clave = Arc::new(Clave::desde_maestra(&config.master_key));

    // El estado de base: el pool **ya abierto y migrado**, la clave y las cuatro capas de
    // acceso. El pool no se queda suelto como variable local en ningún momento.
    let db = EstadoDeBase::nuevo(pool, clave);

    // 5. El motor de copias: **detrás** de abrir y migrar la base y **delante** de empezar
    //    a escuchar. El orden tiene dos razones y ninguna es decorativa: los trabajadores
    //    del motor escriben en `job_executions` desde sus propios hilos, así que necesitan
    //    las cuatro capas construidas sobre la base con el esquema ya aplicado —es el
    //    mismo «abrir, migrar y solo entonces» que rige la base—; y nada de lo que sirve el
    //    §5 —ni una copia, ni el explorador de instantáneas— existe antes del motor.
    //
    //    El socket sale de la configuración —`DOCKER_HOST`, con la ruta del §8 de defecto—,
    //    y no del entorno del proceso: es la misma lectura inyectada que el resto, y es lo
    //    que deja que una prueba quiera otro socket sin tocar el entorno.
    let ajustes = Ajustes::nuevos(config.docker_host.clone());
    // La espera de la parada se lee **antes** de mover los ajustes al motor, porque es el
    // número con el que el apagado de más abajo acota su espera, y de ahí no se puede leer.
    let espera_de_parada = ajustes.espera_de_parada;

    let motor = match Motor::arrancar(ajustes, dependencias_del_motor(&db)) {
        Ok(motor) => motor,
        Err(error) => {
            eprintln!("kesto no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 6. Router, escucha y apagado. El estado se construye desde la configuración ya
    //    resuelta, la base **ya abierta** y el motor **ya arrancado**: en modo `Oidc` lleva
    //    el estado de autenticación, y en desarrollo no lleva ninguno. El motor se clona
    //    hacia el estado —un clon es un `Arc` por dentro, así que no copia ni la cola ni un
    //    evento— porque el apagado de aquí abajo necesita el suyo, y el estado se lo lleva
    //    el servidor.
    let address = config.bind_address;

    let state = match AppState::new(config, db, motor.clone()) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("kesto no puede arrancar: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // 7. El socket del motor de contenedores, comprobado **para avisar** y nunca para
    //    decidir si el proceso arranca. Un socket que no responde no puede dejar al
    //    operador sin la herramienta con la que diagnosticarlo: el panel tiene que poder
    //    levantarse precisamente para enseñar que no responde, y el fallo reaparece donde
    //    importa —al ejecutar una copia—. Aquí queda dicho antes, que es todo lo que se
    //    puede hacer sin inventarse un modo degradado.
    if let Err(error) = motor.comprobar().await {
        tracing::warn!(
            %error,
            "el motor de copias no puede hablar con el motor de contenedores: las tareas que \
             volquen un contenedor fallarán hasta que su socket responda; el resto de Kesto \
             funciona"
        );
    }

    // 8. Descubrimiento del proveedor de identidad: se intenta al arrancar y,
    //    si no contesta, se avisa y se sigue. Kesto es el orquestador de las
    //    **copias**, y negarse a arrancar porque el proveedor está caído
    //    convertiría un problema de autenticación en ausencia de copias. El
    //    reintento llega con el primer login, que además cachea el resultado.
    if let Some(auth) = state.auth.as_ref()
        && let Some(aviso) = auth::descubrimiento_al_arrancar(auth).await
    {
        tracing::warn!("{aviso}");
    }

    tracing::info!(%address, "escuchando");

    let resultado = serve(state, shutdown_signal()).await;

    // 9. La parada del motor va **después** del servidor, y el orden es la mitad del
    //    contrato: `serve` resuelve cuando la señal llega y las peticiones en curso han
    //    terminado de salir —deja de aceptar, y solo entonces vuelve—, así que para cuando
    //    se llega aquí nadie puede pedir una copia. Al revés, una petición que ya estaba
    //    dentro habría encontrado el motor parado y habría contestado `503` en vez de
    //    servirse: el apagado convertiría en error una petición anterior a él.
    //
    //    Y va en `spawn_blocking` porque `Motor::parar` es **bloqueante**: espera, hasta
    //    `espera_de_parada`, a que los trabajadores suelten. Llamarlo aquí mismo dejaría
    //    parado el hilo del runtime que está corriendo este `main` —y con él las tareas que
    //    ese hilo tuviera por delante, entre ellas los futuros con los que los hilos del
    //    motor entran en la base—. El conjunto de bloqueo existe exactamente para esto. Y
    //    no vale `block_in_place`, que es la otra salida: sobre un runtime `current_thread`
    //    —el que monta `#[tokio::test]` por defecto, y el que puede pedir quien despliegue—
    //    entra en **pánico**, así que la elección no es de estilo.
    let parada = tokio::task::spawn_blocking(move || motor.parar(espera_de_parada));

    if let Err(error) = parada.await {
        // Un fallo aquí es la espera que no llegó a término —un pánico dentro de ella—. Su
        // detalle es arbitrario y se queda en el registro: lo que importa es que no impide
        // acabar, porque el proceso tiene que terminar. Lo que no acabe queda registrado
        // como tal en `job_executions`, que es para lo que existe la espera.
        tracing::error!(%error, "la parada del motor no ha llegado a término");
    }

    if let Err(error) = resultado {
        // El registro puede estar apagado (`RUST_LOG=off`), así que el fallo
        // fatal también sale por `stderr`. Devolver el código —en vez de matar
        // el proceso— deja caer `state`, y con él los `SecretString`, que
        // borran su contenido al soltarse.
        eprintln!("el servidor se detuvo con error: {error}");
        return std::process::ExitCode::FAILURE;
    }

    std::process::ExitCode::SUCCESS
}

/// Las cuatro capas de acceso que el motor necesita, tomadas del estado de base.
///
/// Existe porque el motor **no** nombra [`EstadoDeBase`] a propósito: `src/engine` no importa
/// `src/state`, y ese es el motivo de que el día que el `AppState` lleva el motor no haya un
/// ciclo de módulos. Las cuatro líneas que unen las dos piezas viven aquí, donde las dos se
/// tienen a la vista.
///
/// Clonar las capas no vuelve a construirlas: van tras un `Arc`, y el motor se queda con
/// **las mismas** que el estado —que es lo que hace que las filas que escribe un trabajador
/// estén en el pool que sirve el panel—.
fn dependencias_del_motor(db: &EstadoDeBase) -> Dependencias {
    Dependencias {
        repositorios: db.repositorios.clone(),
        targets: db.targets.clone(),
        tareas: db.tareas.clone(),
        ejecuciones: db.ejecuciones.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{DIRECTIVA_DEL_MOTOR, aviso_del_filtro, componer_el_filtro};
    use crate::db;
    use crate::test_support::{config_con_dsn, directorio_de_prueba, migrador_roto};

    /// El filtro que se aplica lleva la directiva del motor, **detrás** de lo que se pidió.
    ///
    /// Es la prueba que fija la decisión, y no una de integración: comprobar que el valor no
    /// aparece en el registro de un volcado de verdad costaría un contenedor, una base y un
    /// socket, y lo que hay que sujetar aquí es que el **proceso** componga así el filtro
    /// —el requisito exige que la directiva esté declarada donde arranca el proceso y no
    /// dejada a la receta con la que se le lance—. Que el valor no salga de verdad se mide
    /// con `just dev`, que es donde se midió que salía.
    #[test]
    fn el_filtro_que_se_aplica_lleva_la_directiva_del_motor_detras() {
        for (puesto, esperado) in [
            // El defecto del proceso, y el caso que motivó la corrección: `debug` a secas
            // —una forma normal de arrancar— dejaba el `env` del `exec` en el registro.
            ("info", "info,bollard=warn"),
            ("debug", "debug,bollard=warn"),
            // Y si el operador ya pedía el motor, la del proyecto va detrás: es la que gana.
            ("debug,bollard=debug", "debug,bollard=debug,bollard=warn"),
        ] {
            let (aplicado, _) = componer_el_filtro(puesto);

            assert_eq!(aplicado, esperado, "el filtro compuesto no es el esperado");
            assert!(
                aplicado.contains(DIRECTIVA_DEL_MOTOR),
                "el filtro que se aplica no lleva la directiva del motor: {aplicado}"
            );
            assert!(
                aplicado.ends_with(DIRECTIVA_DEL_MOTOR),
                "la directiva del proyecto tiene que ir la última para ganar: {aplicado}"
            );
            // Y el compuesto tiene que ser un filtro de verdad: uno que `EnvFilter` no sepa
            // leer caería al de reserva, y el de reserva es otro camino.
            assert!(
                tracing_subscriber::EnvFilter::try_new(&aplicado).is_ok(),
                "el filtro compuesto no lo acepta EnvFilter: {aplicado}"
            );
        }

        // Y el orden hace lo que dice: la biblioteca sustituye la directiva anterior cuando
        // la nueva es igual de específica —medido en `tracing-subscriber-0.3.23`—, así que lo
        // que queda para `bollard` es la del proyecto y no la del operador. Se comprueba por
        // el camino de la biblioteca, imprimiendo el filtro que ella misma construye.
        let (aplicado, _) = componer_el_filtro("debug,bollard=debug");
        let interpretado = tracing_subscriber::EnvFilter::try_new(&aplicado)
            .expect("el filtro compuesto es válido")
            .to_string();

        assert!(
            interpretado.contains(DIRECTIVA_DEL_MOTOR),
            "el filtro interpretado no lleva la directiva del motor: {interpretado}"
        );
        assert!(
            !interpretado.contains("bollard=debug"),
            "la directiva del operador sobrevivió a la del proyecto, que va detrás y tiene que \
             sustituirla: {interpretado}"
        );
    }

    /// El filtro de reserva —el que se aplica cuando `RUST_LOG` no se puede leer— también
    /// lleva la directiva.
    ///
    /// Un `RUST_LOG` mal escrito no puede reabrir la puerta que el proyecto cierra: si el
    /// filtro de reserva no la llevara, un error de configuración dejaría el registro del
    /// motor a merced de lo que pida cualquier otra variable.
    #[test]
    fn el_filtro_de_reserva_lleva_la_directiva() {
        let (reserva, _) = componer_el_filtro(crate::config::DEFAULT_LOG_FILTER);

        assert!(
            reserva.contains(DIRECTIVA_DEL_MOTOR),
            "el filtro de reserva no lleva la directiva del motor: {reserva}"
        );
    }

    /// Un filtro que pide el motor **más hablador** que el proyecto deja constancia, y uno
    /// que pide lo mismo no dice nada.
    ///
    /// El aviso es lo único que separa «el proyecto lo tapa» de «el proyecto lo tapa y no lo
    /// cuenta»: como la exposición se corta por filtro y no por tipos, pisar en silencio la
    /// directiva de quien arranca sería esconderle que su registro iba a llevar la
    /// credencial. Y el caso del sub-objetivo es el que de verdad hay que mirar, porque ahí
    /// la directiva del proyecto **no** lo tapa.
    #[test]
    fn un_filtro_que_pide_el_motor_mas_hablador_deja_constancia() {
        for ruidoso in ["debug,bollard=debug", "trace,bollard=trace", "bollard"] {
            let aviso = aviso_del_filtro(ruidoso).unwrap_or_else(|| {
                panic!("un filtro que pide el motor tiene que avisar: {ruidoso}")
            });

            assert!(
                aviso.contains("bollard") && aviso.contains("contraseña"),
                "el aviso no dice de qué va: {aviso}"
            );
            assert!(
                aviso.contains(DIRECTIVA_DEL_MOTOR),
                "el aviso no nombra la directiva del proyecto: {aviso}"
            );
        }

        // El sub-objetivo es un desenlace distinto: la directiva del proyecto no lo alcanza.
        let del_sub_objetivo = aviso_del_filtro("debug,bollard::docker=debug")
            .expect("un sub-objetivo del motor más hablador tiene que avisar");
        assert!(
            del_sub_objetivo.contains("sub-objetivo"),
            "el aviso no distingue el sub-objetivo, que es el que queda abierto: {del_sub_objetivo}"
        );

        // Y lo que ya está en silencio no avisa: si no, `just dev` —que receta
        // `debug,bollard=warn`— daría un aviso en cada arranque.
        for silencioso in [
            "debug",
            "info",
            "debug,bollard=warn",
            "info,bollard=error",
            "info,bollard=off",
        ] {
            assert!(
                aviso_del_filtro(silencioso).is_none(),
                "un filtro que no pide nada hablador del motor no puede avisar: {silencioso}"
            );
        }
    }

    /// 1.52 — El arranque deja la base preparada antes de aceptar peticiones.
    ///
    /// `db::preparar` es la **única** llamada que hace el arranque para esto:
    /// abre el pool y aplica las migraciones de una vez, y el orden respecto a
    /// `serve` vive en la secuencia de `main`, que ningún test unitario puede
    /// observar sin deformar `main`. Lo que se fija aquí es que esa llamada hace
    /// las dos cosas: si alguien la cambiara por una que solo abriera, el esquema
    /// no estaría y este test caería.
    #[tokio::test]
    async fn el_arranque_prepara_la_base_antes_de_escuchar() {
        let directorio = directorio_de_prueba();

        let pool = db::preparar(&config_con_dsn(&directorio.dsn()))
            .await
            .expect("el arranque debe poder preparar la base");

        let tablas = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("la consulta de tablas no puede fallar");

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

        pool.close().await;
    }

    /// 1.52 — Un fallo de migración impide arrancar.
    ///
    /// El arranque no sigue: `main` devuelve el código de fallo sin escuchar. Y no
    /// deja la base a medias, que es lo que hace que eso sea la decisión correcta.
    #[tokio::test]
    async fn un_fallo_de_migracion_impide_arrancar() {
        let directorio = directorio_de_prueba();

        let error = db::preparar_con(&config_con_dsn(&directorio.dsn()), &migrador_roto())
            .await
            .expect_err("una migración que falla no puede dar un arranque en pie");

        assert!(
            matches!(error, db::DbError::Migracion { .. }),
            "el fallo debe llegar como error de migración: {error}"
        );

        // Y el esquema no quedó a medias: la base se abre, y las cuatro tablas
        // del §3 no están, porque la migración que las crea nunca llegó a
        // aplicarse entera.
        let pool = db::abrir(&directorio.dsn())
            .await
            .expect("la base debe poder abrirse para comprobar que no quedó a medias");

        let tablas = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .expect("la consulta de tablas no puede fallar");

        pool.close().await;

        assert!(
            !tablas.iter().any(|tabla| tabla == "repositories"),
            "el arranque falló pero dejó el esquema a medias: {tablas:?}"
        );
    }
}
