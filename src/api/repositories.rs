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
//! AVISO — el **alta** y la **lectura** están implementadas (GREEN del módulo 1 del change
//! `api-endpoints`); el **listado**, la **actualización** y el **borrado** siguen sin
//! implementarse y llegan con el **módulo 2** del mismo change, el de los recursos, que es
//! el que escribe sus pruebas. Hasta entonces sus cuerpos se quedan como están a propósito:
//! escribirlos ahora sería código que ninguna prueba pide y que nadie habría medido contra
//! el §5.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data};
use crate::db::entities::{AltaDeRepositorio, Repositorio};
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
/// contestar un `400` nombrando el campo. El rechazo del extractor sí sale por la
/// envoltura de la API, pero con un mensaje propio de la clase del fallo —«el cuerpo no
/// tiene los campos que la ruta espera»—, que no nombra el campo que falta; por eso el
/// `400` que sí lo nombra lo tiene que contestar el manejador, y por eso los dos secretos
/// llegan como `Option`.
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

/// Un repositorio tal y como viaja por el cable: lo que devuelve la API.
///
/// **De los secretos sale un booleano, no el valor.** El panel necesita saber si hay
/// credencial guardada para pintar el campo del formulario como configurado y no obligar a
/// reescribirla en cada edición, pero devolver el valor convertiría cada `GET` —y cada
/// listado— en una fuga que viaja al navegador y queda en su historial.
///
/// **El booleano es un campo derivado, y es un añadido al SRS.** El §3 no tiene ninguna
/// columna `has_secret_access_key` y el cuerpo del alta informa `secret_access_key`, así
/// que este campo no lleva el nombre de nada que exista: lo exige el requisito «un secreto
/// no vuelve al cliente», y es el único campo de esta vista que no refleja una columna.
/// Su nombre antepone `has_` al de la columna para que se lea como un sí/no **sobre**
/// ella —«el `secret_access_key` de la tabla, dicho como sí/no»— y no como una segunda
/// credencial.
///
/// **Y se calcula sobre la fila, no se pone a `true` fijo.** La capa de acceso rechaza el
/// secreto vacío al escribir, así que en la práctica vale `true` en todas las filas que
/// pasan por la API; se calcula igualmente para que el campo signifique lo que dice y no
/// una constante que un día mentiría.
///
/// **Las fechas van como cadena** porque `time` viaja con `formatting`: el formato lo fija
/// el servidor, en RFC3339 —lo que `OffsetDateTime::parse(…, &Rfc3339)` relee—, en vez de
/// dejarlo en manos del serializador.
#[derive(Debug, Serialize)]
pub struct RepositorioDelCable {
    /// Identificador en su forma canónica con guiones, la que escribe la capa de acceso.
    pub id: String,
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
    /// Si hay `secret_access_key` guardada. El valor no sale del servidor.
    pub has_secret_access_key: bool,
    /// Si hay `passphrase` guardada. El valor no sale del servidor.
    pub has_passphrase: bool,
    /// Fecha de alta, en RFC3339.
    pub created_at: String,
}

impl RepositorioDelCable {
    /// Vuelca una fila de la capa de acceso en su forma del cable.
    ///
    /// Es el único sitio donde los secretos de una fila se miran, y se miran solo para
    /// decidir un booleano: el contenido se consulta por `expose()` —el único camino por el
    /// que sale del tipo— y nunca se copia a la vista.
    fn desde(fila: &Repositorio) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.id.hyphenated().to_string(),
            name: fila.name.clone(),
            endpoint: fila.endpoint.clone(),
            bucket: fila.bucket.clone(),
            region: fila.region.clone(),
            access_key_id: fila.access_key_id.clone(),
            has_secret_access_key: !fila.secret_access_key.expose().is_empty(),
            has_passphrase: !fila.passphrase.expose().is_empty(),
            created_at: fila.created_at.format(&Rfc3339).map_err(|error| {
                AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
            })?,
        })
    }
}

