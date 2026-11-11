//! Las rutas de `/api/v1/targets`: las cinco del recurso del §5.
//!
//! Es el recurso de `backup_targets`, la tabla con más columnas anulables del §3: un target de
//! tipo `directory` no tiene contenedor ni credenciales, y uno de base de datos no tiene ruta de
//! host. En esa tabla `NULL` y la cadena vacía **no** son lo mismo —`NULL` es «no lo informes», y
//! la columna guarda nulo para lo ausente—, así que lo anulable viaja como `Option` de punta a
//! punta y nunca se rellena con `""`: una cadena vacía donde el esquema espera nulo haría que el
//! motor ejecutase un `docker exec` con el nombre de un contenedor que es la cadena vacía.
//!
//! La coherencia entre `target_type` y el resto de los campos la exige **aquí** el manejador,
//! porque la tabla del §3 la acepta incoherente: sus columnas son anulables y su único `CHECK`
//! restringe el tipo, así que un `directory` con `container_name` entra en la base sin protestar
//! y el fallo aparecería en la primera copia, hablando de un contenedor que no existe en vez de un
//! formulario mal rellenado. Validar en el borde convierte ese fallo tardío en un `400` que nombra
//! el campo.
//!
//! Los tipos de este módulo son los **del cable** (sufijo `DelCable`) y `db_password` **no sale**
//! nunca: se representa con `has_db_password`, un booleano derivado que el §3 no tiene como
//! columna. `target_type` viaja como texto —`directory`, `mariadb`, `postgresql`—, que es lo que
//! el `CHECK` acepta y lo que hace legible la tabla con `sqlite3`.
//!
//! Las cinco rutas del recurso —el listado, el alta, la lectura, la actualización y el borrado—
//! están **implementadas** en el módulo 2 del change `api-endpoints`.

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
use crate::db::entities::{AltaDeTarget, Target, TipoDeTarget};
use crate::error::AppError;
use crate::secret::SecretString;
use crate::state::AppState;

/// Las cinco rutas del recurso, para colgar de la mitad de negocio.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/targets", get(listado).post(alta))
        .route(
            "/api/v1/targets/{id}",
            get(leer).put(actualizar).delete(borrar),
        )
}

/// El cuerpo del alta de un target, tal y como llega por el cable.
///
/// `target_type` viaja como **texto** —`directory`, `mariadb` o `postgresql`—, que es
/// exactamente lo que el `CHECK` del §3 acepta en la columna y lo que hace legible el §3 con
/// `sqlite3`; el enum de la capa de acceso no viaja porque no implementa `serde`, y el
/// vocabulario del cable es de esta capa. Los tres campos de texto opcionales son `Option`
/// porque `NULL` y cadena vacía **no** son lo mismo, y `db_password` es `Option<SecretString>`
/// porque `SecretString` no implementa `Serialize`: una respuesta que lo llevara no compila.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AltaDeTargetDelCable {
    /// Nombre del target.
    pub name: String,
    /// Tipo de fuente: `directory`, `mariadb` o `postgresql`.
    pub target_type: String,
    /// Contenedor del que extraer el volcado; no lo admite un `directory`.
    pub container_name: Option<String>,
    /// Usuario de la base; no lo admite un `directory`.
    pub db_user: Option<String>,
    /// Contraseña de la base; es el tercer secreto del §3 y no lo admite un `directory`.
    pub db_password: Option<SecretString>,
    /// Base concreta del volcado; `None` significa el volcado global.
    pub db_name: Option<String>,
    /// Ruta del host; solo la admite un `directory`.
    pub host_path: Option<String>,
}

/// Un target tal y como viaja por el cable: lo que devuelve la API.
///
/// **De `db_password` sale un booleano, no el valor**, como en los dos secretos de
/// `/repositories`: el panel necesita saber si hay credencial guardada para pintar el campo
/// como configurado, y devolver el valor convertiría cada `GET` en una fuga que viaja al
/// navegador. Es un campo **derivado** y un añadido al SRS: el §3 no tiene ninguna columna
/// `has_db_password`, y su nombre antepone `has_` al de la columna para leerse como un sí/no
/// sobre ella.
#[derive(Debug, Serialize)]
pub struct TargetDelCable {
    /// Identificador en su forma canónica con guiones.
    pub id: String,
    /// Nombre del target.
    pub name: String,
    /// Tipo de fuente, como texto del cable.
    pub target_type: String,
    /// Contenedor del volcado, o `null`.
    pub container_name: Option<String>,
    /// Usuario de la base, o `null`.
    pub db_user: Option<String>,
    /// Si hay `db_password` guardada. El valor no sale del servidor.
    pub has_db_password: bool,
    /// Base concreta del volcado, o `null` para el volcado global.
    pub db_name: Option<String>,
    /// Ruta del host, o `null`.
    pub host_path: Option<String>,
    /// Fecha de alta, en RFC3339.
    pub created_at: String,
}

