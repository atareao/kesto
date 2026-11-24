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
//!
//! El **alta** dejó de ser un apunte en la base con el change `backup-engine`: desde él habla con
//! el destino —conecta si el repositorio existe, lo inicializa si no— y **escribe la fila después**,
//! así que un destino que no responde o una passphrase que no abre rechazan el alta sin dejar fila
//! que limpiar. El contrato entero, con el porqué de cada mitad, está en el rustdoc de [`alta`]; y
//! el `PUT`, que sigue sin hablar con el destino, lo dice en el suyo.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data, PaginatedResponse};
use crate::api::texto;
use crate::api::{contenido, del_motor, del_motor_en_el_alta};
use crate::db::entities::{AltaDeRepositorio, Repositorio, SaludDeRepositorio};
use crate::error::AppError;
use crate::rustic::{Acceso, PuestaEnMarcha};
use crate::secret::SecretString;
use crate::state::AppState;

/// Parámetros de consulta para listados paginados.
///
/// `limit` por defecto `50`, tope `200`; `offset` por defecto `0`. Valida que `limit` esté en
/// `1..=200` y que `offset` no sea negativo; devuelve `400` en caso contrario.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultaDelCable {
    /// Cuántas filas devolver como máximo.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Cuántas filas saltar.
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Límite de paginación por defecto.
const LIMITE_POR_DEFECTO: i64 = 50;

/// Límite máximo de paginación.
const LIMITE_MAXIMO: i64 = 200;

impl ConsultaDelCable {
    /// Valida la paginación y la resuelve a sus dos números, o contesta `400` nombrando el campo.
    pub fn paginacion(&self) -> Result<(i64, i64), AppError> {
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

        Ok((limite, desplazamiento))
    }
}

/// Las siete rutas del recurso, para colgar de la mitad de negocio.
///
/// Las cinco del §5 —listado, alta, lectura, actualización y borrado— más las dos de la
/// comprobación de salud: la de un repositorio y la de todos. Las dos son `POST` y no `GET` porque
/// **tienen efecto de escritura** —persisten el desenlace en la fila—, y el §5 reserva `GET` para lo
/// que no cambia nada; el listado, que sí es una lectura, **no** comprueba nada: sirve lo que ya
/// quedó anotado.
///
/// `/repositories/health` y `/repositories/{id}` conviven porque el segmento fijo gana al de
/// parámetro: una petición a `/repositories/health` no se lee como un identificador llamado
/// `health`, y por eso la ruta global no queda detrás del `400` del identificador mal formado.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/repositories", get(listado).post(alta))
        .route(
            "/api/v1/repositories/health",
            post(comprobar_salud_de_todos),
        )
        .route(
            "/api/v1/repositories/{id}",
            get(leer).put(actualizar).delete(borrar),
        )
        .route("/api/v1/repositories/{id}/health", post(comprobar_salud))
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
    ///
    /// `#[serde(default)]` es lo que hace que un cuerpo **sin** la clave llegue aquí como cadena
    /// vacía en vez de ser rechazado por el extractor: así el `400` lo contesta
    /// [`campos_de_texto`] —que nombra `name`— y no la clase genérica de serde, que no nombra el
    /// campo.
    #[serde(default)]
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

    // Los tres campos de salud de la **lectura** se aceptan y se descartan.
    //
    // La salud no es un campo editable —la mueve la comprobación, no un formulario—, pero
    // rechazarlos no es lo que el contrato pide: un cuerpo que los traiga se **ignora**, y por eso
    // se declaran aquí. Declararlos es lo que deja que `deny_unknown_fields` siga haciendo su
    // trabajo con las erratas —un campo mal escrito sigue siendo un `400` y no un valor perdido en
    // silencio— sin que un campo de solo lectura rompa una petición que por lo demás es válida.
    //
    // El tipo es el JSON crudo porque el valor no se interpreta: cualquiera vale mientras no se
    // cuele en la fila, y quien la escribe es `anotar_salud` y nadie más.
    /// Se acepta y se descarta: la salud no es escribible. Ver arriba.
    pub health: Option<serde_json::Value>,
    /// Se acepta y se descarta: la salud no es escribible. Ver arriba.
    pub health_checked_at: Option<serde_json::Value>,
    /// Se acepta y se descarta: la salud no es escribible. Ver arriba.
    pub health_detail: Option<serde_json::Value>,
}

/// El desenlace de una comprobación de salud, tal y como viaja por el cable.
///
/// Son los tres campos que comparten las **tres** respuestas que hablan de salud: el resultado de
/// `POST /repositories/{id}/health`, cada elemento de `POST /repositories/health` y —por el
/// `flatten` de [`RepositorioDelCable`]— el propio recurso del listado y de la lectura. Que sean un
/// solo tipo es el contrato: el desenlace que la comprobación acaba de anotar y el que sirve el
/// `GET` después son **el mismo** objeto, y un segundo tipo con los mismos tres campos sería un
/// segundo sitio donde se decide cómo se llama cada estado.
///
/// El estado va como **texto** y no como la variante del enum, igual que `target_type` en
/// `src/api/targets.rs`: el nombre que viaja es el del `CHECK` de la columna —lo que hace la tabla
/// legible a mano— y quien lo fija es [`texto_de_la_salud`], que es el único sitio de la capa HTTP
/// donde eso se decide. Un repositorio **nunca comprobado** llega como `desconocido`, que es un
/// valor del cable más y no un nulo: el panel no tiene una rama especial para «no lo sé».
#[derive(Debug, Serialize)]
pub struct SaludComprobadaDelCable {
    /// El estado persistido: `ok`, `sin_repositorio`, `passphrase`, `inaccesible`, `sin_permiso`
    /// o `desconocido`.
    pub health: String,
    /// Cuándo se comprobó, en RFC3339, o `null` si la fila no guarda ninguna comprobación.
    pub health_checked_at: Option<String>,
    /// Motivo acotado y **saneado** de la comprobación, o `null` si no hay nada que contar.
    pub health_detail: Option<String>,
}

impl SaludComprobadaDelCable {
    /// Vuelca los tres campos de salud de una fila en su forma del cable.
    fn desde(fila: &Repositorio) -> Result<Self, AppError> {
        Ok(Self {
            health: texto_de_la_salud(fila.health).to_owned(),
            health_checked_at: fecha_del_cable(fila.health_checked_at)?,
            health_detail: fila.health_detail.clone(),
        })
    }
}

/// El nombre del estado de salud en el vocabulario del cable.
///
/// Es el único sitio donde el estado de la fila se traduce a lo que viaja, y la **ausencia**
/// —`None`, la fila que nunca se comprobó— es un valor más: `desconocido`. El `match` es exhaustivo
/// y sin rama comodín a propósito, como el de [`desenlace`]: una variante nueva lo rompe la
/// compilación en vez de viajar con un texto inventado.
fn texto_de_la_salud(salud: Option<SaludDeRepositorio>) -> &'static str {
    match salud {
        None => "desconocido",
        Some(SaludDeRepositorio::Ok) => "ok",
        Some(SaludDeRepositorio::SinRepositorio) => "sin_repositorio",
        Some(SaludDeRepositorio::Passphrase) => "passphrase",
        Some(SaludDeRepositorio::Inaccesible) => "inaccesible",
        Some(SaludDeRepositorio::SinPermiso) => "sin_permiso",
    }
}

/// Formatea una fecha **anulable** de la fila para el cable, o `null` si la columna estaba a nulo.
///
/// Las fechas van como cadena porque `time` viaja con `formatting`: el formato lo fija el servidor,
/// en RFC3339, en vez de dejarlo en manos del serializador. La que no está se sirve como `null` y
/// no como cadena vacía: «no hay fecha» y «hay una fecha vacía» no son lo mismo, y el panel
/// distingue un repositorio sin comprobar por el nulo.
fn fecha_del_cable(fecha: Option<OffsetDateTime>) -> Result<Option<String>, AppError> {
    fecha
        .map(|valor| {
            valor.format(&Rfc3339).map_err(|error| {
                AppError::Other(format!("la fecha de la fila no se pudo formatear: {error}"))
            })
        })
        .transpose()
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
    /// El estado, la fecha y el motivo de la última comprobación de salud, **persistidos**.
    ///
    /// Van aplanados en el objeto del recurso —y no bajo una clave propia— porque son campos del
    /// repositorio como los demás, y su tipo es el **mismo** que devuelven las dos rutas de
    /// comprobación: lo que el `GET` sirve y lo que la comprobación acaba de anotar son el mismo
    /// objeto. El listado **no** dispara ninguna comprobación: esto es lo que quedó en la fila.
    #[serde(flatten)]
    pub salud: SaludComprobadaDelCable,
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
            salud: SaludComprobadaDelCable::desde(fila)?,
        })
    }
}

/// La vista del **alta**: el repositorio creado, más cuál de los dos desenlaces ocurrió.
///
/// `201` con la fila no basta, y el requisito lo dice: el alta **habla con el destino**, así que su
/// respuesta tiene que contar qué pasó con él. «No había repositorio y lo he creado» y «ya había uno
/// y me he conectado» no se leen igual —el primero deja un historial vacío y el segundo uno que la
/// próxima copia viene a continuar—, y sin el campo el operador tendría que abrir el panel del
/// cubo para averiguarlo. El desenlace viaja en `data.outcome`, y su nombre y sus dos valores en
/// inglés los fija el RED de este change, como fija los del explorador: son vocabulario del cable,
/// que va en el inglés del resto del §5 —`status`, `execution_id`— mientras que lo que se pinta se
/// queda en español.
///
/// # Por qué envuelve la vista del repositorio en vez de copiarla
///
/// Porque el alta devuelve **el mismo recurso** que la lectura y el listado y con los mismos
/// campos: lo único que añade es el desenlace. Un segundo tipo con los campos escritos otra
/// vez sería un segundo sitio donde se decide qué se publica de un repositorio —el día que un campo
/// se añada, o que otro cambie de nombre, el que se olvide rompe el contrato en silencio—, y este
/// fichero ya rechaza esa clase de copia en `contenido` y en la tabla de errores. Con el
/// `flatten`, el `data` del alta es la vista del repositorio **y** el campo nuevo, en un objeto
/// plano, que es la forma que las pruebas del cable fijan.
#[derive(Debug, Serialize)]
struct RepositorioCreadoDelCable {
    /// La fila recién escrita, tal y como la publican la lectura y el listado.
    #[serde(flatten)]
    repositorio: RepositorioDelCable,
    /// Cuál de los dos desenlaces ocurrió: `initialized` o `connected`.
    outcome: &'static str,
}

impl RepositorioCreadoDelCable {
    /// Vuelca la fila y el desenlace en la vista del alta.
    fn desde(fila: &Repositorio, puesta: PuestaEnMarcha) -> Result<Self, AppError> {
        Ok(Self {
            repositorio: RepositorioDelCable::desde(fila)?,
            outcome: desenlace(puesta),
        })
    }
}

/// El nombre del desenlace de la puesta en marcha en el vocabulario del cable.
///
/// Es el único sitio donde los dos desenlaces de [`PuestaEnMarcha`] se traducen a lo que viaja, y
/// está aquí y no en `src/rustic` por lo mismo que el nombre de los campos: la biblioteca describe
/// **qué** pasó con el repositorio —`Inicializado` y `Conectado`— y cómo se llama eso en una
/// respuesta de la API es asunto de la capa HTTP, que es la que tiene un contrato que cumplir. El
/// `match` es exhaustivo y sin rama comodín a propósito: un desenlace nuevo lo rompe la compilación
/// en vez de viajar con un texto inventado.
fn desenlace(puesta: PuestaEnMarcha) -> &'static str {
    match puesta {
        PuestaEnMarcha::Inicializado => "initialized",
        PuestaEnMarcha::Conectado => "connected",
    }
}

/// El acceso al destino que describe un **alta**: se construye desde el cuerpo, no desde una fila.
///
/// Es la diferencia de fondo con el resto de sitios que arman un [`Acceso`] —la copia de una tarea
/// lo hace desde la fila, con los secretos que la capa de acceso acaba de descifrar—: en un alta no
/// hay fila todavía, y no la hay **a propósito**, porque la fila se escribe después de que el
/// destino conteste. Así que los secretos que se le entregan a la biblioteca son los que llegaron
/// en la petición, en claro, y por ese camino no pasa la clave maestra: este módulo no descifra
/// nada, y no puede.
///
/// Se copian los cuatro campos y los dos secretos porque el alta necesita las dos cosas —el acceso
/// para hablar con el destino y los valores para la fila—, y se hace aquí, en un sitio con nombre,
/// para que la copia se vea: es el único punto de este fichero donde los dos secretos existen dos
/// veces a la vez.
fn acceso_del_alta(alta: &AltaDeRepositorio) -> Acceso {
    Acceso {
        endpoint: alta.endpoint.clone(),
        bucket: alta.bucket.clone(),
        region: alta.region.clone(),
        access_key_id: alta.access_key_id.clone(),
        secret_access_key: alta.secret_access_key.clone(),
        passphrase: alta.passphrase.clone(),
    }
}

