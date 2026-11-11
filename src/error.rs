//! Modelo de error de la API y su traducción a respuesta HTTP.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

use crate::api::response::ErrorResponse;
use crate::config::ConfigError;
use crate::db::DbError;

/// Mensaje con el que el cliente recibe un fallo interno.
///
/// Es deliberadamente genérico: el texto de la causa se queda en el registro
/// del servidor, porque es donde acaban las credenciales que manejan las
/// bibliotecas.
const INTERNAL_MESSAGE: &str = "error interno del servidor";

/// Mensaje con el que el cliente recibe un `503`.
///
/// Es propio y no el genérico del `500` porque el desenlace no es el mismo: aquí la petición está
/// bien y el servicio no la puede atender **ahora** —el motor ha dejado de aceptar trabajo o su cola
/// está llena—, así que lo que el cliente puede hacer es reintentar más tarde. El motivo no viaja: es
/// un `5xx` y le rige la misma regla que al resto, se queda en el registro.
const UNAVAILABLE_MESSAGE: &str =
    "el servicio no está disponible para atender la petición ahora mismo";

/// Error de la aplicación, con el código HTTP que le corresponde.
#[derive(Debug, Error)]
pub enum AppError {
    /// La petición traía algo que no se puede atender: el motivo sí se explica.
    #[error("{0}")]
    InvalidInput(String),

    /// El recurso pedido no existe.
    #[error("{0}")]
    NotFound(String),

    /// El método HTTP no está declarado para esta ruta.
    #[error("{0}")]
    MethodNotAllowed(String),

    /// Sin sesión válida no se atiende: el motivo sí se explica.
    #[error("{0}")]
    Unauthorized(String),

    /// La petición contradice el estado actual del recurso: es un `409`.
    ///
    /// Existe desde que hay motor de copias, y nace de un caso que ningún recurso tenía: encolar
    /// dos veces la misma tarea no acelera nada —su repositorio las atiende de una en una—, la pone
    /// a esperar y deja al panel con dos barras para un solo trabajo. No es una petición mal
    /// formada —eso es el `400`— sino una que **está bien** y choca con lo que ya está pasando, que
    /// es exactamente lo que un `409` significa. Como el `400`, explica el motivo: no lleva ninguna
    /// credencial.
    #[error("{0}")]
    Conflict(String),

    /// El servicio no puede atender la petición ahora mismo: es un `503`.
    ///
    /// El motivo **no** se publica —es un `5xx`— y el mensaje es [`UNAVAILABLE_MESSAGE`]: el motor
    /// parado o su cola llena contesta con un texto que nombra el estado del servicio —el operador
    /// ya lo tiene en el registro— y no lo que el cliente mandó. Es lo que permite contestar `503`
    /// sin que el `Display` de un error de base, que nombra rutas del despliegue, asome al cuerpo.
    #[error("{0}")]
    ServiceUnavailable(String),

    /// Fallo interno: el detalle va al registro y al cliente le llega un mensaje genérico.
    #[error("{0}")]
    Other(String),

    /// Un servicio de terceros no contesta: la causa va al registro y al cliente
    /// le llega el mismo mensaje genérico que con un fallo interno.
    #[error("{0}")]
    BadGateway(String),