/// Serializa una vista del cable al `data` de la envoltura.
///
/// Existe porque `serde_json::to_value` devuelve `Result` y un manejador no puede abortar
/// con `panic!`: un fallo aquí es un `500` con la causa al registro, que es lo que el resto
/// de la API hace con lo que no debería ocurrir.
///
/// El mensaje no lleva el valor serializado, y eso **no** es una garantía de `serde_json`
/// sino de estos tipos: `serde_json` sí mete en su error el texto que le dé un serializador
/// propio —`ser::Error::custom`—, así que la propiedad se sostiene mientras ninguna vista
/// del cable declare un `serialize_with` que formatee un valor. La red de verdad es de
/// antes: una respuesta que llevara un secreto no compila, porque
/// [`crate::secret::SecretString`] no implementa `Serialize`.
fn contenido<T: Serialize>(vista: &T) -> Result<Data, AppError> {
    serde_json::to_value(vista)
        .map(Data::Some)
        .map_err(|error| AppError::Other(format!("la respuesta no se pudo serializar: {error}")))
}

/// `GET /api/v1/repositories` — el listado.
pub async fn listado(State(_state): State<AppState>) -> Result<ApiResponse, AppError> {
    todo!("el módulo 2 del change `api-endpoints` implementa el listado")
}

/// `POST /api/v1/repositories` — el alta, que contesta `201` con el recurso creado.
///
/// Los dos secretos del cuerpo son `Option`, y aquí se exigen: las columnas del §3 son
/// `NOT NULL`, así que un alta sin ellos no tiene fila que escribir. El «ausente =
/// consérvalo» es una regla del `PUT`, no del alta. El rechazo se contesta nombrando el
/// campo y **sin haber tocado la base**.
pub async fn alta(
    State(state): State<AppState>,
    Json(cuerpo): Json<AltaDeRepositorioDelCable>,
) -> Result<ApiResponse, AppError> {
    let Some(secret_access_key) = cuerpo.secret_access_key else {
        return Err(AppError::InvalidInput(
            "el campo `secret_access_key` es obligatorio en un alta".to_owned(),
        ));
    };

    let Some(passphrase) = cuerpo.passphrase else {
        return Err(AppError::InvalidInput(
            "el campo `passphrase` es obligatorio en un alta".to_owned(),
        ));
    };

    let fila = state
        .db
        .repositorios
        .alta(AltaDeRepositorio {
            name: cuerpo.name,
            endpoint: cuerpo.endpoint,
            bucket: cuerpo.bucket,
            region: cuerpo.region,
            access_key_id: cuerpo.access_key_id,
            secret_access_key,
            passphrase,
        })
        .await?;

    let vista = RepositorioDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::CREATED,
        "repositorio creado",
        contenido(&vista)?,
    ))
}

/// `GET /api/v1/repositories/{id}` — la lectura de una fila, o `404` si no está.
///
/// Un identificador que no sea un UUID es un `400` que nombra el campo de la ruta y **no**
/// repite el valor: el segmento llegó por el cable, y la casa tiene la regla de que un
/// `400` no filtra el valor. Por lo mismo, el `404` no ecoa el identificador.
pub async fn leer(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| {
        AppError::InvalidInput("el campo `id` de la ruta no tiene la forma de un UUID".to_owned())
    })?;

    let Some(fila) = state.db.repositorios.leer(&uuid).await? else {
        return Err(AppError::NotFound(
            "no existe ningún repositorio con ese identificador".to_owned(),
        ));
    };

    let vista = RepositorioDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "repositorio encontrado",
        contenido(&vista)?,
    ))
}

