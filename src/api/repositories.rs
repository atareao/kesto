//! Las rutas de `/api/v1/repositories`: las cinco del recurso del §5.
//!
//! El listado, el alta, la lectura por identificador, la actualización y el borrado. Los
//! tres últimos **no están en el §5** y se declaran como añadido al SRS, con el motivo
//! escrito en la spec: la capa de acceso ya implementa `leer`, `actualizar` y `borrar` y
//! hoy solo las pruebas las ejercitan, así que sin ellos un repositorio con una errata en
//! la clave de acceso no se puede corregir desde el panel.
//!
//! Los tipos de este módulo son los **del cable**, no las entidades de
//! [`crate::db::entities`], y llevan el sufijo `DelCable` para que no se confundan con
//! los de la capa de acceso, que describen el mismo dato en otro vocabulario. Los nombres
//! de los campos son los del §3 en `snake_case`, el identificador viaja como texto en su
//! forma canónica con guiones y las fechas, como cadena RFC3339.
//!
//! AVISO — los cinco manejadores están **sin implementar** en esta fase (RED del módulo
//! 1): el andamiaje declara la superficie y el tipo del cuerpo para que las pruebas
//! puedan recorrer el camino real —el router de producción, en el modo que corresponda—
//! y el rojo sea «no implementado». Los cuerpos llegan con el módulo de los recursos.

use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;
use serde::Deserialize;

use crate::api::response::ApiResponse;
use crate::error::AppError;
use crate::secret::SecretString;
use crate::state::AppState;

/// Las cinco rutas del recurso, para colgar de la mitad de negocio.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/repositories", get(listado).post(alta))
        .route(
            "/api/v1/repositories/{id}",
            get(leer).put(actualizar).delete(borrar),
        )
}

/// El cuerpo del alta de un repositorio, tal y como llega por el cable.
///
/// Los campos son los del §3. Los dos secretos van como [`SecretString`], que **no
/// implementa `Serialize`**: eso es lo que hace que una respuesta que los llevara no
/// compile, y no una convención que alguien tenga que recordar.
///
/// Los dos son `Option` y no obligatorios **en el tipo** a propósito: quien decide si un
/// alta sin `passphrase` es válida es el manejador, que es el único sitio donde se puede
/// contestar un `400` nombrando el campo. Si los exigiera el tipo, el rechazo llegaría
/// con el texto del intérprete de `serde` —que puede ecoar el valor— en vez de con la
/// envoltura de la API.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AltaDeRepositorioDelCable {
    /// Nombre del repositorio.
    pub name: String,
    /// Endpoint del servicio S3.
    pub endpoint: String,
    /// Cubo de destino.
    pub bucket: String,
    /// Región del cubo.
    pub region: String,
    /// Identificador público de la credencial.
    pub access_key_id: String,
    /// Secreto de la credencial; se cifra antes de escribirlo.
    pub secret_access_key: Option<SecretString>,
    /// Passphrase del repositorio Rustic; se cifra antes de escribirlo.
    pub passphrase: Option<SecretString>,
}

/// `GET /api/v1/repositories` — el listado.
pub async fn listado(State(_state): State<AppState>) -> Result<ApiResponse, AppError> {
    todo!("el módulo 2 del change `api-endpoints` implementa el listado")
}

/// `POST /api/v1/repositories` — el alta.
pub async fn alta(
    State(_state): State<AppState>,
    axum::Json(_cuerpo): axum::Json<AltaDeRepositorioDelCable>,
) -> Result<ApiResponse, AppError> {
    todo!("el módulo 2 del change `api-endpoints` implementa el alta")
}

/// `GET /api/v1/repositories/{id}` — la lectura de una fila.
pub async fn leer(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
) -> Result<ApiResponse, AppError> {
    todo!("el módulo 2 del change `api-endpoints` implementa la lectura por identificador")
}

/// `PUT /api/v1/repositories/{id}` — la actualización, que es reemplazo completo.
pub async fn actualizar(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
    axum::Json(_cuerpo): axum::Json<AltaDeRepositorioDelCable>,
) -> Result<ApiResponse, AppError> {
    todo!("el módulo 2 del change `api-endpoints` implementa la actualización")
}

/// `DELETE /api/v1/repositories/{id}` — el borrado, que contesta `200` con `data` nulo.
pub async fn borrar(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
) -> Result<ApiResponse, AppError> {
    todo!("el módulo 2 del change `api-endpoints` implementa el borrado")
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::api::api_router;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        base_con_clave, body_json, config_con_dsn, estado_desarrollo, get, post_json,
    };

    /// 1.2 — Dos peticiones seguidas al **mismo** router ven lo mismo.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «Dos peticiones seguidas al mismo
    /// router ven lo mismo»). Una petición da de alta el recurso y la siguiente pide su
    /// lectura, las dos **por el router de producción**: si cada petición abriera su
    /// propia base —o si el router no compartiera el estado—, la lectura contestaría
    /// `404` sobre una fila que se acaba de escribir.
    #[tokio::test]
    async fn dos_peticiones_seguidas_al_mismo_router_ven_la_misma_fila() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-2"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con el recurso creado"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_ne!(
            respuesta.status(),
            StatusCode::NOT_FOUND,
            "la segunda petición no ve la fila que escribió la primera: cada una abrió su base"
        );
        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["name"], "repo-1-2",
            "la lectura tiene que devolver la fila que escribió el alta: {cuerpo}"
        );
    }

    /// 1.3 — La escritura fue a **la** base del proceso y no a una copia.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «La escritura fue a la base del
    /// proceso y no a una copia») y el requisito del delta `db`. Lo que da de alta el
    /// router se relee **con SQL directo sobre el mismo fichero**, sin pasar por la API
    /// ni por el estado: lo que se afirma es una propiedad del fichero con el que se
    /// abrió el estado, y una consulta que pasara por la capa de acceso no la probaría.
    #[tokio::test]
    async fn la_escritura_por_el_router_esta_en_el_fichero_de_la_base() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        )
        .expect("el estado se construye sobre la base de prueba");

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-3"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con el recurso creado"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let nombre: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id = ?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        assert_eq!(
            nombre, "repo-1-3",
            "la fila del fichero no tiene los valores que informó la petición"
        );

        let endpoint: String =
            sqlx::query_scalar("SELECT endpoint FROM repositories WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        assert_eq!(
            endpoint, "https://s3.example.test/repo-1-3",
            "la fila del fichero no tiene los valores que informó la petición"
        );
    }

    /// El cuerpo de un alta válida, con valores distinguibles por su nombre.
    fn cuerpo_de_alta(nombre: &str) -> serde_json::Value {
        serde_json::json!({
            "name": nombre,
            "endpoint": format!("https://s3.example.test/{nombre}"),
            "bucket": format!("cubo-{nombre}"),
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "AKIAIOSFODNN7EXAMPLE-secreto-de-prueba",
            "passphrase": "passphrase-distinguible-00000000000000000000000001",
        })
    }
}