    /// La base de datos falló: es un fallo del servidor, así que al cliente le
    /// llega el mismo mensaje genérico que con [`AppError::Other`] y la causa se
    /// queda en el registro.
    ///
    /// El mensaje de [`crate::db::DbError`] **nombra rutas** —lo hace a propósito,
    /// para que el operador sepa qué directorio falta—, y por eso no puede salir
    /// al cuerpo: una ruta del despliegue no es asunto de quien pregunta por una
    /// API.
    #[error(transparent)]
    Database(#[from] DbError),

    /// La configuración no permite arrancar.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            // El cliente necesita saber qué traía mal la petición: el motivo se explica. Se
            // repite a `debug` porque el contrato del delta `secrets` pide que quede la traza
            // del campo que no cumplía, y el motivo lo nombra. Se registra **solo** el motivo y
            // **nunca** el cuerpo: el mensaje de esta variante se construye nombrando el campo y
            // jamás su valor —es una invariante de la casa, no una casualidad—, así que
            // registrarlo no puede publicar lo que el cliente mandó.
            Self::InvalidInput(motivo) => {
                tracing::debug!(motivo = %motivo, "petición rechazada");
                (StatusCode::BAD_REQUEST, motivo)
            }
            Self::NotFound(motivo) => (StatusCode::NOT_FOUND, motivo),
            // El cliente se equivocó de método, no de recurso: se le explica.
            Self::MethodNotAllowed(motivo) => (StatusCode::METHOD_NOT_ALLOWED, motivo),
            // Al cliente que no ha entrado se le explica el motivo, como en el 400.
            Self::Unauthorized(motivo) => (StatusCode::UNAUTHORIZED, motivo),
            // Y al que choca con el estado actual del recurso, también: el `409` es un rechazo de
            // la petición y su motivo —«ya hay una ejecución en curso»— es lo único que le permite
            // saber qué hacer. Se traza a `debug` igual que el `400`.
            Self::Conflict(motivo) => {
                tracing::debug!(motivo = %motivo, "petición rechazada");
                (StatusCode::CONFLICT, motivo)
            }
            // El `503` sí es del servidor: la causa va al registro y al cuerpo llega el mensaje
            // fijo de la indisponibilidad, que no repite ni el motivo ni nada de la petición.
            Self::ServiceUnavailable(causa) => {
                tracing::error!(error = %causa, "el servicio no puede atender la petición");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    UNAVAILABLE_MESSAGE.to_owned(),
                )
            }
            // Estos dos son fallos del servidor: la causa va al log, no al cuerpo.
            Self::Other(causa) => {
                tracing::error!(error = %causa, "fallo interno al atender la petición");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_MESSAGE.to_owned(),
                )
            }
            // También es un fallo del servidor: el 502 dice que el que falla está
            // fuera, pero el cuerpo no publica la causa —es un `5xx`—, así que
            // lleva el mismo mensaje genérico que el 500.
            Self::BadGateway(causa) => {
                tracing::error!(error = %causa, "un servicio de terceros no contestó");
                (StatusCode::BAD_GATEWAY, INTERNAL_MESSAGE.to_owned())
            }
            // Y el de la base de datos ya no es siempre un `5xx`: se distinguen las dos
            // únicas variantes que son culpa de quien pregunta y **todo lo demás** sigue
            // en `500` con el mensaje genérico. Ver [`traducir_base`].
            Self::Database(causa) => traducir_base(causa),
            Self::Config(error) => {
                // Por construcción, esta variante solo puede dispararse al
                // atender una petición: el arranque no pasa por aquí.
                tracing::error!(error = %error, "error de configuración al atender la petición");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_MESSAGE.to_owned(),
                )
            }
        };

        ErrorResponse::new(status, message).into_response()
    }
}

