//! Las rutas de `/api/v1/restorations`: el histórico de las restauraciones.
//!
//! Es un recurso **de solo lectura** como el histórico de copias: no tiene `POST` ni `/:id`, y no se
//! le añaden, porque una restauración no se crea desde el panel —la crea el `POST` de la
//! restauración de una instantánea, y la escribe el motor—. Lo que publica es el rastro que hasta
//! ahora no existía: qué se restauró, dónde, cuándo y con qué desenlace.
//!
//! El listado es el de la tabla `restore_executions`, ordenado por fecha de comienzo **descendente**
//! —lo último primero, que es lo que el panel pinta— y con **ventana**: `limit` con su valor por
//! defecto y su tope, y `offset`. Publica además el **total** de lo que cumple el filtro, que es lo
//! que permite al panel saber cuántas páginas hay sin pedirlas todas.
//!
//! Cada fila sale con los datos del **target** —su nombre y su tipo—, resueltos desde el vínculo
//! `restore_executions.target_id → backup_targets.id` por la capa de acceso: la tabla de
//! restauraciones guarda **cuál** es el target, no cómo se llama. Los dos se publican como `null`
//! cuando la fila no tiene target, que es un hueco legítimo del contrato y no un error.

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
use crate::api::targets::texto_del_tipo;
use crate::db::entities::{
    Estado, FiltroDeRestauraciones, PaginaDeRestauraciones, RestauracionListada,
};
use crate::error::AppError;
use crate::state::AppState;

/// Cuántas filas devuelve un listado que no informa `limit`.
///
/// Cincuenta y no todas: el panel pinta una página, y un defecto generoso convertiría cada carga
/// del histórico en la tabla entera. Quien quiera más lo pide, hasta el tope. Es el mismo valor que
/// el histórico de copias, y por el mismo motivo.
pub const LIMITE_POR_DEFECTO: i64 = 50;

/// El tope de `limit`.
///
/// Existe porque el histórico crece: sin tope, un `limit` grande materializaría la tabla entera en
/// memoria y ocuparía una de las conexiones del pool mientras el motor quiere escribir.
pub const LIMITE_MAXIMO: i64 = 200;

/// Las rutas del recurso: el listado, y solo el listado.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/restorations", get(listado))
}

/// Los parámetros de consulta, tal y como llegan por el cable.
///
/// `deny_unknown_fields` porque un `?estado=failed` ignorado en silencio devolvería el listado sin
/// filtrar y quien llamó creería haber acotado: es la misma razón por la que el cuerpo de la
/// restauración lo lleva. `target_id` y `status` son los dos filtros del contrato y `limit`/`offset`
/// la ventana.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FiltrosDelCable {
    /// Estado por el que filtrar.
    pub status: Option<String>,
    /// Target por el que filtrar.
    pub target_id: Option<String>,
    /// Cuántas filas devolver como mucho.
    pub limit: Option<i64>,
    /// Cuántas filas saltar.
    pub offset: Option<i64>,
}

