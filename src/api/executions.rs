//! Las rutas de `/api/v1/executions`: el histórico de las copias y sus métricas.
//!
//! Es el único recurso **de solo lectura** del §5: no tiene `POST` ni `/:id`, y no se le
//! añaden, porque una ejecución no se crea desde el panel —la crea una copia que corre—. Las
//! tres escrituras de la capa de acceso existen desde `db-schema` y las usará el **motor**;
//! hoy no tienen ruta.
//!
//! El listado es el de la tabla que más crece de las cuatro —una fila por intento de copia—,
//! así que es el único que lleva **ventana**: `limit` con su valor por defecto y su tope, y
//! `offset`. Sin tope, un `limit` grande materializaría la tabla entera en memoria y ocuparía
//! una de las conexiones del pool mientras el motor quiere escribir.
//!
//! Cada fila sale con sus **métricas** —los dos contadores de bytes y sus dos fechas—, que es
//! el sentido del «y métricas» del §5: no hay endpoint de agregados, y quien sume los totales
//! es el panel.
//!
//! El manejador está **implementado**: valida lo que llega por el cable, se lo pasa a la capa
//! de acceso como un filtro ya resuelto y publica las filas con el formato del cable —el mismo
//! de los otros tres recursos—.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::contenido;
use crate::api::extract::Query;
use crate::api::response::ApiResponse;
use crate::db::entities::{Ejecucion, Estado, FiltroDeEjecuciones};
use crate::error::AppError;
use crate::state::AppState;

/// Cuántas filas devuelve un listado que no informa `limit`.
///
/// Cincuenta y no todas: el panel pinta una página, y un defecto generoso convertiría cada
/// carga del histórico en la tabla entera. Quien quiera más lo pide, hasta el tope.
pub const LIMITE_POR_DEFECTO: i64 = 50;

/// El tope de `limit`.
///
/// Existe porque el histórico es la tabla que más crece: sin tope, un `limit` grande
/// materializaría la tabla entera en memoria y ocuparía una de las conexiones del pool
/// mientras el motor quiere escribir.
pub const LIMITE_MAXIMO: i64 = 200;

/// Las rutas del recurso: el listado, y solo el listado.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/executions", get(listado))
}

/// Los parámetros de consulta, tal y como llegan por el cable.
///
/// `deny_unknown_fields` porque un `?limits=5` ignorado en silencio devolvería el listado por
/// defecto y quien llamó creería haber acotado: es la misma razón por la que el cuerpo lo
/// lleva.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiltrosDelCable {
    /// Tarea por la que filtrar.
    pub job_id: Option<String>,
    /// Estado por el que filtrar.
    pub status: Option<String>,
    /// Cuántas filas devolver como mucho.
    pub limit: Option<i64>,
    /// Cuántas filas saltar.
    pub offset: Option<i64>,
}

impl FiltrosDelCable {
    /// Valida los parámetros de la consulta y los convierte en el filtro de la capa de acceso.
    ///
    /// Todo lo que no encaja contesta `400` **nombrando el campo**: un `job_id` que no es un
    /// UUID, un `status` que no es uno de los cuatro, un `limit` fuera de `1..=200` o un
    /// `offset` negativo. El mensaje dice el rango cuando el campo lo tiene y **nunca** repite
    /// el valor: la regla de la casa es que un `400` diga qué campo está mal, no lo que traía.
    ///
    /// Un `limit` fuera de rango **no se recorta**: recortarlo dejaría a quien pidió creyendo
    /// que ha recibido lo que pidió.
    fn validar(self) -> Result<FiltroDeEjecuciones, AppError> {
        let job_id = match self.job_id.as_deref() {
            Some(valor) => Some(Uuid::parse_str(valor).map_err(|_| {
                AppError::InvalidInput(
                    "el campo `job_id` de la consulta no tiene la forma de un UUID".to_owned(),
                )
            })?),
            None => None,
        };

        let status = match self.status.as_deref() {
            Some(valor) => Some(Estado::desde_texto(valor).ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "el campo `status` de la consulta no es uno de los cuatro estados admitidos: {}",
                    estados_admitidos()
                ))
            })?),
            None => None,
        };

        let limite = match self.limit {
            Some(valor) if !(1..=LIMITE_MAXIMO).contains(&valor) => {
                return Err(AppError::InvalidInput(format!(
                    "el campo `limit` de la consulta tiene que ser un número entre 1 y {LIMITE_MAXIMO}"
                )));
            }
            Some(valor) => valor,
            None => LIMITE_POR_DEFECTO,
        };

        let desplazamiento = match self.offset {
            Some(valor) if valor < 0 => {
                return Err(AppError::InvalidInput(
                    "el campo `offset` de la consulta no puede ser negativo".to_owned(),
                ));
            }
            Some(valor) => valor,
            None => 0,
        };

        Ok(FiltroDeEjecuciones {
            job_id,
            status,
            limite,
            desplazamiento,
        })
    }
}