/// `PUT /api/v1/repositories/{id}` — la actualización, que es reemplazo completo.
pub async fn actualizar(
    State(_state): State<AppState>,
    Path(_id): Path<String>,
    Json(_cuerpo): Json<AltaDeRepositorioDelCable>,
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
    use axum::http::{StatusCode, header};

    use crate::api::api_router;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        base_con_clave, body_json, body_text, capturar_log, config_con_dsn, estado_desarrollo, get,
        post_crudo, post_json,
    };

    /// Credencial distinguible que las pruebas de rechazo de cuerpo no pueden ver salir.
    ///
    /// Es larga y con una coletilla propia a propósito: una cadena corta —una que pudiera
    /// aparecer en un mensaje de `serde` por casualidad— daría un `contains` con falso
    /// positivo, y lo que estas pruebas afirman es exactamente que este valor concreto,
    /// que llega en el cuerpo de la petición, no sale del proceso.
    const CREDENCIAL_DE_RECHAZO: &str = "AKIAIOSFODNN7EXAMPLE-credencial-distinguible-0001";

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

    /// 1.23 — Un cuerpo que no es JSON contesta `400` con la envoltura.
    ///
    /// Cubre `api-shell`, requisito «Un cuerpo o una consulta que no se pueden interpretar
    /// contestan 400 con la envoltura, y su texto no sale del proceso» (escenario «Un
    /// cuerpo que no es JSON contesta `400` con la envoltura»). El porqué de lo que se
    /// afirma aquí es de **seguridad**, no de estilo, y éste es el defecto que la prueba
    /// deja candado: el extractor por defecto de axum rechaza **antes** de entrar al
    /// manejador y contesta en texto plano con el código del intérprete dentro, que puede
    /// ecoar el valor del campo. Lo intercepta [`Json`] —el extractor propio de este
    /// crate—, que traduce el rechazo a la envoltura; y por eso la prueba mira el tipo de
    /// contenido **además** del código: un `400` en texto plano seguiría publicando el
    /// texto del intérprete.
    #[tokio::test]
    async fn un_cuerpo_que_no_es_json_contesta_400_con_la_envoltura() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_crudo(router, "/api/v1/repositories", "esto no es json").await;

        let status = respuesta.status();
        let tipo_de_contenido = respuesta
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|valor| valor.to_str().ok())
            .map(ToOwned::to_owned);

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un cuerpo que no se puede interpretar tiene que contestar 400 por la envoltura, \
             no el código con el que lo rechaza el intérprete"
        );

        let tipo_de_contenido = tipo_de_contenido
            .expect("la respuesta tiene que declarar un tipo de contenido, no salir sin cabecera");

        assert!(
            tipo_de_contenido.starts_with("application/json"),
            "el rechazo no puede salir en texto plano: publicaría el texto del intérprete, \
             que ecoa el valor del campo; el tipo de contenido fue {tipo_de_contenido}"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["status"], 400,
            "la envoltura tiene que repetir el código en `status`: {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            !mensaje.is_empty(),
            "la envoltura tiene que explicar el rechazo, no traer un mensaje vacío: {cuerpo}"
        );
    }

    /// 1.24 — El rechazo de un cuerpo no publica la credencial en ninguna parte.
    ///
    /// Cubre `api-shell`, requisito «Un cuerpo o una consulta que no se pueden interpretar
    /// contestan 400 con la envoltura, y su texto no sale del proceso» (escenario «El
    /// rechazo de un cuerpo no publica la credencial en ninguna parte»). Los dos vectores
    /// de abajo están medidos **en vivo** contra el binario: el cuerpo entero es la
    /// credencial, y lo contesta con `invalid type: string "AKIA…", expected struct …`; y
    /// la credencial como nombre de un campo que no existe, que lo contesta con
    /// `unknown field 'AKIA…'`. Los dos textos llevan la credencial dentro, así que la
    /// prueba afirma dos cosas a la vez: que el rechazo contesta por la envoltura y que su
    /// texto **no** lleva el valor. Y sobre el registro, las dos mitades del requisito: la
    /// credencial no está, pero sí queda constancia de **que** una petición no se pudo
    /// interpretar —sin el texto—, que es lo que permite diagnosticar un panel roto sin
    /// escribir credenciales en los registros.
    #[tokio::test]
    async fn el_rechazo_de_un_cuerpo_no_publica_la_credencial_en_ninguna_parte() {
        let (buffer, _guard) = capturar_log();

        let router = api_router(estado_desarrollo().await);

        let vectores = [
            (
                "el cuerpo entero es la credencial",
                format!("\"{CREDENCIAL_DE_RECHAZO}\""),
            ),
            (
                "la credencial es el nombre de un campo que no existe",
                format!("{{\"{CREDENCIAL_DE_RECHAZO}\":1}}"),
            ),
        ];

        for (motivo, cuerpo_de_la_peticion) in vectores {
            let respuesta = post_crudo(
                router.clone(),
                "/api/v1/repositories",
                &cuerpo_de_la_peticion,
            )
            .await;

            let status = respuesta.status();
            let texto = body_text(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "el rechazo tiene que salir por la envoltura ({motivo}); contestó {status} \
                 con {texto}"
            );

            assert!(
                !texto.contains(CREDENCIAL_DE_RECHAZO),
                "el rechazo publicó la credencial al cliente ({motivo}): {texto}"
            );
        }

        let registrado = buffer.texto();

        assert!(
            !registrado.contains(CREDENCIAL_DE_RECHAZO),
            "el rechazo escribió la credencial en el registro ({registrado})"
        );

        assert!(
            registrado.contains("no se pudo interpretar"),
            "tiene que quedar registrado que una petición no se pudo interpretar —sin su \
             texto—, y el registro capturado fue: {registrado}"
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