/// Traduce un fallo de la base al código y el mensaje que le corresponden.
///
/// Es la **frontera** del requisito «Un error de la base causado por la petición contesta
/// `4xx`, y el resto sigue contestando `5xx`»: de todo [`DbError`] solo dos variantes son
/// culpa de quien pregunta, y **todo lo demás** sigue contestando `500` con el mensaje
/// genérico y la causa al registro. El `500` no se relaja: se le escribe la frontera, que
/// hasta ahora no estaba dicha porque ningún manejador tocaba la base.
///
/// * Un **secreto vacío** contesta `400`: la petición se puede atender y lo que trae mal
///   es un campo. La causa ya existe y es tipada —[`DbError::SecretoVacio`]—, y se responde
///   con un mensaje **propio** que nombra el campo: el `Display` de [`DbError`] está escrito
///   para el registro y puede cambiar para dar más contexto —una ruta, un detalle del
///   motor—, así que publicarlo ataría el contrato del cable a un texto interno sin que
///   nadie pensara que está rompiendo la API.
/// * Una **clave foránea violada** contesta `409`: la petición está **bien formada** y lo
///   que ocurre es que contradice una relación que existe —la tarea apunta a un target o a
///   un repositorio que ya no está—, así que un `400` diría que está mal escrita, y no lo
///   está. El mensaje no copia el texto de la sentencia ni el nombre de la restricción, y
///   **no adivina** qué campo falló: `backup_jobs` tiene **dos** claves foráneas y SQLite
///   contesta «FOREIGN KEY constraint failed» sin decir cuál se contradijo, así que un
///   mensaje que dijera «el target indicado no existe» estaría eligiendo entre dos a
///   ciegas y mandaría a corregir un campo que puede estar bien. Lo que hay que poder
///   hacer con un `409` es **enterarse de que ocurrió**, y para eso está el registro, que
///   recibe la causa entera.
///
/// Y el `409` es una **red de seguridad, no el camino normal**: el manejador valida que las
/// referencias existen antes de escribir, así que solo se llega aquí si algo cambia **entre
/// la comprobación y el `INSERT`** —otra petición borró la fila referenciada—. Si aparece
/// en un caso de uso normal, la comprobación previa no está haciendo su trabajo.
fn traducir_base(causa: DbError) -> (StatusCode, String) {
    if let DbError::SecretoVacio { campo } = &causa {
        // El `400` que nombra el campo es un rechazo de validación como el de
        // `AppError::InvalidInput`, y el contrato del delta `secrets` pide la traza del
        // campo que no cumplía **también aquí**: el camino real de un formulario con el
        // secreto vacío no pasa por el manejador, baja hasta la capa de acceso —es ella
        // la que rechaza el vacío— y vuelve traducido por esta función, así que
        // instrumentar solo la rama de `InvalidInput` dejaría sin traza el rechazo más
        // probable de un formulario. Se registra el **nombre** del campo y **nunca** su
        // valor, y el mensaje de arriba se construye igual —nombrando el campo—, así que
        // esto no puede publicar lo que el cliente mandó.
        tracing::debug!(campo = %campo, "petición rechazada");

        return (
            StatusCode::BAD_REQUEST,
            format!("el campo `{campo}` no puede ir vacío"),
        );
    }

    if let DbError::Consulta {
        causa: sqlx_causa, ..
    } = &causa
        && es_violacion_de_clave_foranea(sqlx_causa)
    {
        tracing::error!(
            error = %causa,
            "la petición contradice una relación existente: se violó una clave foránea"
        );

        return (
            StatusCode::CONFLICT,
            "la petición contradice el estado actual: algún recurso que referencia ya no existe"
                .to_owned(),
        );
    }

    // Todo lo demás sigue siendo un fallo del servidor: la causa —que nombra rutas del
    // despliegue y puede llevar el texto de una consulta— va al registro, y al cliente le
    // llega el mensaje genérico de los fallos internos.
    tracing::error!(error = %causa, "la base de datos falló");

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        INTERNAL_MESSAGE.to_owned(),
    )
}

