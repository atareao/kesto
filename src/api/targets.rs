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
use crate::api::texto;
use crate::db::entities::{AltaDeTarget, Target, TipoDeTarget};
use crate::engine::frontera::{self, FronteraError};
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
/// `target_type` viaja como **texto** —`directory`, `mariadb`, `postgresql` o `sqlite`—, que es
/// exactamente lo que el `CHECK` del §3 acepta en la columna; el enum de la capa de acceso no
/// viaja porque no implementa `serde`, y el vocabulario del cable es de esta capa. Los campos de
/// texto opcionales son `Option` porque `NULL` y cadena vacía **no** son lo mismo, y
/// `db_password` es `Option<SecretString>` porque `SecretString` no implementa `Serialize`: una
/// respuesta que lo llevara no compila.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AltaDeTargetDelCable {
    /// Nombre del target.
    ///
    /// `#[serde(default)]` es lo que hace que un cuerpo **sin** la clave llegue aquí como cadena
    /// vacía en vez de ser rechazado por el extractor: así el `400` lo contesta
    /// [`alta_desde`] —que nombra `name`— y no la clase genérica de serde, que no nombra el campo.
    #[serde(default)]
    pub name: String,
    /// Tipo de fuente: `directory`, `mariadb`, `postgresql` o `sqlite`.
    pub target_type: String,
    /// Contenedor del que extraer el volcado; no lo admite un `directory`.
    pub container_name: Option<String>,
    /// Usuario de la base; no lo admite un `directory` ni un `sqlite`.
    pub db_user: Option<String>,
    /// Contraseña de la base; es el tercer secreto del §3 y no lo admiten `directory` ni `sqlite`.
    pub db_password: Option<SecretString>,
    /// Base concreta del volcado; `None` significa el volcado global.
    pub db_name: Option<String>,
    /// Ruta del host; solo la admite un `directory`.
    pub host_path: Option<String>,
    /// Ruta absoluta del `.db` **dentro** del contenedor; solo la admite un `sqlite`.
    pub db_path: Option<String>,
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
    /// Ruta del `.db` dentro del contenedor, o `null`; solo la lleva un `sqlite`.
    ///
    /// Sale en claro, como el resto de campos no secretos: es lo que el panel necesita para
    /// repintar el formulario de edición sin volver a pedir la fila, y no hay nada que proteger
    /// en una ruta.
    pub db_path: Option<String>,
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
            db_path: fila.db_path.clone(),
            created_at: fila.created_at.format(&Rfc3339).map_err(|error| {
                AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
            })?,
        })
    }
}

/// El texto del cable de un tipo de target.
///
/// El cable habla el mismo texto que la columna del §3 guarda —su `CHECK` acepta `mariadb`,
/// `postgresql`, `directory` y `sqlite`, en minúsculas—, y el vocabulario es de esta capa: el
/// enum de la capa de acceso no implementa `serde` y no viaja. Las pruebas fijan los literales,
/// así que un cambio de vocabulario en la base no puede pasar inadvertido en el cable. El texto
/// es `sqlite` y no `sqlite3`: el segundo es el binario que hace la copia.
///
/// Es `pub(crate)` porque **no es el texto de este recurso**: el listado del explorador publica el
/// tipo de cada target y tiene que decirlo con estas mismas cuatro palabras. Una segunda tabla de
/// literales en `snapshots.rs` sería una segunda boca del vocabulario del cable, y el día que se
/// añada un tipo las dos se separarían sin que nada lo delatara.
pub(crate) fn texto_del_tipo(tipo: TipoDeTarget) -> &'static str {
    match tipo {
        TipoDeTarget::Mariadb => "mariadb",
        TipoDeTarget::Postgresql => "postgresql",
        TipoDeTarget::Directory => "directory",
        TipoDeTarget::Sqlite => "sqlite",
    }
}