impl TargetDelCable {
    /// Vuelca una fila de la capa de acceso en su forma del cable.
    ///
    /// `has_db_password` **se calcula** sobre el dato leído, y no se pone a `true` fijo: la capa de
    /// acceso rechaza el secreto vacío al escribir, así que en la práctica es `true` en las filas
    /// que pasan por la API; calcularlo sobre lo leído hace que el campo signifique lo que dice
    /// —«hay contraseña guardada»— y no una constante que un día mentiría.
    fn desde(fila: &Target) -> Result<Self, AppError> {
        Ok(Self {
            id: fila.id.hyphenated().to_string(),
            name: fila.name.clone(),
            target_type: texto_del_tipo(fila.target_type).to_owned(),
            container_name: fila.container_name.clone(),
            db_user: fila.db_user.clone(),
            has_db_password: fila
                .db_password
                .as_ref()
                .is_some_and(|secreto| !secreto.expose().is_empty()),
            db_name: fila.db_name.clone(),
            host_path: fila.host_path.clone(),
            created_at: fila.created_at.format(&Rfc3339).map_err(|error| {
                AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
            })?,
        })
    }
}

/// El texto del cable de un tipo de target.
///
/// El cable habla el mismo texto que la columna del §3 guarda —su `CHECK` acepta `mariadb`,
/// `postgresql` y `directory`, en minúsculas—, y el vocabulario es de esta capa: el enum de
/// la capa de acceso no implementa `serde` y no viaja. Las pruebas fijan los tres literales,
/// así que un cambio de vocabulario en la base no puede pasar inadvertido en el cable.
fn texto_del_tipo(tipo: TipoDeTarget) -> &'static str {
    match tipo {
        TipoDeTarget::Mariadb => "mariadb",
        TipoDeTarget::Postgresql => "postgresql",
        TipoDeTarget::Directory => "directory",
    }
}

/// El tipo del cable, convertido al de la capa de acceso.
///
/// Un valor que no sea uno de los tres contesta `400` **nombrando el campo** y sin repetir el
/// valor: el texto llegó por el cable, y la regla de la casa es nombrar el campo, nunca lo que
/// traía.
fn tipo_del_cable(texto: &str) -> Result<TipoDeTarget, AppError> {
    match texto {
        "mariadb" => Ok(TipoDeTarget::Mariadb),
        "postgresql" => Ok(TipoDeTarget::Postgresql),
        "directory" => Ok(TipoDeTarget::Directory),
        _ => Err(AppError::InvalidInput(
            "el campo `target_type` no admite ese valor: se espera `directory`, `mariadb` o \
             `postgresql`"
                .to_owned(),
        )),
    }
}

/// Convierte el identificador de la ruta en un `Uuid`, o contesta `400`.
fn identificador(id: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(id).map_err(|_| {
        AppError::InvalidInput("el campo `id` de la ruta no tiene la forma de un UUID".to_owned())
    })
}

/// El `404` de una fila que no está, con el mismo texto en las tres rutas del identificador.
fn target_ausente() -> AppError {
    AppError::NotFound("no existe ningún target con ese identificador".to_owned())
}

/// El `400` de un campo obligatorio que no viene.
fn falta(campo: &str) -> AppError {
    AppError::InvalidInput(format!(
        "el campo `{campo}` es obligatorio para este tipo de target"
    ))
}

/// El `400` de un campo que viene vacío.
///
/// Un campo informado y vacío **no** es «no lo informes» —para eso está omitirlo—, sino una
/// cadena vacía, y la columna del §3 guarda nulo para lo ausente: admitirla escribiría una
/// cadena vacía donde el esquema espera nulo, y el motor ejecutaría un `docker exec` con el
/// nombre de un contenedor que es la cadena vacía.
fn vacio(campo: &str) -> AppError {
    AppError::InvalidInput(format!("el campo `{campo}` no puede ir vacío"))
}

/// El `400` de un campo que este tipo de target no admite.
fn no_admitido(campo: &str) -> AppError {
    AppError::InvalidInput(format!(
        "el campo `{campo}` no se admite para este tipo de target"
    ))
}

/// Devuelve el texto de un campo obligatorio, o el `400` que lo nombra.
fn exigir_presente(campo: &str, valor: Option<&str>) -> Result<String, AppError> {
    match valor {
        Some(texto) if !texto.is_empty() => Ok(texto.to_owned()),
        Some(_) => Err(vacio(campo)),
        None => Err(falta(campo)),
    }
}

/// Devuelve el texto de un campo opcional, o el `400` si viene vacío.
///
/// Ausente se queda en `None` —que es lo que la columna guarda como `NULL`— y presente pero
/// vacío se rechaza: son dos cosas distintas, y la segunda no tiene significado en el §3.
fn opcional_no_vacio(campo: &str, valor: Option<&str>) -> Result<Option<String>, AppError> {
    match valor {
        None => Ok(None),
        Some("") => Err(vacio(campo)),
        Some(texto) => Ok(Some(texto.to_owned())),
    }
}