impl FiltrosDelCable {
    /// Valida los parámetros de la consulta y los convierte en el filtro de la capa de acceso.
    ///
    /// Todo lo que no encaja contesta `400` **nombrando el campo**: un `target_id` que no es un
    /// UUID, un `status` que no es uno de los cuatro, un `limit` fuera de `1..=200` o un `offset`
    /// negativo. El mensaje dice el rango cuando el campo lo tiene y **nunca** repite el valor: la
    /// regla de la casa es que un `400` diga qué campo está mal, no lo que traía.
    ///
    /// Un `limit` fuera de rango **no se recorta**: recortarlo dejaría a quien pidió creyendo que ha
    /// recibido lo que pidió.
    fn validar(self) -> Result<FiltroDeRestauraciones, AppError> {
        let status = match self.status.as_deref() {
            Some(valor) => Some(Estado::desde_texto(valor).ok_or_else(|| {
                AppError::InvalidInput(format!(
                    "el campo `status` de la consulta no es uno de los cuatro estados admitidos: {}",
                    estados_admitidos()
                ))
            })?),
            None => None,
        };

        let target_id = match self.target_id.as_deref() {
            Some(valor) => Some(Uuid::parse_str(valor).map_err(|_| {
                AppError::InvalidInput(
                    "el campo `target_id` de la consulta no tiene la forma de un UUID".to_owned(),
                )
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

        Ok(FiltroDeRestauraciones {
            target_id,
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
/// Son los nueve campos del contrato y **no** la entidad: el `status` sale como el texto que el
/// `CHECK` admite, las dos fechas como cadena RFC3339 y los tres identificadores en su forma
/// canónica con guiones. `target_name` y `target_type` son los dos únicos que **no** salen de la
/// tabla de restauraciones —los resuelve el vínculo con `backup_targets`—, y van como `null` cuando
/// la fila no tiene target, que es un hueco y no un error.
#[derive(Debug, Serialize)]
pub struct RestauracionDelCable {
    /// Identificador de la ejecución de restauración.
    pub id: String,
    /// La instantánea que se restauró.
    pub snapshot_id: String,
    /// Target sobre el que se escribió; `null` si la fila no resolvió ninguno.
    pub target_id: Option<String>,
    /// Nombre del target; `null` si la fila no tiene target.
    pub target_name: Option<String>,
    /// Tipo del target, con el mismo texto del cable que publica `/api/v1/targets`; `null` si la
    /// fila no tiene target.
    pub target_type: Option<String>,
    /// Estado, en el texto del `CHECK`.
    pub status: String,
    /// Motivo del fallo; `null` si no falló.
    pub error_message: Option<String>,
    /// Cuándo empezó, en RFC3339.
    pub started_at: String,
    /// Cuándo terminó, en RFC3339; `null` si sigue en curso.
    pub finished_at: Option<String>,
}

impl RestauracionDelCable {
    /// Convierte una fila listada en su vista del cable.
    ///
    /// Las fechas son las que la base guardó: la capa de acceso las normaliza a UTC **al
    /// enlazarlas**, así que el formateador no tiene que corregir nada. El tipo del target sale por
    /// [`texto_del_tipo`], la **única** boca que traduce el enumerado al vocabulario del cable, para
    /// que este listado y el de `/api/v1/targets` no puedan decir cosas distintas del mismo tipo.
    fn desde(fila: &RestauracionListada) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.restauracion.id.hyphenated().to_string(),
            snapshot_id: fila.restauracion.snapshot_id.clone(),
            target_id: fila
                .restauracion
                .target_id
                .map(|id| id.hyphenated().to_string()),
            target_name: fila.target_name.clone(),
            target_type: fila.target_type.map(|tipo| texto_del_tipo(tipo).to_owned()),
            status: fila.restauracion.status.texto().to_owned(),
            error_message: fila.restauracion.error_message.clone(),
            started_at: formatear(fila.restauracion.started_at)?,
            finished_at: fila.restauracion.finished_at.map(formatear).transpose()?,
        })
    }
}

/// Formatea una fecha como RFC3339, que es el formato del cable.
fn formatear(fecha: time::OffsetDateTime) -> Result<String, AppError> {
    fecha.format(&Rfc3339).map_err(|error| {
        AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
    })
}

/// El `data` del listado: la ventana y el total.
///
/// Es un objeto y no un array pelado —como el histórico de copias— porque el contrato publica **una
/// página acotada con el total**: un array no tiene dónde llevar el total, y el total es lo que
/// permite al panel saber cuántas páginas hay sin pedirlas todas. La ventana y su total viajan
/// juntos y salen de los mismos filtros, así que no pueden acotar por cosas distintas.
#[derive(Debug, Serialize)]
struct ListadoDelCable {
    /// Las restauraciones de la ventana, de la más reciente a la más antigua.
    restorations: Vec<RestauracionDelCable>,
    /// Cuántas filas cumplen el filtro en total, sin aplicar la ventana.
    total: i64,
}

/// `GET /api/v1/restorations` — el histórico de restauraciones, con filtros, orden y ventana.
///
/// Una tabla sin filas es un listado de cero elementos, **no** un recurso ausente: contesta `200`
/// con una página vacía y su total a cero.
pub async fn listado(
    State(estado): State<AppState>,
    Query(filtros): Query<FiltrosDelCable>,
) -> Result<ApiResponse, AppError> {
    let filtro = filtros.validar()?;

    let PaginaDeRestauraciones { filas, total } =
        estado.db.restauraciones.listar_filtrado(&filtro).await?;

    let restorations = filas
        .iter()
        .map(RestauracionDelCable::desde)
        .collect::<Result<_, _>>()?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "restauraciones listadas",
        contenido(&ListadoDelCable {
            restorations,
            total,
        })?,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::StatusCode;
    use time::macros::datetime;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::db::entities::{
        AltaDeRestauracion, AltaDeTarget, Estado, Restauraciones, Target, TipoDeTarget,
    };
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        DirectorioDePrueba, base_con_clave, body_json, config_con_dsn, estado_de_prueba, get,
        router_oidc,
    };

    /// El estado con su pool y su clave, para sembrar antes de pedir.
    ///
    /// Devuelve el `AppState` y la guarda del directorio temporal, que tiene que seguir **viva**
    /// hasta el final de la prueba: si se suelta, el directorio se borra y la base desaparece bajo
    /// los pies del router.
    async fn estado_con_base() -> (AppState, DirectorioDePrueba) {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, Arc::clone(&clave)),
        );

        (estado, directorio)
    }

    /// Da de alta un target de directorio y lo devuelve.
    async fn sembrar_target(estado: &AppState, nombre: &str) -> Target {
        estado
            .db
            .targets
            .alta(AltaDeTarget {
                name: nombre.to_owned(),
                target_type: TipoDeTarget::Directory,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: Some(format!("/srv/{nombre}")),
                db_path: None,
            })
            .await
            .expect("el target de prueba se da de alta")
    }

    /// Da de alta una restauración y le fija el estado, que el alta deja siempre en `pending`.
    async fn sembrar_restauracion(
        restauraciones: &Restauraciones,
        target_id: Option<Uuid>,
        status: Estado,
        snapshot_id: &str,
        started_at: OffsetDateTime,
    ) -> Uuid {
        let mut fila = restauraciones
            .alta(AltaDeRestauracion {
                snapshot_id: snapshot_id.to_owned(),
                target_id,
                started_at,
            })
            .await
            .expect("la restauración de prueba se da de alta");

        // El ciclo de vida lo escribe el motor por su camino; aquí se fija el desenlace con el
        // mismo `actualizar` que él usa.
        fila.status = status;
        fila.finished_at = matches!(status, Estado::Success | Estado::Failed)
            .then_some(started_at + Duration::seconds(30));

        assert!(
            restauraciones
                .actualizar(&fila)
                .await
                .expect("la actualización no puede fallar"),
            "la fila recién dada de alta tiene que poder actualizarse"
        );

        fila.id
    }

    /// Escenario 1.8a — El listado devuelve las restauraciones, la más reciente primero.
    ///
    /// Cubre `api-resources`, escenario «El listado devuelve las restauraciones»: cada fila publica
    /// su `status` y sus fechas, y el orden es por fecha de comienzo **descendente**. Se afirma
    /// contra el `snapshot_id`, que distingue una fila de otra.
    #[tokio::test]
    async fn el_listado_devuelve_las_restauraciones_con_sus_datos() {
        let (estado, _directorio) = estado_con_base().await;
        let target = sembrar_target(&estado, "target-del-historico").await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        sembrar_restauracion(
            &estado.db.restauraciones,
            Some(target.id),
            Estado::Success,
            "instantanea-vieja-0000000000000000000001",
            base,
        )
        .await;
        sembrar_restauracion(
            &estado.db.restauraciones,
            Some(target.id),
            Estado::Failed,
            "instantanea-reciente-00000000000000000001",
            base + Duration::hours(1),
        )
        .await;

        let respuesta = get(api_router(estado), "/api/v1/restorations").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado contesta {status}: {cuerpo}"
        );

        assert_eq!(cuerpo["data"]["total"], 2, "el total son las dos filas");

        let filas = cuerpo["data"]["restorations"]
            .as_array()
            .expect("el listado tiene que llegar como un array");

        assert_eq!(filas.len(), 2, "sin filtros salen las dos filas");

        assert_eq!(
            filas[0]["snapshot_id"], "instantanea-reciente-00000000000000000001",
            "el orden es descendente por la fecha de comienzo y la más reciente va primero: {cuerpo}"
        );
        assert_eq!(
            filas[1]["snapshot_id"], "instantanea-vieja-0000000000000000000001",
            "la más antigua tiene que quedar la última: {cuerpo}"
        );

        for clave in ["id", "status", "started_at", "finished_at"] {
            assert!(
                filas[0].get(clave).is_some(),
                "cada fila tiene que publicar `{clave}`: {cuerpo}"
            );
        }

        assert_eq!(filas[0]["status"], "failed", "y el estado es el de la fila");

        assert_eq!(
            filas[0]["target_id"].as_str(),
            Some(target.id.hyphenated().to_string().as_str()),
            "cada fila tiene que publicar el target sobre el que se restauró: {cuerpo}"
        );
        assert_eq!(
            filas[0]["target_name"].as_str(),
            Some("target-del-historico"),
            "y su nombre, resuelto por el vínculo: {cuerpo}"
        );
        assert_eq!(
            filas[0]["target_type"].as_str(),
            Some("directory"),
            "y su tipo, con el mismo texto del cable que `/api/v1/targets`: {cuerpo}"
        );

        let started_at = filas[0]["started_at"]
            .as_str()
            .expect("la fecha de comienzo tiene que ser una cadena");
        assert!(
            started_at.ends_with('Z') || started_at.contains('+'),
            "la fecha tiene que publicarse en RFC3339: {started_at}"
        );
    }

    /// Escenario 1.8b — El filtro por estado acota el listado.
    ///
    /// Cubre `api-resources`, escenario «El filtro por estado acota el listado»: con `status=failed`
    /// solo vuelven las fallidas. Se siembran estados **distintos** para que un filtro que no
    /// acotara fuera indistinguible de uno que sí.
    #[tokio::test]
    async fn el_filtro_por_estado_acota_el_listado() {
        let (estado, _directorio) = estado_con_base().await;
        let target = sembrar_target(&estado, "target-del-filtro").await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        for (indice, estado_fila) in [
            Estado::Success,
            Estado::Failed,
            Estado::Failed,
            Estado::Running,
        ]
        .into_iter()
        .enumerate()
        {
            sembrar_restauracion(
                &estado.db.restauraciones,
                Some(target.id),
                estado_fila,
                &format!("instantanea-{indice:02}-00000000000000000000000000"),
                base + Duration::minutes(indice as i64),
            )
            .await;
        }

        let respuesta = get(api_router(estado), "/api/v1/restorations?status=failed").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el filtro por estado contesta {status}: {cuerpo}"
        );

        assert_eq!(cuerpo["data"]["total"], 2, "solo dos filas fallaron");

        let filas = cuerpo["data"]["restorations"]
            .as_array()
            .expect("el listado es un array");

        assert_eq!(filas.len(), 2, "el filtro tiene que acotar a las fallidas");

        for fila in filas {
            assert_eq!(
                fila["status"], "failed",
                "el filtro no puede traer otra cosa"
            );
        }
    }