/// El acceso al destino de una **fila**: los dos secretos que la capa de acceso acaba de descifrar.
///
/// Es la conversión simétrica de [`acceso_del_alta`]: allí los secretos vienen del cuerpo —en un
/// alta todavía no hay fila—, y aquí de una fila que ya existe y cuyos dos secretos llegan ya en
/// claro desde la capa de acceso, que es la única que tiene la clave maestra. Este módulo no
/// descifra nada, y no puede: solo copia a lo que la biblioteca necesita lo que la fila trae, y los
/// dos siguen siendo [`SecretString`] hasta el final, así que no pueden acabar en un registro ni en
/// el cuerpo de una respuesta.
///
/// La misma conversión existe hoy en `src/api/snapshots.rs` y en `src/engine/ejecucion.rs`, y
/// ninguna se puede reutilizar sin un salto entre módulos hermanos: el constructor que las
/// unificaría es un `Acceso::desde(&Repositorio)` del módulo del tipo, y mientras no exista la
/// copia de seis campos vive junto a cada uso.
fn acceso_del_repositorio(fila: &Repositorio) -> Acceso {
    Acceso {
        endpoint: fila.endpoint.clone(),
        bucket: fila.bucket.clone(),
        region: fila.region.clone(),
        access_key_id: fila.access_key_id.clone(),
        secret_access_key: fila.secret_access_key.clone(),
        passphrase: fila.passphrase.clone(),
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
pub async fn listado(
    State(state): State<AppState>,
    Query(consulta): Query<ConsultaDelCable>,
) -> Result<ApiResponse, AppError> {
    let (limite, desplazamiento) = consulta.paginacion()?;

    let filas = state.db.repositorios.listar().await?;

    let vistas: Vec<RepositorioDelCable> = filas
        .iter()
        .map(RepositorioDelCable::desde)
        .collect::<Result<_, _>>()?;

    let total = vistas.len() as i64;
    let pagina: Vec<RepositorioDelCable> = vistas
        .into_iter()
        .skip(desplazamiento as usize)
        .take(limite as usize)
        .collect();

    Ok(ApiResponse::new(
        StatusCode::OK,
        "repositorios listados",
        Data::Paginated(PaginatedResponse::new(
            pagina
                .into_iter()
                .map(|v| {
                    serde_json::to_value(v)
                        .map_err(|e| AppError::Other(format!("error serializando: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?,
            total,
            limite,
            desplazamiento,
        )),
    ))
}

/// Juzga el `endpoint` de un cuerpo contra la regla del módulo puro.
///
/// Es la **única** puerta del borde para esta regla: el alta y la edición la llaman aquí y nadie
/// más decide qué `endpoint` vale, así que las dos rutas no pueden discrepar. El motivo se
/// devuelve tal cual —ya nombra el campo `endpoint` y **no** repite la URL recibida, que es la
/// regla de los `400` de esta casa—.
///
/// El permiso entra por parámetro y sale de la configuración, no de la petición: es una decisión
/// del operador sobre el despliegue, no un campo del cuerpo, y por eso un cliente no puede
/// concedérselo a sí mismo.
fn endpoint_admisible(endpoint: &str, permitir_inseguro: bool) -> Result<(), AppError> {
    crate::rustic::endpoint::validar(endpoint, permitir_inseguro)
        .map_err(|error| AppError::InvalidInput(error.to_string()))
}

/// Los cinco campos de texto del cuerpo, ya recortados y medidos contra su cota.
///
/// Los dos secretos **no** están: un secreto no se recorta —recortarlo lo cambiaría— ni se acota.
/// Por eso este tipo no los lleva y quien lo usa los resuelve por su cuenta, que además es distinto
/// en el alta y en la edición.
struct CamposDeTexto {
    name: String,
    endpoint: String,
    bucket: String,
    region: String,
    access_key_id: String,
}

/// Recorta y mide los cinco campos de texto, o contesta el `400` que nombra el que falle.
///
/// La comparten el alta y la edición —las dos puertas de escritura del recurso—, así que las dos
/// aplican la **misma** regla y no hay una tercera que se olvide. El `endpoint` sale ya recortado
/// para que la validación de admisibilidad de después juzgue el valor que se va a guardar y no lo
/// que se envió con espacios alrededor.
fn campos_de_texto(cuerpo: &AltaDeRepositorioDelCable) -> Result<CamposDeTexto, AppError> {
    Ok(CamposDeTexto {
        name: texto::obligatorio("name", Some(cuerpo.name.as_str()), texto::COTA_NOMBRE)?,
        endpoint: texto::obligatorio(
            "endpoint",
            Some(cuerpo.endpoint.as_str()),
            texto::COTA_ENDPOINT,
        )?,
        bucket: texto::obligatorio("bucket", Some(cuerpo.bucket.as_str()), texto::COTA_BUCKET)?,
        region: texto::obligatorio("region", Some(cuerpo.region.as_str()), texto::COTA_REGION)?,
        access_key_id: texto::obligatorio(
            "access_key_id",
            Some(cuerpo.access_key_id.as_str()),
            texto::COTA_ACCESS_KEY,
        )?,
    })
}

/// `POST /api/v1/repositories` — el alta, que habla con el destino y contesta `201`.
///
/// Los dos secretos del cuerpo son `Option`, y aquí se exigen: las columnas del §3 son
/// `NOT NULL`, así que un alta sin ellos no tiene fila que escribir. El «ausente =
/// consérvalo» es una regla del `PUT`, no del alta. El rechazo se contesta nombrando el
/// campo y **sin haber tocado la base** —y desde este change, sin haber hablado tampoco
/// con el destino: se decide sobre el cuerpo, que es lo único que hace falta—.
///
/// # El alta habla con el destino, y la fila se escribe **después**
///
/// Un alta no es un apunte en la base: **pone el repositorio en marcha**. Conecta con el destino
/// que describe el cuerpo y, si ese repositorio no existe todavía, lo **inicializa**; y solo
/// cuando el destino ha contestado bien se escribe la fila. Las dos mitades del contrato están
/// juntas a propósito, y el orden no es un detalle de implementación:
///
/// - sin la primera, el alta contesta `201` sobre un cubo que no existe, una `passphrase` con una
///   errata o un endpoint mal escrito, y el error aparece mucho después, al ejecutar la primera
///   copia, cuando ya nadie lo relaciona con el alta;
/// - sin la segunda, un destino que no responde deja una fila que describe un repositorio
///   inalcanzable: el panel lo lista como disponible, una tarea puede apuntarle y el fallo sale
///   como una ejecución fallida en vez de como un alta rechazada. **Es más barato no escribir que
///   limpiar**, y aquí sale gratis por construcción: hay un solo `INSERT` y va detrás de la única
///   llamada que puede fallar por el destino.
///
/// Lo que **sí** puede quedar a medias es lo de fuera: si el destino se inicializa y la escritura de
/// la fila falla después, queda un repositorio vacío en el almacenamiento y ninguna fila que lo
/// describa. Es el menos malo de los dos desenlaces —un repositorio vacío no le dice a nadie que hay
/// una copia hecha, una fila sin repositorio sí— y se deja escrito en vez de fingir que esto es
/// atómico. Y no se intenta deshacer: borrar lo que se acaba de crear en el destino es más
/// peligroso que dejarlo, porque el destino puede tener contenido que este proceso no puso.
///
/// El `PUT` de este mismo recurso hace lo contrario, y a propósito: **no** conecta ni inicializa.
/// Editar una fila no puede depender de que el destino esté en pie, y el motivo entero está en su
/// manejador.
///
/// # El recorte y las cotas
///
/// Los cinco campos de texto —`name`, `endpoint`, `bucket`, `region` y `access_key_id`— pasan por
/// [`campos_de_texto`], que los recorta y los mide contra su cota antes de nada: el tope se cuenta
/// sobre lo que se va a guardar y un rechazo nombra el campo sin repetir el valor. Los dos secretos
/// —`secret_access_key` y `passphrase`— **no** pasan por ahí: un secreto no se recorta ni se acota.
///
/// # Los dos desenlaces, y el `400` frente al `502`
///
/// La respuesta dice cuál de los dos ocurrió en `data.outcome` —`initialized` o `connected`—, porque
/// `201` con la fila no distingue «lo he creado» de «me he conectado con lo que había» y eso no se
/// lee igual: uno deja un historial vacío y el otro uno que la próxima copia viene a continuar. La
/// forma de la vista está en [`RepositorioCreadoDelCable`].
///
/// Y el fallo lo traduce [`del_motor_en_el_alta`], el **único** sitio donde el código de un
/// [`crate::engine::ErrorDelMotor::Repositorio`] lo elige quien llama: `400` si lo que falla es un
/// dato del formulario —la passphrase que no abre el repositorio que ya está, o un campo que no
/// puede funcionar—, y `502` si el que no contesta es el destino. La frontera es de quién es la
/// culpa, y el porqué está escrito allí, con el de la tabla que **no** elige.
///
/// El oráculo que este change **no** cierra: la validación del `endpoint` es una puerta **previa**
/// —se decide sobre el cuerpo y llega antes de hablar con el destino—, no un sustituto de la
/// comprobación de conectividad. Por eso el alta sigue distinguiendo los dos desenlaces y **no** los
/// uniforma: «la petición no vale» sigue siendo `400` —y desde este change ese `400` ya no depende
/// del destino, porque un `endpoint` inadmisible se rechaza sin llegar a tocarlo—, y «el destino no
/// responde» sigue siendo `502`. La respuesta del alta sigue diciendo cuál de las dos cosas pasó.
///
/// # Lo que cuesta, y por qué **sí** está acotado
///
/// Hablar con el destino no es gratis cuando el destino no está, y el alta lo habla: abre el
/// repositorio con [`crate::rustic::Repositorio::poner_en_marcha`], que va bajo el tope del destino
/// —diez segundos— y declara un reintento acotado (`retry = 1`) en las opciones de `opendal`. Un
/// alta contra un S3 inalcanzable contesta el `502` **dentro de ese plazo**, no en los 41–54 s que
/// se medían antes (cinco reintentos, `DEFAULT_RETRY`). El tope no se pone aquí, en el manejador,
/// sino en la construcción del destino: es una política del **repositorio**, no del alta, y así vale
/// para todo lo que abre o lista —el panel, el `404` de la restauración y esta misma alta— con un
/// solo número.
///
/// Lo que se acepta a cambio, dicho claro: el tope **no distingue** un destino muerto de uno lento.
/// A los diez segundos contesta «el destino no se alcanza» también sobre un cubo que sí está y va
/// despacio, y ese diagnóstico falso es un precio declarado —el operador lo tiene que entender como
/// «no contesta», que es lo que pasó—. Se eligió así porque la alternativa medida era peor: cuarenta
/// segundos de espera en un formulario, sin forma de cancelar el trabajo, y el mismo diagnóstico
/// falso al final. Y lo que **no** se puede prometer es un techo contra un destino que traga la
/// conexión y no contesta: ahí el tope sí acota la **espera** —el hilo de la biblioteca queda
/// abandonado, no se espera por él—, pero el trabajo sigue corriendo por detrás hasta que el
/// transporte se rinda.
///
/// La otra mitad —de quién es la culpa— no cambia: [`del_motor_en_el_alta`] sigue mirando la
/// **variante**, y [`crate::rustic::Error::DestinoNoAlcanzable`] —la que devuelve el tope agotado— es
/// la que produce el `502`.
///
/// # Errors
///
/// `400` — un secreto que falta en el cuerpo, un campo que no puede funcionar, o una passphrase que
/// no abre el repositorio que ya está en el destino. `502` — el destino no se alcanza, o se alcanza y
/// no se deja inicializar. Y `500` — la base no atendió la escritura de la fila, o el hilo que
/// hablaba con el repositorio se cayó: las dos las traduce [`crate::api::del_motor`].
///
/// # El identificador y la fecha los pone la capa de acceso
///
/// Este manejador no inventa ni el `id` ni el `created_at`: los genera el `INSERT` de la capa de
/// acceso, y la respuesta los publica tal y como quedaron en la fila. Un `id` compuesto aquí sería
/// un segundo generador de identificadores que el día que discrepara del de la base dejaría al panel
/// pidiendo una fila que no existe.
pub async fn alta(
    State(state): State<AppState>,
    Json(cuerpo): Json<AltaDeRepositorioDelCable>,
) -> Result<ApiResponse, AppError> {
    // El recorte y las cotas de los cinco campos de texto, antes de nada: los campos de **secreto**
    // no pasan por ahí —un secreto no se recorta ni se acota— y se comprueban aparte.
    let campos = campos_de_texto(&cuerpo)?;

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

    // El `endpoint` se juzga antes de montar la fila y antes de hablar con el destino: es el único
    // campo que decide a dónde conecta el proceso, y un valor inadmisible no puede llegar ni a la
    // base ni al motor. Se juzga el valor **recortado**, que es el que se va a guardar. Los dos
    // secretos se comprueban antes porque ése es el orden que el alta ya tenía y el que sus
    // mensajes tienen probado.
    endpoint_admisible(&campos.endpoint, state.config.allow_insecure_endpoint)?;

    // La fila, montada y **sin escribir**: es el valor con el que se arma el acceso y el que se
    // guardará si el destino contesta bien. Montarla aquí no adelanta nada de lo que el orden evita,
    // porque no toca la base.
    let alta = AltaDeRepositorio {
        name: campos.name,
        endpoint: campos.endpoint,
        bucket: campos.bucket,
        region: campos.region,
        access_key_id: campos.access_key_id,
        secret_access_key,
        passphrase,
    };

    // 1. El destino, que es lo único que puede fallar por fuera. El motor saca esta llamada del
    //    runtime por dentro —la biblioteca entra en pánico si se la llama desde dentro de uno, y con
    //    el backend de S3 ninguna prueba del CI lo vería—, así que aquí solo hay que esperarla.
    let puesta = state
        .motor
        .abrir_repositorio(acceso_del_alta(&alta))
        .await
        .map_err(del_motor_en_el_alta)?;

    // 2. La fila, ya con el destino en marcha: es la mitad que no deja rastro de un alta rechazada.
    let fila = state.db.repositorios.alta(alta).await?;

    let vista = RepositorioCreadoDelCable::desde(&fila, puesta)?;

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
///
/// # Y no conecta con el destino, a diferencia del alta
///
/// Es la asimetría que este change deja escrita a propósito, y **no** es un olvido: editar una fila
/// no puede depender de que el destino esté en pie. El alta habla con el destino porque está
/// **creando** la fila y su trabajo es averiguar si ese repositorio existe o hay que inicializarlo
/// —una pregunta que solo tiene sentido una vez, y cuya respuesta se lleva la respuesta del `201`—;
/// el `PUT` no crea nada: cambia el nombre, el cubo o la credencial de una fila que ya está, y hasta
/// el caso en el que el destino ya no responde tiene que poder arreglarse desde aquí, porque puede
/// ser justo lo que se viene a corregir. Un `PUT` que exigiera al destino convertiría la edición de
/// una errata en una operación que falla precisamente cuando la errata es la que rompe el destino, y
/// dejaría la fila sin forma de arreglarse desde el panel.
///
/// Lo que se paga, y se dice: si el `PUT` cambia el endpoint o la credencial a algo que no funciona,
/// la fila queda guardada y el fallo aparecerá en la primera copia. Es el mismo trato que tenía el
/// alta antes de este change, y se acepta porque la alternativa —no poder editar— es peor.
///
/// Lo que **sí** comparte con el alta desde ahora es la regla del `endpoint`: se juzga en el borde
/// de las dos rutas y en un solo sitio —[`endpoint_admisible`]—, así que un valor inadmisible se
/// rechaza aquí igual que allí y el `PUT` no puede ser la puerta de atrás del `POST`.
pub async fn actualizar(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(cuerpo): Json<AltaDeRepositorioDelCable>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(actual) = state.db.repositorios.leer(&uuid).await? else {
        return Err(repositorio_ausente());
    };

    // El recorte y las cotas de los cinco campos de texto, con la **misma** regla que el alta.
    let campos = campos_de_texto(&cuerpo)?;

    // La edición aplica **la misma** regla de `endpoint` que el alta: sin esto, el `PUT` sería la
    // puerta de atrás del `POST` —bastaría dar de alta un `endpoint` admisible y editarlo después
    // por el que no lo es—. El `404` de la fila ausente se decide antes, igual que antes de este
    // change: la validación no cambia qué desenlace tiene una fila que no está.
    endpoint_admisible(&campos.endpoint, state.config.allow_insecure_endpoint)?;

    let fila = Repositorio {
        id: actual.id,
        name: campos.name,
        endpoint: campos.endpoint,
        bucket: campos.bucket,
        region: campos.region,
        access_key_id: campos.access_key_id,
        secret_access_key: resolver_secreto(
            cuerpo.secret_access_key,
            actual.secret_access_key,
            "secret_access_key",
        )?,
        passphrase: resolver_secreto(cuerpo.passphrase, actual.passphrase, "passphrase")?,
        created_at: actual.created_at,
        // La salud **no** es un campo que el `PUT` edite: se arrastra la que ya tenía la
        // fila para que la vista que se devuelve diga la verdad, pero quien la escribe es
        // la comprobación —`actualizar` ni siquiera nombra esas columnas—.
        health: actual.health,
        health_checked_at: actual.health_checked_at,
        health_detail: actual.health_detail,
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

/// Comprueba la salud de una fila, la **anota** y devuelve el desenlace ya persistido.
///
/// Es la mitad común de las dos rutas de comprobación, y su contrato tiene tres piezas que no se
/// pueden separar:
///
/// 1. **Pregunta al destino** con la comprobación del motor, que la saca del runtime de Tokio por
///    dentro —la biblioteca entra en pánico si se la llama desde dentro de uno— y traduce el
///    desenlace al vocabulario de la columna. Aquí no se decide qué significa cada desenlace: eso
///    vive en un solo traductor, en el motor.
/// 2. **Lo anota en la fila** antes de responder, con [`anotar_salud`](crate::db::entities::Repositorios::anotar_salud),
///    que es la única sentencia que toca esas tres columnas. La respuesta publica lo que quedó
///    escrito y no lo que se calculó: así una lectura posterior del recurso dice **lo mismo** que
///    la comprobación, que es lo que el contrato exige.
/// 3. **Vuelve a leer la fila** para servir la fecha: `health_checked_at` la pone la base con
///    `CURRENT_TIMESTAMP`, así que el valor verdadero solo se conoce leyéndolo. Un segundo `SELECT`
///    sobre una fila que acaba de escribirse es barato y no tiene carrera —nadie más escribe esas
///    columnas—, y a cambio la fecha que viaja es la del reloj de la fila y no la del proceso.
///
/// # El desenlace del destino **no** es un error HTTP
///
/// «No hay repositorio», «la passphrase no abre», «el destino no se alcanza» y «abre pero no deja
/// listar» son lo que la comprobación ha **medido**, no fallos de la petición: viajan como el estado
/// de la fila y un `200`, y el panel los pinta. Lo único que sale como error es que el hilo que
/// hablaba con la biblioteca se haya caído, y eso lo traduce [`del_motor`] a un `500`.
///
/// # `Ok(None)` es «la fila se borró por el camino»
///
/// La comprobación ocurrió —y costó lo suyo—, pero ya no hay dónde anotarla. No es un fallo del
/// servidor y no se puede inventar un desenlace para una fila que no está: quien llama decide. La
/// ruta de un repositorio lo cuenta como el mismo `404` de una fila que no existe, y la de todos
/// **salta** esa fila, porque un repositorio que se borró mientras se comprobaba no convierte en
/// fallida la comprobación de los demás.
async fn comprobar_y_anotar(
    state: &AppState,
    fila: &Repositorio,
) -> Result<Option<SaludComprobadaDelCable>, AppError> {
    let (salud, motivo) = state
        .motor
        .comprobar_salud(acceso_del_repositorio(fila))
        .await
        .map_err(del_motor)?;

    if !state
        .db
        .repositorios
        .anotar_salud(&fila.id, salud, motivo.as_deref())
        .await?
    {
        return Ok(None);
    }

    let Some(anotada) = state.db.repositorios.leer(&fila.id).await? else {
        return Ok(None);
    };

    Ok(Some(SaludComprobadaDelCable::desde(&anotada)?))
}

/// `POST /api/v1/repositories/{id}/health` — comprueba un repositorio, lo anota y lo devuelve.
///
/// Es una acción con **efecto de escritura** —persiste el desenlace—, y por eso es `POST` y no
/// `GET`: un `GET` que escribiera rompería lo que un `GET` promete, y el panel lo podría disparar
/// sin querer. Lo que se pide es «mira **ahora** cómo está este repositorio y déjalo anotado», y
/// eso no es leer un recurso.
///
/// El identificador que no tiene forma de UUID es un `400` que nombra el campo, y una fila que no
/// está es un `404`, igual que en la lectura: las dos puertas comprueban lo mismo antes de mirar el
/// destino.
///
/// # Errors
///
/// `400` — el identificador de la ruta no es un UUID. `404` — no hay fila con ese identificador (ni
/// la había, ni se borró mientras se comprobaba). `500` — el hilo que hablaba con el repositorio se
/// cayó, o la base no atendió la anotación. Y el destino que no contesta **no** es un error: llega
/// como estado `inaccesible` con `200`.
pub async fn comprobar_salud(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<ApiResponse, AppError> {
    let uuid = identificador(&id)?;

    let Some(fila) = state.db.repositorios.leer(&uuid).await? else {
        return Err(repositorio_ausente());
    };

    let Some(salud) = comprobar_y_anotar(&state, &fila).await? else {
        return Err(repositorio_ausente());
    };

    Ok(ApiResponse::new(
        StatusCode::OK,
        "salud del repositorio comprobada",
        contenido(&salud)?,
    ))
}

/// `POST /api/v1/repositories/health` — comprueba **todos** y devuelve uno por repositorio.
///
/// Recorre los repositorios del listado, comprueba y anota cada uno —con el mismo camino que la
/// ruta de uno solo— y devuelve un elemento por cada fila que seguía existiendo al comprobarla. El
/// orden es el del listado, que ordena por `created_at` con el identificador de desempate.
///
/// # Lo que cuesta, y por qué es una acción explícita
///
/// Cada comprobación va bajo el tope del destino —diez segundos—, así que el peor caso es **N ×
/// diez segundos**: con diez repositorios caídos, casi dos minutos. Es el precio declarado de
/// comprobar de verdad y por eso esto no lo dispara el panel al cargar: lo pide el operador, con un
/// botón, y sabe que tarda. Un repositorio que se borre a mitad de la vuelta se salta, y los demás
/// se comprueban igual.
///
/// # Errors
///
/// `500` — el hilo que hablaba con un repositorio se cayó, o la base no atendió una anotación. Un
/// destino que no contesta no es un error: llega como estado `inaccesible` dentro de su elemento.
pub async fn comprobar_salud_de_todos(
    State(state): State<AppState>,
) -> Result<ApiResponse, AppError> {
    let filas = state.db.repositorios.listar().await?;

    let mut saludes = Vec::with_capacity(filas.len());
    for fila in &filas {
        if let Some(salud) = comprobar_y_anotar(&state, fila).await? {
            saludes.push(salud);
        }
    }

    Ok(ApiResponse::new(
        StatusCode::OK,
        "salud de los repositorios comprobada",
        contenido(&saludes)?,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::{StatusCode, header};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::db::entities::SaludDeRepositorio;
    use crate::rustic::{Acceso, BarrasDelMotor, LoQueContestaElDestino};
    use crate::state::EstadoDeBase;
    use crate::test_support::{
        OTRA_PASSPHRASE_DE_PRUEBA, PASSPHRASE_DE_PRUEBA, abrir_local, base_con_clave, body_json,
        body_text, capturar_log, config_con_dsn, config_con_dsn_y_endpoint_inseguro, delete,
        destino_local_de_prueba, directorio_de_prueba, endpoint_de_prueba, estado_de_prueba,
        estado_de_prueba_con_comprobador, estado_desarrollo, get, post, post_crudo, post_json,
        put_json, ruta_del_endpoint, sembrar_snapshots,
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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let cuerpo_de_la_peticion = cuerpo_de_alta("repo-1-3");
        let endpoint_informado = cuerpo_de_la_peticion["endpoint"]
            .as_str()
            .expect("el cuerpo de la prueba informa el endpoint")
            .to_owned();

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_de_la_peticion,
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
            endpoint, endpoint_informado,
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

        let filas = cuerpo["data"].as_object().expect("data debe ser objeto")["items"]
            .as_array()
            .expect("items debe ser array");

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

        let filas = cuerpo["data"].as_object().expect("data")["items"]
            .as_array()
            .expect("items debe ser array");

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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

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

    #[tokio::test]
    async fn un_put_con_el_endpoint_inadmisible_sobre_una_fila_inexistente_contesta_404() {
        // El orden importa y es lo que mide esta prueba: en `actualizar` la fila se busca **antes**
        // de juzgar el `endpoint`, así que un `PUT` a una fila que no está es un `404` aunque su
        // `endpoint` tampoco valga. Si el `400` de la validación se adelantara, el panel dejaría de
        // poder distinguir «no existe» de «no vale».
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let id = Uuid::new_v4().to_string();

        let respuesta = put_json(
            api_router(estado),
            &format!("/api/v1/repositories/{id}"),
            &cuerpo_contra(
                "repo-cruce-404",
                "http://s3.example.test/cruce",
                PASSPHRASE_DE_PRUEBA,
            ),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "la fila ausente se decide antes que el `endpoint`: {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["status"], 404,
            "la envoltura repite el código: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn un_alta_sin_secreto_con_el_endpoint_inadmisible_contesta_400_por_el_secreto() {
        // El otro cruce: en `alta` los dos secretos obligatorios se comprueban **antes** que el
        // `endpoint`, así que a un cuerpo al que le falta `secret_access_key` y además trae un
        // `endpoint` por `http` se le contesta por el secreto. El mensaje tiene que nombrar el
        // campo del secreto, no el `endpoint`.
        let router = api_router(estado_desarrollo().await);

        let mut cuerpo = cuerpo_contra(
            "repo-cruce-400",
            "http://s3.example.test/cruce",
            PASSPHRASE_DE_PRUEBA,
        );
        cuerpo
            .as_object_mut()
            .expect("el cuerpo es un objeto")
            .remove("secret_access_key");

        let respuesta = post_json(router, "/api/v1/repositories", &cuerpo).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un alta sin secreto es un 400, y se decide antes que el `endpoint`: {status} con {cuerpo}"
        );

        let mensaje = cuerpo["message"].as_str().unwrap_or_default();
        assert!(
            mensaje.contains("secret_access_key"),
            "el 400 tiene que nombrar el campo que falta: {cuerpo}"
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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

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

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

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
    ///
    /// El `endpoint` es una URL `https` de anfitrión fingido cuyo **camino** es la ruta de un
    /// directorio temporal —lo que deja [`destino_local_de_prueba`]—, y eso no es un detalle del
    /// arnés: desde el change `backup-engine` el alta **habla con el destino** —lo inicializa si
    /// está vacío—, así que un `endpoint` cuyo camino no exista convierte un alta válida en un fallo
    /// por destino inalcanzable. El abridor local del arnés traduce ese camino a la ruta y abre ahí
    /// el repositorio, y [`destino_local_de_prueba`] deja además la ruta viva mientras el proceso de
    /// pruebas corra, que es lo que hace falta porque el alta ocurre después de construir el cuerpo.
    fn cuerpo_de_alta(nombre: &str) -> serde_json::Value {
        serde_json::json!({
            "name": nombre,
            "endpoint": destino_local_de_prueba(nombre),
            "bucket": format!("cubo-{nombre}"),
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "AKIAIOSFODNN7EXAMPLE-secreto-de-prueba",
            "passphrase": "passphrase-distinguible-00000000000000000000000001",
        })
    }

    /// 1.10 — Un `GET` sobre una fila inexistente contesta `404` con la envoltura.
    ///
    /// Cubre `api-resources`, requisito 2 (escenario «Un GET sobre una fila inexistente
    /// contesta 404»). El identificador es un UUID **bien formado** que no existe —y no una
    /// cadena cualquiera— a propósito: así el `404` lo decide la ausencia de la fila y no el
    /// rechazo del identificador, que sería otro caso y ya tiene su prueba. El `404` es para
    /// el recurso que no está, y ésa es la forma en que el panel distingue «no existe» de
    /// «no puedes»: la prueba hermana del `PUT` (1.12) ya lo fija para la escritura, y ésta
    /// lo fija para la lectura, que es la que el panel pide primero al abrir una ficha.
    ///
    /// El cuerpo se mira y no solo el código porque la envoltura única es el contrato del
    /// §5: un `404` pelado, sin `status` ni `message`, dejaría al panel sin nada que pintar.
    #[tokio::test]
    async fn un_get_de_una_fila_inexistente_contesta_404_con_la_envoltura() {
        let router = api_router(estado_desarrollo().await);

        let id = Uuid::new_v4().to_string();

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "un GET sobre una fila que no existe tiene que contestar 404; contestó {status} \
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
            "el 404 tiene que explicar que no hay fila que leer: {cuerpo}"
        );
    }

    /// 1.14 — El identificador del cable es la forma canónica con guiones.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «El identificador del cable es la forma
    /// canónica con guiones»). El §3 guarda el identificador como `TEXT` y la trampa medida
    /// por este proyecto es que un `Uuid` de `sqlx` enlazado contra una columna `TEXT` se
    /// escribe como un **BLOB de 16 bytes** ilegible con `sqlite3`; la forma canónica con
    /// guiones es la que hace la tabla legible a mano y la única que el panel puede meter en
    /// una URL. Por eso la prueba no se queda en el aspecto del texto: además lo **relee**
    /// con el mismo camino que usará el panel al construir la ruta del `GET`.
    #[tokio::test]
    async fn el_identificador_del_cable_es_la_forma_canonica_con_guiones() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-14"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder mirar el identificador"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let uuid = Uuid::parse_str(&id).expect(
            "el identificador tiene que poder releerse como UUID —el panel lo usa para \
             construir las rutas de GET, PUT y DELETE—",
        );

        assert_eq!(
            uuid.hyphenated().to_string(),
            id,
            "el identificador releído tiene que volver a escribirse igual: {id}"
        );

        let posiciones: Vec<usize> = id
            .char_indices()
            .filter(|(_, caracter)| *caracter == '-')
            .map(|(indice, _)| indice)
            .collect();

        assert_eq!(
            posiciones,
            vec![8, 13, 18, 23],
            "los guiones tienen que estar en las posiciones de la forma canónica: {id}"
        );

        assert_eq!(
            id.len(),
            36,
            "la forma canónica con guiones mide 36 bytes, y éste mide {}: {id}",
            id.len()
        );

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "el identificador que salió del alta tiene que servir para pedir la fila: {id}"
        );
    }

    /// 1.15 — La fecha del cable se relee con RFC3339 y es el mismo instante.
    ///
    /// Cubre `api-resources`, requisito 1 (escenario «La fecha del cable se relee con RFC3339
    /// y es el mismo instante»). El contrato fija el literal medido
    /// (`2026-10-25T12:34:56Z`: emite `Z` y **omite la fracción** cuando los nanosegundos son
    /// cero), y esta prueba es la que impide que una actualización de `time` cambie la forma
    /// del cable sin que nadie se entere: la serialización por defecto de `time` en este
    /// crate **no** produce una cadena —`serde-human-readable` es una feature aparte y no
    /// está activada—, así que una fecha que no fuera cadena delataría que alguien cambió el
    /// formateo. La última mitad es la que de verdad importa: la fecha publicada tiene que
    /// ser **la que la base guardó** y no la del reloj de la respuesta.
    #[tokio::test]
    async fn la_fecha_del_cable_se_relee_con_rfc3339_y_es_el_mismo_instante() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let respuesta = post_json(
            api_router(estado.clone()),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-1-15"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder mirar la fecha"
        );

        let cuerpo = body_json(respuesta).await;

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let texto = cuerpo["data"]["created_at"]
            .as_str()
            .expect(
                "la fecha del cable tiene que ser una **cadena** RFC3339 y no un número: \
                     `time` sin `serde-human-readable` serializa por defecto en su forma \
                     binaria, y una fecha numérica delataría ese cambio",
            )
            .to_owned();

        let instante_publicado = OffsetDateTime::parse(&texto, &Rfc3339).expect(
            "la fecha del cable tiene que releerse con RFC3339, que es el formato que el \
             contrato fija",
        );

        assert!(
            texto.ends_with('Z'),
            "la fecha del cable termina en `Z` cuando el instante va en UTC: {texto}"
        );

        assert!(
            !texto.contains('.'),
            "la fecha del cable omite la fracción de segundo cuando los nanosegundos son \
             cero, y ésta la lleva: {texto}"
        );

        let uuid = Uuid::parse_str(&id).expect("el identificador del alta tiene que ser un UUID");

        let fila = estado
            .db
            .repositorios
            .leer(&uuid)
            .await
            .expect("la fila recién dada de alta tiene que poder releerse")
            .expect("la fila recién dada de alta tiene que seguir en la base");

        assert_eq!(
            fila.created_at, instante_publicado,
            "la fecha que publica la API tiene que ser la que guardó la base, y la base \
             guardó {} mientras el cable publicó {texto}",
            fila.created_at
        );
    }

    /// 1.16 — El alta y la lectura no traen los secretos y sí los booleanos.
    ///
    /// Cubre `api-resources`, requisito 4 (escenario «El alta y la lectura no traen los
    /// secretos y sí los booleanos»). El §3 guarda las dos credenciales y el panel necesita
    /// saber **si están configuradas** para pintar el campo como ya relleno; devolver el
    /// valor convertiría cada `GET` en una fuga que viaja al navegador y a cualquier registro
    /// intermedio, así que de cada credencial sale un booleano derivado y nunca el valor. Los
    /// valores que se buscan en el texto crudo se leen del **propio cuerpo de la petición** y
    /// no se escriben aquí: si el ayudante cambia, la prueba sigue midiendo lo correcto. Y se
    /// mira el texto crudo además del JSON porque un campo que se colara **dentro de otro**
    /// —o un `data` que serializara la entidad entera— no aparecería como clave propia y sí
    /// en el texto.
    #[tokio::test]
    async fn el_alta_y_la_lectura_no_traen_los_secretos_y_si_los_booleanos() {
        /// Comprueba las cuatro propiedades de la vista de un repositorio, en el JSON y en
        /// el texto que viajó por el cable.
        fn comprobar_vista(
            etiqueta: &str,
            cuerpo: &serde_json::Value,
            texto: &str,
            secretos: [&str; 2],
        ) {
            let data = &cuerpo["data"];

            for (campo, presente) in [
                (
                    "has_secret_access_key",
                    data["has_secret_access_key"].as_bool(),
                ),
                ("has_passphrase", data["has_passphrase"].as_bool()),
            ] {
                assert_eq!(
                    presente,
                    Some(true),
                    "el {etiqueta} tiene que decir que `{campo}` está guardada: {cuerpo}"
                );
            }

            for campo in ["secret_access_key", "passphrase"] {
                assert!(
                    data[campo].is_null(),
                    "el {etiqueta} no puede traer `{campo}`: de un secreto guardado sale un \
                     booleano y nunca el valor: {cuerpo}"
                );
            }

            for secreto in secretos {
                assert!(
                    !texto.contains(secreto),
                    "el {etiqueta} no puede contener el secreto ni dentro de otro campo, y \
                     el texto crudo lo lleva: {texto}"
                );
            }
        }

        let router = api_router(estado_desarrollo().await);

        let cuerpo_de_peticion = cuerpo_de_alta("repo-1-16");

        let secreto = cuerpo_de_peticion["secret_access_key"]
            .as_str()
            .expect("el cuerpo de alta del ayudante tiene que traer `secret_access_key`")
            .to_owned();
        let passphrase = cuerpo_de_peticion["passphrase"]
            .as_str()
            .expect("el cuerpo de alta del ayudante tiene que traer `passphrase`")
            .to_owned();

        let respuesta =
            post_json(router.clone(), "/api/v1/repositories", &cuerpo_de_peticion).await;

        let status = respuesta.status();
        let texto_del_alta = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta tiene que contestar 201; contestó {status} con {texto_del_alta}"
        );

        let cuerpo_del_alta: serde_json::Value = serde_json::from_str(&texto_del_alta)
            .expect("el cuerpo del alta tiene que ser JSON válido");

        comprobar_vista(
            "alta",
            &cuerpo_del_alta,
            &texto_del_alta,
            [&secreto, &passphrase],
        );

        let id = cuerpo_del_alta["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        let status = respuesta.status();
        let texto_de_la_lectura = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "la lectura de la fila que se acaba de dar de alta tiene que contestar 200; \
             contestó {status} con {texto_de_la_lectura}"
        );

        let cuerpo_de_la_lectura: serde_json::Value = serde_json::from_str(&texto_de_la_lectura)
            .expect("el cuerpo de la lectura tiene que ser JSON válido");

        comprobar_vista(
            "lectura",
            &cuerpo_de_la_lectura,
            &texto_de_la_lectura,
            [&secreto, &passphrase],
        );
    }

    // ── El alta habla con el destino ─────────────────────────────────────────
    //
    // Cubre `api-resources`, requisito «El alta de un repositorio conecta con él, y lo inicializa
    // si todavía no existe», con sus cuatro escenarios. El instrumento es el abridor **local** del
    // arnés: el `endpoint` de la fila es una URL `https` de anfitrión fingido cuyo camino es la ruta
    // de un directorio temporal —el abridor local traduce ese camino y abre ahí—, así que los cuatro
    // desenlaces se recorren sin red, sin S3 y sin credenciales —que es lo que permite que esto
    // corra en el CI—.

    /// El secreto de estas pruebas, distinguible para que un `contains` no dé un falso positivo.
    const SECRETO_DEL_ALTA_1_31: &str = "AKIAIOSFODNN7EXAMPLE-secreto-del-alta-1-31";

    /// El cuerpo de un alta contra un destino y con una passphrase **elegidos**.
    ///
    /// El ayudante general del módulo informa un destino nuevo en cada llamada; estas pruebas
    /// necesitan decidir el destino —para poder abrirlo antes, o para que no sea un directorio— y
    /// la passphrase —para que no abra—, así que construyen el cuerpo aquí.
    fn cuerpo_contra(nombre: &str, endpoint: &str, passphrase: &str) -> serde_json::Value {
        serde_json::json!({
            "name": nombre,
            "endpoint": endpoint,
            "bucket": format!("cubo-{nombre}"),
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": SECRETO_DEL_ALTA_1_31,
            "passphrase": passphrase,
        })
    }

    /// Cuántas filas hay en `repositories`, contadas con SQL directo sobre el fichero.
    ///
    /// Se cuenta por SQL y no por el listado de la API porque lo que hay que afirmar es que **no
    /// quedó fila**: un listado que contestara `200` con un array vacío por otro motivo —o una
    /// lectura que fallara— diría lo mismo sin haber mirado la tabla.
    async fn filas_de_repositorios(pool: &sqlx::SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM repositories")
            .fetch_one(pool)
            .await
            .expect("la tabla de repositorios tiene que poder contarse")
    }

    /// Todo lo que hay dentro de un directorio, con el contenido de cada fichero.
    ///
    /// Es lo que permite afirmar que un destino **no** se tocó: el nombre y el tamaño no bastan
    /// —un fichero reescrito con el mismo tamaño pasaría—, así que se compara el contenido.
    fn inventario(ruta: &Path) -> Vec<(String, Vec<u8>)> {
        let mut encontrado = Vec::new();
        let mut pendientes = vec![ruta.to_path_buf()];

        while let Some(actual) = pendientes.pop() {
            let Ok(entradas) = std::fs::read_dir(&actual) else {
                continue;
            };

            for entrada in entradas.flatten() {
                let camino = entrada.path();

                if camino.is_dir() {
                    pendientes.push(camino);
                } else if let Ok(contenido) = std::fs::read(&camino) {
                    encontrado.push((camino.to_string_lossy().into_owned(), contenido));
                }
            }
        }

        encontrado.sort_by(|uno, otro| uno.0.cmp(&otro.0));

        encontrado
    }

    /// 1.31a — Un alta contra un destino **vacío** lo inicializa, y la respuesta lo dice.
    ///
    /// Cubre `api-resources`, escenario «Un alta contra un destino vacío lo inicializa». Se
    /// afirman las tres cosas a la vez porque ninguna basta: que el alta contesta `201` (el
    /// contrato del alta), que la respuesta **dice** cuál de los dos desenlaces ocurrió —un `201`
    /// mudo dejaría al operador sin saber si su repositorio se creó o ya estaba— y que el destino
    /// quedó con un repositorio de verdad, que se comprueba **abriéndolo con la biblioteca**: es el
    /// camino que recorre quien restaura, y un repositorio que solo abriera el motor no está
    /// probado.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_alta_contra_un_destino_vacio_lo_inicializa() {
        let router = api_router(estado_desarrollo().await);

        let destino = directorio_de_prueba();

        let respuesta = post_json(
            router,
            "/api/v1/repositories",
            &cuerpo_contra(
                "repo-1-31a",
                &endpoint_de_prueba(destino.ruta()),
                PASSPHRASE_DE_PRUEBA,
            ),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta contra un destino vacío tiene que contestar 201; contestó {status} con \
             {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["outcome"], "initialized",
            "la respuesta del alta tiene que decir que el repositorio se ha inicializado, y \
             llegó {cuerpo}"
        );

        // Y el destino quedó inicializado de verdad: la biblioteca lo abre con esa passphrase.
        let _ = abrir_local(destino.ruta(), PASSPHRASE_DE_PRUEBA);
    }

    /// 1.31b — Un alta contra un destino que **ya tiene** repositorio conecta, y lo dice.
    ///
    /// Cubre `api-resources`, escenario «Un alta contra un destino que ya tiene repositorio
    /// conecta». El destino se siembra con la **biblioteca** y no con el motor, para que el alta
    /// tenga que darse cuenta de que el repositorio ya está: si la discriminación fuera «abro y si
    /// falla, inicializo», este escenario seguiría contestando `201` —y diría lo contrario de lo
    /// que pasó—, así que la prueba afirma el **desenlace** y no solo el código.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_alta_contra_un_destino_con_repositorio_conecta() {
        let router = api_router(estado_desarrollo().await);

        let destino = directorio_de_prueba();
        let antes = inventario(destino.ruta());

        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let respuesta = post_json(
            router,
            "/api/v1/repositories",
            &cuerpo_contra(
                "repo-1-31b",
                &endpoint_de_prueba(destino.ruta()),
                PASSPHRASE_DE_PRUEBA,
            ),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el alta contra un destino que ya tiene repositorio tiene que contestar 201 y no \
             fallar por «ya existe»; contestó {status} con {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["outcome"], "connected",
            "la respuesta del alta tiene que decir que se ha conectado con un repositorio que ya \
             estaba, y llegó {cuerpo}"
        );

        // Y un alta que conecta no vuelve a inicializar nada: lo que había sigue ahí.
        let despues = inventario(destino.ruta());

        assert!(
            despues.starts_with(&antes),
            "el alta borró o reescribió ficheros del repositorio que ya estaba: de {} a {} \
             entradas",
            antes.len(),
            despues.len()
        );
    }

    /// 1.31c — Un alta contra un destino inalcanzable contesta un error y **no deja fila**.
    ///
    /// Cubre `api-resources`, escenario «Un alta contra un destino inalcanzable no deja fila». El
    /// destino es un **fichero** donde el motor espera un directorio: la biblioteca no puede
    /// listarlo ni escribir en él, así que el desenlace es el de un destino que no responde, y se
    /// recorre sin red. Se afirman tres cosas: que el error sale por la envoltura, que **no queda
    /// fila** —una fila que describe un repositorio inalcanzable hace que el panel lo liste como
    /// disponible y que el fallo aparezca en la primera copia— y que el error no publica la clave
    /// secreta.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_alta_contra_un_destino_inalcanzable_contesta_error_y_no_deja_fila() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        // El destino es un fichero de verdad en la ruta donde el motor espera un directorio.
        let endpoint = destino_local_de_prueba("repo-1-31c");
        let ruta = ruta_del_endpoint(&endpoint);
        std::fs::write(&ruta, b"esto no es un repositorio").expect("el fichero del destino");

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-1-31c", &endpoint, PASSPHRASE_DE_PRUEBA),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_GATEWAY,
            "un destino que no responde es un fallo del servicio de fuera, no de la petición: \
             contestó {status} con {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el error tiene que salir por la envoltura");

        assert_eq!(
            cuerpo["status"], 502,
            "la envoltura tiene que repetir el código: {texto}"
        );

        assert_eq!(
            filas_de_repositorios(&pool).await,
            0,
            "un alta que no pudo hablar con el destino dejó una fila: el panel lo listaría como \
             disponible y el fallo saldría en la primera copia"
        );

        assert!(
            !texto.contains(SECRETO_DEL_ALTA_1_31),
            "el error publicó la clave secreta que iba en la petición: {texto}"
        );
        assert!(
            !texto.contains(PASSPHRASE_DE_PRUEBA),
            "el error publicó la passphrase que iba en la petición: {texto}"
        );
    }

    /// 1.31d — Un alta cuya passphrase no abre el repositorio existente no deja fila **ni toca el
    /// destino**.
    ///
    /// Cubre `api-resources`, escenario «Un alta cuya passphrase no abre el repositorio existente
    /// no deja fila», que es el desenlace que más importa de los cuatro: es el error de una errata
    /// en el formulario, y sin esta mitad el panel daría por buena una fila que apunta a un
    /// repositorio al que no puede acceder. Se afirman las tres cosas —el `400`, la ausencia de
    /// fila y que el destino queda **byte a byte** como estaba—: un alta que «probara» escribiendo
    /// algo antes de comprobar la passphrase pasaría las dos primeras y no la tercera.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_alta_cuya_passphrase_no_abre_no_deja_fila_ni_toca_el_destino() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let destino = directorio_de_prueba();

        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let antes = inventario(destino.ruta());

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra(
                "repo-1-31d",
                &endpoint_de_prueba(destino.ruta()),
                OTRA_PASSPHRASE_DE_PRUEBA,
            ),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "una passphrase que no abre el repositorio que ya está es un fallo del formulario, no \
             del servicio de fuera: contestó {status} con {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el error tiene que salir por la envoltura");

        assert_eq!(cuerpo["status"], 400, "la envoltura del 400: {texto}");

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 400 tiene que explicar que la passphrase no abre: {texto}"
        );

        assert_eq!(
            filas_de_repositorios(&pool).await,
            0,
            "el alta con la passphrase equivocada dejó fila"
        );

        assert_eq!(
            inventario(destino.ruta()),
            antes,
            "el alta con la passphrase equivocada modificó el destino"
        );

        assert!(
            !texto.contains(SECRETO_DEL_ALTA_1_31),
            "el error publicó la clave secreta que iba en la petición: {texto}"
        );
    }

    // ── El borde valida el `endpoint` ────────────────────────────────────────
    //
    // Cubre `api-resources`, requisito «El `endpoint` de una fila de repositorio tiene que ser
    // admisible». El `endpoint` es el único campo de la fila que decide **a dónde** conecta el
    // proceso, y su comprobación es pura: no sale a la red ni toca el destino, así que un valor
    // inadmisible se corta en el borde con un `400` y sin escribir nada. Los cinco desenlaces
    // —los dos rechazos, los dos contrapesos y el del permiso declarado— se recorren aquí.

    /// El `endpoint` de prueba con el esquema rebajado a `http`.
    ///
    /// El valor de [`destino_local_de_prueba`] es una URL `https://destino.local/<ruta>`; lo único
    /// que se cambia es el esquema, así que el resto de la URL —anfitrión y camino— queda igual y
    /// el único motivo de rechazo posible es el esquema.
    fn con_esquema_http(endpoint: &str) -> String {
        endpoint.replacen("https://", "http://", 1)
    }

    /// El `endpoint` de prueba con el anfitrión rebajado al loopback.
    ///
    /// Se queda el **camino** de la URL de prueba —que es la ruta real donde el abridor local
    /// abre el repositorio— y se le pone `127.0.0.1` de anfitrión, así que el único motivo de
    /// rechazo posible es la dirección.
    fn con_anfitrion_de_loopback(endpoint: &str) -> String {
        format!("https://127.0.0.1{}", ruta_del_endpoint(endpoint).display())
    }

    /// 5.1 — Un alta con el `endpoint` por `http` y sin permiso contesta `400` y no deja fila.
    #[tokio::test]
    async fn un_alta_con_el_endpoint_por_http_sin_permiso_contesta_400_y_no_deja_fila() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let endpoint = con_esquema_http(&destino_local_de_prueba("repo-5-1"));

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-5-1", &endpoint, PASSPHRASE_DE_PRUEBA),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `endpoint` por http sin permiso es una petición mal formada: contestó {status} \
             con {texto}"
        );
        assert!(
            texto.contains("endpoint"),
            "el 400 tiene que nombrar el campo: {texto}"
        );
        assert!(
            !texto.contains("destino.local"),
            "el 400 no puede repetir el `endpoint` recibido: {texto}"
        );
        assert_eq!(
            filas_de_repositorios(&pool).await,
            0,
            "un alta rechazada no puede dejar fila"
        );
    }

    /// 5.2 — Un alta con el `endpoint` al loopback y sin permiso contesta `400` y no deja fila.
    #[tokio::test]
    async fn un_alta_con_el_endpoint_al_loopback_sin_permiso_contesta_400_y_no_deja_fila() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let endpoint = con_anfitrion_de_loopback(&destino_local_de_prueba("repo-5-2"));

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-5-2", &endpoint, PASSPHRASE_DE_PRUEBA),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `endpoint` contra el loopback sin permiso es una petición mal formada: contestó \
             {status} con {texto}"
        );
        assert!(
            texto.contains("endpoint"),
            "el 400 tiene que nombrar el campo: {texto}"
        );
        assert_eq!(
            filas_de_repositorios(&pool).await,
            0,
            "un alta rechazada no puede dejar fila"
        );
    }

    /// 5.3 — Un `endpoint` admisible contra un destino que no responde sigue siendo `502`.
    ///
    /// Es el contrapeso del oráculo que este change **no** cierra: el `endpoint` pasa la
    /// validación —es una URL `https` con anfitrión público— y el fallo es del destino, así que el
    /// código tiene que seguir siendo el `502` de siempre y no un `400`. El destino es un fichero
    /// de verdad en la ruta donde el motor espera un directorio, que es lo que hace que el motor
    /// falle en el acto y sin salir de la máquina.
    #[tokio::test]
    async fn un_endpoint_admisible_contra_un_destino_que_falla_sigue_contestando_502() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let endpoint = destino_local_de_prueba("repo-5-3");
        std::fs::write(ruta_del_endpoint(&endpoint), b"esto no es un repositorio")
            .expect("el fichero del destino");

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-5-3", &endpoint, PASSPHRASE_DE_PRUEBA),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_GATEWAY,
            "un destino que no responde es un fallo del servicio de fuera, no de la petición: \
             contestó {status} con {texto}"
        );
        assert_eq!(
            filas_de_repositorios(&pool).await,
            0,
            "un alta contra un destino que falla no puede dejar fila"
        );
    }

    /// 5.4 — Un `PUT` con el `endpoint` por `http` y sin permiso contesta `400` y no cambia la fila.
    #[tokio::test]
    async fn un_put_con_el_endpoint_por_http_sin_permiso_contesta_400_y_no_cambia_la_fila() {
        let router = api_router(estado_desarrollo().await).clone();

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-5-4"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta de partida tiene que contestar 201"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador")
            .to_owned();
        let endpoint = cuerpo["data"]["endpoint"]
            .as_str()
            .expect("la vista del alta trae el endpoint")
            .to_owned();

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/repositories/{id}"),
            &cuerpo_contra(
                "repo-5-4-modificado",
                &con_esquema_http(&endpoint),
                PASSPHRASE_DE_PRUEBA,
            ),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `endpoint` por http sin permiso es una petición mal formada: contestó {status} \
             con {texto}"
        );
        assert!(
            texto.contains("endpoint"),
            "el 400 tiene que nombrar el campo: {texto}"
        );

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "la fila tiene que seguir estando"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["name"], "repo-5-4",
            "un PUT rechazado no puede haber cambiado el nombre"
        );
        assert_eq!(
            cuerpo["data"]["endpoint"], endpoint,
            "un PUT rechazado no puede haber cambiado el endpoint"
        );
    }

    /// 5.5 — Con el permiso declarado, el alta por `http` contra el loopback sigue adelante.
    ///
    /// Es el contrapeso que justifica que el permiso exista: es el caso del laboratorio local y el
    /// de la batería e2e —un almacén en la propia máquina y en claro—, y sin este desenlace la
    /// validación sería una prohibición sin salida.
    #[tokio::test]
    async fn con_el_permiso_declarado_el_alta_por_http_al_loopback_sigue_adelante() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn_y_endpoint_inseguro(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let endpoint = format!(
            "http://127.0.0.1{}",
            ruta_del_endpoint(&destino_local_de_prueba("repo-5-5")).display()
        );

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-5-5", &endpoint, PASSPHRASE_DE_PRUEBA),
        )
        .await;

        let status = respuesta.status();
        let texto = body_text(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "con el permiso declarado el alta tiene que seguir adelante: contestó {status} con \
             {texto}"
        );
        assert_eq!(
            filas_de_repositorios(&pool).await,
            1,
            "el alta admitida tiene que dejar su fila"
        );
    }

    /// 5.6 — Un `endpoint` con espacios alrededor se guarda **recortado**.
    ///
    /// El `endpoint` se juzga por su valor **recortado**, y el valor que se guarda es el recortado:
    /// el diseño no dice «se acepta con espacios», dice que los espacios de los extremos no forman
    /// parte del valor. Lo que fija esta prueba es que la fila guarda el valor limpio y que la
    /// petición sigue adelante: si el recorte se perdiera, la columna llevaría los espacios y la
    /// conexión posterior saldría contra una URL que no es la que el operador quiso.
    #[tokio::test]
    async fn un_endpoint_con_espacios_se_guarda_recortado() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
        );

        let endpoint = destino_local_de_prueba("repo-5-6");

        let mut cuerpo_de_la_peticion = cuerpo_contra("repo-5-6", &endpoint, PASSPHRASE_DE_PRUEBA);
        cuerpo_de_la_peticion["endpoint"] = serde_json::json!(format!("  {endpoint}  "));

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_de_la_peticion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "los espacios de los extremos no impiden dar de alta un `endpoint` válido: contestó \
             {status} con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let guardado: String =
            sqlx::query_scalar("SELECT endpoint FROM repositories WHERE id = ?1")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .expect("la fila tiene que estar en el fichero de la base");

        assert_eq!(
            guardado, endpoint,
            "el `endpoint` se guarda recortado, sin los espacios de los extremos"
        );
    }

    // ── La salud por repositorio ─────────────────────────────────────────────────
    //
    // `POST /api/v1/repositories/{id}/health` y `POST /api/v1/repositories/health`, más los
    // tres campos de salud que el recurso sirve aplanados. Estas pruebas montan el estado con
    // un comprobador **inyectado** en vez del local del arnés: lo que hay que medir es el
    // camino de la capa HTTP —leer la fila, comprobar, anotar y devolver— y no el backend, así
    // que el desenlace lo fija la prueba y no habla con ningún destino. El alta sí va contra un
    // destino local de verdad, porque quien la atiende es el abridor, que no se toca.

    /// El comprobador falso que dice «el destino responde y no tiene repositorio».
    ///
    /// Es un estado distinguible del `desconocido` de una fila nunca comprobada, y del `ok` de
    /// una fila sana sin más: lo que las pruebas de estas rutas afirman es que el desenlace
    /// **comprobado** queda anotado, y con un estado que también fuera el de una fila sin
    /// comprobar la afirmación sería más débil.
    fn comprobador_sin_repositorio(
        _acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<LoQueContestaElDestino, crate::rustic::Error> {
        Ok(LoQueContestaElDestino::SinRepositorio)
    }

    /// El comprobador falso que dice «abre y lista»: el estado `ok`, sin motivo.
    fn comprobador_que_abre_y_lista(
        _acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<LoQueContestaElDestino, crate::rustic::Error> {
        Ok(LoQueContestaElDestino::AbreYLista)
    }

    /// Cuántas veces se ha llamado al comprobador del listado.
    ///
    /// Es el instrumento de «el listado no comprueba nada»: un comprobador que solo devolviera
    /// un desenlace no probaría que **no** se llamó, y un `panic!` dentro se perdería en el hilo
    /// del motor —`fuera_del_runtime` lo recogería como un trabajo interrumpido y saldría un
    /// `500`— en vez de fallar la prueba. El contador se lee desde la prueba, que corre en otro
    /// hilo que el comprobador.
    static COMPROBACIONES_DEL_LISTADO: AtomicUsize = AtomicUsize::new(0);

    /// El comprobador que **no** debe llamarse: cuenta la llamada y devuelve un desenlace fijo.
    fn comprobador_que_no_debe_llamarse(
        _acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<LoQueContestaElDestino, crate::rustic::Error> {
        COMPROBACIONES_DEL_LISTADO.fetch_add(1, Ordering::SeqCst);

        Ok(LoQueContestaElDestino::AbreYLista)
    }

    /// El comprobador que devuelve un estado **distinto por destino**.
    ///
    /// Es lo que permite afirmar «un elemento por repositorio y cada estado en su fila» sin
    /// depender del orden del listado: el estado sale del `endpoint` de la fila —que es
    /// distinguible por el nombre con el que se dio de alta— y la prueba vuelve a leer cada fila
    /// por su nombre para comprobar que le tocó el suyo.
    fn comprobador_segun_el_destino(
        acceso: &Acceso,
        _barras: BarrasDelMotor,
    ) -> Result<LoQueContestaElDestino, crate::rustic::Error> {
        if acceso.endpoint.contains("repo-salud-tres") {
            Err(crate::rustic::Error::DestinoNoAlcanzable {
                destino: acceso.nombre_del_destino(),
                motivo: "prueba: el destino no contesta".to_owned(),
            })
        } else if acceso.endpoint.contains("repo-salud-dos") {
            Ok(LoQueContestaElDestino::SinRepositorio)
        } else {
            Ok(LoQueContestaElDestino::AbreYLista)
        }
    }

    /// El estado de salud anotado en la fila de ese nombre, leído con SQL directo.
    ///
    /// Se lee la **tabla** y no la API porque lo que hay que afirmar es que la comprobación
    /// dejó el estado escrito: una lectura que pasara por el mismo camino que la escritura
    /// diría lo mismo sin haber mirado la columna.
    async fn salud_de_la_fila(pool: &sqlx::SqlitePool, nombre: &str) -> Option<String> {
        sqlx::query_scalar("SELECT health FROM repositories WHERE name = ?1")
            .bind(nombre)
            .fetch_one(pool)
            .await
            .expect("la fila del repositorio tiene que estar en la tabla")
    }

    /// La salud por repositorio — 1: comprobar uno lo persiste y lo devuelve.
    ///
    /// Cubre `api-resources`, escenario «Comprobar un repositorio persiste y devuelve su
    /// estado». Se afirma la ida **y** la vuelta: lo que contesta el `POST` y lo que sirve
    /// después el `GET`, y que los dos digan lo mismo —estado, fecha y motivo—, que es lo que
    /// el contrato exige («el estado SHALL quedar persistido y visible en la lectura
    /// posterior»). El comprobador va inyectado, así que la prueba mide el camino de la capa
    /// HTTP y no habla con ningún destino.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn comprobar_un_repositorio_persiste_y_devuelve_su_estado() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba_con_comprobador(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
            comprobador_sin_repositorio,
        );
        let router = api_router(estado);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-salud-uno"),
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "el alta tiene que contestar 201 antes de poder comprobar la salud"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let respuesta = post(router.clone(), &format!("/api/v1/repositories/{id}/health")).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "la comprobación de un repositorio que existe tiene que contestar 200"
        );

        let cuerpo = body_json(respuesta).await;
        let comprobado = cuerpo["data"].clone();

        assert_eq!(
            comprobado["health"], "sin_repositorio",
            "la respuesta tiene que traer el estado que devolvió la comprobación: {cuerpo}"
        );
        assert!(
            comprobado["health_checked_at"].is_string(),
            "una comprobación recién anotada tiene que traer su fecha: {cuerpo}"
        );
        assert!(
            comprobado["health_detail"].is_string(),
            "un desenlace que no es `ok` tiene que traer su motivo: {cuerpo}"
        );

        // Y el `GET` posterior sirve **lo mismo**: la fecha la pone la base, así que solo
        // comparando las dos respuestas se prueba que la comprobación quedó anotada.
        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;
        let leido = cuerpo["data"].clone();

        assert_eq!(
            leido["health"], comprobado["health"],
            "la lectura posterior no ve el estado que anotó la comprobación: {cuerpo}"
        );
        assert_eq!(
            leido["health_checked_at"], comprobado["health_checked_at"],
            "la lectura posterior no ve la fecha que anotó la comprobación: {cuerpo}"
        );
        assert_eq!(
            leido["health_detail"], comprobado["health_detail"],
            "la lectura posterior no ve el motivo que anotó la comprobación: {cuerpo}"
        );
    }

    /// La salud por repositorio — 2: el listado sirve el estado sin comprobar nada.
    ///
    /// Cubre `api-resources`, escenario «El listado sirve el estado sin comprobar nada». Los
    /// estados se **anotan por la capa de acceso** y no por la API, que es lo que describe el
    /// escenario —«varios repositorios con estados de salud anotados»—; y lo que se afirma del
    /// listado es doble: que sirve el estado y la fecha de cada fila, y que **no** invoca al
    /// comprobador ni una vez, medido con el contador.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_listado_sirve_el_estado_sin_comprobar_nada() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba_con_comprobador(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
            comprobador_que_no_debe_llamarse,
        );
        let router = api_router(estado.clone());

        let mut identificadores = Vec::new();
        for nombre in ["repo-salud-uno", "repo-salud-dos"] {
            let respuesta = post_json(
                router.clone(),
                "/api/v1/repositories",
                &cuerpo_de_alta(nombre),
            )
            .await;

            assert_eq!(respuesta.status(), StatusCode::CREATED);

            let cuerpo = body_json(respuesta).await;
            let id = Uuid::parse_str(
                cuerpo["data"]["id"]
                    .as_str()
                    .expect("el alta debe devolver el identificador"),
            )
            .expect("el identificador del alta es un UUID");

            identificadores.push(id);
        }

        estado
            .db
            .repositorios
            .anotar_salud(&identificadores[0], SaludDeRepositorio::Ok, None)
            .await
            .expect("la anotación de prueba tiene que escribir la fila");

        estado
            .db
            .repositorios
            .anotar_salud(
                &identificadores[1],
                SaludDeRepositorio::SinPermiso,
                Some("el repositorio abre, pero no deja listar sus instantáneas"),
            )
            .await
            .expect("la anotación de prueba tiene que escribir la fila");

        // El contador se pone a cero **después** del montaje: lo que se afirma es que el listado
        // no comprueba nada, no que el montaje no lo hiciera.
        COMPROBACIONES_DEL_LISTADO.store(0, Ordering::SeqCst);

        let respuesta = get(router, "/api/v1/repositories").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;
        let elementos = cuerpo["data"].as_object().expect("data")["items"]
            .as_array()
            .expect("items debe ser array");

        assert_eq!(
            elementos.len(),
            2,
            "el listado tiene que traer las dos filas: {cuerpo}"
        );

        for (nombre, esperado) in [("repo-salud-uno", "ok"), ("repo-salud-dos", "sin_permiso")] {
            let Some(elemento) = elementos.iter().find(|elemento| elemento["name"] == nombre)
            else {
                panic!("el listado no trae la fila `{nombre}`: {cuerpo}");
            };

            assert_eq!(
                elemento["health"], esperado,
                "el listado tiene que servir el estado anotado de `{nombre}`: {cuerpo}"
            );
            assert!(
                elemento["health_checked_at"].is_string(),
                "el listado tiene que servir la fecha de la comprobación de `{nombre}`: {cuerpo}"
            );
        }

        assert_eq!(
            COMPROBACIONES_DEL_LISTADO.load(Ordering::SeqCst),
            0,
            "el listado no puede comprobar la salud de ningún repositorio"
        );
    }

    /// La salud por repositorio — 3: comprobar todos devuelve un elemento por repositorio.
    ///
    /// Cubre `api-resources`, escenario «Comprobar todos devuelve un elemento por repositorio».
    /// El comprobador devuelve un estado **distinto por destino**, así que los tres elementos
    /// son distinguibles entre sí y cada uno se puede casar con su fila: se relee la tabla por
    /// el nombre y se comprueba que a cada repositorio le quedó **el suyo**.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn comprobar_todos_devuelve_un_elemento_por_repositorio() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba_con_comprobador(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
            comprobador_segun_el_destino,
        );
        let router = api_router(estado);

        for nombre in ["repo-salud-uno", "repo-salud-dos", "repo-salud-tres"] {
            let respuesta = post_json(
                router.clone(),
                "/api/v1/repositories",
                &cuerpo_de_alta(nombre),
            )
            .await;

            assert_eq!(
                respuesta.status(),
                StatusCode::CREATED,
                "las tres altas tienen que contestar 201"
            );
        }

        let respuesta = post(router, "/api/v1/repositories/health").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;
        let elementos = cuerpo["data"]
            .as_array()
            .expect("la comprobación de todos tiene que traer un array en `data`");

        assert_eq!(
            elementos.len(),
            3,
            "tiene que traer un elemento por repositorio: {cuerpo}"
        );

        let mut estados: Vec<&str> = elementos
            .iter()
            .map(|elemento| {
                elemento["health"]
                    .as_str()
                    .expect("cada elemento tiene que traer su estado como texto")
            })
            .collect();

        estados.sort_unstable();

        assert_eq!(
            estados,
            vec!["inaccesible", "ok", "sin_repositorio"],
            "los tres elementos tienen que traer los tres estados que devolvió el comprobador: \
             {cuerpo}"
        );

        for (nombre, esperado) in [
            ("repo-salud-uno", "ok"),
            ("repo-salud-dos", "sin_repositorio"),
            ("repo-salud-tres", "inaccesible"),
        ] {
            assert_eq!(
                salud_de_la_fila(&pool, nombre).await.as_deref(),
                Some(esperado),
                "el estado de `{nombre}` no quedó anotado en su fila"
            );
        }
    }

    /// La salud por repositorio — 4: no se puede escribir por el alta ni la actualización.
    ///
    /// Cubre `api-resources`, escenario «La salud no se puede escribir por el alta ni la
    /// actualización». El cuerpo que trae los campos de salud **no se rechaza** —la petición es
    /// válida y los campos se ignoran—, pero la fila no puede quedar con la salud que traía ese
    /// cuerpo: se afirma primero en el alta, y después en un `PUT` que llega **con la fila ya
    /// comprobada** en `ok` y con un `health` de mentira, para que la afirmación no se confunda
    /// con el `desconocido` de una fila nunca comprobada.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_salud_no_se_puede_escribir_por_el_alta_ni_la_actualizacion() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba_con_comprobador(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), clave),
            comprobador_que_abre_y_lista,
        );
        let router = api_router(estado);

        let mut cuerpo = cuerpo_de_alta("repo-salud-alta-ignora");
        cuerpo["health"] = serde_json::json!("inaccesible");
        cuerpo["health_checked_at"] = serde_json::json!("2020-01-01T00:00:00Z");
        cuerpo["health_detail"] = serde_json::json!("mentira del cuerpo");

        let respuesta = post_json(router.clone(), "/api/v1/repositories", &cuerpo).await;

        assert_eq!(
            respuesta.status(),
            StatusCode::CREATED,
            "un cuerpo con los campos de salud no puede rechazarse: tienen que ignorarse, no \
             fallar"
        );

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador")
            .to_owned();

        assert_eq!(
            cuerpo["data"]["health"], "desconocido",
            "la salud del alta no puede venir del cuerpo: {cuerpo}"
        );

        // Se comprueba de verdad para dejar la fila en `ok`…
        let respuesta = post(router.clone(), &format!("/api/v1/repositories/{id}/health")).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["health"], "ok",
            "la comprobación deja la fila en `ok`: {cuerpo}"
        );

        // …y ahora un `PUT` que trae la salud por el cuerpo: la fila tiene que quedarse igual.
        let mut cuerpo = cuerpo_de_alta("repo-salud-put-ignora");
        cuerpo["health"] = serde_json::json!("inaccesible");
        cuerpo["health_detail"] = serde_json::json!("mentira del cuerpo");

        let respuesta = put_json(
            router.clone(),
            &format!("/api/v1/repositories/{id}"),
            &cuerpo,
        )
        .await;

        assert_eq!(
            respuesta.status(),
            StatusCode::OK,
            "la actualización con los campos de salud tiene que aceptarse e ignorarlos"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["health"], "ok",
            "la actualización no puede escribir la salud por el cuerpo: {cuerpo}"
        );

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["health"], "ok",
            "la salud de la fila no puede venir del cuerpo del PUT: {cuerpo}"
        );
    }

    /// La salud por repositorio — 5: un repositorio nunca comprobado se sirve «desconocido».
    ///
    /// Cubre `api-resources`, escenario «Un repositorio nunca comprobado se sirve como
    /// desconocido». Recién dado de alta y sin pasar por ninguna comprobación, su lectura tiene
    /// que traer `health: "desconocido"` y `health_checked_at: null`: el estado es un valor del
    /// cable más —no un nulo— y la fecha sí es nula, que es como el panel distingue «no lo sé»
    /// de «se comprobó».
    #[tokio::test]
    async fn un_repositorio_nunca_comprobado_se_sirve_como_desconocido_con_fecha_nula() {
        let router = api_router(estado_desarrollo().await);

        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta("repo-salud-nunca"),
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::CREATED);

        let cuerpo = body_json(respuesta).await;
        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador")
            .to_owned();

        let respuesta = get(router, &format!("/api/v1/repositories/{id}")).await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            cuerpo["data"]["health"], "desconocido",
            "un repositorio nunca comprobado tiene que servirse como `desconocido`: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["health_checked_at"],
            serde_json::Value::Null,
            "un repositorio nunca comprobado tiene que servirse con la fecha a nulo: {cuerpo}"
        );
    }

    /// La salud por repositorio — el `404` de comprobar una fila que no existe.
    ///
    /// Es la mitad del contrato de `POST /{id}/health` que no cubre el escenario de la
    /// comprobación con éxito: una fila que no está contesta el mismo `404` que la lectura —el
    /// identificador es un UUID bien formado que no existe, así que lo decide la ausencia de la
    /// fila y no el rechazo del identificador—, y por la envoltura del §5, para que el panel
    /// tenga algo que pintar.
    #[tokio::test]
    async fn comprobar_la_salud_de_una_fila_inexistente_contesta_404() {
        let router = api_router(estado_desarrollo().await);

        let id = Uuid::new_v4().to_string();

        let respuesta = post(router, &format!("/api/v1/repositories/{id}/health")).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "comprobar la salud de una fila que no existe tiene que contestar 404; contestó \
             {status} con {cuerpo}"
        );

        assert_eq!(
            cuerpo["status"], 404,
            "la envoltura tiene que repetir el código en `status`: {cuerpo}"
        );

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 404 tiene que explicar que no hay fila que comprobar: {cuerpo}"
        );
    }

    // ── Cotas y recorte de los campos de texto (SEC-13) ──────────────────────
    //
    // Cubre `api-resources`, requisito «Los campos de texto se recortan y tienen una cota por
    // campo» en la mitad de `/repositories`: el recorte va antes de medir y de guardar, un
    // obligatorio que queda vacío es un `400` que nombra el campo, y lo que supera su cota es un
    // `400` que nombra el campo y **no repite el valor**. Los dos secretos —`secret_access_key` y
    // `passphrase`— **no** se acotan: un secreto no se recorta ni se mide.

    /// 3.1 — Un `name` en blanco o por encima de su cota contesta `400` nombrando `name`.
    #[tokio::test]
    async fn un_name_vacio_o_demasiado_largo_contesta_400_nombrando_name() {
        let router = api_router(estado_desarrollo().await);

        let mut en_blanco = cuerpo_de_alta("repo-3-1");
        en_blanco["name"] = serde_json::json!("   ");

        let respuesta = post_json(router.clone(), "/api/v1/repositories", &en_blanco).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `name` de solo espacios es un campo vacío: contestó {status} con {cuerpo}"
        );
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| mensaje.contains("name")),
            "el motivo tiene que nombrar `name`: {cuerpo}"
        );

        let largo = "n".repeat(121);
        let mut por_encima = cuerpo_de_alta("repo-3-1");
        por_encima["name"] = serde_json::json!(largo);

        let respuesta = post_json(router, "/api/v1/repositories", &por_encima).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `name` de 121 caracteres supera su cota de 120: contestó {status} con {cuerpo}"
        );
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| mensaje.contains("name")),
            "el motivo tiene que nombrar `name`: {cuerpo}"
        );
        assert!(
            !cuerpo.to_string().contains(&largo),
            "el rechazo no puede repetir el valor recibido: {cuerpo}"
        );
    }

    /// 3.1b — Un cuerpo **sin** la clave `name` contesta `400` nombrando `name`.
    ///
    /// Un `name` ausente no es lo mismo que uno en blanco para el extractor: sin
    /// `#[serde(default)]` el cuerpo lo rechazaría la clase genérica de serde, que **no** nombra el
    /// campo. Con el defecto declarado, el cuerpo entra al manejador con el nombre vacío y contesta
    /// el mismo `400` que el blanco —el de [`texto::obligatorio`]—, que sí nombra `name`.
    #[tokio::test]
    async fn un_name_ausente_contesta_400_nombrando_name() {
        let router = api_router(estado_desarrollo().await);

        let mut cuerpo = cuerpo_de_alta("repo-3-1b");
        if let Some(mapa) = cuerpo.as_object_mut() {
            mapa.remove("name");
        }

        let respuesta = post_json(router, "/api/v1/repositories", &cuerpo).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un cuerpo sin `name` tiene que rechazarse con un 400 que nombre el campo: contestó \
             {status} con {cuerpo}"
        );
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| mensaje.contains("name")),
            "el motivo tiene que nombrar `name`: {cuerpo}"
        );
    }

    /// 3.2 — Un `bucket`, `region`, `access_key_id` o `endpoint` por encima de su cota, `400`.
    #[tokio::test]
    async fn un_campo_por_encima_de_su_cota_contesta_400_nombrando_el_campo() {
        let router = api_router(estado_desarrollo().await);

        let casos = [
            ("bucket", "b".repeat(64)),
            ("region", "r".repeat(65)),
            ("access_key_id", "a".repeat(129)),
        ];

        for (campo, valor) in casos {
            let mut cuerpo = cuerpo_de_alta("repo-3-2");
            cuerpo[campo] = serde_json::json!(valor);

            let respuesta = post_json(router.clone(), "/api/v1/repositories", &cuerpo).await;

            let status = respuesta.status();
            let respuesta = body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "el campo `{campo}` por encima de su cota tiene que rechazarse: contestó {status} \
                 con {respuesta}"
            );
            assert!(
                respuesta["message"]
                    .as_str()
                    .is_some_and(|mensaje| mensaje.contains(campo)),
                "el motivo tiene que nombrar `{campo}`: {respuesta}"
            );
        }

        // El `endpoint` va aparte porque su cota es 2048 y el valor tiene que seguir teniendo forma
        // de URL para que el rechazo sea por la cota y no por el esquema.
        let mut cuerpo = cuerpo_de_alta("repo-3-2");
        cuerpo["endpoint"] = serde_json::json!(format!("https://{}", "e".repeat(2048)));

        let respuesta = post_json(router, "/api/v1/repositories", &cuerpo).await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `endpoint` por encima de su cota tiene que rechazarse: contestó {status} con \
             {cuerpo}"
        );
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| mensaje.contains("endpoint")),
            "el motivo tiene que nombrar `endpoint`: {cuerpo}"
        );
    }

    /// 3.3 — Un `bucket` con espacios a los lados se guarda recortado.
    #[tokio::test]
    async fn un_bucket_con_espacios_se_guarda_recortado() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool.clone(), Arc::clone(&clave)),
        );

        let mut cuerpo_de_la_peticion = cuerpo_de_alta("repo-3-3");
        cuerpo_de_la_peticion["bucket"] = serde_json::json!("  cubo-3-3  ");

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_de_la_peticion,
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el recorte deja un bucket válido y el alta tiene que seguir adelante: contestó \
             {status} con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador que generó la capa de acceso")
            .to_owned();

        let bucket: String = sqlx::query_scalar("SELECT bucket FROM repositories WHERE id = ?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("la fila tiene que estar en el fichero de la base");

        assert_eq!(
            bucket, "cubo-3-3",
            "el bucket se guarda recortado, sin los espacios de los extremos"
        );
    }

    /// El listado de repositorios debe responder con paginación.
    ///
    /// El endpoint `GET /api/v1/repositories` debe aceptar los parámetros de
    /// consulta `limit` y `offset` y devolver un **wrapper** con `items`
    /// (el trozo de la página) y `total` (el número de filas en la tabla),
    /// en lugar de un array plano. Esta prueba queda en rojo porque la
    /// implementación actual devuelve un array sin paginar.
    #[tokio::test]
    async fn el_listado_de_repositorios_debe_responder_con_paginacion() {
        let router = api_router(estado_desarrollo().await).clone();

        // Damos de alta dos repositorios para tener algo que paginar.
        for nombre in ["repo-pag-uno", "repo-pag-dos"] {
            let respuesta = post_json(
                router.clone(),
                "/api/v1/repositories",
                &cuerpo_de_alta(nombre),
            )
            .await;

            assert_eq!(
                respuesta.status(),
                StatusCode::CREATED,
                "el alta de `{nombre}` tiene que contestar 201"
            );
        }

        // Pedimos una página con `limit` y `offset`.
        let respuesta = get(router, "/api/v1/repositories?limit=1&offset=0").await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado paginado tiene que contestar 200; contestó {status} con {cuerpo}"
        );

        // La respuesta debe ser un **objeto** con `items` y `total`, no un array plano.
        let data = cuerpo["data"].as_object().unwrap_or_else(|| {
            panic!(
                "el listado paginado tiene que traer un objeto en `data` con `items` y \
                     `total`, y llegó {cuerpo}"
            )
        });

        assert!(
            data.contains_key("items"),
            "el wrapper de paginación tiene que llevar la clave `items`: {cuerpo}"
        );
        assert!(
            data.contains_key("total"),
            "el wrapper de paginación tiene que llevar la clave `total`: {cuerpo}"
        );

        let items = data["items"]
            .as_array()
            .unwrap_or_else(|| panic!("`items` tiene que ser un array: {cuerpo}"));

        assert_eq!(
            items.len(),
            1,
            "con `limit=1` la página tiene que traer un solo elemento, y llegó {}: {cuerpo}",
            items.len()
        );

        let total = data["total"]
            .as_u64()
            .unwrap_or_else(|| panic!("`total` tiene que ser un número: {cuerpo}"));

        assert_eq!(
            total, 2,
            "con dos repositorios en la tabla, `total` tiene que ser 2: {cuerpo}"
        );
    }

    /// SEC-22 (resto): POST /api/v1/repositories con endpoint inseguro sin permiso → 400 insecure_endpoint.
    ///
    /// Cubre el spec delta `socket-security`: si el `endpoint` apunta a un socket Docker `http://`
    /// y no se ha declarado `KESTO_ALLOW_INSECURE_ENDPOINT=1`, la API debe contestar `400`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn endpoint_docker_sin_permiso_contesta_400() {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, clave),
        );

        // Un endpoint que apunta a localhost por http es inseguro sin permiso.
        let mut cuerpo = serde_json::json!({
            "name": "repo-sec-22",
            "endpoint": "http://localhost:2375",
            "bucket": "cubo-de-seguridad",
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "clave-secreta-distinguible-0000000000000001",
            "passphrase": PASSPHRASE_DE_PRUEBA
        });
        if let Some(mapa) = cuerpo.as_object_mut() {
            mapa.remove("health");
            mapa.remove("health_checked_at");
            mapa.remove("health_detail");
        }

        let router = api_router(estado);
        let respuesta = post_json(router, "/api/v1/repositories", &cuerpo).await;
        let status = respuesta.status();
        let cuerpo_respuesta = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un endpoint inseguro sin permiso ha de contestar 400: {cuerpo_respuesta}"
        );

        let mensaje = cuerpo_respuesta["message"]
            .as_str()
            .expect("el 400 tiene que traer mensaje")
            .to_owned();

        assert!(
            mensaje.contains("endpoint"),
            "el mensaje ha de nombrar el campo endpoint: {mensaje}"
        );
    }

    /// SEC-22 (resto): POST /api/v1/repositories con permiso de endpoint inseguro → 201.
    ///
    /// Cubre el spec delta `socket-security`: cuando se declara `KESTO_ALLOW_INSECURE_ENDPOINT=1`
    /// y la configuración completa `KESTO_ALLOW_INSECURE=1`, el motor se arranca con los permisos
    /// y un alta con directorio local contesta `201`. Valida que las variables de entorno del
    /// permiso se leen correctamente y no bloquean el flujo normal de alta.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn endpoint_local_con_permiso_contesta_201() {
        let destino = destino_local_de_prueba("repo-sec-permiso");
        let (directorio, pool, clave) = base_con_clave().await;

        // Esta configuración habilita `KESTO_ALLOW_INSECURE=1` y `KESTO_ALLOW_INSECURE_ENDPOINT=1`,
        // que es lo que mide esta prueba: los permisos están activos y el alta avanza.
        let estado = estado_de_prueba(
            config_con_dsn_y_endpoint_inseguro(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, clave),
        );

        let mut cuerpo = serde_json::json!({
            "name": "repo-sec-22-con-permiso",
            "endpoint": &destino,
            "bucket": "cubo-de-seguridad-permiso",
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "clave-secreta-distinguible-0000000000000001",
            "passphrase": PASSPHRASE_DE_PRUEBA
        });
        if let Some(mapa) = cuerpo.as_object_mut() {
            mapa.remove("health");
            mapa.remove("health_checked_at");
            mapa.remove("health_detail");
        }

        let router = api_router(estado);
        let respuesta = post_json(router, "/api/v1/repositories", &cuerpo).await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un endpoint local con permiso ha de contestar 201"
        );
    }

    /// SEC-22 (resto): POST /api/v1/repositories con endpoint HTTPS → 201.
    ///
    /// Cubre el spec delta `socket-security`: un endpoint por https siempre se admite (sin
    /// necesidad de `KESTO_ALLOW_INSECURE_ENDPOINT`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn endpoint_https_se_acepta() {
        let destino = destino_local_de_prueba("repo-https");
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, clave),
        );

        let mut cuerpo = serde_json::json!({
            "name": "repo-sec-22-https",
            "endpoint": &destino,
            "bucket": "cubo-de-seguridad-https",
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "clave-secreta-distinguible-0000000000000001",
            "passphrase": PASSPHRASE_DE_PRUEBA
        });
        if let Some(mapa) = cuerpo.as_object_mut() {
            mapa.remove("health");
            mapa.remove("health_checked_at");
            mapa.remove("health_detail");
        }

        let router = api_router(estado);
        let respuesta = post_json(router, "/api/v1/repositories", &cuerpo).await;
        let status = respuesta.status();
        let cuerpo_respuesta = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "un endpoint por https ha de contestar 201: {cuerpo_respuesta}"
        );

        let id = cuerpo_respuesta["data"]["id"]
            .as_str()
            .expect("el alta debe devolver el identificador");

        assert!(!id.is_empty(), "la ID no puede estar vacía");
    }

    /// SEC-22 (resto): PUT /api/v1/repositories con endpoint inseguro sin permiso → 400.
    ///
    /// Cubre el spec delta `socket-security`: la edición aplica la misma regla de admisión
    /// que el alta: un endpoint inseguro sin permiso se rechaza.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn put_con_endpoint_inseguro_sin_permiso_contesta_400() {
        let destino = destino_local_de_prueba("repo-put-inseguro");
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, clave),
        );

        // Primero dar de alta un repositorio válido.
        let mut alta_cuerpo = serde_json::json!({
            "name": "repo-put-base",
            "endpoint": &destino,
            "bucket": "cubo-base",
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "clave-secreta-distinguible-0000000000000001",
            "passphrase": PASSPHRASE_DE_PRUEBA
        });
        if let Some(mapa) = alta_cuerpo.as_object_mut() {
            mapa.remove("health");
            mapa.remove("health_checked_at");
            mapa.remove("health_detail");
        }

        let router = api_router(estado);

        let respuesta_alta = post_json(router.clone(), "/api/v1/repositories", &alta_cuerpo).await;
        assert_eq!(
            respuesta_alta.status(),
            StatusCode::CREATED,
            "el alta inicial tiene que contestar 201"
        );

        let body = body_json(respuesta_alta).await;
        let id_alta = body["data"]["id"].as_str().expect("el alta devuelve el id");

        // Ahora intentar editar con un endpoint inseguro sin permiso.
        let mut edita_cuerpo = serde_json::json!({
            "name": "repo-put-editado",
            "endpoint": "http://localhost:2375",
            "bucket": "cubo-editado",
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "clave-secreta-distinguible-0000000000000001",
            "passphrase": PASSPHRASE_DE_PRUEBA
        });
        if let Some(mapa) = edita_cuerpo.as_object_mut() {
            mapa.remove("health");
            mapa.remove("health_checked_at");
            mapa.remove("health_detail");
        }

        let respuesta_edita = put_json(
            router,
            &format!("/api/v1/repositories/{id_alta}"),
            &edita_cuerpo,
        )
        .await;

        assert_eq!(
            respuesta_edita.status(),
            StatusCode::BAD_REQUEST,
            "editar con endpoint inseguro sin permiso ha de contestar 400"
        );
    }
}
