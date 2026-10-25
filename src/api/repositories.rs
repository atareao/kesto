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
//! AVISO — las **cinco** rutas de `/repositories` están implementadas (módulo 2 del change
//! `api-endpoints`): el listado, el alta, la lectura, la actualización y el borrado. Las de
//! `/targets`, `/jobs` y `/executions` llegan con el resto del módulo 2 y con el módulo 3, y
//! hasta entonces no existen.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::contenido;
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

/// Convierte el identificador de la ruta en un `Uuid`, o contesta `400`.
///
/// El mensaje **nombra el campo y no repite el valor**: el segmento de la ruta llegó por el
/// cable, y la regla de la casa es que un `400` diga qué campo está mal, nunca lo que traía.
fn identificador(id: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(id).map_err(|_| {
        AppError::InvalidInput("el campo `id` de la ruta no tiene la forma de un UUID".to_owned())
    })
}

/// El `404` de una fila que no está, con el mismo texto en las tres rutas del identificador.
fn repositorio_ausente() -> AppError {
    AppError::NotFound("no existe ningún repositorio con ese identificador".to_owned())
}

/// Resuelve el secreto de una actualización: el ausente conserva el almacenado y el vacío se
/// rechaza.
///
/// Los dos desenlaces son distintos a propósito. Ausente es «no lo toques», y es el único
/// camino que puede recorrer un panel que no tiene el valor, porque la API nunca lo devuelve;
/// vacío es «déjalo en blanco», y borrar una credencial no es una operación que esta
/// superficie ofrezca. El rechazo se contesta **antes** de escribir y nombrando el campo, y
/// usa el mismo texto que la traducción de `DbError::SecretoVacio` para que el cliente vea un
/// solo mensaje: la capa de acceso sigue rechazando el vacío por su cuenta, y esa es la red
/// de seguridad; esto es la validación previa que el contrato pide al manejador.
fn resolver_secreto(
    nuevo: Option<SecretString>,
    almacenado: SecretString,
    campo: &'static str,
) -> Result<SecretString, AppError> {
    match nuevo {
        None => Ok(almacenado),
        Some(valor) if valor.expose().is_empty() => Err(AppError::InvalidInput(format!(
            "el campo `{campo}` no puede ir vacío"
        ))),
        Some(valor) => Ok(valor),
    }
}

/// `GET /api/v1/repositories` — el listado.
///
/// Una tabla sin filas es un listado de cero elementos, **no** un recurso ausente: contesta
/// `200` con un array vacío, no `404` y no un `data` nulo. El array va vacío y no en `null`
/// porque `Data::None` está para las respuestas que no tienen contenido —el `DELETE`— y un
/// listado sí lo tiene: el suyo, aunque sea de longitud cero. Un `null` obligaría al panel a
/// tratar «no hay repositorios» como un error, que es el estado normal de un Kesto recién
/// instalado.
pub async fn listado(State(state): State<AppState>) -> Result<ApiResponse, AppError> {
    let filas = state.db.repositorios.listar().await?;

    let vistas: Vec<RepositorioDelCable> = filas
        .iter()
        .map(RepositorioDelCable::desde)
        .collect::<Result<_, _>>()?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "repositorios listados",
        contenido(&vistas)?,
    ))
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
    let uuid = identificador(&id)?;

    let Some(fila) = state.db.repositorios.leer(&uuid).await? else {
        return Err(repositorio_ausente());
    };

    let vista = RepositorioDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "repositorio encontrado",
        contenido(&vista)?,
    ))
}

/// `PUT /api/v1/repositories/{id}` — la actualización, que es reemplazo completo.
///
/// Los cinco campos del cuerpo sustituyen a los de la fila; el identificador y la fecha de
/// alta **no** se reescriben —el identificador no es un campo editable y `created_at` es la
/// fecha de nacimiento de la fila, no un dato del formulario—. Y un secreto **ausente**
/// conserva el almacenado mientras que uno **vacío** se rechaza: la diferencia importa
/// porque el panel **no tiene** el valor —la API no devuelve los secretos—, así que sin la
/// primera mitad una edición de cualquier otro campo borraría la credencial, y sin la
/// segunda un campo en blanco la borraría sin decirlo.
///
/// Si la fila desaparece entre la lectura y la escritura —otra petición la borró en el
/// hueco—, contesta el mismo `404` que si no hubiera estado nunca: es la carrera que la
/// lectura previa no puede cerrar, y el desenlace tiene que ser el del contrato.
pub async fn actualizar(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(cuerpo): Json<AltaDeRepositorioDelCable>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(actual) = state.db.repositorios.leer(&uuid).await? else {
        return Err(repositorio_ausente());
    };

    let fila = Repositorio {
        id: actual.id,
        name: cuerpo.name,
        endpoint: cuerpo.endpoint,
        bucket: cuerpo.bucket,
        region: cuerpo.region,
        access_key_id: cuerpo.access_key_id,
        secret_access_key: resolver_secreto(
            cuerpo.secret_access_key,
            actual.secret_access_key,
            "secret_access_key",
        )?,
        passphrase: resolver_secreto(cuerpo.passphrase, actual.passphrase, "passphrase")?,
        created_at: actual.created_at,
    };

    if !state.db.repositorios.actualizar(&fila).await? {
        return Err(repositorio_ausente());
    }

    let vista = RepositorioDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "repositorio actualizado",
        contenido(&vista)?,
    ))
}

