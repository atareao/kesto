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
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::Json;
use crate::api::response::{ApiResponse, Data};
use crate::api::{contenido, del_motor_en_el_alta};
use crate::db::entities::{AltaDeRepositorio, Repositorio};
use crate::error::AppError;
use crate::rustic::{Acceso, PuestaEnMarcha};
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
/// Porque el alta devuelve **el mismo recurso** que la lectura y el listado y con los mismos nueve
/// campos: lo único que añade es el desenlace. Un segundo tipo con los nueve campos escritos otra
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
/// # Lo que cuesta, y por qué no se acota aquí
///
/// Hablar con el destino no es gratis cuando el destino no está: un alta contra un S3 inalcanzable
/// se lleva **41–54 s** medidos —cinco reintentos de `rustic_backend` con retroceso y temblor,
/// `DEFAULT_RETRY`— antes de contestar el `502`. Se ha decidido **no acotarlo** con un plazo en este
/// manejador, y los motivos van en orden de peso:
///
/// 1. **Un plazo devolvería una respuesta equivocada.** No distingue un destino muerto de uno lento:
///    contestaría «el destino no se alcanza» sobre un cubo que sí está y que iba despacio, y un
///    diagnóstico falso es peor que una espera, porque manda a buscar el fallo donde no está.
/// 2. **El trabajo no se puede cancelar.** El alta espera a un hilo de bloqueo
///    (`Motor::abrir_repositorio` entra por `spawn_blocking`), y un plazo sobre esa espera **no**
///    para el hilo: los reintentos siguen corriendo, con los dos secretos vivos dentro, y cada alta
///    abandonada dejaría un hilo ocupado. La respuesta se adelantaría a los hechos.
/// 3. **Aquí no hay un número con el que acertar.** Los tres plazos que este proyecto sí pone —el de
///    la petición al motor de contenedores, el de la inactividad del volcado y el de la parada—
///    acotan cosas cuya lentitud está medida y cuyo coste de equivocarse es una copia que falla, no
///    un alta que se rechaza. Éste sería el único plazo elegido contra un desconocido.
///
/// Lo que se acepta a cambio, dicho claro: la petición se queda colgada ese tiempo y el operador
/// espera sin ver nada. Es asumible donde esto se usa —un alta es una acción humana, delante del
/// formulario, no una llamada en un bucle—, y la espera está **acotada por la biblioteca**, no
/// abierta. Lo que **no** está medido es un destino que traga la conexión y no contesta nunca —un
/// cortafuegos que descarta en silencio—: ahí el techo lo pone el tiempo de transporte de `opendal`,
/// que no se ha podido medir sin red de verdad. Queda declarado como lo que es.
///
/// Y la palanca buena, si algún día duele, no está en este fichero: `rustic_backend` construye su
/// cliente con las opciones de `opendal`, y bajar ahí la política de reintentos —`("retry","0")` deja
/// el mismo caso en **13 ms** medidos— es un cambio de `src/rustic/repositorio.rs`, que es donde vive
/// la construcción del destino. Eso sería una política del alta decidida en un solo sitio, y no un
/// plazo puesto encima. Se deja escrito en vez de callado porque el precio existe y alguien lo va a
/// preguntar.
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

    // La fila, montada y **sin escribir**: es el valor con el que se arma el acceso y el que se
    // guardará si el destino contesta bien. Montarla aquí no adelanta nada de lo que el orden evita,
    // porque no toca la base.
    let alta = AltaDeRepositorio {
        name: cuerpo.name,
        endpoint: cuerpo.endpoint,
        bucket: cuerpo.bucket,
        region: cuerpo.region,
        access_key_id: cuerpo.access_key_id,
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
    use std::path::Path;
    use std::sync::Arc;

    use axum::http::{StatusCode, header};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::state::EstadoDeBase;
    use crate::test_support::{
        OTRA_PASSPHRASE_DE_PRUEBA, PASSPHRASE_DE_PRUEBA, abrir_local, base_con_clave, body_json,
        body_text, capturar_log, config_con_dsn, delete, destino_local_de_prueba,
        directorio_de_prueba, estado_de_prueba, estado_desarrollo, get, post_crudo, post_json,
        put_json, sembrar_snapshots,
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
    /// El `endpoint` es la ruta de un directorio temporal y no una URL de mentira, y eso no es un
    /// detalle del arnés: desde el change `backup-engine` el alta **habla con el destino** —lo
    /// inicializa si está vacío—, así que una URL que no existe convierte un alta válida en un
    /// fallo por destino inalcanzable. [`destino_local_de_prueba`] deja además la ruta viva mientras
    /// el proceso de pruebas corra, que es lo que hace falta porque el alta ocurre después de
    /// construir el cuerpo.
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
    // arnés: el `endpoint` de la fila es la ruta de un directorio temporal y el motor abre ahí un
    // repositorio del sistema de ficheros, así que los cuatro desenlaces se recorren sin red, sin
    // S3 y sin credenciales —que es lo que permite que esto corra en el CI—.

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
        let ruta = destino.ruta().to_string_lossy().into_owned();

        let respuesta = post_json(
            router,
            "/api/v1/repositories",
            &cuerpo_contra("repo-1-31a", &ruta, PASSPHRASE_DE_PRUEBA),
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
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let antes = inventario(destino.ruta());

        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let respuesta = post_json(
            router,
            "/api/v1/repositories",
            &cuerpo_contra("repo-1-31b", &ruta, PASSPHRASE_DE_PRUEBA),
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
        let ruta = destino_local_de_prueba("repo-1-31c");
        std::fs::write(&ruta, b"esto no es un repositorio").expect("el fichero del destino");

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-1-31c", &ruta, PASSPHRASE_DE_PRUEBA),
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
        let ruta = destino.ruta().to_string_lossy().into_owned();

        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let antes = inventario(destino.ruta());

        let respuesta = post_json(
            api_router(estado),
            "/api/v1/repositories",
            &cuerpo_contra("repo-1-31d", &ruta, OTRA_PASSPHRASE_DE_PRUEBA),
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
}