/// El `400` si un campo que este tipo no admite viene informado.
fn prohibir(campo: &str, valor: Option<&str>) -> Result<(), AppError> {
    match valor {
        None => Ok(()),
        Some(_) => Err(no_admitido(campo)),
    }
}

/// Comprueba la coherencia del cuerpo con su tipo y lo convierte en el alta de la capa de
/// acceso.
///
/// **La coherencia se exige aquí** porque la tabla del §3 la acepta incoherente: sus columnas
/// son anulables y su único `CHECK` restringe `target_type`, así que un `directory` con
/// `container_name` y sin `host_path` es una fila que la base guarda tan contenta y que el
/// motor no sabrá usar; el fallo aparecería en la primera copia, hablando de un contenedor que
/// no existe en vez de un formulario mal rellenado. El borde HTTP es el único sitio donde se
/// conoce la intención de quien escribe.
///
/// `almacenada` es el secreto que la fila ya tenía, y **solo lo consulta la rama de base de
/// datos**: con él, un `PUT` que no informe `db_password` conserva el que estaba —el panel no
/// tiene el valor, porque la API no lo devuelve—; en un alta llega `None`, y entonces el
/// secreto es obligatorio. Un `directory` no lo mira nunca, porque no admite credenciales: si
/// el tipo nuevo es `directory`, el secreto se descarta aunque la fila lo tuviera.
fn alta_desde(
    cuerpo: &AltaDeTargetDelCable,
    almacenada: Option<&SecretString>,
) -> Result<AltaDeTarget, AppError> {
    let target_type = tipo_del_cable(&cuerpo.target_type)?;

    match target_type {
        TipoDeTarget::Directory => {
            let host_path = exigir_presente("host_path", cuerpo.host_path.as_deref())?;
            prohibir("container_name", cuerpo.container_name.as_deref())?;
            prohibir("db_user", cuerpo.db_user.as_deref())?;
            prohibir("db_name", cuerpo.db_name.as_deref())?;

            if cuerpo.db_password.is_some() {
                return Err(no_admitido("db_password"));
            }

            Ok(AltaDeTarget {
                name: cuerpo.name.clone(),
                target_type,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: Some(host_path),
            })
        }
        TipoDeTarget::Mariadb | TipoDeTarget::Postgresql => {
            let container_name =
                exigir_presente("container_name", cuerpo.container_name.as_deref())?;
            let db_user = exigir_presente("db_user", cuerpo.db_user.as_deref())?;

            let db_password = match &cuerpo.db_password {
                Some(secreto) if secreto.expose().is_empty() => return Err(vacio("db_password")),
                Some(secreto) => secreto.clone(),
                None => match almacenada {
                    Some(secreto) => secreto.clone(),
                    None => return Err(falta("db_password")),
                },
            };

            let db_name = opcional_no_vacio("db_name", cuerpo.db_name.as_deref())?;

            prohibir("host_path", cuerpo.host_path.as_deref())?;

            Ok(AltaDeTarget {
                name: cuerpo.name.clone(),
                target_type,
                container_name: Some(container_name),
                db_user: Some(db_user),
                db_password: Some(db_password),
                db_name,
                host_path: None,
            })
        }
    }
}

/// `GET /api/v1/targets` — el listado.
///
/// Una tabla sin filas es un listado de cero elementos y **no** un recurso ausente: contesta `200`
/// con el array vacío en `data`, que es lo que el panel puede pintar; un `404` diría que el
/// recurso no existe, y `/targets` existe siempre.
pub async fn listado(State(state): State<AppState>) -> Result<ApiResponse, AppError> {
    let filas = state.db.targets.listar().await?;

    let vistas: Vec<TargetDelCable> = filas
        .iter()
        .map(TargetDelCable::desde)
        .collect::<Result<_, _>>()?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "targets listados",
        contenido(&vistas)?,
    ))
}

/// `POST /api/v1/targets` — el alta.
///
/// El `201` trae el recurso con el `id` que **generó la capa de acceso**: quien llama no lo
/// informa —el tipo del cable no tiene ese campo—, así que el identificador que ve es el que
/// quedó escrito, no uno que hubiera propuesto el cliente.
pub async fn alta(
    State(state): State<AppState>,
    Json(cuerpo): Json<AltaDeTargetDelCable>,
) -> Result<ApiResponse, AppError> {
    let alta = alta_desde(&cuerpo, None)?;

    let fila = state.db.targets.alta(alta).await?;
    let vista = TargetDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::CREATED,
        "target creado",
        contenido(&vista)?,
    ))
}