/// `DELETE /api/v1/repositories/{id}` — el borrado, que contesta `200` con `data` nulo.
///
/// `data` nulo y **no** `204`: la envoltura de la API lleva `{status, message, data}` en
/// **todas** sus respuestas, y un `204` no tiene cuerpo —sería la única respuesta capaz de
/// romperla, y el panel tendría una rama especial para un solo endpoint—.
///
/// Borrar un repositorio **arrastra sus tareas y con ellas sus ejecuciones**, por el
/// `ON DELETE CASCADE` del §3: el histórico de copias desaparece con el repositorio que lo
/// produjo. Queda escrito aquí para que nadie lo descubra vaciando una tabla por curiosidad.
/// El `false` de la capa de acceso —«no estaba»— se traduce a `404` porque el contrato del
/// `DELETE` lo fija así, por encima de la lectura de «quien pidió borrar algo que ya no está
/// consiguió lo que quería»: el recurso pedido no existe, y eso es lo que el código dice.
pub async fn borrar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    if !state.db.repositorios.borrar(&uuid).await? {
        return Err(repositorio_ausente());
    }

    Ok(ApiResponse::new(
        StatusCode::OK,
        "repositorio borrado",
        Data::None,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::{StatusCode, header};
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        base_con_clave, body_json, body_text, capturar_log, config_con_dsn, delete,
        estado_desarrollo, get, post_crudo, post_json, put_json,
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

    /// 1.5 — Las cinco rutas del recurso contestan su código.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «Las cinco rutas del recurso contestan
    /// su código»). Las cinco peticiones salen del **mismo** router —el de producción, con
    /// su estado— porque lo que se afirma es la superficie del recurso y no cada manejador
    /// por separado: un `PUT` que contestara el código del alta, o un `DELETE` que no
    /// dejara la fila fuera, se ven aquí y no en una prueba que abriera un router nuevo por
    /// petición. El orden importa: el alta va primero porque sin su `id` las otras tres
    /// rutas del identificador no tienen a qué apuntar.
    #[tokio::test]
    async fn las_cinco_rutas_del_recurso_contestan_su_codigo() {
        let router = api_router(estado_desarrollo().await).clone();

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-5"),
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

        let respuesta = get(router.clone(), "/api/v1/repositories").await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "el listado tiene que contestar 200 con el array de repositorios"
        );

        let respuesta = get(router.clone(), &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "la lectura de una fila que se acaba de dar de alta tiene que contestar 200"
        );

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/repositories/{id}"),
            &cuerpo_de_alta("repo-1-5-modificado"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "la actualización de una fila existente tiene que contestar 200"
        );

        let respuesta = delete(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "el borrado de una fila existente tiene que contestar 200"
        );
    }

    /// 1.7 — El listado contesta `200` con un array en `data`.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «El listado contesta 200 con un array
    /// en data»). Se afirman las tres cosas a la vez porque el `200` solo no dice nada: que
    /// `data` sea un **array** —la forma del recurso en el listado— y que ese array traiga
    /// la fila que se acaba de dar de alta. Sin la segunda mitad, un listado que contestara
    /// `200` con `data: []` pasaría la prueba sin listar nada, que es justo el defecto que
    /// la prueba tiene que dejar candado.
    #[tokio::test]
    async fn el_listado_contesta_200_con_un_array_en_data() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-7"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder listar nada"
        );

        let respuesta = get(router, "/api/v1/repositories").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200; contestó {status} con {cuerpo}"
        );

        let filas = cuerpo["data"].as_array().unwrap_or_else(|| {
            panic!("el listado tiene que traer un array en `data`, y llegó {cuerpo}")
        });

        assert!(
            filas.iter().any(|fila| fila["name"] == "repo-1-7"),
            "el listado tiene que incluir la fila que se acaba de dar de alta, y llegó {cuerpo}"
        );
    }

    /// 1.8 — Una tabla vacía contesta `200` con un array vacío.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «Una tabla vacía contesta 200 con un
    /// array vacío»). El porqué es semántico y no de estilo: una tabla sin filas es un
    /// listado de cero elementos, no un recurso ausente. Un listado que contestara `404`
    /// —o `null` en `data`— obligaría al panel a tratar «no hay repositorios» como un
    /// error, que es el estado normal de un Kesto recién instalado.
    #[tokio::test]
    async fn una_tabla_vacia_contesta_200_con_array_vacio() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = get(router, "/api/v1/repositories").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "una tabla sin filas es un listado vacío, no un recurso ausente: contestó \
             {status} con {cuerpo}"
        );

        let filas = cuerpo["data"]
            .as_array()
            .unwrap_or_else(|| panic!("el listado vacío tiene que traer un array en `data`, no un `null`, y llegó {cuerpo}"));

        assert!(
            filas.is_empty(),
            "sin altas, el listado tiene que traer un array vacío, y llegó {cuerpo}"
        );
    }

    /// 1.11 — El `DELETE` contesta `200` con `data` nulo y la lectura posterior da `404`.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «El DELETE contesta 200 con data nulo y
    /// la lectura posterior 404»). El `assert_ne!` contra `204` es el candado del SRS: un
    /// `204` no tiene cuerpo por definición, así que rompería la envoltura `{status,
    /// message, data}` que devuelven todas las rutas de la API, y el panel tendría que
    /// tratar esa ruta como un caso aparte. La lectura posterior es la otra mitad: un `200`
    /// que no borrara nada también pasaría la primera aserción.
    #[tokio::test]
    async fn el_delete_contesta_200_con_data_nulo_y_la_lectura_posterior_404() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-11"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder borrar nada"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = delete(router.clone(), &format!("/api/v1/repositories/{id}")).await;

        let status = respuesta.status();

        assert_ne!(
            status,
            StatusCode::NO_CONTENT,
            "el borrado no puede contestar 204: un 204 no tiene cuerpo y rompería la \
             envoltura de la API"
        );

        assert_eq!(
            status,
            StatusCode::OK,
            "el borrado tiene que contestar 200 con la envoltura; contestó {status}"
        );

        let cuerpo = body_json(respuesta).await;

        assert!(
            cuerpo["data"].is_null(),
            "el borrado no tiene recurso que devolver: `data` tiene que ser nulo, y llegó \
             {cuerpo}"
        );

        assert_eq!(
            cuerpo["status"], 200,
            "la envoltura tiene que repetir el código en `status`: {cuerpo}"
        );

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::NOT_FOUND,
            "la fila borrada no puede seguir leyéndose: la lectura tiene que contestar 404"
        );
    }

    /// 1.12 — El `PUT` sobre una fila inexistente contesta `404`.
    ///
    /// Cubre `api-resources`, requisito 3 (escenario «El PUT sobre una fila inexistente
    /// contesta 404»). El identificador es un UUID **bien formado** que no existe —y no una
    /// cadena cualquiera— a propósito: así el `404` lo decide la ausencia de la fila y no el
    /// rechazo del identificador, que sería otro caso. La cuenta por SQL directo es la mitad
    /// que impide el peor desenlace: una actualización que, al no encontrar la fila,
    /// insertara una nueva contestando `404` de propina.
    #[tokio::test]
    async fn el_put_sobre_una_fila_inexistente_contesta_404() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        let id = Uuid::new_v4().to_string();

        let respuesta = put_json(
            api_router(estado),
            &format!("/api/v1/repositories/{id}"),
            &cuerpo_de_alta("repo-1-12"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "un PUT sobre una fila que no existe tiene que contestar 404; contestó {status} \
             con {cuerpo}"
        );

        assert_eq!(
            cuerpo["status"], 404,
            "la envoltura tiene que repetir el código en `status`: {cuerpo}"
        );

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 404 tiene que explicar que no hay fila que actualizar: {cuerpo}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories")
            .fetch_one(&pool)
            .await
            .expect("la cuenta de filas tiene que poder leerse del fichero de la base");

        assert_eq!(
            filas, 0,
            "un PUT sobre una fila inexistente no puede escribir ninguna fila: quedaron \
             {filas}"
        );
    }

    /// 1.32 — Un `PUT` sin el secreto conserva el almacenado.
    ///
    /// Cubre `api-resources`, requisito 3 (escenario «Un PUT sin el secreto conserva el
    /// almacenado»). El porqué es que el panel **no tiene** el valor: la API nunca devuelve
    /// los secretos —los dos viajan como `SecretString`, que no implementa `Serialize`—,
    /// así que un `PUT` que reemplazara la credencial con lo que llega la borraría sin que
    /// nadie pudiera evitarlo. Lo que se compara aquí es el valor **en claro**, descifrando
    /// el sobre de la columna, y no los bytes del sobre: la capa de acceso vuelve a cifrar
    /// en cada escritura, con nonce aleatorio por valor, así que el sobre cambia siempre
    /// aunque el secreto no cambie y comparar sobres mediría lo que **no** es. La otra
    /// mitad de la prueba es que la actualización sí se aplicó: sin eso, un `PUT` que no
    /// tocara la fila también «conservaría» la passphrase.
    #[tokio::test]
    async fn un_put_sin_passphrase_conserva_la_almacenada() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        let router = api_router(estado);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-32"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder actualizar nada"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let sobre_antes: String =
            sqlx::query_scalar("SELECT passphrase FROM repositories WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        let passphrase_antes = clave
            .descifrar(&sobre_antes)
            .expect("el alta tiene que haber guardado la passphrase cifrada y descifrable")
            .expose()
            .to_owned();

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/repositories/{id}"),
            &serde_json::json!({
                "name": "repo-1-32-modificado",
                "endpoint": "https://s3.example.test/repo-1-32-modificado",
                "bucket": "cubo-modificado",
                "region": "eu-west-1",
                "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la actualización de una fila existente tiene que contestar 200; contestó \
             {status} con {cuerpo}"
        );

        let sobre_despues: String =
            sqlx::query_scalar("SELECT passphrase FROM repositories WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila actualizada tiene que seguir en el fichero de la base");

        let passphrase_despues = clave
            .descifrar(&sobre_despues)
            .expect("la actualización tiene que dejar la passphrase cifrada y descifrable")
            .expose()
            .to_owned();

        assert_eq!(
            passphrase_despues, passphrase_antes,
            "un PUT que no informa la passphrase no puede borrarla: el panel no tiene el \
             valor —la API no lo devuelve— y sin esta regla reescribiría la credencial con \
             nada"
        );

        let nombre: String = sqlx::query_scalar("SELECT name FROM repositories WHERE id = ?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("la fila actualizada tiene que seguir en el fichero de la base");

        assert_eq!(
            nombre, "repo-1-32-modificado",
            "el PUT tiene que haber aplicado los campos que sí informó"
        );

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "la fila actualizada tiene que seguir leyéndose por la API"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["name"], "repo-1-32-modificado",
            "la lectura tiene que devolver el nombre que escribió el PUT: {cuerpo}"
        );
    }

    /// 1.33 — Un `PUT` con el secreto vacío se rechaza sin cambiar el almacenado.
    ///
    /// Cubre `api-resources`, requisito 3 (escenario «Un PUT con el secreto vacío se
    /// rechaza»). El porqué: un campo presente y vacío significa «pon el secreto en blanco»,
    /// no «no lo toques» —para eso está omitirlo, que es el escenario de la prueba 1.32—,
    /// así que el manejador lo rechaza nombrando el campo en el `message`. Y la segunda
    /// mitad es la que impide el rechazo a medias: el `400` tiene que llegar **antes** de
    /// escribir, porque un rechazo que ya hubiera guardado el vacío dejaría la credencial
    /// perdida con una respuesta de error.
    #[tokio::test]
    async fn un_put_con_la_passphrase_vacia_se_rechaza_sin_cambiar_la_almacenada() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = AppState::new(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        )
        .expect("el estado se construye sobre la base de prueba");

        let router = api_router(estado);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-33"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder actualizar nada"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let sobre_antes: String =
            sqlx::query_scalar("SELECT passphrase FROM repositories WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        let passphrase_antes = clave
            .descifrar(&sobre_antes)
            .expect("el alta tiene que haber guardado la passphrase cifrada y descifrable")
            .expose()
            .to_owned();

        let mut cuerpo_de_la_actualizacion = cuerpo_de_alta("repo-1-33");
        cuerpo_de_la_actualizacion["passphrase"] = serde_json::Value::String(String::new());

        let respuesta = put_json(
            router,
            &format!("/api/v1/repositories/{id}"),
            &cuerpo_de_la_actualizacion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un secreto presente y vacío es un intento de dejarlo en blanco, y eso se \
             rechaza: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("passphrase"),
            "el rechazo tiene que nombrar el campo que lo provoca, y el mensaje fue \
             {mensaje:?}"
        );

        let sobre_despues: String =
            sqlx::query_scalar("SELECT passphrase FROM repositories WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila rechazada tiene que seguir en el fichero de la base");

        let passphrase_despues = clave
            .descifrar(&sobre_despues)
            .expect("el rechazo no puede haber dejado la passphrase indescifrable")
            .expose()
            .to_owned();

        assert_eq!(
            passphrase_despues, passphrase_antes,
            "el rechazo no puede haber cambiado nada: la passphrase almacenada tiene que \
             seguir siendo la del alta"
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