    /// Escenario 1.8c — El filtro por `target_id` acota el listado.
    ///
    /// Se siembran dos targets porque sin el segundo un filtro que no acotara pasaría inadvertido.
    #[tokio::test]
    async fn el_filtro_por_target_acota_el_listado() {
        let (estado, _directorio) = estado_con_base().await;
        let target_a = sembrar_target(&estado, "target-a").await;
        let target_b = sembrar_target(&estado, "target-b").await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        sembrar_restauracion(
            &estado.db.restauraciones,
            Some(target_a.id),
            Estado::Success,
            "instantanea-de-a-000000000000000000000000",
            base,
        )
        .await;
        sembrar_restauracion(
            &estado.db.restauraciones,
            Some(target_b.id),
            Estado::Success,
            "instantanea-de-b-000000000000000000000000",
            base + Duration::minutes(1),
        )
        .await;

        let respuesta = get(
            api_router(estado),
            &format!("/api/v1/restorations?target_id={}", target_b.id),
        )
        .await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el filtro por target contesta {status}: {cuerpo}"
        );

        assert_eq!(cuerpo["data"]["total"], 1, "el target B solo tiene una");

        let filas = cuerpo["data"]["restorations"]
            .as_array()
            .expect("el listado es un array");

        assert_eq!(filas.len(), 1, "el filtro tiene que acotar");