/// `GET /api/v1/targets/{id}` — la lectura.
///
/// Un identificador que no tiene la forma de un UUID es un `400` que nombra el campo de la ruta y
/// **no** repite el valor: el segmento llegó por el cable, y la regla de la casa es nombrar el
/// campo, nunca lo que traía. Un UUID bien formado que no está en la tabla es un `404`.
pub async fn leer(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(fila) = state.db.targets.leer(&uuid).await? else {
        return Err(target_ausente());
    };

    let vista = TargetDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "target encontrado",
        contenido(&vista)?,
    ))
}

/// `PUT /api/v1/targets/{id}` — la actualización, que es **reemplazo completo**.
///
/// El `id` y `created_at` no se reescriben —el identificador viene de la ruta y la fecha del alta
/// no es un dato del cuerpo—, y el resto de los campos se sustituyen por los que traiga el cuerpo.
/// Un `db_password` **ausente conserva el almacenado**, porque la API no lo devuelve y el panel no
/// lo tiene; uno presente y vacío se rechaza, como en el alta. Si la fila desaparece entre la
/// lectura y la escritura —dos `PUT` a la vez, o un `DELETE` en medio— contesta el mismo `404` que
/// si no hubiera estado nunca.
pub async fn actualizar(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(cuerpo): Json<AltaDeTargetDelCable>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(actual) = state.db.targets.leer(&uuid).await? else {
        return Err(target_ausente());
    };

    // El secreto almacenado se le pasa al validador para que un `PUT` que no lo informe lo
    // conserve; y si el tipo nuevo es un `directory`, esa rama no lo mira y la credencial se
    // descarta con el resto de los campos de base de datos.
    let alta = alta_desde(&cuerpo, actual.db_password.as_ref())?;

    let fila = Target {
        id: actual.id,
        name: alta.name,
        target_type: alta.target_type,
        container_name: alta.container_name,
        db_user: alta.db_user,
        db_password: alta.db_password,
        db_name: alta.db_name,
        host_path: alta.host_path,
        created_at: actual.created_at,
    };

    if !state.db.targets.actualizar(&fila).await? {
        return Err(target_ausente());
    }

    let vista = TargetDelCable::desde(&fila)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "target actualizado",
        contenido(&vista)?,
    ))
}