/// El tipo del cable, convertido al de la capa de acceso.
///
/// Un valor que no sea uno de los del vocabulario contesta `400` **nombrando el campo** y sin
/// repetir el valor: el texto llegó por el cable, y la regla de la casa es nombrar el campo,
/// nunca lo que traía.
fn tipo_del_cable(texto: &str) -> Result<TipoDeTarget, AppError> {
    match texto {
        "mariadb" => Ok(TipoDeTarget::Mariadb),
        "postgresql" => Ok(TipoDeTarget::Postgresql),
        "directory" => Ok(TipoDeTarget::Directory),
        "sqlite" => Ok(TipoDeTarget::Sqlite),
        _ => Err(AppError::InvalidInput(
            "el campo `target_type` no admite ese valor: se espera `directory`, `mariadb`, \
             `postgresql` o `sqlite`"
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
/// Se usa para el **secreto** `db_password`, que no pasa por [`texto`]: un secreto no se recorta
/// —recortarlo lo cambiaría— ni se acota. El resto de campos de texto resuelven la misma
/// distinción en [`texto::obligatorio`] y [`texto::opcional`].
fn vacio(campo: &str) -> AppError {
    AppError::InvalidInput(format!("el campo `{campo}` no puede ir vacío"))
}

/// El `400` de un campo que este tipo de target no admite.
fn no_admitido(campo: &str) -> AppError {
    AppError::InvalidInput(format!(
        "el campo `{campo}` no se admite para este tipo de target"
    ))
}

/// Devuelve el nombre del contenedor recortado y validado contra el juego de Docker, o el `400`.
///
/// Es obligatorio en los tipos que lo admiten, así que un valor ausente o en blanco se traduce al
/// `400` de [`falta`]: el juego de caracteres se aplica en [`texto::nombre_de_contenedor`] y la
/// obligatoriedad, aquí, porque el juego es una regla del campo y la obligatoriedad, del tipo.
fn contenedor_obligatorio(valor: Option<&str>) -> Result<String, AppError> {
    texto::nombre_de_contenedor(valor)?.ok_or_else(|| falta("container_name"))
}

/// El `400` si un campo que este tipo no admite viene informado.
fn prohibir(campo: &str, valor: Option<&str>) -> Result<(), AppError> {
    match valor {
        None => Ok(()),
        Some(_) => Err(no_admitido(campo)),
    }
}

/// El `400` de una ruta que no cabe en su alcance.
///
/// El texto lo pone [`frontera`], que es la única pieza que sabe qué alcance tiene cada campo y
/// **nombra el campo** sin publicar ninguna ruta: lo único que se lee en el mensaje es el nombre
/// de lo que el operador escribió.
fn fuera_de_alcance(error: FronteraError) -> AppError {
    AppError::InvalidInput(error.to_string())
}

/// Comprueba la **forma** del `host_path` contra la raíz de copias, sin tocar el disco.
///
/// Es la comprobación textual —la ruta puede no existir todavía, y al dar de alta un directorio es
/// legítimo que no esté montado—; la canónica, que resuelve enlaces simbólicos, la hace el motor.
fn exigir_host_path_bajo_la_raiz(raiz: &std::path::Path, host_path: &str) -> Result<(), AppError> {
    frontera::forma_de_host_path(raiz, host_path).map_err(fuera_de_alcance)
}

/// Comprueba la **forma** del `db_path` de un `sqlite`: absoluta, sin `..` y sin segmento vacío.
fn exigir_db_path_sin_ambiguedad(db_path: &str) -> Result<(), AppError> {
    frontera::forma_de_db_path(db_path).map_err(fuera_de_alcance)
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
/// el tipo nuevo es `directory`, el secreto se descarta aunque la fila lo tuviera. Un `sqlite`
/// tampoco: no tiene credenciales de base que conservar.
///
/// `raiz` es la raíz de copias efectiva —`Config::raiz_de_copias`, que llega desde el estado— y
/// es contra ella contra la que se mide el `host_path`. La comprobación es **textual** y va al
/// final, después de las de presencia y coherencia: un cuerpo al que le falta un campo tiene que
/// seguir fallando por el campo que falta, no por el alcance de una ruta que ni siquiera venía.
///
/// # El recorte y las cotas
///
/// Cada campo de texto pasa por [`texto`], que lo **recorta** antes de medirlo y guardarlo y le
/// aplica su cota: el tope se mide sobre lo que de verdad se va a escribir, y los espacios de los
/// extremos no son parte del valor. Un campo **obligatorio** que tras recortar quede vacío es un
/// `400` que nombra el campo —la misma regla que la falta—; un campo **opcional** —`db_name`— se
/// guarda como `NULL`, nunca como la cadena vacía. El `container_name` lleva además el juego de
/// caracteres de Docker, que cierra la inyección de ruta en la API de Docker.
///
/// **Un cambio de comportamiento deliberado**: antes, un opcional informado en blanco se rechazaba
/// con un `400` y se exigía distinguirlo de su ausencia. Ahora el blanco se normaliza al `NULL` —los
/// dos significan «no lo he informado»—, que es lo que el §3 guarda para lo ausente y lo que el
/// motor lee como el volcado global.
fn alta_desde(
    cuerpo: &AltaDeTargetDelCable,
    almacenada: Option<&SecretString>,
    raiz: &std::path::Path,
) -> Result<AltaDeTarget, AppError> {
    let target_type = tipo_del_cable(&cuerpo.target_type)?;

    // El nombre es obligatorio en los **cuatro** tipos y se recorta antes de medirlo, así que se
    // valida una sola vez, antes de la rama: es el mismo campo para todos y no hay motivo para que
    // tres ramas lo comprueben y una se olvide. Va antes de las de presencia y coherencia porque un
    // cuerpo sin nombre falla por el nombre y no por el alcance de una ruta que ni siquiera venía.
    let name = texto::obligatorio("name", Some(cuerpo.name.as_str()), texto::COTA_NOMBRE)?;

    match target_type {
        TipoDeTarget::Directory => {
            let host_path = texto::obligatorio(
                "host_path",
                cuerpo.host_path.as_deref(),
                texto::COTA_HOST_PATH,
            )?;
            prohibir("container_name", cuerpo.container_name.as_deref())?;
            prohibir("db_user", cuerpo.db_user.as_deref())?;
            prohibir("db_name", cuerpo.db_name.as_deref())?;
            prohibir("db_path", cuerpo.db_path.as_deref())?;

            if cuerpo.db_password.is_some() {
                return Err(no_admitido("db_password"));
            }

            exigir_host_path_bajo_la_raiz(raiz, &host_path)?;

            Ok(AltaDeTarget {
                name,
                target_type,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: Some(host_path),
                db_path: None,
            })
        }
        TipoDeTarget::Mariadb | TipoDeTarget::Postgresql => {
            let container_name = contenedor_obligatorio(cuerpo.container_name.as_deref())?;
            let db_user =
                texto::obligatorio("db_user", cuerpo.db_user.as_deref(), texto::COTA_DB_USER)?;

            let db_password = match &cuerpo.db_password {
                Some(secreto) if secreto.expose().is_empty() => return Err(vacio("db_password")),
                Some(secreto) => secreto.clone(),
                None => match almacenada {
                    Some(secreto) => secreto.clone(),
                    None => return Err(falta("db_password")),
                },
            };

            // Un `db_name` ausente **o en blanco** acaba como `NULL`: los dos significan lo mismo
            // —el volcado global—, y la columna del §3 guarda nulo para lo ausente. Antes se
            // rechazaba el blanco; desde esta regla se normaliza al nulo, que es lo que el motor lee.
            let db_name =
                texto::opcional("db_name", cuerpo.db_name.as_deref(), texto::COTA_DB_NAME)?;

            prohibir("host_path", cuerpo.host_path.as_deref())?;
            prohibir("db_path", cuerpo.db_path.as_deref())?;

            Ok(AltaDeTarget {
                name,
                target_type,
                container_name: Some(container_name),
                db_user: Some(db_user),
                db_password: Some(db_password),
                db_name,
                host_path: None,
                db_path: None,
            })
        }
        // Una fuente `sqlite` se describe con las **dos** piezas que la sitúan: el contenedor
        // donde vive el fichero y la ruta del `.db` dentro de él. Sin cualquiera de las dos el
        // motor no tiene a qué apuntar, y el fallo aparecería en la primera copia en vez de en
        // el formulario. No admite ninguna pieza de una base de cliente/servidor ni una ruta de
        // host: el `.db` no se lee del sistema de ficheros de Kesto, se volca desde dentro del
        // contenedor.
        TipoDeTarget::Sqlite => {
            let container_name = contenedor_obligatorio(cuerpo.container_name.as_deref())?;
            let db_path =
                texto::obligatorio("db_path", cuerpo.db_path.as_deref(), texto::COTA_DB_PATH)?;

            prohibir("db_user", cuerpo.db_user.as_deref())?;
            prohibir("db_name", cuerpo.db_name.as_deref())?;
            prohibir("host_path", cuerpo.host_path.as_deref())?;

            if cuerpo.db_password.is_some() {
                return Err(no_admitido("db_password"));
            }

            exigir_db_path_sin_ambiguedad(&db_path)?;

            Ok(AltaDeTarget {
                name,
                target_type,
                container_name: Some(container_name),
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: None,
                db_path: Some(db_path),
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
    let alta = alta_desde(&cuerpo, None, &state.config.raiz_de_copias)?;

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
    let alta = alta_desde(
        &cuerpo,
        actual.db_password.as_ref(),
        &state.config.raiz_de_copias,
    )?;

    let fila = Target {
        id: actual.id,
        name: alta.name,
        target_type: alta.target_type,
        container_name: alta.container_name,
        db_user: alta.db_user,
        db_password: alta.db_password,
        db_name: alta.db_name,
        host_path: alta.host_path,
        db_path: alta.db_path,
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
    use sqlx::SqlitePool;

    use crate::api::api_router;
    use crate::db::crypto::Clave;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        base_con_clave, body_json, body_text, config_con_dsn, config_con_dsn_y_raiz_de_copias,
        delete, estado_de_prueba, estado_desarrollo, get, post_json, put_json,
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

    /// 1.29 — Un campo **obligatorio** de texto vacío se rechaza nombrando el campo.
    ///
    /// Cubre el requisito de `api-resources` sobre «un campo obligatorio que tras recortar quede
    /// vacío es un `400` que nombra el campo». Un campo informado y vacío no es «no lo informes»
    /// —para eso está omitirlo, que es el escenario de la 1.28—, sino una cadena vacía, y el motor
    /// ejecutaría un `docker exec` con el nombre de un contenedor que es la cadena vacía.
    ///
    /// **Solo los obligatorios.** Un opcional —`db_name`— en blanco ya no se rechaza: se guarda como
    /// `NULL`, que es lo que el §3 significa para «ausente», y tiene su propia prueba (2.5). El
    /// blanco se prueba con espacios, no con la cadena vacía, para que el rechazo dependa del
    /// recorte y no de mirar `is_empty()`.
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
                    "db_user": "   ",
                    "db_password": "c",
                }),
                "db_user",
            ),
            (
                serde_json::json!({
                    "name": "target-1-29",
                    "target_type": "directory",
                    "host_path": "   ",
                }),
                "host_path",
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

        let config = config_con_dsn(&directorio.dsn());

        // El `host_path` se compone sobre la raíz que declara la configuración del arnés: esta
        // prueba mide el descarte de la credencial al cambiar de tipo, no el alcance de la ruta,
        // así que la ruta tiene que quedar bajo la raíz para que lo que se mida sea lo que se
        // quiere medir.
        let host_path = config
            .raiz_de_copias
            .join("datos")
            .to_string_lossy()
            .to_string();

        let estado = estado_de_prueba(
            config,
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
                "host_path": host_path,
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

    /// 1.55 — Un alta `sqlite` con `container_name` y `db_path` contesta `201` y escribe la ruta.
    ///
    /// Cubre `api-resources` (escenario «Un sqlite con sus campos es válido»). La ruta se
    /// afirma en los **dos** sitios: en la respuesta, porque es lo que el panel necesita para
    /// pintar el formulario de edición sin volver a pedir la fila, y en la columna, porque es
    /// de donde el motor la leerá para armar su `exec`. Y se afirma que la fuente `sqlite` no
    /// arrastra ninguna credencial: no tiene usuario ni contraseña de base que guardar.
    #[tokio::test]
    async fn un_alta_sqlite_con_sus_campos_es_valida_y_escribe_la_ruta() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let cuerpo_de_la_peticion = serde_json::json!({
            "name": "target-1-55",
            "target_type": "sqlite",
            "container_name": "contenedor-sqlite",
            "db_path": "/var/lib/kesto/kesto.db",
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
            "un `sqlite` con su contenedor y su ruta es un target completo: contestó {status} con \
             {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["target_type"], "sqlite",
            "el tipo tiene que volver como el texto del cable desde el que se pidió: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["db_path"], "/var/lib/kesto/kesto.db",
            "el `db_path` tiene que volver en la respuesta, que es lo que el panel pinta al \
             editar: {cuerpo}"
        );
        assert!(
            cuerpo["data"]["db_user"].is_null() && cuerpo["data"]["host_path"].is_null(),
            "un `sqlite` no tiene usuario de base ni ruta de host, así que sus campos tienen que \
             quedar nulos: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["has_db_password"], false,
            "un `sqlite` no tiene credencial de base: {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let guardado: (String, String) =
            sqlx::query_as("SELECT target_type, db_path FROM backup_targets WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero con el que se abrió el estado");

        assert_eq!(
            guardado,
            ("sqlite".to_owned(), "/var/lib/kesto/kesto.db".to_owned()),
            "la columna tiene que guardar el texto del enumerado y la ruta tal cual"
        );
    }

    /// 1.56 — Un `sqlite` sin `container_name` o sin `db_path` se rechaza nombrando el campo.
    ///
    /// Cubre la mitad de «exigir» para el tipo nuevo: las dos piezas que describen la fuente
    /// son el contenedor donde vive el fichero y la ruta del fichero dentro de él, y sin
    /// cualquiera de las dos el motor no tiene a qué apuntar. El rechazo tiene que llegar en
    /// el borde HTTP —la tabla del §3 acepta esa fila, porque sus columnas son anulables— y
    /// no puede dejar fila escrita.
    #[tokio::test]
    async fn un_sqlite_sin_container_name_o_sin_db_path_se_rechaza_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let router = api_router(estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        ));

        // El otro campo de los dos sí va en cada vuelta: así lo que provoca el rechazo es el
        // que falta, y no una petición a la que le faltara todo.
        for (campo, cuerpo_de_la_peticion) in [
            (
                "container_name",
                serde_json::json!({
                    "name": "target-1-56-contenedor",
                    "target_type": "sqlite",
                    "db_path": "/var/lib/kesto/kesto.db",
                }),
            ),
            (
                "db_path",
                serde_json::json!({
                    "name": "target-1-56-ruta",
                    "target_type": "sqlite",
                    "container_name": "contenedor-sqlite",
                }),
            ),
        ] {
            let respuesta =
                post_json(router.clone(), "/api/v1/targets", &cuerpo_de_la_peticion).await;

            let status = respuesta.status();
            let cuerpo = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "un `sqlite` sin `{campo}` está incompleto: contestó {status} con {cuerpo}"
            );

            let mensaje = cuerpo["message"]
                .as_str()
                .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

            assert!(
                mensaje.contains(campo),
                "el rechazo tiene que nombrar el campo que falta —`{campo}`—, y el mensaje fue \
                 {mensaje:?}"
            );

            let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
                .fetch_one(&pool)
                .await
                .expect("la tabla tiene que ser contable sobre el fichero del estado");

            assert_eq!(
                filas, 0,
                "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
            );
        }
    }

    /// 1.57 — Un `sqlite` que informa un campo de base o de host se rechaza nombrando el campo.
    ///
    /// Cubre la mitad de «prohibir» para el tipo nuevo, y son **cuatro** campos los que no
    /// admite: `db_user`, `db_password`, `db_name` y `host_path`. Es la misma razón que en el
    /// `directory`: la tabla los guardaría tan contenta y el motor no sabría qué hacer con
    /// ellos, así que el fallo aparecería en la primera copia en vez de en el formulario.
    /// Ninguno de los cuatro puede dejar fila escrita.
    #[tokio::test]
    async fn un_sqlite_con_campos_de_base_o_de_host_se_rechaza_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let router = api_router(estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        ));

        for (campo, valor) in [
            ("db_user", serde_json::json!("usuario-de-base")),
            ("db_password", serde_json::json!(CONTRASENA_DE_TARGET)),
            ("db_name", serde_json::json!("base-de-pruebas")),
            ("host_path", serde_json::json!("/srv/datos")),
        ] {
            let mut cuerpo_de_la_peticion = serde_json::json!({
                "name": "target-1-57",
                "target_type": "sqlite",
                "container_name": "contenedor-sqlite",
                "db_path": "/var/lib/kesto/kesto.db",
            });
            cuerpo_de_la_peticion[campo] = valor;

            let respuesta =
                post_json(router.clone(), "/api/v1/targets", &cuerpo_de_la_peticion).await;

            let status = respuesta.status();
            let cuerpo = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "un `sqlite` que informa `{campo}` es un formulario incoherente: contestó {status} \
                 con {cuerpo}"
            );

            let mensaje = cuerpo["message"]
                .as_str()
                .expect("la envoltura tiene que traer un `message` de texto: {cuerpo}");

            assert!(
                mensaje.contains(campo),
                "el rechazo tiene que nombrar el campo que lo provoca —`{campo}`—, y el mensaje \
                 fue {mensaje:?}"
            );

            let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
                .fetch_one(&pool)
                .await
                .expect("la tabla tiene que ser contable sobre el fichero del estado");

            assert_eq!(
                filas, 0,
                "una petición rechazada no puede dejar fila escrita: quedaron {filas}"
            );
        }
    }

    // ── El alcance de las rutas: `confinamiento` en el borde HTTP ──────────────

    /// La raíz de copias del §8, que es la que declaran las pruebas de alcance.
    ///
    /// No es la raíz de los temporales del arnés —la que lleva [`config_con_dsn`]—: lo que estas
    /// pruebas miden es justo el alcance contra una raíz concreta, y la del despliegue de
    /// referencia es esta.
    const RAIZ_DEL_S8: &str = "/backup_sources";

    /// El estado de las pruebas de alcance: una base de verdad y la raíz del §8 declarada.
    fn estado_del_alcance(dsn: &str, pool: &SqlitePool, clave: &Arc<Clave>) -> AppState {
        estado_de_prueba(
            config_con_dsn_y_raiz_de_copias(dsn, RAIZ_DEL_S8),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(clave)),
        )
    }

    /// El mensaje de un rechazo, con la comprobación de que nombra el campo que lo provoca.
    fn mensaje_que_nombra(cuerpo: &serde_json::Value, campo: &str) -> String {
        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto")
            .to_owned();

        assert!(
            mensaje.contains(campo),
            "el rechazo tiene que nombrar el campo `{campo}`, y el mensaje fue {mensaje:?}"
        );

        mensaje
    }

    /// Las filas de `backup_targets` que hay en el fichero con el que se abrió el estado.
    async fn filas_de_targets(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(pool)
            .await
            .expect("la tabla tiene que ser contable sobre el fichero del estado")
    }

    /// 2.1 — Un `host_path` fuera de la raíz se rechaza y no deja fila.
    ///
    /// Cubre `confinamiento`, escenario «Un host_path fuera de la raíz se rechaza» en lo que toca
    /// al borde HTTP. La raíz del sistema de ficheros no es un directorio copiable, y aceptarla
    /// archivaría a S3 todo lo que Kesto ve —incluida su propia base, con los tokens y los
    /// secretos—. El rechazo tiene que llegar en el formulario y no en la primera copia.
    #[tokio::test]
    async fn un_host_path_fuera_de_la_raiz_se_rechaza_sin_fila() {
        let (directorio, pool, clave) = base_con_clave().await;

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-1",
                "target_type": "directory",
                "host_path": "/",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "la raíz del sistema de ficheros no es un directorio copiable: contestó {status} con \
             {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "host_path");

        assert_eq!(
            filas_de_targets(&pool).await,
            0,
            "una petición rechazada no puede dejar fila escrita"
        );
    }

    /// 2.2 — Una raíz que solo coincide como prefijo de cadena no vale.
    ///
    /// Cubre `confinamiento`, escenario «Una raíz parecida no basta». `/backup_sources_x` empieza
    /// por `/backup_sources` como **texto** y no está dentro de él: un confinamiento por prefijo de
    /// cadena dejaría pasar justo lo que quiere cerrar.
    #[tokio::test]
    async fn una_raiz_que_solo_coincide_como_prefijo_de_cadena_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-2",
                "target_type": "directory",
                "host_path": "/backup_sources_x",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un directorio hermano de la raíz no está dentro de ella: contestó {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "host_path");

        assert_eq!(
            filas_de_targets(&pool).await,
            0,
            "una petición rechazada no puede dejar fila escrita"
        );
    }

    /// 2.3 — Un `host_path` relativo se rechaza.
    ///
    /// Cubre `api-resources`, escenario «Un host_path relativo se rechaza»: una ruta relativa no
    /// dice de qué directorio del host se habla, así que el motor la resolvería contra su propio
    /// directorio de trabajo.
    #[tokio::test]
    async fn un_host_path_relativo_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-3",
                "target_type": "directory",
                "host_path": "backup_sources/var_www",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una ruta de host relativa es un formulario mal escrito: contestó {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "host_path");

        assert_eq!(
            filas_de_targets(&pool).await,
            0,
            "una petición rechazada no puede dejar fila escrita"
        );
    }

    /// 2.3b — Un `host_path` con `..` se rechaza.
    ///
    /// Cubre `api-resources`, escenario «Un host_path con `..` se rechaza». `/backup_sources/../etc`
    /// **empieza por** la raíz como texto y `..` es un componente válido para `Path::starts_with`,
    /// así que sin este rechazo el alta pasaría y el fallo aparecería en la primera copia.
    #[tokio::test]
    async fn un_host_path_con_punto_doble_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-3b",
                "target_type": "directory",
                "host_path": "/backup_sources/../etc",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "`..` en la cadena alcanza lo que no se declara: contestó {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "host_path");

        assert_eq!(
            filas_de_targets(&pool).await,
            0,
            "una petición rechazada no puede dejar fila escrita"
        );
    }

    /// 2.4 — Un `host_path` bajo la raíz se acepta aunque no exista todavía.
    ///
    /// Es el **contrapeso** de las tres anteriores: la comprobación del borde es **textual** y no
    /// exige que la ruta exista. Al dar de alta un directorio puede no estar montado aún, y exigir
    /// su existencia convertiría el alta en una comprobación del despliegue.
    #[tokio::test]
    async fn un_host_path_bajo_la_raiz_que_no_existe_se_acepta() {
        let (directorio, pool, clave) = base_con_clave().await;

        // La prueba afirma sobre la forma, no sobre el disco, así que la ruta tiene que faltar de
        // verdad para que el contrapeso signifique algo.
        assert!(
            !std::path::Path::new("/backup_sources/var_www").exists(),
            "esta prueba necesita que `/backup_sources/var_www` no exista en el sistema"
        );

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-4",
                "target_type": "directory",
                "host_path": "/backup_sources/var_www",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un directorio bajo la raíz es un alta válida aunque todavía no esté montado: contestó \
             {status} con {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["host_path"], "/backup_sources/var_www",
            "la fila tiene que quedarse con la ruta declarada: {cuerpo}"
        );
    }

    /// 2.5 — Un `db_path` con `..` se rechaza.
    ///
    /// Cubre `api-resources`, escenario «Un db_path con `..` se rechaza». El `.db` se ejecuta
    /// dentro del contenedor y habitualmente como `root`, así que un `..` en la cadena alcanza
    /// ficheros que nadie declaró; el fallo tiene que verse en el formulario.
    #[tokio::test]
    async fn un_db_path_con_punto_doble_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-5",
                "target_type": "sqlite",
                "container_name": "contenedor-sqlite",
                "db_path": "/datos/../etc/base.db",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `db_path` con `..` no declara el fichero que dice: contestó {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "db_path");

        assert_eq!(
            filas_de_targets(&pool).await,
            0,
            "una petición rechazada no puede dejar fila escrita"
        );
    }

    /// 2.6 — Un `db_path` relativo se rechaza.
    ///
    /// Cubre `api-resources`, escenario «Un db_path relativo se rechaza»: el `.db` se declara con
    /// su ruta absoluta dentro del contenedor, y una relativa dependería del directorio de trabajo
    /// con el que el `exec` arranque.
    #[tokio::test]
    async fn un_db_path_relativo_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let respuesta = post_json(
            api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave)),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-6",
                "target_type": "sqlite",
                "container_name": "contenedor-sqlite",
                "db_path": "datos/base.db",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `db_path` relativo es un formulario mal escrito: contestó {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "db_path");

        assert_eq!(
            filas_de_targets(&pool).await,
            0,
            "una petición rechazada no puede dejar fila escrita"
        );
    }

    /// 2.7 — La edición aplica la misma regla y no cambia la fila.
    ///
    /// Cubre `api-resources`, escenario «La edición aplica la misma regla». El alta y la edición
    /// comparten el punto de validación, así que un `PUT` que ponga un `host_path` fuera de la raíz
    /// tiene que contestar el mismo `400` y **dejar la fila como estaba**: un rechazo que
    /// actualizara a medias sería peor que no rechazar.
    #[tokio::test]
    async fn un_put_con_host_path_fuera_de_la_raiz_no_cambia_la_fila() {
        let (directorio, pool, clave) = base_con_clave().await;
        let router = api_router(estado_del_alcance(&directorio.dsn(), &pool, &clave));

        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "target-2-7",
                "target_type": "directory",
                "host_path": "/backup_sources/var_www",
            }),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "la prueba necesita un target de directorio válido del que partir"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = put_json(
            router,
            &format!("/api/v1/targets/{id}"),
            &serde_json::json!({
                "name": "target-2-7",
                "target_type": "directory",
                "host_path": "/etc",
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `PUT` con el `host_path` fuera de la raíz tiene que rechazarse: contestó {status} \
             con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "host_path");

        let guardado: String =
            sqlx::query_scalar("SELECT host_path FROM backup_targets WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que seguir en el fichero de la base");

        assert_eq!(
            guardado, "/backup_sources/var_www",
            "una edición rechazada no puede haber cambiado la fila"
        );
    }

    // ── Cotas y recorte de los campos de texto (SEC-13) ──────────────────────
    //
    // Cubre `api-resources`, requisito «Los campos de texto se recortan y tienen una cota por
    // campo»: el borde recorta cada campo antes de medirlo y guardarlo, un obligatorio que queda
    // vacío es un `400` que nombra el campo, un opcional en blanco se guarda como `NULL`, y lo que
    // supera su cota es un `400` que nombra el campo y **no repite el valor**. El porqué: la tabla
    // del §3 no acota sus columnas `TEXT`, así que sin esta regla un `bucket` de miles de
    // caracteres o un `name` de solo espacios entran en la base y reaparecen más tarde.

    /// 2.1 — Un `name` de solo espacios se rechaza nombrando `name`, y no deja fila.
    #[tokio::test]
    async fn un_name_en_blanco_contesta_400_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/targets",
            &cuerpo_de_target("   "),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `name` de solo espacios es un campo vacío: contestó {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "name");

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(&pool)
            .await
            .expect("la tabla tiene que poder contarse");
        assert_eq!(filas, 0, "un rechazo no puede dejar fila: quedaron {filas}");
    }

    /// 2.1b — Un cuerpo **sin** la clave `name` contesta `400` nombrando `name`.
    ///
    /// Un `name` ausente no es lo mismo que uno en blanco para el extractor: sin
    /// `#[serde(default)]` el cuerpo lo rechazaría la clase genérica de serde, que **no** nombra el
    /// campo. Con el defecto declarado, el cuerpo entra al manejador con el nombre vacío y contesta
    /// el mismo `400` que el blanco —el de [`texto::obligatorio`]—, que sí nombra `name`.
    #[tokio::test]
    async fn un_name_ausente_contesta_400_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let mut cuerpo_de_la_peticion = cuerpo_de_target("target-2-1b");
        if let Some(mapa) = cuerpo_de_la_peticion.as_object_mut() {
            mapa.remove("name");
        }

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
            "un cuerpo sin `name` tiene que rechazarse con un 400 que nombre el campo: contestó \
             {status} con {cuerpo}"
        );

        mensaje_que_nombra(&cuerpo, "name");

        let filas = filas_de_targets(&pool).await;
        assert_eq!(filas, 0, "un rechazo no puede dejar fila: quedaron {filas}");
    }

    /// 2.2 — Un `name` de 121 caracteres se rechaza sin repetir el valor.
    #[tokio::test]
    async fn un_name_por_encima_de_la_cota_contesta_400_sin_repetir_el_valor() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let largo = "n".repeat(121);

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/targets",
            &cuerpo_de_target(&largo),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `name` de 121 caracteres supera su cota de 120: contestó {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"]
            .as_str()
            .expect("la envoltura tiene que traer un `message` de texto");

        assert!(
            mensaje.contains("name"),
            "el rechazo tiene que nombrar el campo, y el mensaje fue {mensaje:?}"
        );
        assert!(
            !cuerpo.to_string().contains(&largo),
            "el rechazo no puede repetir el valor recibido: {cuerpo}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(&pool)
            .await
            .expect("la tabla tiene que poder contarse");
        assert_eq!(filas, 0, "un rechazo no puede dejar fila: quedaron {filas}");
    }

    /// 2.3 y 2.4 — Cada campo por encima de su cota contesta `400` nombrando el campo.
    #[tokio::test]
    async fn un_campo_por_encima_de_su_cota_contesta_400_nombrando_el_campo() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        let casos = [
            (
                serde_json::json!({
                    "name": "target-cota",
                    "target_type": "mariadb",
                    "container_name": "a".repeat(129),
                    "db_user": "usuario",
                    "db_password": "c",
                }),
                "container_name",
            ),
            (
                serde_json::json!({
                    "name": "target-cota",
                    "target_type": "mariadb",
                    "container_name": "contenedor",
                    "db_user": "u".repeat(64),
                    "db_password": "c",
                }),
                "db_user",
            ),
            (
                serde_json::json!({
                    "name": "target-cota",
                    "target_type": "mariadb",
                    "container_name": "contenedor",
                    "db_user": "usuario",
                    "db_password": "c",
                    "db_name": "d".repeat(64),
                }),
                "db_name",
            ),
            (
                serde_json::json!({
                    "name": "target-cota",
                    "target_type": "sqlite",
                    "container_name": "contenedor",
                    "db_path": format!("/{}", "p".repeat(1024)),
                }),
                "db_path",
            ),
            (
                serde_json::json!({
                    "name": "target-cota",
                    "target_type": "directory",
                    "host_path": format!("/srv/{}", "h".repeat(4096)),
                }),
                "host_path",
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
                "el campo `{campo}` por encima de su cota tiene que rechazarse: contestó {status} \
                 con {cuerpo}"
            );

            mensaje_que_nombra(&cuerpo, campo);

            let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
                .fetch_one(&pool)
                .await
                .expect("la tabla tiene que poder contarse");
            assert_eq!(
                filas, 0,
                "un rechazo por `{campo}` no puede dejar fila: quedaron {filas}"
            );
        }
    }

    /// 2.5 — Un `db_name` informado y en blanco se guarda como `NULL`.
    #[tokio::test]
    async fn un_db_name_en_blanco_se_guarda_como_null() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let mut cuerpo_de_la_peticion = cuerpo_de_target("target-2-5");
        cuerpo_de_la_peticion["db_name"] = serde_json::json!("   ");

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
            "un `db_name` en blanco es un campo ausente y el alta es válida: contestó {status} con \
             {cuerpo}"
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
                .expect("la fila tiene que estar en el fichero de la base");

        assert!(
            db_name.is_none(),
            "un `db_name` en blanco tiene que quedar NULL y no la cadena vacía ni la de espacios: \
             quedó {db_name:?}"
        );
    }

    /// 2.6 — Los campos de texto se guardan recortados.
    #[tokio::test]
    async fn los_campos_de_texto_se_guardan_recortados() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let mut cuerpo_de_la_peticion = cuerpo_de_target("  target-2-6  ");
        cuerpo_de_la_peticion["container_name"] = serde_json::json!("  contenedor-de-pruebas  ");
        cuerpo_de_la_peticion["db_user"] = serde_json::json!("  usuario-de-pruebas  ");
        cuerpo_de_la_peticion["db_name"] = serde_json::json!("  base-de-pruebas  ");

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
            "los espacios de los extremos no impiden dar de alta: contestó {status} con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let (name, container_name, db_user, db_name): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT name, container_name, db_user, db_name FROM backup_targets WHERE id = ?1",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .expect("la fila tiene que estar en el fichero de la base");

        assert_eq!(name, "target-2-6", "el nombre se guarda recortado");
        assert_eq!(
            container_name.as_deref(),
            Some("contenedor-de-pruebas"),
            "el contenedor se guarda recortado"
        );
        assert_eq!(
            db_user.as_deref(),
            Some("usuario-de-pruebas"),
            "el usuario se guarda recortado"
        );
        assert_eq!(
            db_name.as_deref(),
            Some("base-de-pruebas"),
            "la base se guarda recortada"
        );
    }

    // ── El nombre del contenedor contra Docker (SEC-22) ──────────────────────
    //
    // Cubre `api-resources`, requisito «El nombre de un contenedor se valida contra el juego de
    // caracteres de Docker». El porqué: bollard interpola el nombre **sin codificar** en la ruta de
    // la petición, así que un nombre con `/`, `..` o `?` mueve la petición a otro punto de la API de
    // Docker en vez de dar un error de nombre; el juego de Docker no admite ninguno de ésos.

    /// 4.1 — Un `container_name` con caracteres de ruta se rechaza sin tocar la base.
    #[tokio::test]
    async fn un_container_name_con_caracteres_de_ruta_se_rechaza() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let router = api_router(estado);

        for malicioso in [
            "../../v1.40/containers/x",
            "contenedor/otro",
            "contenedor?x",
            "..",
            "contenedor/",
        ] {
            let cuerpo_de_la_peticion = serde_json::json!({
                "name": "target-4-1",
                "target_type": "sqlite",
                "container_name": malicioso,
                "db_path": "/var/lib/base.db",
            });

            let respuesta =
                post_json(router.clone(), "/api/v1/targets", &cuerpo_de_la_peticion).await;

            let status = respuesta.status();
            let cuerpo = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "`{malicioso}` no puede llegar a la API de Docker: contestó {status} con {cuerpo}"
            );

            mensaje_que_nombra(&cuerpo, "container_name");

            assert!(
                !cuerpo.to_string().contains(malicioso),
                "el rechazo no puede repetir el valor `{malicioso}`: {cuerpo}"
            );
        }

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_targets")
            .fetch_one(&pool)
            .await
            .expect("la tabla tiene que poder contarse");
        assert_eq!(
            filas, 0,
            "ningún rechazo puede dejar fila: quedaron {filas}"
        );
    }

    /// 4.2 — Un `container_name` válido para Docker se acepta.
    #[tokio::test]
    async fn un_container_name_valido_se_acepta() {
        let router = api_router(estado_desarrollo().await);

        for valido in ["contenedor_de-la.base-01", "a1b2c3d4"] {
            let mut cuerpo_de_la_peticion = cuerpo_de_target(&format!("target-4-2-{valido}"));
            cuerpo_de_la_peticion["container_name"] = serde_json::json!(valido);

            let respuesta =
                post_json(router.clone(), "/api/v1/targets", &cuerpo_de_la_peticion).await;

            let status = respuesta.status();
            let cuerpo = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::CREATED,
                "`{valido}` es un nombre de contenedor válido: contestó {status} con {cuerpo}"
            );
        }
    }
}