        assert_eq!(
            filas[0]["target_id"].as_str(),
            Some(target_b.id.hyphenated().to_string().as_str()),
            "y la que vuelve es la del target B"
        );
    }

    /// Escenario 1.8d — El listado pagina de forma acotada y la siguiente página trae el resto.
    ///
    /// Cubre `api-resources`, escenario «El listado pagina de forma acotada»: con la ventana llena
    /// el **total** no encoge —es lo que promete cuántas quedan— y el `offset` **avanza** en vez de
    /// repetir la primera página.
    #[tokio::test]
    async fn el_listado_pagina_de_forma_acotada_con_el_total() {
        let (estado, _directorio) = estado_con_base().await;
        let target = sembrar_target(&estado, "target-de-la-ventana").await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        for indice in 0..3_i64 {
            sembrar_restauracion(
                &estado.db.restauraciones,
                Some(target.id),
                Estado::Success,
                &format!("instantanea-{indice:02}-00000000000000000000000000"),
                base + Duration::minutes(indice),
            )
            .await;
        }

        let primera = get(
            api_router(estado.clone()),
            "/api/v1/restorations?limit=2&offset=0",
        )
        .await;
        let status = primera.status();
        let cuerpo = body_json(primera).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la primera página contesta {status}: {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["total"], 3,
            "el total no puede encogerse con la ventana"
        );

        let filas = cuerpo["data"]["restorations"]
            .as_array()
            .expect("el listado es un array");
        assert_eq!(filas.len(), 2, "la ventana acota a dos");

        let primera_fila = filas[0]["snapshot_id"].clone();

        let segunda = get(api_router(estado), "/api/v1/restorations?limit=2&offset=2").await;
        let status = segunda.status();
        let cuerpo = body_json(segunda).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la segunda página contesta {status}: {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["total"], 3,
            "el total no cambia entre páginas"
        );

        let filas = cuerpo["data"]["restorations"]
            .as_array()
            .expect("el listado es un array");
        assert_eq!(filas.len(), 1, "la segunda página trae la restante");

        assert_ne!(
            filas[0]["snapshot_id"], primera_fila,
            "el `offset` tiene que avanzar y no repetir la primera página"
        );
    }

    /// Escenario 1.8e — Una fila sin target publica `target_name`/`target_type` a `null`.
    ///
    /// Es la mitad del `LEFT JOIN` que mira el hueco: una restauración sin target —el contrato lo
    /// admite— no puede inventarse un nombre ni un tipo, y tampoco puede desaparecer del listado por
    /// no tenerlos. Se siembra también una **con** target, para que un `null` sistemático no pase
    /// por bueno.
    #[tokio::test]
    async fn una_fila_sin_target_publica_su_target_a_null() {
        let (estado, _directorio) = estado_con_base().await;
        let target = sembrar_target(&estado, "target-con-nombre").await;

        let base = datetime!(2026-10-25 03:00:00 UTC);

        sembrar_restauracion(
            &estado.db.restauraciones,
            Some(target.id),
            Estado::Success,
            "instantanea-con-target-000000000000000000",
            base,
        )
        .await;
        sembrar_restauracion(
            &estado.db.restauraciones,
            None,
            Estado::Success,
            "instantanea-sin-target-000000000000000000",
            base + Duration::minutes(1),
        )
        .await;

        let respuesta = get(api_router(estado), "/api/v1/restorations").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado contesta {status}: {cuerpo}"
        );

        let filas = cuerpo["data"]["restorations"]
            .as_array()
            .expect("el listado es un array");

        assert_eq!(filas.len(), 2, "las dos filas tienen que seguir listándose");

        let sin_target = &filas[0];
        let con_target = &filas[1];

        assert_eq!(
            sin_target["target_id"],
            serde_json::Value::Null,
            "la fila sin target no puede publicar un identificador: {cuerpo}"
        );
        assert_eq!(
            sin_target["target_name"],
            serde_json::Value::Null,
            "ni un nombre inventado: {cuerpo}"
        );
        assert_eq!(
            sin_target["target_type"],
            serde_json::Value::Null,
            "ni un tipo inventado: {cuerpo}"
        );

        assert_eq!(
            con_target["target_name"].as_str(),
            Some("target-con-nombre"),
            "la fila con target sí trae su nombre: {cuerpo}"
        );
    }

    /// Escenario 1.8f — Un `status` que no es uno de los cuatro contesta `400`.
    ///
    /// Un valor fuera del vocabulario es una petición mal formada, no un listado vacío: acotar por
    /// un estado que no existe y devolver cero filas haría creer que no hay ninguna, cuando lo que
    /// hay es una errata. El motivo **nombra el campo** y no repite el valor.
    #[tokio::test]
    async fn un_status_fuera_del_vocabulario_contesta_400() {
        let (estado, _directorio) = estado_con_base().await;

        let respuesta = get(api_router(estado), "/api/v1/restorations?status=terminada").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un estado fuera del vocabulario tiene que contestar 400: {cuerpo}"
        );

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|motivo| motivo.contains("status") && !motivo.contains("terminada")),
            "el 400 tiene que nombrar el campo y no repetir el valor: {cuerpo}"
        );
    }

    /// Escenario 1.8g — El listado exige sesión cuando el modo la exige.
    ///
    /// Cubre `api-resources`, escenario «El listado exige sesión cuando el modo la exige»: la ruta
    /// cuelga de la mitad protegida —**no** de la lista blanca—, así que sin cookie el *gate* corta
    /// antes del manejador y contesta `401` con la envoltura.
    #[tokio::test]
    async fn el_listado_exige_sesion_en_modo_oidc() {
        let respuesta = get(router_oidc().await, "/api/v1/restorations").await;
        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "sin sesión el listado tiene que exigirla y contestó {status}: {cuerpo}"
        );

        assert_eq!(cuerpo["status"], 401, "la envoltura del 401: {cuerpo}");
    }
}