/// ¿El fallo de `sqlx` es una violación de clave foránea?
///
/// `sqlx::Error::Database` conserva la comprobación del motor, y es lo que permite
/// distinguir la violación de clave foránea de cualquier otra consulta fallida sin mirar
/// el texto del mensaje, que no es contrato de nadie. La causa no se pierde por contestar
/// `4xx`: se pierde solo si el código dejara de mirarla.
fn es_violacion_de_clave_foranea(causa: &sqlx::Error) -> bool {
    matches!(causa, sqlx::Error::Database(error) if error.is_foreign_key_violation())
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use super::AppError;
    use crate::db::DbError;
    use crate::db::entities::{AltaDeRepositorio, AltaDeTarea, Repositorios, Tareas};
    use crate::secret::SecretString;
    use crate::test_support::{base_con_clave, body_json, body_text, capturar_log};

    /// Credencial distinguible: si aparece en el cuerpo, el error la filtró.
    const CREDENCIAL: &str = "AKIAIOSFODNN7EXAMPLE";

    #[tokio::test]
    async fn un_error_de_entrada_da_400_con_el_motivo() {
        let motivo = "el repositorio ya existe";
        let respuesta = AppError::InvalidInput(motivo.to_owned()).into_response();

        assert_eq!(respuesta.status(), StatusCode::BAD_REQUEST);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 400);
        assert!(
            cuerpo["message"]
                .as_str()
                .unwrap_or_default()
                .contains(motivo),
            "el 400 debe explicar el motivo: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_error_interno_no_filtra_la_causa() {
        let respuesta = AppError::Other(format!("credencial S3: {CREDENCIAL}")).into_response();

        assert_eq!(respuesta.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains(CREDENCIAL),
            "el 500 publicó la credencial de la causa: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 500 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 500);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el 500 debe traer exactamente el mensaje genérico: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn una_peticion_sin_sesion_da_401_con_el_motivo() {
        let motivo = "la cookie de sesión no es válida";
        let respuesta = AppError::Unauthorized(motivo.to_owned()).into_response();

        assert_eq!(respuesta.status(), StatusCode::UNAUTHORIZED);

        let texto = body_text(respuesta).await;

        // El 401 explica lo suyo —no es un 5xx— pero no deja el cuerpo vacío ni
        // redirige: la API contesta JSON y es el panel quien decide qué hacer.
        assert!(!texto.is_empty(), "el 401 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 401 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 401);
        assert!(
            cuerpo["message"]
                .as_str()
                .unwrap_or_default()
                .contains(motivo),
            "el 401 debe explicar el motivo: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_502_no_filtra_la_causa() {
        let respuesta = AppError::BadGateway(format!(
            "el proveedor de identidad no contesta: {CREDENCIAL}"
        ))
        .into_response();

        assert_eq!(respuesta.status(), StatusCode::BAD_GATEWAY);

        let texto = body_text(respuesta).await;

        // El 502 es de un servicio de terceros, pero es un 5xx: al cuerpo le rige
        // la misma regla que al fallo interno, y la causa se queda en el registro.
        assert!(
            !texto.contains(CREDENCIAL),
            "el 502 publicó el contenido de la causa: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 502 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 502);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el 502 debe traer el mismo mensaje genérico que el 500: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    /// 1.51 — Un fallo de base de datos no publica la causa.
    ///
    /// El mensaje de `DbError` nombra rutas a propósito —es lo que hace útil el
    /// log—, y es justo lo que no puede salir al cuerpo: una ruta del despliegue
    /// no es asunto de quien pregunta a la API.
    #[tokio::test]
    async fn un_error_de_base_no_filtra_la_causa() {
        let respuesta = AppError::Database(crate::db::DbError::DirectorioAusente {
            ruta: std::path::PathBuf::from(format!("/srv/{CREDENCIAL}/data/kesto.db")),
        })
        .into_response();

        assert_eq!(respuesta.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains(CREDENCIAL),
            "el 500 publicó la ruta de la causa: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 500 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 500);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el fallo de base debe traer el mensaje genérico: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
    }

    /// Passphrase distinguible del alta de repositorio: es el secreto que llevaba la
    /// petición, así que si asoma en el cuerpo de un `4xx` el error lo publicó.
    const PASS_DISTINGUIBLE: &str = "passphrase-distinguible-00000000000000000000000001";

    /// Expresión cron distinguible del alta de tarea: mismo papel que la passphrase,
    /// pero para el camino del `409`, donde lo que viajaba era una tarea.
    const CRON_DISTINGUIBLE: &str = "37 51 4 17 6 2";

    /// 1.34 — Un secreto vacío que llega desde la capa de acceso se traduce a `400` con el campo nombrado.
    ///
    /// Cubre el escenario del secreto vacío del requisito «un error de base causado por
    /// la petición contesta `4xx`, y el resto sigue contestando `5xx`»: quien integró la
    /// API mandó algo que la base no admite, así que el fallo es de la petición y no del
    /// servidor. El error se construye **de verdad** —base de pruebas, capa de acceso y
    /// un alta con la clave secreta vacía— y no a mano: si la comprobación del vacío se
    /// moviera de sitio, una prueba cosida a mano seguiría verde y esta no. Se afirma que
    /// el `4xx` **nombra el campo**, porque sin el nombre quien pregunta no tiene forma de
    /// saber qué mandó mal; y que el código no es `500`, que es la traducción que se está
    /// cerrando.
    #[tokio::test]
    async fn un_secreto_vacio_da_400_con_el_campo_nombrado() {
        let (_directorio, pool, clave) = base_con_clave().await;
        let repositorios = Repositorios::nuevo(pool, clave);

        let error = repositorios
            .alta(AltaDeRepositorio {
                name: "repositorio-con-la-clave-secreta-vacia".to_owned(),
                endpoint: "https://s3.eu-west-1.amazonaws.com".to_owned(),
                bucket: "kesto-pruebas-00000000000000000000000001".to_owned(),
                region: "eu-west-1".to_owned(),
                access_key_id: CREDENCIAL.to_owned(),
                secret_access_key: SecretString::new(String::new()),
                passphrase: SecretString::new(PASS_DISTINGUIBLE.to_owned()),
            })
            .await
            .expect_err("el alta con la clave secreta vacía debe fallar en la capa de acceso");

        assert!(
            matches!(&error, DbError::SecretoVacio { campo } if *campo == "secret_access_key"),
            "se esperaba el rechazo del secreto vacío nombrando el campo: {error}"
        );

        let respuesta = AppError::Database(error).into_response();
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un error causado por la petición no puede seguir contestando 5xx"
        );
        assert_ne!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "el secreto vacío lo mandó la petición: no es un fallo del servidor"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 400);
        assert!(
            cuerpo["message"]
                .as_str()
                .unwrap_or_default()
                .contains("secret_access_key"),
            "el 400 debe nombrar el campo que llegó mal: {cuerpo}"
        );
    }

    /// 1.35 — Una clave foránea violada se traduce a `409`, sin publicar la sentencia ni adivinar el campo.
    ///
    /// Cubre el escenario de la clave foránea del mismo requisito. La violación se
    /// provoca **de verdad** —dos identificadores que no existen contra un `INSERT` con
    /// las claves foráneas encendidas en el pool—, para que el `409` no sea una promesa
    /// del comentario sino el camino que recorre el motor.
    ///
    /// Se afirma que el cuerpo no lleva ni el texto de la sentencia, ni el nombre de la
    /// restricción, ni el de las columnas: `backup_jobs` tiene **dos** claves foráneas y
    /// SQLite no dice cuál falló, así que atribuir el fallo a un campo concreto sería
    /// inventárselo. El mensaje tampoco es el `Display` de la causa —eso es lo que haría
    /// un `format!("{causa}")`—, y sin embargo la causa **sí** queda en el registro: se
    /// pierde para el cliente, no para quien opera el servicio.
    ///
    /// El `capturar_log()` se coge **antes** de provocar el error y el buffer es por
    /// hilo: nunca `set_default` ni `with_default`, que dejan un callsite cacheado como
    /// `Interest::never` para todo el proceso y el registro desaparece en silencio.
    #[tokio::test]
    async fn una_clave_foranea_violada_da_409_sin_publicar_la_sentencia() {
        let (_directorio, pool, _clave) = base_con_clave().await;
        let (buffer, _guard) = capturar_log();
        let tareas = Tareas::nuevo(pool);

        let error = tareas
            .alta(AltaDeTarea {
                target_id: uuid::Uuid::new_v4(),
                repository_id: uuid::Uuid::new_v4(),
                cron_expression: CRON_DISTINGUIBLE.to_owned(),
                keep_daily: 7,
                keep_weekly: 4,
                keep_monthly: 12,
                enabled: true,
            })
            .await
            .expect_err(
                "el alta con dos identificadores inexistentes debe violar la clave foránea",
            );

        assert!(
            matches!(&error, DbError::Consulta { .. }),
            "la violación de clave foránea llega como una consulta fallida: {error}"
        );

        let causa = error.to_string();
        let respuesta = AppError::Database(error).into_response();
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "la petición contradice el estado de la base: es un 409, no un 500"
        );

        let texto = body_text(respuesta).await;
        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 409 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 409);
        assert!(
            cuerpo["message"].as_str().is_some(),
            "el 409 debe explicar el conflicto: {cuerpo}"
        );

        for prohibido in ["INSERT", "constraint", "target_id", "repository_id"] {
            assert!(
                !texto.contains(prohibido),
                "el 409 publicó «{prohibido}» del motor: {texto}"
            );
        }

        let mensaje = cuerpo["message"].as_str().unwrap_or_default();

        assert!(
            !mensaje.contains(&causa),
            "el 409 repitió el texto de la causa: {mensaje}"
        );
        assert!(
            buffer.texto().contains("FOREIGN KEY constraint failed"),
            "la causa debe quedar en el registro aunque no salga al cliente: {}",
            buffer.texto()
        );
    }

    /// 1.36 — Una consulta que falla por otra causa sigue contestando `500` con el mensaje genérico, y la causa va al registro.
    ///
    /// Es la **frontera** del requisito: demuestra que al abrir el `4xx` no se ha abierto
    /// en canal la traducción, y que «cualquier otra consulta fallida» sigue siendo un
    /// fallo del servidor con el mensaje genérico y sin `data`.
    ///
    /// El fallo se provoca de verdad y sin violar ninguna clave foránea: se cierra el
    /// pool y se lista sobre él, así que la capa de acceso devuelve una consulta fallida
    /// cuya causa es un `sqlx::Error` de pool cerrado. Es una prueba de frontera, no de
    /// comportamiento nuevo —como la 1.1 del módulo—, y por eso puede nacer verde: la
    /// rama del `500` ya la dejó `api-shell`. Lo que sí añade es que la causa sigue
    /// viajando al registro aunque el cliente reciba solo el mensaje genérico.
    #[tokio::test]
    async fn una_consulta_que_falla_por_otra_causa_sigue_dando_500_generico() {
        let (_directorio, pool, clave) = base_con_clave().await;
        let (buffer, _guard) = capturar_log();
        let repositorios = Repositorios::nuevo(pool.clone(), clave);

        pool.close().await;

        let error = repositorios
            .listar()
            .await
            .expect_err("listar sobre un pool cerrado debe fallar");

        let respuesta = AppError::Database(error).into_response();
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "un fallo de la base que no viene de la petición sigue siendo un 500"
        );

        let texto = body_text(respuesta).await;
        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 500 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 500);
        assert_eq!(
            cuerpo["message"],
            super::INTERNAL_MESSAGE,
            "el fallo de base debe traer exactamente el mensaje genérico: {cuerpo}"
        );
        assert!(
            cuerpo.get("data").is_none(),
            "el error no debe traer la clave data: {cuerpo}"
        );
        assert!(
            !buffer.texto().is_empty(),
            "la causa debe viajar al registro aunque no al cliente"
        );
    }

    /// 1.37 — El `4xx` no filtra ni una credencial ni la ruta del fichero de la base.
    ///
    /// Cubre la última línea del escenario de la clave foránea. Un `4xx` **explica** el
    /// fallo, y explicarlo es justo lo que tienta a poner la causa en el mensaje: el
    /// `Display` de `DbError` nombra rutas y valores a propósito —es lo que hace útil el
    /// log—, así que publicarlo dejaría la ruta del fichero de la base y lo que mandó la
    /// petición a la vista de cualquiera que se equivoque al llamar.
    ///
    /// Se recorren **los dos** caminos `4xx` que existen —el `400` del secreto vacío y el
    /// `409` de la clave foránea— porque la regla es del `4xx` entero, no de una de sus
    /// ramas. En los dos se afirma que el texto crudo de la respuesta no lleva la
    /// credencial que iba en la petición, ni la ruta de la base, ni el `Display` de la
    /// causa; y que el cuerpo trae **solo** `status` y `message`: un `data` en un error
    /// sería una segunda puerta por la que colar la causa.
    #[tokio::test]
    async fn los_4xx_no_filtran_ni_la_credencial_ni_la_ruta_de_la_base() {
        // — Camino del 400: la petición traía un secreto vacío y una passphrase de verdad.
        {
            let (directorio, pool, clave) = base_con_clave().await;
            let ruta_de_la_base = directorio.ruta_de_la_base().display().to_string();
            let repositorios = Repositorios::nuevo(pool, clave);

            let error = repositorios
                .alta(AltaDeRepositorio {
                    name: "repositorio-con-la-clave-secreta-vacia".to_owned(),
                    endpoint: "https://s3.eu-west-1.amazonaws.com".to_owned(),
                    bucket: "kesto-pruebas-00000000000000000000000001".to_owned(),
                    region: "eu-west-1".to_owned(),
                    access_key_id: CREDENCIAL.to_owned(),
                    secret_access_key: SecretString::new(String::new()),
                    passphrase: SecretString::new(PASS_DISTINGUIBLE.to_owned()),
                })
                .await
                .expect_err("el alta con la clave secreta vacía debe fallar en la capa de acceso");

            let causa = error.to_string();
            let respuesta = AppError::Database(error).into_response();

            assert_eq!(
                respuesta.status(),
                StatusCode::BAD_REQUEST,
                "el secreto vacío es un error de la petición: 400"
            );

            let texto = body_text(respuesta).await;

            assert!(
                !texto.contains(PASS_DISTINGUIBLE),
                "el 400 publicó la passphrase de la petición: {texto}"
            );
            assert!(
                !texto.contains(CREDENCIAL),
                "el 400 publicó la credencial de la petición: {texto}"
            );
            assert!(
                !texto.contains(&ruta_de_la_base),
                "el 400 publicó la ruta del fichero de la base: {texto}"
            );

            let cuerpo: serde_json::Value =
                serde_json::from_str(&texto).expect("el 400 debe traer JSON en el cuerpo");

            assert_eq!(cuerpo["status"], 400);
            assert!(
                !cuerpo["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(&causa),
                "el 400 copió el Display de la causa en el mensaje: {cuerpo}"
            );
            assert!(
                cuerpo.get("data").is_none(),
                "el error no debe traer la clave data: {cuerpo}"
            );
            assert_eq!(
                cuerpo
                    .as_object()
                    .expect("el cuerpo de un error debe ser un objeto JSON")
                    .len(),
                2,
                "el 4xx debe traer solo status y message: {cuerpo}"
            );
        }

        // — Camino del 409: la petición traía una tarea con una cron distinguible.
        {
            let (directorio, pool, _clave) = base_con_clave().await;
            let ruta_de_la_base = directorio.ruta_de_la_base().display().to_string();
            let tareas = Tareas::nuevo(pool);

            let error = tareas
                .alta(AltaDeTarea {
                    target_id: uuid::Uuid::new_v4(),
                    repository_id: uuid::Uuid::new_v4(),
                    cron_expression: CRON_DISTINGUIBLE.to_owned(),
                    keep_daily: 7,
                    keep_weekly: 4,
                    keep_monthly: 12,
                    enabled: true,
                })
                .await
                .expect_err(
                    "el alta con dos identificadores inexistentes debe violar la clave foránea",
                );

            let causa = error.to_string();
            let respuesta = AppError::Database(error).into_response();

            assert_eq!(
                respuesta.status(),
                StatusCode::CONFLICT,
                "la clave foránea violada es un conflicto: 409"
            );

            let texto = body_text(respuesta).await;

            assert!(
                !texto.contains(CRON_DISTINGUIBLE),
                "el 409 publicó la cron de la petición: {texto}"
            );
            assert!(
                !texto.contains(&ruta_de_la_base),
                "el 409 publicó la ruta del fichero de la base: {texto}"
            );

            let cuerpo: serde_json::Value =
                serde_json::from_str(&texto).expect("el 409 debe traer JSON en el cuerpo");

            assert_eq!(cuerpo["status"], 409);
            assert!(
                !cuerpo["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(&causa),
                "el 409 copió el Display de la causa en el mensaje: {cuerpo}"
            );
            assert!(
                cuerpo.get("data").is_none(),
                "el error no debe traer la clave data: {cuerpo}"
            );
            assert_eq!(
                cuerpo
                    .as_object()
                    .expect("el cuerpo de un error debe ser un objeto JSON")
                    .len(),
                2,
                "el 4xx debe traer solo status y message: {cuerpo}"
            );
        }
    }
}