/// Los cuatro estados admitidos, para poder nombrarlos sin volver a escribir sus literales.
///
/// El único sitio que los escribe es [`Estado::texto`]; aquí solo se reúnen para el mensaje.
fn estados_admitidos() -> String {
    [
        Estado::Pending,
        Estado::Running,
        Estado::Success,
        Estado::Failed,
    ]
    .map(Estado::texto)
    .join(", ")
}

/// Una fila del histórico tal y como viaja por el cable.
///
/// Son las nueve columnas del §3 con sus nombres, y **no** la entidad: el `status` sale como el
/// texto que el `CHECK` admite y las dos fechas como cadena RFC3339. `error_message` sale tal
/// cual —el panel necesita saber por qué falló una copia— y quien la escribe es el motor, que
/// no debe volcar credenciales ahí.
#[derive(Debug, Serialize)]
pub struct EjecucionDelCable {
    /// Identificador de la ejecución.
    pub id: String,
    /// Tarea a la que pertenece.
    pub job_id: String,
    /// Estado, en el texto del `CHECK`.
    pub status: String,
    /// Marca del `snapshot`; `null` mientras no hay.
    pub snapshot_id: Option<String>,
    /// Bytes procesados.
    pub bytes_processed: i64,
    /// Bytes subidos.
    pub bytes_uploaded: i64,
    /// Motivo del fallo; `null` si no falló.
    pub error_message: Option<String>,
    /// Cuándo empezó, en RFC3339.
    pub started_at: String,
    /// Cuándo terminó, en RFC3339; `null` si sigue en curso.
    pub finished_at: Option<String>,
}

impl EjecucionDelCable {
    /// Convierte una fila en su vista del cable.
    ///
    /// Las fechas son las que la base guardó: la capa de acceso las normaliza a UTC **al
    /// enlazarlas**, así que el formateador no tiene que corregir nada —es el mismo reparto que
    /// en los otros tres recursos, y por eso el formato del cable es idéntico en los cuatro—.
    fn desde(fila: &Ejecucion) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.id.hyphenated().to_string(),
            job_id: fila.job_id.hyphenated().to_string(),
            status: fila.status.texto().to_owned(),
            snapshot_id: fila.snapshot_id.clone(),
            bytes_processed: fila.bytes_processed,
            bytes_uploaded: fila.bytes_uploaded,
            error_message: fila.error_message.clone(),
            started_at: formatear(fila.started_at)?,
            finished_at: fila.finished_at.map(formatear).transpose()?,
        })
    }
}

/// Formatea una fecha como RFC3339, que es el formato del cable.
fn formatear(fecha: time::OffsetDateTime) -> Result<String, AppError> {
    fecha.format(&Rfc3339).map_err(|error| {
        AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
    })
}