/// `DELETE /api/v1/targets/{id}` — el borrado.
///
/// Contesta `200` con `data` nulo y **no** `204`: un `204` no tiene cuerpo, y rompería la
/// envoltura única de la API —el `status` y el `message` que el panel lee en todas las demás
/// respuestas— justo en la ruta que menos lo espera.
pub async fn borrar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    if !state.db.targets.borrar(&uuid).await? {
        return Err(target_ausente());
    }

    Ok(ApiResponse::new(
        StatusCode::OK,
        "target borrado",
        Data::None,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::StatusCode;

    use crate::api::api_router;
    use crate::state::EstadoDeBase;
    use crate::test_support::{
        base_con_clave, body_json, body_text, config_con_dsn, delete, estado_de_prueba,
        estado_desarrollo, get, post_json, put_json,
    };

    /// Credencial distinguible que las pruebas de rechazo de cuerpo no pueden ver salir.
    ///
    /// Es larga y con una coletilla propia a propósito: una cadena corta —una que pudiera
    /// aparecer en un mensaje de `serde` por casualidad— daría un `contains` con falso
    /// positivo, y lo que estas pruebas afirman es exactamente que este valor concreto,
    /// que llega en el cuerpo de la petición, no sale del proceso.
    const CREDENCIAL_DE_PRUEBA: &str = "AKIAIOSFODNN7EXAMPLE-credencial-del-target-0001";

    /// La contraseña de la base que mandan las pruebas que necesitan un alta válida.
    ///
    /// Distinguible por la misma razón que [`CREDENCIAL_DE_PRUEBA`]: es el tercer secreto del §3
    /// y hay que poder afirmar que **este** valor no viaja en la respuesta.
    const CONTRASENA_DE_TARGET: &str = "contrasena-distinguible-0000000000000001";

    /// El cuerpo de un alta válida de target de base de datos, con el nombre puesto.
    ///
    /// Hermano del `cuerpo_de_alta` de `repositories.rs`. Los valores no informados —los que un
    /// `directory` sí lleva, como `host_path`— se omiten a propósito: la petición no los envía,
    /// así que su columna tiene que quedar nula.
    fn cuerpo_de_target(nombre: &str) -> serde_json::Value {
        serde_json::json!({
            "name": nombre,
            "target_type": "mariadb",
            "container_name": "contenedor-de-pruebas",
            "db_user": "usuario-de-pruebas",
            "db_password": CONTRASENA_DE_TARGET,
            "db_name": "base-de-pruebas",
        })
    }

    /// 1.4 — Las cinco rutas del recurso contestan su código.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «Las cinco rutas del recurso contestan su
    /// código»). Las cinco peticiones van **por el router de producción** y encadenadas: el alta
    /// devuelve el identificador que consumen las cuatro siguientes, así que una ruta mal colgada
    /// —o un método que no esté montado— se ve aquí y no en la primera petición que la use de
    /// verdad. Los códigos son los del recurso hermano, para que las dos mitades de la API se
    /// comporten igual.
    #[tokio::test]
    async fn las_cinco_rutas_del_recurso_contestan_su_codigo() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &cuerpo_de_target("target-1-4"),
        )
        .await;

        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con el recurso creado, y contestó {status}"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = get(router.clone(), "/api/v1/targets").await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200 con un array en `data`, y contestó {status}"
        );
        let _ = body_json(respuesta).await;

        let respuesta = get(router.clone(), &format!("/api/v1/targets/{id}")).await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "la lectura de una fila recién creada tiene que contestar 200, y contestó {status}"
        );
        let _ = body_json(respuesta).await;

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/targets/{id}"),
            &cuerpo_de_target("target-1-4-modificado"),
        )
        .await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "la actualización tiene que contestar 200 con el recurso, y contestó {status}"
        );
        let _ = body_json(respuesta).await;

        let respuesta = delete(router, &format!("/api/v1/targets/{id}")).await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "el borrado tiene que contestar 200 con `data` nulo, y contestó {status}"
        );
        let _ = body_json(respuesta).await;
    }

    /// 1.16t — El alta de un target no trae la contraseña y sí el booleano.
    ///
    /// Cubre `api-security` (el tercer secreto del §3 no sale del servidor por ninguna ruta). La
    /// contraseña de la base se afirma ausente **por el texto crudo del cuerpo**, no solo por el
    /// JSON: una respuesta que la llevara dentro de un mensaje de error o de un campo anidado
    /// tendría la clave `db_password` a `null` y aun así estaría filtrando. En su lugar viaja
    /// `has_db_password`, que es lo que el panel necesita para pintar el campo como configurado.
    /// La lectura repite las tres cosas para que el filtrado no valga solo en el alta.
    #[tokio::test]
    async fn el_alta_de_un_target_no_trae_la_contrasena_y_si_el_booleano() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &cuerpo_de_target("target-1-16"),
        )
        .await;

        let status = respuesta.status();
        let crudo = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con el recurso creado, y contestó {status} con {crudo}"
        );

        let cuerpo: serde_json::Value = serde_json::from_str(&crudo)
            .expect("el alta tiene que contestar un objeto JSON con la envoltura");

        assert_eq!(
            cuerpo["data"]["has_db_password"], true,
            "el alta tiene que marcar que hay contraseña guardada: {cuerpo}"
        );
        assert!(
            cuerpo["data"]["db_password"].is_null(),
            "la contraseña no puede salir del servidor, ni siquiera con otro nombre: {cuerpo}"
        );
        assert!(
            !crudo.contains(CONTRASENA_DE_TARGET),
            "el cuerpo crudo de la respuesta no puede contener la contraseña que se mandó"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = get(router, &format!("/api/v1/targets/{id}")).await;

        let status = respuesta.status();
        let crudo = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la lectura del target recién creado tiene que contestar 200, y contestó {status} con \
             {crudo}"
        );

        let cuerpo: serde_json::Value = serde_json::from_str(&crudo)
            .expect("la lectura tiene que contestar un objeto JSON con la envoltura");

        assert_eq!(
            cuerpo["data"]["has_db_password"], true,
            "la lectura tiene que marcar que hay contraseña guardada: {cuerpo}"
        );
        assert!(
            cuerpo["data"]["db_password"].is_null(),
            "la lectura tampoco puede devolver la contraseña: {cuerpo}"
        );
        assert!(
            !crudo.contains(CONTRASENA_DE_TARGET),
            "el cuerpo crudo de la lectura no puede contener la contraseña que se mandó"
        );
        assert_eq!(
            cuerpo["data"]["target_type"], "mariadb",
            "el tipo viaja como el texto que acepta el `CHECK` del §3: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["db_name"], "base-de-pruebas",
            "la base concreta informada tiene que volver tal cual: {cuerpo}"
        );
    }

    /// 1.26 — Un `directory` con `container_name` se rechaza nombrando el campo.
    ///
    /// Cubre la mitad de «prohibir» de la coherencia entre `target_type` y los campos de base de
    /// datos. La tabla del §3 acepta esa fila —todas sus columnas son anulables y su único `CHECK`
    /// restringe solo el tipo—, así que si la API no la rechaza, el error no aparece hasta la
    /// primera copia, y aparece hablando de un contenedor que no existe en vez de un formulario
    /// mal rellenado. Se afirma además que la petición rechazada **no deja fila**: un rechazo que
    /// escribiera primero sería peor que no rechazar.
    #[tokio::test]
    async fn un_directory_con_container_name_se_rechaza_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let cuerpo_de_la_peticion = serde_json::json!({
            "name": "target-1-26",
            "target_type": "directory",
            "host_path": "/srv/datos",
            "container_name": "contenedor-que-no-toca",
        });

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/targets",
            &cuerpo_de_la_peticion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `directory` que informa un contenedor es un formulario incoherente, no un servidor \
             roto: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("container_name"),
            "el rechazo tiene que nombrar el campo que lo provoca, y el mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.26b — Un `directory` sin `host_path` se rechaza nombrando el campo.
    ///
    /// Cubre la otra mitad del requisito: un `directory` **exige** `host_path`. Es la mitad de
    /// «exigir» que la 1.26 no toca —allí se prohíbe un campo de más, aquí falta uno obligatorio—
    /// y sin ella un directorio sin ruta entraría en la base con la columna anulable a `NULL`, que
    /// es una copia que no copia nada.
    #[tokio::test]
    async fn un_directory_sin_host_path_se_rechaza_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let cuerpo_de_la_peticion = serde_json::json!({
            "name": "target-1-26b",
            "target_type": "directory",
        });

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/targets",
            &cuerpo_de_la_peticion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `directory` sin ruta de host es un formulario incompleto: contestó {status} con \
             {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("host_path"),
            "el rechazo tiene que nombrar el campo que falta, y el mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.27 — Un `mariadb` sin `db_password` se rechaza nombrando el campo.
    ///
    /// Cubre la mitad de «exigir» para los targets de base de datos: sin contraseña el `docker
    /// exec` del §4.A no puede autenticarse contra el servidor de la base, así que la fila sería
    /// un job que falla en cada ejecución en vez de un formulario rechazado una vez. Se nombra el
    /// campo que falta y se comprueba que no quedó fila.
    #[tokio::test]
    async fn un_mariadb_sin_db_password_se_rechaza_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let cuerpo_de_la_peticion = serde_json::json!({
            "name": "target-1-27",
            "target_type": "mariadb",
            "container_name": "contenedor",
            "db_user": "usuario",
        });

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/targets",
            &cuerpo_de_la_peticion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un target de base de datos sin contraseña no puede extraer nada: contestó {status} con \
             {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("db_password"),
            "el rechazo tiene que nombrar el campo que falta, y el mensaje fue {mensaje:?}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(&pool)
            .await
            .expect(
                "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
            );

        assert_eq!(
            filas, 0,
            "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
        );
    }

    /// 1.28 — Un `db_name` ausente queda nulo en la columna.
    ///
    /// Cubre el requisito de `api-resources` sobre la distinción entre `NULL` y cadena vacía. Se
    /// lee la columna **con SQL directo sobre el mismo fichero** y se lee como `Option<String>`:
    /// lo que se afirma es una propiedad del fichero, y una lectura que pasara por la capa de
    /// acceso podría estar normalizando el valor sin que la prueba lo viera. `NULL` significa el
    /// volcado global —el `--all-databases`/`pg_dumpall` que el motor sustituirá— y `''` sería una
    /// base llamada cadena vacía, que no existe.
    #[tokio::test]
    async fn un_db_name_ausente_queda_nulo_en_la_columna() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let cuerpo_de_la_peticion = serde_json::json!({
            "name": "target-1-28",
            "target_type": "mariadb",
            "container_name": "contenedor",
            "db_user": "usuario",
            "db_password": "contrasena-distinguible-0000000000000002",
        });

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/targets",
            &cuerpo_de_la_peticion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un target válido de base de datos sin `db_name` es un volcado global, que es válido: \
             contestó {status} con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let db_name: Option<String> =
            sqlx::query_scalar("SELECT db_name FROM backup_targets WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        assert!(
            db_name.is_none(),
            "`db_name` ausente tiene que quedar NULL de verdad —lo que significa volcado \
             global— y no una cadena vacía, que sería una base llamada «»: quedó {db_name:?}"
        );
    }

    /// 1.29 — Un campo de texto vacío se rechaza nombrando el campo.
    ///
    /// Cubre el requisito de `api-resources` sobre `NULL` y cadena vacía, y la mitad de «exigir»
    /// del tipo. Un campo informado y vacío no es «no lo informes» —para eso está omitirlo, que es
    /// el escenario de la 1.28—, sino una cadena vacía, y la columna del §3 guarda `NULL` para lo
    /// ausente: admitirla escribiría una cadena vacía donde el esquema espera nulo, y el motor
    /// ejecutaría un `docker exec` con el nombre de un contenedor que es la cadena vacía.
    #[tokio::test]
    async fn un_campo_de_texto_vacio_se_rechaza_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let casos = [
            (
                serde_json::json!({
                    "name": "target-1-29",
                    "target_type": "mariadb",
                    "container_name": "",
                    "db_user": "usuario",
                    "db_password": "c",
                }),
                "container_name",
            ),
            (
                serde_json::json!({
                    "name": "target-1-29",
                    "target_type": "mariadb",
                    "container_name": "contenedor",
                    "db_user": "usuario",
                    "db_password": "c",
                    "db_name": "",
                }),
                "db_name",
            ),
        ];

        for (cuerpo_de_la_peticion, campo) in casos {
            let respuesta =
                post_json(router.clone(), "/api/v1/targets", &cuerpo_de_la_peticion).await;

            let status = respuesta.status();
            let cuerpo = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "un `{campo}` informado y vacío es una cadena vacía, no un campo ausente: contestó \
                 {status} con {cuerpo}"
            );

            let mensaje = cuerpo["message"]
                .as_str()
                .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

            assert!(
                mensaje.contains(campo),
                "el rechazo tiene que nombrar el campo que lo provoca —{campo}—, y el mensaje fue \
                 {mensaje:?}"
            );

            let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
                .fetch_one(&pool)
                .await
                .expect(
                    "la tabla tiene que ser contable sobre el fichero con el que se abrió el estado",
                );

            assert_eq!(
                filas, 0,
                "la petición rechazada por `{campo}` no puede dejar fila escrita: quedaron {filas}"
            );
        }
    }

    /// 1.31 — El `400` nombra el campo y no filtra el valor.
    ///
    /// Los cuerpos de estas peticiones llevan credenciales, y un mensaje que repita el valor lo
    /// pone en el cuerpo de la respuesta y de ahí a cualquier registro intermedio. Quien depura un
    /// alta necesita saber **qué campo** está mal, y el valor ya lo tiene delante quien lo envió.
    /// Por eso el valor mal puesto es la credencial distinguible: el `contains` sobre el texto
    /// crudo no puede dar un falso positivo por casualidad.
    #[tokio::test]
    async fn el_400_nombra_el_campo_y_no_filtra_el_valor() {
        let cuerpo_de_la_peticion = serde_json::json!({
            "name": "target-1-31",
            "target_type": "directory",
            "host_path": "/srv/datos",
            "container_name": CREDENCIAL_DE_PRUEBA,
        });

        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(router, "/api/v1/targets", &cuerpo_de_la_peticion).await;

        let status = respuesta.status();
        let crudo = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `directory` que informa un contenedor se rechaza: contestó {status}"
        );

        let cuerpo: serde_json::Value = serde_json::from_str(&crudo)
            .expect("la envoltura del rechazo tiene que ser un objeto JSON");

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

        assert!(
            mensaje.contains("container_name"),
            "el rechazo tiene que nombrar el campo que lo provoca, y el mensaje fue {mensaje:?}"
        );
        assert!(
            !crudo.contains(CREDENCIAL_DE_PRUEBA),
            "el rechazo no puede repetir el valor del campo: lo pondría en el cuerpo de la \
             respuesta y de ahí a cualquier registro intermedio; el mensaje fue {mensaje:?}"
        );
    }

    /// 1.46t — Un identificador de target mal formado contesta `400`.
    ///
    /// Cubre el requisito de `api-resources` sobre los identificadores. Un identificador mal
    /// formado es una petición mal formada, no un recurso ausente: un `404` le diría a quien
    /// pregunta que su recurso se ha borrado cuando lo que hay es una errata de teclado, y un
    /// `500` culparía al servidor de una petición que no llegó a ser una consulta.
    #[tokio::test]
    async fn un_identificador_de_target_mal_formado_contesta_400() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = get(router, "/api/v1/targets/no-es-un-uuid").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un identificador que no es un UUID no puede ser un recurso ausente ni un fallo del \
             servidor: contestó {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["status"], 400,
            "el rechazo tiene que llegar en la envoltura del resto de la API, con el código \
             dentro: {cuerpo}"
        );
        assert!(
            cuerpo["message"].is_string(),
            "la envoltura de error tiene que traer un `message` de texto: {cuerpo}"
        );
    }

    /// 1.32t — Un `PUT` sin la contraseña conserva la almacenada.
    ///
    /// Cubre `api-resources`, escenario «Un `PUT` sin el secreto conserva el almacenado», la mitad
    /// que faltaba para este recurso —el hermano de `repositories`
    /// (`un_put_sin_passphrase_conserva_la_almacenada`) ya la tenía—. El panel no **tiene** la
    /// contraseña: la API no la devuelve por ninguna ruta, solo el booleano `has_db_password`. Sin
    /// esta regla, reescribir un target para cambiarle el nombre mandaría el secreto vacío y
    /// borraría la credencial de una copia que hasta ese momento funcionaba.
    ///
    /// La conservación se afirma **descifrando** los dos sobres, nunca comparándolos: el nonce es
    /// aleatorio por valor y `Targets::actualizar` vuelve a cifrar, así que la fila estrena sobre
    /// —con otro nonce— aunque el valor en claro no haya cambiado. Comparar los bytes mediría el
    /// nonce, no el secreto que hay que conservar.
    #[tokio::test]
    async fn un_put_sin_db_password_conserva_la_almacenada() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &cuerpo_de_target("target-1-32t"),
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

        let sobre_antes: Option<String> =
            sqlx::query_scalar("SELECT db_password FROM backup_targets WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        let sobre_antes = sobre_antes.expect(
            "el alta de un target de base de datos tiene que dejar la contraseña guardada en la \
             columna, cifrada",
        );

        let contrasena_antes = clave
            .descifrar(&sobre_antes)
            .expect("el alta tiene que haber guardado la contraseña cifrada y descifrable")
            .expose()
            .to_owned();

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/targets/{id}"),
            &serde_json::json!({
                "name": "target-1-32t-editado",
                "target_type": "mariadb",
                "container_name": "contenedor-editado",
                "db_user": "usuario-editado",
                "db_name": "base-editada",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la actualización de una fila existente tiene que contestar 200; contestó {status} con \
             {cuerpo}"
        );

        let sobre_despues: Option<String> =
            sqlx::query_scalar("SELECT db_password FROM backup_targets WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila actualizada tiene que seguir en el fichero de la base");

        let sobre_despues = sobre_despues.expect(
            "un PUT que no informa la contraseña no puede vaciar la columna: la fila actualizada \
             tiene que seguir teniendo su sobre",
        );

        let contrasena_despues = clave
            .descifrar(&sobre_despues)
            .expect("la actualización tiene que dejar la contraseña cifrada y descifrable")
            .expose()
            .to_owned();

        assert_eq!(
            contrasena_despues, contrasena_antes,
            "un PUT que no informa `db_password` no puede borrarlo: el panel no tiene el valor —la \
             API no lo devuelve— y sin esta regla reescribiría la credencial con nada"
        );

        let respuesta = get(router, &format!("/api/v1/targets/{id}")).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la fila actualizada tiene que seguir leyéndose por la API; contestó {status} con \
             {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["name"], "target-1-32t-editado",
            "el PUT tiene que haber aplicado el nombre que sí informó: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["container_name"], "contenedor-editado",
            "el PUT tiene que haber aplicado el contenedor que sí informó: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["db_user"], "usuario-editado",
            "el PUT tiene que haber aplicado el usuario que sí informó: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["has_db_password"], true,
            "la credencial conservada tiene que seguir anunciándose como presente: {cuerpo}"
        );
    }

    /// 1.32t-bis — Un `PUT` que cambia el tipo a `directory` descarta la credencial.
    ///
    /// Cubre el otro desenlace del validador de `api-resources`: si el tipo nuevo es `directory`,
    /// la contraseña se descarta **aunque la fila la tuviera**. Un directorio no admite
    /// credenciales, y arrastrarlas sería guardar una contraseña que ninguna copia va a usar nunca
    /// —y que seguiría ahí, cifrada y viva, después de que el recurso haya dejado de ser una base
    /// de datos—. Por eso la prueba no se fía solo de la vista del cable: va a la columna y afirma
    /// que es `NULL`, no un sobre huérfano.
    #[tokio::test]
    async fn un_put_que_cambia_el_tipo_a_directory_descarta_la_credencial() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &cuerpo_de_target("target-1-32t-bis"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 con la credencial guardada antes de poder cambiarla \
             de tipo"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["has_db_password"], true,
            "el alta de un target de base de datos tiene que dejar credencial antes del cambio de \
             tipo: {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = put_json(
            router,
            &format!("/api/v1/targets/{id}"),
            &serde_json::json!({
                "name": "target-1-32t-bis",
                "target_type": "directory",
                "host_path": "/srv/datos",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el cambio de tipo a `directory` es una actualización válida y tiene que contestar 200; \
             contestó {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["target_type"], "directory",
            "la respuesta tiene que devolver el tipo que acaba de escribir el PUT: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["has_db_password"], false,
            "un directorio no tiene credencial, así que la vista no puede anunciarla como \
             presente: {cuerpo}"
        );
        assert!(
            cuerpo["data"]["container_name"].is_null(),
            "un `directory` no tiene contenedor, así que la columna tiene que quedar nula en la \
             respuesta: {cuerpo}"
        );
        assert!(
            cuerpo["data"]["db_user"].is_null(),
            "un `directory` no tiene usuario de base, así que la columna tiene que quedar nula en \
             la respuesta: {cuerpo}"
        );
        assert!(
            cuerpo["data"]["db_name"].is_null(),
            "un `directory` no tiene nombre de base, así que la columna tiene que quedar nula en \
             la respuesta: {cuerpo}"
        );

        let guardada: Option<String> =
            sqlx::query_scalar("SELECT db_password FROM backup_targets WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila actualizada tiene que seguir en el fichero de la base");

        assert!(
            guardada.is_none(),
            "la credencial tiene que descartarse de verdad, no quedarse huérfana en la columna que \
             el tipo nuevo ya no va a mirar: quedó {guardada:?}"
        );
    }
}