/// `GET /api/v1/executions` — el histórico, con filtros y ventana.
///
/// Una tabla sin filas es un listado de cero elementos, **no** un recurso ausente: contesta
/// `200` con un array vacío, como los otros tres recursos.
pub async fn listado(
    State(estado): State<AppState>,
    Query(filtros): Query<FiltrosDelCable>,
) -> Result<ApiResponse, AppError> {
    let filtro = filtros.validar()?;

    let filas = estado.db.ejecuciones.listar_filtrado(&filtro).await?;

    let vistas: Vec<EjecucionDelCable> = filas
        .iter()
        .map(EjecucionDelCable::desde)
        .collect::<Result<_, _>>()?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "ejecuciones listadas",
        contenido(&vistas)?,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::{StatusCode, header};
    use time::format_description::well_known::Rfc3339;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::db::entities::{
        AltaDeEjecucion, AltaDeRepositorio, AltaDeTarea, AltaDeTarget, Estado, TipoDeTarget,
    };
    use crate::secret::SecretString;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        DirectorioDePrueba, base_con_clave, body_json, body_text, config_con_dsn,
        estado_desarrollo, get,
    };

    /// Siembra un target, un repositorio y una tarea, y devuelve el identificador de la tarea.
    ///
    /// `job_executions` tiene clave foránea a `backup_jobs`, y ésta a su vez a `backup_targets`
    /// y a `repositories`: no hay forma de tener una ejecución sin la cadena entera. Se siembra
    /// por la **capa de acceso** y no por la API porque lo que estas pruebas miden es el
    /// listado, no el alta de sus antecesores.
    async fn sembrar_tarea(estado: &AppState) -> Uuid {
        let target = estado
            .db
            .targets
            .alta(AltaDeTarget {
                name: "target-de-ejecuciones".to_owned(),
                target_type: TipoDeTarget::Directory,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: Some("/srv/datos".to_owned()),
            })
            .await
            .expect("el target de prueba se da de alta");

        let repositorio = estado
            .db
            .repositorios
            .alta(AltaDeRepositorio {
                name: "repositorio-de-ejecuciones".to_owned(),
                endpoint: "https://s3.example.test/ejecuciones".to_owned(),
                bucket: "cubo-de-ejecuciones".to_owned(),
                region: "eu-west-1".to_owned(),
                access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
                secret_access_key: SecretString::new(
                    "credencial-de-prueba-0000000000000001".to_owned(),
                ),
                passphrase: SecretString::new("passphrase-de-prueba-00000000000001".to_owned()),
            })
            .await
            .expect("el repositorio de prueba se da de alta");

        let tarea = estado
            .db
            .tareas
            .alta(AltaDeTarea {
                target_id: target.id,
                repository_id: repositorio.id,
                cron_expression: "0 3 * * *".to_owned(),
                keep_daily: 7,
                keep_weekly: 4,
                keep_monthly: 6,
                enabled: true,
            })
            .await
            .expect("la tarea de prueba se da de alta");

        tarea.id
    }

    /// Da de alta una ejecución y devuelve su identificador.
    ///
    /// La fecha de inicio y el estado son **parámetros** porque varias pruebas necesitan
    /// controlarlos: la del orden empatado necesita dos filas con el mismo instante, y la de la
    /// ventana necesita un orden conocido de antemano.
    async fn sembrar_ejecucion(
        estado: &AppState,
        job_id: Uuid,
        estado_de_la_fila: Estado,
        iniciada: OffsetDateTime,
        procesados: i64,
    ) -> Uuid {
        let fila = estado
            .db
            .ejecuciones
            .alta(AltaDeEjecucion {
                job_id,
                status: estado_de_la_fila,
                snapshot_id: Some("snapshot-de-prueba-00000000000000000001".to_owned()),
                bytes_processed: procesados,
                bytes_uploaded: procesados / 2,
                error_message: None,
                started_at: iniciada,
                finished_at: Some(iniciada + Duration::seconds(30)),
            })
            .await
            .expect("la ejecución de prueba se da de alta");

        fila.id
    }

    /// Monta el estado **con su pool y su clave**, para poder sembrar antes de pedir.
    ///
    /// Devuelve el `AppState` y la guarda del directorio temporal, que tiene que seguir **viva**
    /// hasta el final de la prueba: si se suelta, el directorio se borra y la base desaparece
    /// bajo los pies del router.
    async fn estado_con_base() -> (AppState, DirectorioDePrueba) {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        (estado, directorio)
    }

    #[tokio::test]
    async fn el_listado_sin_filtros_devuelve_las_ejecuciones_con_sus_metricas() {
        // Escenario 1.38.
        //
        // El listado es el del histórico: sin filtros devuelve las filas, cada una con sus
        // cuatro métricas, y **la más reciente primero**. El orden no es cosmético —el panel
        // pinta lo último arriba— y por eso se comprueba por el `bytes_processed`, que
        // distingue una fila de otra.
        let (estado, _directorio) = estado_con_base().await;
        let tarea = sembrar_tarea(&estado).await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        for (desplazamiento, procesados) in [(0_i64, 1000_i64), (10, 2000), (20, 3000)].into_iter()
        {
            sembrar_ejecucion(
                &estado,
                tarea,
                Estado::Success,
                base + Duration::seconds(desplazamiento),
                procesados,
            )
            .await;
        }

        let respuesta = get(api_router(estado), "/api/v1/executions").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado contesta {status}: {cuerpo}"
        );

        let filas = cuerpo["data"]
            .as_array()
            .expect("el listado tiene que llegar como un array");

        assert_eq!(filas.len(), 3, "sin filtros salen las tres filas");

        for clave in [
            "bytes_processed",
            "bytes_uploaded",
            "started_at",
            "finished_at",
        ] {
            assert!(
                filas[0].get(clave).is_some(),
                "la fila del listado tiene que traer la métrica `{clave}`: {}",
                filas[0]
            );
        }

        assert_eq!(
            filas[0]["bytes_processed"], 3000,
            "el orden es descendente por la fecha de inicio y la más reciente es la primera"
        );
        assert_eq!(
            filas[2]["bytes_processed"], 1000,
            "la más antigua tiene que quedar la última"
        );
    }

    #[tokio::test]
    async fn los_filtros_por_job_id_y_por_status_acotan_el_listado() {
        // Escenario 1.39.
        //
        // Los dos filtros del contrato, y su combinación. Se siembran **dos** tareas porque un
        // filtro que no acotara sería indistinguible con una sola: las tres filas caen en dos
        // tareas distintas y el filtro por `job_id` tiene que dejar fuera la de la otra.
        let (estado, _directorio) = estado_con_base().await;
        let tarea_a = sembrar_tarea(&estado).await;
        let tarea_b = sembrar_tarea(&estado).await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        sembrar_ejecucion(&estado, tarea_a, Estado::Success, base, 1000).await;
        sembrar_ejecucion(
            &estado,
            tarea_a,
            Estado::Failed,
            base + Duration::seconds(10),
            2000,
        )
        .await;
        sembrar_ejecucion(
            &estado,
            tarea_b,
            Estado::Success,
            base + Duration::seconds(20),
            3000,
        )
        .await;

        let respuesta = get(
            api_router(estado.clone()),
            &format!("/api/v1/executions?job_id={tarea_a}"),
        )
        .await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el filtro por tarea contesta {status}"
        );

        let filas = cuerpo["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 2, "la tarea A tiene dos ejecuciones");

        for fila in filas {
            assert_eq!(
                fila["job_id"],
                tarea_a.hyphenated().to_string(),
                "el filtro por tarea no puede devolver filas de otra"
            );
        }

        let respuesta = get(
            api_router(estado.clone()),
            "/api/v1/executions?status=failed",
        )
        .await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el filtro por estado contesta {status}"
        );

        let filas = cuerpo["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 1, "solo una ejecución falló");
        assert_eq!(filas[0]["status"], "failed", "y es la fallida");

        let respuesta = get(
            api_router(estado),
            &format!("/api/v1/executions?job_id={tarea_a}&status=success"),
        )
        .await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "los dos filtros juntos contestan {status}"
        );

        let filas = cuerpo["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 1, "de la tarea A solo una terminó bien");
        assert_eq!(filas[0]["status"], "success", "y es la satisfactoria");
    }

    #[tokio::test]
    async fn el_limite_acota_y_el_desplazamiento_avanza() {
        // Escenario 1.40.
        //
        // La ventana sobre la tabla que más crece. `limit=2` está para que se vea que acota y
        // no que devuelve una de casualidad, y el `assert_ne!` de las dos primeras filas es lo
        // que fija que `offset` **avanza** en vez de repetir la primera página.
        let (estado, _directorio) = estado_con_base().await;
        let tarea = sembrar_tarea(&estado).await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        for (desplazamiento, procesados) in [(0_i64, 1000_i64), (10, 2000), (20, 3000)].into_iter()
        {
            sembrar_ejecucion(
                &estado,
                tarea,
                Estado::Success,
                base + Duration::seconds(desplazamiento),
                procesados,
            )
            .await;
        }

        let primera =
            body_json(get(api_router(estado.clone()), "/api/v1/executions?limit=1").await).await;
        let filas = primera["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 1, "`limit=1` tiene que devolver una sola fila");

        let identificador_primero = filas[0]["id"]
            .as_str()
            .expect("la fila trae su identificador")
            .to_owned();

        let segunda = body_json(
            get(
                api_router(estado.clone()),
                "/api/v1/executions?limit=1&offset=1",
            )
            .await,
        )
        .await;
        let filas = segunda["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 1, "`limit=1&offset=1` devuelve una sola fila");

        let identificador_segundo = filas[0]["id"]
            .as_str()
            .expect("la fila trae su identificador")
            .to_owned();

        assert_ne!(
            identificador_primero, identificador_segundo,
            "el desplazamiento tiene que avanzar, no repetir la primera página"
        );

        let dos = body_json(get(api_router(estado), "/api/v1/executions?limit=2").await).await;
        let filas = dos["data"].as_array().expect("el listado es un array");

        assert_eq!(filas.len(), 2, "`limit=2` tiene que devolver dos filas");
    }

    #[tokio::test]
    async fn un_limite_fuera_de_rango_o_no_numerico_contesta_400_sin_recortar() {
        // Escenario 1.41.
        //
        // Los cinco desenlaces del parámetro `limit`/`offset` que no se pueden admitir. Ninguno
        // recorta el valor: un `limit=201` recortado a 200 dejaría a quien pidió creyendo que
        // ha recibido lo que pidió. Y ninguno devuelve listado: `data` es `null` en el cuerpo.
        let (estado, _directorio) = estado_con_base().await;
        let router = api_router(estado);

        for ruta in [
            "/api/v1/executions?limit=201",
            "/api/v1/executions?limit=0",
            "/api/v1/executions?limit=-1",
            "/api/v1/executions?limit=abc",
            "/api/v1/executions?offset=-1",
        ] {
            let respuesta = get(router.clone(), ruta).await;
            let status = respuesta.status();
            let cuerpo = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "`{ruta}` tiene que contestar 400 y contestó {status}: {cuerpo}"
            );
            assert!(
                cuerpo["data"].is_null(),
                "un rechazo no puede traer listado: {cuerpo}"
            );
        }

        let respuesta = get(router, "/api/v1/executions?limit=201").await;
        let cuerpo = body_json(respuesta).await;
        let mensaje = cuerpo["message"]
            .as_str()
            .expect("el rechazo trae mensaje")
            .to_owned();

        assert!(
            mensaje.contains("limit"),
            "el mensaje tiene que nombrar el campo: {mensaje}"
        );
        assert!(
            !mensaje.contains("201"),
            "el mensaje no repite el valor que llegó: {mensaje}"
        );
    }

    #[tokio::test]
    async fn dos_llamadas_seguidas_dan_el_mismo_orden_con_filas_empatadas() {
        // Escenario 1.42.
        //
        // El desempate del contrato: `ORDER BY started_at DESC, id`. `CURRENT_TIMESTAMP` no
        // tiene más resolución que el segundo, así que dos ejecuciones que arrancan a la vez
        // empatan; sin desempate, dos peticiones seguidas con el mismo `offset` pueden repetir
        // una fila y saltarse otra. Se comprueba por partida doble: las dos llamadas dan la
        // misma secuencia, y esa secuencia está ordenada por identificador.
        let (estado, _directorio) = estado_con_base().await;
        let tarea = sembrar_tarea(&estado).await;

        let empate = datetime!(2026-10-25 03:00:00 UTC);

        sembrar_ejecucion(&estado, tarea, Estado::Success, empate, 1000).await;
        sembrar_ejecucion(&estado, tarea, Estado::Failed, empate, 2000).await;

        let mut secuencias = Vec::new();

        for _ in 0..2 {
            let cuerpo =
                body_json(get(api_router(estado.clone()), "/api/v1/executions").await).await;
            let filas = cuerpo["data"].as_array().expect("el listado es un array");

            assert_eq!(filas.len(), 2, "las dos filas empatadas tienen que salir");

            secuencias.push(
                filas
                    .iter()
                    .map(|fila| {
                        fila["id"]
                            .as_str()
                            .expect("la fila trae su identificador")
                            .to_owned()
                    })
                    .collect::<Vec<_>>(),
            );
        }

        assert_eq!(
            secuencias[0], secuencias[1],
            "el orden tiene que ser estable entre dos llamadas seguidas"
        );

        let mut ordenada = secuencias[0].clone();
        ordenada.sort();

        assert_eq!(
            secuencias[0], ordenada,
            "con la misma fecha, el desempate es el identificador"
        );
    }

    #[tokio::test]
    async fn las_fechas_de_una_fila_de_ejecucion_son_cadenas_rfc3339() {
        // Escenario 1.47.
        //
        // Las dos fechas del cable. La serialización por defecto de `time` **no** es una
        // cadena, así que esta prueba delata un cambio de formateo: compara el literal exacto,
        // sin fracción —solo se escribe si los nanosegundos no son cero— y con la `Z` del UTC,
        // y vuelve a leerlo para que el texto signifique el instante que se sembró.
        let (estado, _directorio) = estado_con_base().await;
        let tarea = sembrar_tarea(&estado).await;

        let inicio = datetime!(2026-10-25 03:00:00 UTC);

        sembrar_ejecucion(&estado, tarea, Estado::Success, inicio, 1000).await;

        let cuerpo = body_json(get(api_router(estado), "/api/v1/executions").await).await;
        let fila = &cuerpo["data"][0];

        let empezada = fila["started_at"]
            .as_str()
            .unwrap_or_else(|| panic!("`started_at` tiene que ser una cadena: {fila}"));
        let terminada = fila["finished_at"]
            .as_str()
            .unwrap_or_else(|| panic!("`finished_at` tiene que ser una cadena: {fila}"));

        assert_eq!(
            empezada, "2026-10-25T03:00:00Z",
            "el formato del cable es RFC3339"
        );
        assert_eq!(
            terminada, "2026-10-25T03:00:30Z",
            "y el fin, treinta segundos después"
        );

        assert!(
            !empezada.contains('.'),
            "la fracción solo se escribe si los nanosegundos no son cero: {empezada}"
        );

        let instante = OffsetDateTime::parse(empezada, &Rfc3339)
            .expect("la fecha del cable tiene que releerse como RFC3339");

        assert_eq!(
            instante, inicio,
            "el texto tiene que significar el instante que se sembró"
        );
    }

    #[tokio::test]
    async fn un_parametro_de_consulta_mal_formado_contesta_400_con_la_envoltura() {
        // Escenario 1.25.
        //
        // El delta `api-shell` para la otra mitad de la familia del cuerpo ilegible: lo que no
        // se puede interpretar. El rechazo por defecto describe el fallo con el texto del
        // intérprete —`invalid digit found in string`— y ese texto **no** puede salir por el
        // cable: la API contesta JSON con un mensaje propio. No hace falta base sembrada: el
        // rechazo ocurre antes de consultar nada.
        let respuesta = get(
            api_router(estado_desarrollo().await),
            "/api/v1/executions?limit=abc",
        )
        .await;

        let status = respuesta.status();
        let tipo = respuesta
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|valor| valor.to_str().ok())
            .map(ToOwned::to_owned);
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "el rechazo es un 400: {texto}"
        );
        assert!(
            tipo.as_deref()
                .is_some_and(|tipo| tipo.starts_with("application/json")),
            "el rechazo tiene que salir por la envoltura, no en texto plano: {tipo:?}"
        );
        assert!(
            !texto.contains("invalid digit"),
            "el texto del intérprete no puede salir por el cable: {texto}"
        );
        assert!(
            !texto.contains("abc"),
            "el valor del parámetro no puede volver en el mensaje: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el rechazo tiene que traer JSON en el cuerpo");

        assert_eq!(
            cuerpo["status"], 400,
            "y el estado del cuerpo es el del 400"
        );
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el rechazo trae un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn un_parametro_de_consulta_desconocido_se_rechaza() {
        // La errata del parámetro: `limits` por `limit`. Sin `deny_unknown_fields` se ignoraría
        // en silencio y devolvería el listado por defecto —cincuenta filas— mientras quien
        // llamó creería haber acotado. Se comprueba además que el texto del intérprete
        // —`unknown field`— no sale por el cable.
        let respuesta = get(
            api_router(estado_desarrollo().await),
            "/api/v1/executions?limits=5",
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "la errata es un 400: {texto}"
        );
        assert!(
            !texto.contains("unknown field"),
            "el texto del intérprete no puede salir por el cable: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el rechazo tiene que traer JSON en el cuerpo");

        assert_eq!(
            cuerpo["status"], 400,
            "y el estado del cuerpo es el del 400"
        );
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el rechazo trae un mensaje: {texto}"
        );
    }
}
