//! Las pruebas del explorador de instantáneas: `GET /api/v1/snapshots?repository_id=<id>`.
//!
//! Es la ruta que el §5 llama «explorador de snapshots en el almacenamiento S3», y esa frase decide
//! de dónde sale la respuesta: del **repositorio**, no de `job_executions`. La base sabe qué copias
//! se intentaron; el repositorio sabe qué copias están, y la diferencia es lo que hace útil al
//! explorador —una instantánea que está en el repositorio y no en el histórico es una copia que
//! alguien hizo con otra herramienta, y una ejecución en el histórico sin instantánea es una copia
//! que se dio por buena y no está—. Por eso estas pruebas siembran el repositorio con la
//! **biblioteca** y piden el listado por HTTP: si la respuesta estuviera deducida del histórico, no
//! habría forma de que saliera una instantánea ajena.
//!
//! # El listado sale del repositorio, y el target de cada elemento de la base
//!
//! Cada elemento del listado lleva tres campos más —`target_id`, `target_name` y `target_type`— que
//! **no** salen del repositorio: son del target sobre el que esa copia se restauraría, y el único
//! camino que el §3 deja escrito de una instantánea a su target es el vínculo que dejó su ejecución
//! (`job_executions.snapshot_id` → la tarea → su `target_id`). Los resuelve este módulo —el
//! manejador, con la capa de datos— y **no** el motor, que no lee la base: el motor lista lo que hay
//! en el almacenamiento y el manejador completa cada elemento con lo que la base sabe de él. Los tres
//! son anulables porque una instantánea puede no casar con ninguna ejecución —una copia que hizo otra
//! herramienta, o cuya fila de `job_executions` se borró en cascada con su tarea— y el panel necesita
//! saber **antes** de ofrecer la restauración si esa copia tiene destino, y cuál es.
//!
//! Y cada elemento lleva además **`job_name`** (anulable), el nombre de su tarea: sale del mismo
//! vínculo —`snapshot → job_executions → backup_jobs`— sin pasar por el target, así que una
//! instantánea cuya tarea no tenga nombre lo publica a `null` en vez de dejar el hueco. La `label`
//! del elemento **no** cambia: sigue siendo el identificador de la tarea, y el nombre legible vive
//! en su propio campo.
//!
//! # De dónde sale el listado, y por dónde **no** se abre el repositorio
//!
//! La lista la resuelve [`Motor::listar_snapshots`](crate::engine::Motor::listar_snapshots) y **no**
//! [`Repositorio::listar_snapshots`](crate::rustic::Repositorio::listar_snapshots) ni
//! [`Repositorio::poner_en_marcha`](crate::rustic::Repositorio::poner_en_marcha): con el backend de
//! S3 la biblioteca construye su **propio** runtime y bloquea el hilo para hablar con el destino,
//! así que llamarla desde dentro del runtime de Tokio —y este manejador es `async` por
//! definición— entra en **pánico**, con el literal «Cannot start a runtime from within a runtime.»,
//! y ninguna prueba del CI lo vería porque toda la suite abre un repositorio **local**. La fachada
//! del motor saca ese trabajo del runtime por dentro, en un solo sitio, en vez de repartir la regla
//! entre quien se acuerde de que existe.
//!
//! El [`Acceso`] se arma **desde la fila** del repositorio, con sus dos secretos ya descifrados por
//! la capa de acceso —la única que tiene la clave maestra—. El alta de un repositorio los toma del
//! cuerpo de la petición porque en un alta todavía no hay fila; aquí sí la hay, y es de donde salen.
//!
//! # La ventana acota el número, y `truncated` dice que puede haber más
//!
//! La ventana —[`LA_VENTANA`]— no es una comodidad del panel: listar es la lectura más cara de la
//! API, porque hay que abrir el repositorio y recorrer su índice, y el formato del repositorio lo
//! hace además inevitable —el identificador de una instantánea es el nombre de un fichero y no
//! lleva fecha, así que acotar **antes** de resolver es la única forma de no recorrerlo entero—.
//!
//! La consecuencia, que hay que leer junto al campo: el subconjunto devuelto **no** tiene por qué
//! ser el de las instantáneas más recientes. Lo que se garantiza es el orden de lo que se devuelve
//! y que quien pregunte pueda saber que hay más; el detalle de qué promete `truncated` y qué no
//! está en el rustdoc de ese campo, en la vista del cable.
//!
//! # Las pruebas
//!
//! Al final del fichero, en su `mod tests`: siembran el repositorio **con la biblioteca** y piden el
//! listado **por HTTP**, que es la única forma de medir de dónde sale la respuesta —una lista
//! deducida del histórico también tendría elementos, y lo que la delata es la instantánea **ajena**,
//! que la escribió la biblioteca sin que Kesto interviniera y por eso no tiene ninguna fila—. Y lo
//! que esas pruebas **no** hacen es abrir el repositorio por su cuenta: llamar a
//! [`crate::rustic::Repositorio::poner_en_marcha`] desde una prueba entraría en pánico dentro del
//! runtime de Tokio con el backend de S3 y ninguna prueba del CI lo vería, porque toda la suite abre
//! un repositorio local. Lo abre el manejador, por la capa de Rustic, que es donde tiene que
//! abrirlo.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::{Json, Query};
use crate::api::response::ApiResponse;
use crate::api::targets::texto_del_tipo;
use crate::api::{contenido, del_motor};
use crate::db::entities::{AltaDeRestauracion, ObjetivoDeRestauracion, Repositorio, TipoDeTarget};
use crate::engine::restauracion::destino_del_objetivo;
use crate::error::AppError;
use crate::rustic::{Acceso, ResumenDeSnapshot};
use crate::state::AppState;

/// Cuántas instantáneas devuelve el explorador como mucho.
///
/// Es la misma ventana que la del histórico de ejecuciones, y por el mismo motivo: listar es la
/// lectura más cara de la API —hay que abrir el repositorio y recorrer su índice—, así que el
/// número no es una comodidad del panel sino una cota del servidor. Dos ventanas distintas para dos
/// listados del mismo panel serían dos números que se desincronizan.
///
/// El RED de este change declara por su cuenta una constante con **este mismo nombre y este mismo
/// valor** —`LA_VENTANA`, al principio de su bloque de pruebas—, y las dos quedan atadas por la
/// prueba de la ventana: siembra una instantánea **más** que la ventana y exige que vuelvan
/// exactamente las de la ventana, así que un número distinto en cualquiera de los dos lados la pone
/// roja. No se comparte la constante porque el RED está escrito y no se toca; lo que se comparte es
/// la medición.
pub const LA_VENTANA: usize = 200;

/// Las rutas del explorador: el listado de un repositorio y la restauración de una instantánea.
///
/// Se declara como las rutas de los otros recursos —un `router()` que
/// [`api_router`](crate::api::api_router) mezcla— para que dónde se cuelga lo decida quien lo llama
/// y este módulo no tenga que conocer la partición del router. Lo que sí se decide aquí es lo que
/// **no** se hace: ninguna de las dos rutas entra en la lista blanca de rutas públicas, porque son
/// superficie de negocio —una publica qué hay en el almacenamiento y la otra **escribe** en el
/// host— y les toca el *gate* de sesión como al resto.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/snapshots", get(listado))
        .route("/api/v1/snapshots/{id}/restore", post(restaurar))
}

/// Los parámetros de la consulta, tal y como llegan por el cable.
///
/// `repository_id` es un `Option<String>` y **no** un campo obligatorio, y esa es la decisión que
/// hace que el `400` de la falta pueda **nombrar el parámetro**: con el campo obligatorio el rechazo
/// lo produce el extractor, y su motivo —«los parámetros de la consulta no tienen el tipo que la
/// ruta espera»— no dice cuál falta. Aquí la falta se mira a mano, y por eso el motivo se puede
/// escribir.
///
/// `deny_unknown_fields` por la misma razón que en el histórico de ejecuciones: un
/// `?repositori_id=` ignorado en silencio haría creer a quien llamó que ha elegido el repositorio
/// cuando lo que ha hecho es una errata, y el listado que vuelve es el de ninguno. El rechazo sale
/// por la envoltura como cualquier otro parámetro que no se puede interpretar, y ni el nombre ni el
/// valor del campo que sobra viajan al cliente.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsultaDelCable {
    /// El repositorio cuyas instantáneas se listan.
    pub repository_id: Option<String>,
}

/// El cuerpo de la petición de restauración, tal y como llega por el cable.
///
/// La instantánea viaja en el **camino** —`/api/v1/snapshots/{id}/restore`— porque es el recurso
/// sobre el que se actúa, y el repositorio en el **cuerpo** porque es el contexto desde el que se
/// lee: la misma instantánea puede estar en dos repositorios, así que sin decir cuál, la operación
/// no está determinada. Es la partición inversa a la del listado, y no por capricho: allí no hay
/// recurso al que colgar la instantánea y solo se pregunta por un repositorio.
///
/// `deny_unknown_fields` por la misma razón que en la consulta del listado: un campo de más
/// ignorado en silencio haría creer a quien llamó que ha elegido lo que no ha elegido. El rechazo
/// sale por la envoltura como cualquier otro cuerpo que no se puede interpretar, y ni el nombre ni
/// el valor del campo que sobra viajan al cliente.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CuerpoDeLaRestauracion {
    /// El repositorio del que se lee la instantánea.
    pub repository_id: String,
    /// La confirmación de sobrescritura: obligatoria para un target de base de datos.
    ///
    /// Nace en `false` y **no** en `true`: un cuerpo que no la traiga —el de un target de
    /// directorio, o el de una petición a la que le falta la confirmación— no autoriza a pisar datos
    /// vivos. Por eso lleva `#[serde(default)]` y no un campo obligatorio: la falta de la
    /// confirmación no es «un campo mal escrito» sino «una condición previa sin cumplir», y quien la
    /// decide es la puerta del manejador —que contesta `428`—, no el extractor —que contestaría
    /// `400`—. `deny_unknown_fields` sigue rechazando cualquier otro campo.
    ///
    /// En un target de **directorio** no se exige, y no es un campo que se ignore: viaja al motor y
    /// es lo que autoriza a **podar** el destino —retirar de él lo que no está en la instantánea—,
    /// de modo que sin ella un directorio solo recibe. Ver
    /// [`Motor::restaurar`](crate::engine::Motor::restaurar).
    #[serde(default)]
    pub sobrescribir: bool,
}

/// `GET /api/v1/snapshots?repository_id=<id>` — el explorador de instantáneas del §5.
///
/// El listado sale **del repositorio** y no del histórico de ejecuciones, y esa es la mitad que hace
/// útil al explorador: la base sabe qué copias se intentaron y el almacenamiento sabe qué copias
/// están. Por eso la fila se lee aquí —para saber **qué** destino abrir— y el contenido se le
/// pregunta al destino. El encabezado del módulo cuenta por qué el listado no lo resuelve este
/// módulo por su cuenta.
///
/// # Este `GET` **puede crear el repositorio** en el destino
///
/// El listado abre el repositorio por la misma fachada que el alta
/// —[`Motor::listar_snapshots`](crate::engine::Motor::listar_snapshots)—, y esa apertura
/// **inicializa** el destino cuando no lo encuentra: si la fila existe pero el repositorio no está
/// allí, esta petición lo crea con la passphrase de la fila y contesta `200` con la lista vacía. Un
/// `GET` con efecto de escritura, y se dice aquí, donde está la ruta, porque es lo que un operador
/// tiene que poder leer: el día que encuentre un repositorio donde no lo había, no lo ha traído el
/// alta necesariamente.
///
/// No se arregla en esta capa a propósito: distinguir «abre» de «crea» pide una variante de la
/// apertura que no inicialice, y eso es reabrir `src/rustic`, que está auditado y cerrado. El efecto
/// acaba ocurriendo igual —el alta y la primera copia crean ese repositorio—, así que lo que no
/// puede ocurrir es **en silencio**. Lo que sí se elige aquí es el otro extremo: la fila resuelve
/// **qué** destino abrir y su ausencia es un `404` **antes** de tocar el almacenamiento, así que un
/// repositorio inventado no crea nada en ninguna parte.
///
/// # Los tres rechazos, y por qué son ésos
///
/// * **`400` si falta `repository_id`, nombrando el parámetro**: un explorador mira **un**
///   repositorio cada vez, y la ruta no tiene un recurso al que colgarse, así que el repositorio
///   viaja en la consulta. Lo que falla es una petición a la que le falta un dato: ni un recurso
///   ausente (`404`) ni un fallo del servidor (`500`). Nombrarlo es lo que convierte ese `400` en
///   algo que se puede arreglar sin leer el código.
/// * **`400` si no es un UUID**: es el mismo reparto que en los tres recursos con identificador en
///   el camino —una errata de teclado no es un recurso ausente—, y aquí importa además por coste:
///   resolver el listado abre el repositorio, que es la operación más cara de la API, así que un
///   identificador que no puede corresponder a ninguna fila no debe llegar hasta ahí.
/// * **`404` si el repositorio no existe**: la fila la resuelve este manejador, que es el único que
///   puede leerla —el motor no sabe de la base—, y sin fila no hay destino que abrir. Se contesta
///   antes de tocar el almacenamiento, así que un repositorio inventado no cuesta una apertura.
///
/// # La ventana y `truncated`
///
/// La ventana la aplica el motor —que acota los identificadores **antes** de resolverlos, porque
/// resolver es una lectura por instantánea— y aquí solo se lee si se ha llenado. `truncated`
/// significa **«el listado se ha recortado»** y **no** promete que los 200 que devuelve sean **los
/// más recientes**: la ventana acota el **número**, no el orden de antigüedad, y lo único que se
/// garantiza es el orden de lo que devuelve. El campo lo dice palabra por palabra en la vista del
/// cable, que es donde lo lee quien construye el panel.
///
/// # El target de cada elemento lo resuelve este manejador
///
/// Los tres campos del target —`target_id`, `target_name` y `target_type`— no los puede resolver el
/// motor, que no lee la base, así que el listado se completa aquí: primero se le piden al motor las
/// instantáneas del almacenamiento, y **después** —con sus identificadores— se le piden a la capa de
/// datos los objetivos, en una sola lectura para todo el lote. La fusión es por el identificador de
/// la instantánea y el resultado de no encontrarlo es `null`, no un elemento menos: una copia sin
/// vínculo sigue siendo una copia del repositorio y tiene que poder listarse.
///
/// # Errors
///
/// Un `400` de [`AppError::InvalidInput`] para el parámetro que falta o que no es un UUID, un `404`
/// de [`AppError::NotFound`] para la fila que no está, lo que devuelva la traducción del motor si el
/// repositorio no se puede listar, y un `500` de [`AppError::Database`] si la fila del repositorio no
/// se puede leer o si los objetivos de sus instantáneas no se pueden resolver.
pub async fn listado(
    State(estado): State<AppState>,
    Query(consulta): Query<ConsultaDelCable>,
) -> Result<ApiResponse, AppError> {
    let fila = repositorio_del_explorador(&estado, consulta).await?;

    // El listado, por la fachada del motor y **nunca** por `Repositorio::listar_snapshots`: con el
    // backend de S3 la biblioteca construye su propio runtime, y llamarla desde dentro del de Tokio
    // —que es donde corre un manejador— entra en pánico. El porqué entero, en el encabezado.
    let instantaneas = estado
        .motor
        .listar_snapshots(acceso_del_repositorio(&fila), LA_VENTANA)
        .await
        .map_err(del_motor)?;

    // La ventana ya la aplicó el motor, así que aquí solo se mira si se ha llenado: una ventana
    // llena y el silencio serían indistinguibles de «esto es todo». Ver el rustdoc del campo.
    let truncated = instantaneas.len() >= LA_VENTANA;

    // El target de cada instantánea lo resuelve **este** manejador y no el motor: el motor lista lo
    // que hay en el almacenamiento y no lee la base —no sabe de ella—, y el vínculo
    // `instantánea → ejecución → tarea → target` solo existe ahí. Va después de listar y no antes
    // porque los identificadores que hay que resolver son los que ha devuelto el listado.
    //
    // Una sola lectura para todo el lote: resolver los 200 identificadores de la ventana de uno en
    // uno serían 200 consultas para pintar una pantalla.
    let identificadores: Vec<String> = instantaneas
        .iter()
        .map(|resumen| resumen.id.clone())
        .collect();

    let objetivos = estado
        .db
        .ejecuciones
        .objetivos_de_las_instantaneas(&identificadores, &fila.id)
        .await?;

    // Y el nombre de la tarea, por el mismo vínculo pero sin pasar por el target: lo que hace falta
    // es el `job_id` del objetivo —que ya vino resuelto en la lectura anterior— y el nombre de esa
    // tarea. Otra vez **una sola** lectura para todo el lote, y solo de las tareas que aparecen:
    // una instantánea sin vínculo no aporta identificador y sale con `null`, que es lo que el
    // contrato publica para «no se sabe el nombre».
    let tareas: Vec<Uuid> = objetivos.values().map(|objetivo| objetivo.job_id).collect();

    let nombres = estado.db.tareas.nombres(&tareas).await?;

    // La vista se compone antes de envolverla porque formatear una fecha puede fallar: ese fallo es
    // un `500` con la causa al registro, y no una respuesta a la que le falte un elemento.
    let vista = ListadoDelCable::desde(&instantaneas, &objetivos, &nombres, truncated)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "instantáneas listadas",
        contenido(&vista)?,
    ))
}

/// `POST /api/v1/snapshots/{id}/restore` — restaurar una instantánea sobre el target que la copió.
///
/// El vínculo entre una instantánea y su target no lo guarda la instantánea —la biblioteca la
/// escribe sin nombre de tarea—, así que se resuelve por el único camino que el §3 sí deja escrito:
/// `job_executions.snapshot_id` → la tarea → su `target_id`. Ese camino lo recorre la capa de
/// acceso —[`Ejecuciones::objetivo_de_la_instantanea`](crate::db::entities::Ejecuciones::objetivo_de_la_instantanea)—
/// y el manejador solo decide qué hacer con su desenlace.
///
/// # Los rechazos, y por qué son ésos
///
/// * **`400` si el `repository_id` del cuerpo no es un UUID**: es el mismo reparto que en el
///   listado —una errata de teclado no es un recurso ausente—, y el rechazo nombra el campo.
/// * **`404` si el repositorio no existe**: sin fila no hay destino que abrir, y se contesta antes
///   de tocar el almacenamiento.
/// * **`409` si la instantánea no casa con ninguna ejecución**: la instantánea **existe** en el
///   repositorio —si no, el `409` no tendría sentido—, lo que falta es el vínculo con un target, y
///   eso no es un recurso ausente sino un conflicto con el estado del sistema: por eso no se
///   contesta `404`, que el panel leería como «esa copia no está» cuando sí está.
/// * **`404` si la instantánea no está en el repositorio**: el único `404` que exige abrir el
///   almacenamiento. El vínculo de la fila dice cuál se copió, no si sigue ahí —una copia purgada o
///   un repositorio recreado dejan el vínculo intacto y la instantánea ausente—, así que el `200` no
///   puede ser optimista: sin esta comprobación prometería un trabajo que no puede ocurrir y el
///   fallo solo se vería en el registro. Se distingue del `404` del repositorio ausente por el
///   mensaje, para que el operador sepa si lo que falta es el repositorio o la copia.
/// * **`422` si un target `directory` no declara su ruta**: aquí el vínculo **sí** existe y la
///   petición es correcta; lo que no se puede cumplir es la operación por la forma del target. Eso
///   no es un conflicto con el estado —`409`— sino una entidad que no se puede procesar, y el motivo
///   **nombra el campo** que falta. El `host_path` es `Option` porque el §3 solo lo pide en un
///   comentario de columna —«Requerido para tipo `directory`»— y **ningún `CHECK` de la base lo
///   garantiza**: sus columnas son anulables y su único `CHECK` restringe el tipo, así que un
///   `directory` sin ruta es una fila que la tabla guarda tan contenta —lo dice el alta de targets—
///   y que aquí no se puede dar por hecha.
/// * **`428` si un target de base de datos no trae la confirmación de sobrescritura**: la operación
///   pisa datos vivos, así que sin `sobrescribir: true` no se lanza nada. El `428 Precondition
///   Required` y **no** un `400` —la petición está bien formada— ni un `409` —no choca con lo que ya
///   está pasando— ni un `422` —la operación sí se puede cumplir—: lo que falta es un acto explícito
///   de quien pregunta, y ése es justo el sentido de la condición previa.
/// * **`409` si ese destino ya tiene una restauración en vuelo**: dos restauraciones sobre el mismo
///   destino escribirían a la vez en el mismo sitio, así que la segunda se rechaza antes de lanzarse.
///   El sujeto es el **destino** y no la instantánea: dos instantáneas distintas del mismo target
///   chocan igual, porque el disco —o el contenedor— es el mismo. La reserva se suelta cuando el
///   trabajo acaba —ver [`ReservaDeRestauracion`](crate::engine::ReservaDeRestauracion)—, de modo que
///   un `409` solo habla de una restauración que está corriendo **ahora**.
///
/// # El **orden** de los rechazos es contrato
///
/// La fila y el vínculo se resuelven sin abrir el almacenamiento, así que los dos primeros rechazos
/// —`404` del repositorio y `409` del vínculo— se deciden sin pagar una apertura. Después va la
/// existencia **real** de la copia, que es la que sí la paga, y solo entonces la puerta por tipo
/// —`422` o `428`—, la reserva del destino y la fila. La comprobación de la instantánea va **antes**
/// de la puerta de la confirmación a propósito: al revés, el panel pediría confirmar una
/// sobrescritura sobre una copia que no existe. Y todo lo anterior va **antes** de crear la fila y
/// lanzar el trabajo: un `404`, un `422` o un `428` **no publican** ningún `restore_progress` ni
/// lanzan ninguna ejecución, y eso es contrato.
///
/// # Por qué la restauración va **en segundo plano**, y por qué la fila va **antes**
///
/// El contrato del §5 es que este endpoint devuelve la **ejecución**, no su resultado: restaurar
/// abre el repositorio y recorre el árbol escribiendo en el host, y eso puede tardar minutos. Si el
/// manejador esperara a que terminara, el cliente tendría la respuesta bloqueada todo ese tiempo. Se
/// lanza en un task propio y el `200` vuelve ya. Pero la fila de `restore_executions` se crea
/// **antes** del `200` y es la que da el `execution_id`: una restauración existe en el histórico en
/// `pending` desde antes de contestar, y el motor la pasa a `running` y la cierra con su desenlace
/// —`success` o `failed` con su motivo—. Un `execution_id` efímero no diría nada después del `200`,
/// que es lo que la auditoría marcó.
///
/// # Errors
///
/// Un `400` de [`AppError::InvalidInput`] para el identificador mal formado, un `404` de
/// [`AppError::NotFound`] para la fila que no está **o** para la instantánea que el repositorio no
/// tiene, un `409` de [`AppError::Conflict`] si la instantánea no tiene vínculo **o** si ese destino
/// ya tiene una restauración en vuelo, un `422` de [`AppError::UnprocessableEntity`] si el target no
/// declara lo que su tipo necesita, un `428` de [`AppError::PreconditionRequired`] si un target de
/// base de datos llega sin la confirmación, y un `500` de [`AppError::Database`] si el vínculo no se
/// puede resolver. Un fallo al abrir el repositorio para comprobar la instantánea llega como
/// [`AppError::BadGateway`], que es lo que la traducción del motor da a un destino que no responde.
pub async fn restaurar(
    State(estado): State<AppState>,
    Path(instantanea): Path<String>,
    Json(cuerpo): Json<CuerpoDeLaRestauracion>,
) -> Result<ApiResponse, AppError> {
    let id = identificador(&cuerpo.repository_id)?;

    // La fila resuelve **qué** destino abrir, y su ausencia es un `404` antes de tocar el
    // almacenamiento: el mismo texto que el `404` del explorador, para que el panel no tenga que
    // distinguir por el mensaje de qué ruta viene.
    let fila = estado.db.repositorios.leer(&id).await?.ok_or_else(|| {
        AppError::NotFound("no existe ningún repositorio con ese identificador".to_owned())
    })?;

    // El vínculo lo resuelve la capa de acceso: la instantánea no guarda su tarea, así que el único
    // camino es el que dejó escrito la ejecución. `None` es «esa instantánea no casa con ninguna
    // ejecución de este repositorio» —un `409`, no un `404`: la copia existe, lo que falta es el
    // vínculo—.
    let objetivo = estado
        .db
        .ejecuciones
        .objetivo_de_la_instantanea(&instantanea, &id)
        .await?;

    let Some(objetivo) = objetivo else {
        return Err(AppError::Conflict(
            "la instantánea no está asociada a ninguna ejecución ni target: no hay sobre qué \
             restaurarla"
                .to_owned(),
        ));
    };

    // Dos accesos y no uno: el tipo no es `Clone` —lleva los secretos ya descifrados, y clonarlos
    // por comodidad sería repartirlos—, así que la comprobación y el trabajo se llevan cada uno el
    // suyo. Construirlo cuesta leer seis campos de una fila que ya está en memoria.
    let acceso_de_la_comprobacion = acceso_del_repositorio(&fila);

    // El `404` **de verdad**: la fila y el vínculo se resuelven sin abrir el almacenamiento, así que
    // hasta aquí no se sabe si la copia sigue existiendo. Una instantánea que ya no está —una que
    // purgó la retención, un repositorio recreado en el mismo destino— no se puede restaurar, y
    // decirlo exige preguntárselo al repositorio. `job_executions.snapshot_id` dice **cuál** se
    // copió, no si sigue ahí: sin esta comprobación el `200` prometería un trabajo contra una copia
    // ausente y el fallo solo saldría en el registro, que es lo que la auditoría marcó.
    //
    // Va **antes** de la puerta de la confirmación, y el **orden es contrato**: al revés, el panel
    // pediría confirmar una sobrescritura sobre una copia que no existe y el operador confirmaría
    // una operación imposible. Y va antes de crear la fila y lanzar el trabajo: el escenario del
    // contrato dice que este `404` no publica ningún `restore_progress` ni lanza ninguna ejecución.
    if !estado
        .motor
        .tiene_la_instantanea(acceso_de_la_comprobacion, &instantanea)
        .await
        .map_err(del_motor)?
    {
        return Err(AppError::NotFound(
            "el repositorio indicado no tiene ninguna instantánea con ese identificador: no hay \
             nada que restaurar"
                .to_owned(),
        ));
    }

    // La puerta por **tipo**, ya con la copia confirmada en el almacenamiento y antes de reservar
    // nada: las dos ramas son rechazos de la petición y ninguna lanza trabajo ni publica aviso.
    if objetivo.target_type == TipoDeTarget::Directory {
        // El §3 pide la ruta del host para un target de directorio, pero **la base no la
        // garantiza**: su único `CHECK` restringe `target_type` y `host_path` es anulable, así que
        // un `directory` sin ruta es una fila que la tabla guarda —lo dice el alta de targets— y
        // aquí no se da por hecha: sin destino no hay dónde restaurar. El motivo **nombra el campo**
        // que falta, que es lo único que permite arreglarlo sin leer el código, y el blanco cuenta
        // como falta —una ruta de espacios no nombra ningún sitio—.
        if objetivo
            .host_path
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(AppError::UnprocessableEntity(
                "el target de la instantánea es de directorio pero le falta `host_path`: sin la \
                 ruta del host no hay dónde restaurarla"
                    .to_owned(),
            ));
        }
    } else if !cuerpo.sobrescribir {
        // Un target de base de datos pisa datos vivos: sin la confirmación explícita no se lanza el
        // trabajo. El `428` y **no** un `400` —la petición está bien formada— ni un `422` —la
        // operación sí se puede cumplir—: lo que falta es que quien pregunta confirme, y el panel
        // tiene que poder distinguir «confirma esto» de «ese target está mal».
        return Err(AppError::PreconditionRequired(
            "restaurar un target de base de datos sobrescribe datos vivos: confirma la \
             sobrescritura con `sobrescribir: true`"
                .to_owned(),
        ));
    }

    // El **destino** de la guarda de exclusión, que depende del tipo: la ruta del host en un
    // directorio y la clave del contenedor y la base en un target de base. Se resuelve aquí —y no
    // con un `host_path` suelto— porque una base se protege por su contenedor: el mismo contenedor
    // puede alojar dos servidores distintos, y los dos no pueden escribir a la vez. Su incoherencia
    // —un campo que el tipo necesita y no está— se rechaza con el mismo `422` que el resto de lo que
    // no se puede procesar por su forma.
    let destino = destino_del_objetivo(&objetivo)
        .map_err(|causa| AppError::UnprocessableEntity(causa.to_string()))?;

    // La guarda de exclusión por **destino**, antes de responder: dos restauraciones sobre el mismo
    // destino escribirían a la vez en el mismo sitio. La reserva viaja con el trabajo —ver
    // `ReservaDeRestauracion`— y se suelta cuando el trabajo acabe, sea como sea.
    let reserva = estado
        .motor
        .reservar_el_destino(&destino)
        .map_err(del_motor)?;

    // Los dos campos que la respuesta publica, antes de mover el objetivo a la tarea.
    let target_id = objetivo.target_id;
    let target_name = objetivo.target_name.clone();

    // La fila **antes** del `200`, y su identificador es el `execution_id`: una restauración existe
    // en el histórico **en `pending`** desde antes de contestar, y el ciclo de vida —`running` y su
    // desenlace— lo escribe el motor sobre esta misma fila. Un `execution_id` efímero no diría nada
    // después del `200` —ni si acabó, ni cuándo, ni sobre qué—, que es lo que la auditoría marcó. La
    // fecha la pone aquí el manejador porque la columna es `NOT NULL` y sin valor por defecto.
    let restauracion = estado
        .db
        .restauraciones
        .alta(AltaDeRestauracion {
            snapshot_id: instantanea.clone(),
            target_id: Some(target_id),
            started_at: OffsetDateTime::now_utc(),
        })
        .await?;

    let execution_id = restauracion.id;

    // La restauración va **en segundo plano** para que el `200` vuelva ya: el contrato devuelve la
    // ejecución, no su resultado. El fallo no puede viajar al cliente —ya tiene su respuesta—, así
    // que se registra, que es lo único que se puede hacer con él.
    let motor = estado.motor.clone();
    let acceso = acceso_del_repositorio(&fila);
    let instantanea_tarea = instantanea.clone();

    // La confirmación viaja **entera** al motor, y no solo por la puerta del `428` de más arriba:
    // para un target de base la API la exige, y para uno de directorio es la que autoriza a **podar**
    // el destino —retirar lo que hay en él y no está en la instantánea—, de modo que un directorio
    // sin confirmar solo recibe. Ver [`Motor::restaurar`](crate::engine::Motor::restaurar).
    let sobrescribir = cuerpo.sobrescribir;

    tokio::spawn(async move {
        // La reserva vive lo que dure el trabajo: se suelta al terminar —y también si el futuro se
        // desmonta por un pánico o por una cancelación, porque es un local suyo y su `Drop` corre
        // igual—. Por eso no hay un `liberar` explícito aquí: el que se puede olvidar es el que
        // deja el destino reservado para siempre.
        let _reserva = reserva;

        if let Err(error) = motor
            .restaurar(
                acceso,
                instantanea_tarea,
                objetivo,
                execution_id,
                sobrescribir,
            )
            .await
        {
            tracing::error!(%error, "la restauración en segundo plano falló");
        }
    });

    let vista = RestauracionDelCable {
        execution_id: execution_id.to_string(),
        snapshot_id: instantanea,
        target_id: target_id.to_string(),
        target_name,
    };

    Ok(ApiResponse::new(
        StatusCode::OK,
        "restauración lanzada",
        contenido(&vista)?,
    ))
}

/// El `data` de la respuesta de la restauración: en qué ejecución corre, qué se restaura y dónde.
///
/// # Estos nombres son un **añadido al SRS**
///
/// El §5 publica el `execution_id` del evento SSE de la restauración pero **no** el JSON de esta
/// respuesta, así que los cuatro nombres los fija el RED de este change, igual que los seis campos
/// del explorador. Van en snake_case porque el test los lee así y porque el resto del §5 usa ese
/// estilo.
#[derive(Debug, Serialize)]
struct RestauracionDelCable {
    /// La ejecución de la restauración, que **es** la fila persistida en `restore_executions`.
    execution_id: String,
    /// La instantánea que se está restaurando.
    snapshot_id: String,
    /// El target sobre el que se restaura.
    target_id: String,
    /// El nombre del target, para que el panel pueda anunciarlo sin otra consulta.
    target_name: String,
}

/// Convierte el `repository_id` del cuerpo en un `Uuid`, o contesta `400` nombrando su campo.
///
/// El motivo **nombra el campo y no repite el valor** que llegó: la regla de la casa es que un
/// rechazo diga qué campo está mal, nunca lo que traía. Es el mismo reparto que en los tres recursos
/// con identificador en el camino —una errata de teclado no es un recurso ausente— y que en el
/// listado de este mismo módulo.
fn identificador(repository_id: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(repository_id).map_err(|_| {
        AppError::InvalidInput(
            "el campo `repository_id` del cuerpo no tiene la forma de un UUID".to_owned(),
        )
    })
}

/// Resuelve la fila del repositorio que pide la consulta, o el rechazo que le corresponda.
///
/// Los tres desenlaces de aquí son los del contrato y están escritos en el rustdoc del manejador:
/// el `400` que nombra el parámetro que falta, el `400` del identificador mal formado y el `404` de
/// la fila que no está. Vive aparte del manejador porque son tres desenlaces con tres motivos
/// distintos y el manejador, con ellos dentro, dejaría de leerse lo que hace: preguntarle al
/// repositorio qué tiene.
///
/// El identificador se enlaza a la base en su forma canónica —la capa de acceso lo hace— y **nunca**
/// como un `Uuid` suelto: `sqlx` lo escribiría como un BLOB de 16 bytes y la columna es `TEXT`.
async fn repositorio_del_explorador(
    estado: &AppState,
    consulta: ConsultaDelCable,
) -> Result<Repositorio, AppError> {
    let Some(crudo) = consulta.repository_id.as_deref() else {
        return Err(AppError::InvalidInput(
            "el parámetro `repository_id` de la consulta es obligatorio: el explorador mira un \
             repositorio cada vez"
                .to_owned(),
        ));
    };

    // El motivo nombra el campo y **no** repite el valor que llegó: la regla de la casa es que un
    // rechazo diga qué campo está mal, nunca lo que traía.
    let id = Uuid::parse_str(crudo).map_err(|_| {
        AppError::InvalidInput(
            "el parámetro `repository_id` de la consulta no tiene la forma de un UUID".to_owned(),
        )
    })?;

    // El mismo texto que el `404` de las otras tres rutas que resuelven un repositorio por su
    // identificador, para que el panel no tenga que distinguir por el mensaje de qué ruta viene.
    estado.db.repositorios.leer(&id).await?.ok_or_else(|| {
        AppError::NotFound("no existe ningún repositorio con ese identificador".to_owned())
    })
}

/// El acceso al destino que describe la **fila** del repositorio.
///
/// Los dos secretos llegan **ya descifrados** por la capa de acceso, que es la única que tiene la
/// clave maestra: este módulo no descifra nada, solo copia a lo que la biblioteca necesita lo que la
/// fila trae. Y los dos siguen siendo [`crate::secret::SecretString`] hasta el final, así que no
/// pueden acabar en un registro ni en el cuerpo de una respuesta.
///
/// # Por qué la copia se repite en vez de compartirse
///
/// La misma conversión existe hoy en otros dos sitios, y ninguna de las tres se puede reutilizar sin
/// tocar `src/rustic`: el motor la hace inline al copiar una tarea
/// (`src/engine/ejecucion.rs`), y el alta de un repositorio la hace **desde el cuerpo** de la
/// petición y no desde una fila —en un alta todavía no hay fila, y por eso aquélla es otra
/// conversión—. El constructor que unificaría las tres es un `Acceso::desde(&Repositorio)` del
/// módulo del repositorio, que es donde vive el tipo; mientras no exista, la copia de seis campos
/// vive junto a cada uso en vez de detrás de un salto de módulo.
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

/// El `data` del listado: los elementos, y si la ventana se ha llenado.
///
/// Es un objeto y no un array pelado como el listado del histórico, y no por gusto de forma: allí el
/// cliente controla la ventana con `limit` y `offset` y puede deducir que hay más, y aquí solo manda
/// `repository_id`, así que la respuesta tiene que decir por sí misma que el listado se ha
/// recortado. Un array pelado no tiene dónde decirlo.
#[derive(Debug, Serialize)]
struct ListadoDelCable {
    /// Las instantáneas del repositorio, ordenadas de la más reciente a la más antigua.
    snapshots: Vec<SnapshotDelCable>,
    /// Si el listado se ha recortado: la ventana se ha llenado y **puede** haber más.
    ///
    /// Significa **«el listado se ha recortado»**, y **no** promete que los 200 que lo acompañan
    /// sean **los más recientes**: la ventana acota el **número**, no el orden de antigüedad —el
    /// identificador de una instantánea es el nombre de un fichero y no lleva fecha, así que hay que
    /// acotar antes de resolver, y acotar antes de resolver implica que el subconjunto devuelto no
    /// tiene por qué ser el de las últimas—. Lo único que se garantiza es el **orden** de lo que
    /// devuelve: de la más reciente a la más antigua.
    ///
    /// Y llenar la ventana se informa como «puede haber más» aunque eso no se pueda distinguir de
    /// «hay exactamente esta ventana»: lo que **no** se puede hacer es lo contrario —devolver la
    /// ventana llena y callarse—, porque entonces «lleno» y «esto es todo» se leerían igual y quien
    /// mire el panel creería que el repositorio tiene exactamente esas instantáneas y ninguna más.
    /// Distinguir los dos casos exigiría resolver una instantánea de más, y resolver es justamente
    /// la lectura cara que la ventana existe para acotar: de los dos errores posibles —decir «puede
    /// haber más» cuando no lo hay, o decir «esto es todo» cuando no lo es— se elige el que no
    /// esconde copias.
    truncated: bool,
}

impl ListadoDelCable {
    /// Vuelca las instantáneas del motor en la vista del cable, casando cada una con su objetivo.
    ///
    /// Puede fallar porque formatear una fecha es falible: ver [`SnapshotDelCable::desde`].
    ///
    /// El `objetivos` lo trae **quien llama** —el manejador, de la capa de datos— y no el motor, que
    /// no lee la base: aquí solo se casa por el identificador de la instantánea, y la que no esté en
    /// el mapa sale con los tres campos de su target a `null`, que es lo que significa «esta copia no
    /// casó con ninguna ejecución» y **no** un hueco que rellenar.
    ///
    /// El `nombres` es la otra mitad del mismo reparto: el nombre de cada tarea, ya resuelto por
    /// lotes, casado por el `job_id` que el objetivo trae. Una instantánea sin objetivo no tiene
    /// `job_id` con el que casar y sale con `job_name` a `null` sin consultar el mapa —no hay tarea
    /// cuyo nombre pedir—, y una cuya tarea no esté en el mapa sale igual: `null`.
    fn desde(
        instantaneas: &[ResumenDeSnapshot],
        objetivos: &HashMap<String, ObjetivoDeRestauracion>,
        nombres: &HashMap<Uuid, String>,
        truncated: bool,
    ) -> Result<Self, AppError> {
        let snapshots = instantaneas
            .iter()
            .map(|resumen| SnapshotDelCable::desde(resumen, objetivos.get(&resumen.id), nombres))
            .collect::<Result<_, _>>()?;

        Ok(Self {
            snapshots,
            truncated,
        })
    }
}

/// Una instantánea del repositorio, tal y como viaja por el cable.
///
/// # Estos nombres son un **añadido al SRS**
///
/// El §5 describe esta ruta con cuatro palabras —«Explorador de snapshots en el almacenamiento
/// S3»— y **no** publica el JSON de su respuesta, así que los nombres de estos seis campos y los dos
/// de la respuesta los fija el RED de este change, igual que en su día se añadieron las tres rutas
/// de `api-endpoints` y como el alta de un repositorio añadió `data.outcome`. Son vocabulario del
/// cable, en el inglés del resto del §5 —que publica `execution_id`, `bytes_processed` y
/// `current_action`—, mientras que lo que se pinta se queda en español. Quedan listados en el
/// documento de la feature, que es donde se leen las incorporaciones al SRS.
///
/// # Por qué estos seis y no otros
///
/// Porque son los que el panel necesita para pintar una fila del explorador, y ninguno es
/// deducible de otro: `id` en **hex completo** —la forma corta de la biblioteca es un prefijo
/// ambiguo, y con ella dos instantáneas que empiecen igual se confunden: es el único valor con el
/// que se resuelve una instantánea sin ambigüedad—, `time` en RFC3339 —el formato del resto de la
/// API—, `paths` con las rutas que la instantánea **declara** contener, `label` con la etiqueta de
/// la tarea, y `files` y `bytes` como el resumen de lo que la copia procesó.
///
/// # Y por qué los tres del target van **aparte** de esos seis
///
/// Porque no salen del repositorio ni de la instantánea, sino del vínculo que dejó la ejecución que
/// la copió: son la respuesta a «¿sobre qué se restauraría esto y se puede?», que es la pregunta que
/// el panel hace **antes** de ofrecer el botón. Su `null` no es un dato que falte sino un dato: una
/// copia sin vínculo —de otra herramienta, o cuya fila se borró en cascada con su tarea— no tiene
/// destino conocido, y eso es justo lo que hay que poder leer.
#[derive(Debug, Serialize)]
struct SnapshotDelCable {
    /// El identificador de la instantánea, en hex completo.
    id: String,
    /// La fecha de la copia, en RFC3339.
    time: String,
    /// Las rutas que la instantánea declara contener.
    paths: Vec<String>,
    /// La etiqueta que identifica a la tarea; `null` si la instantánea no lleva ninguna.
    ///
    /// `null` y no una cadena vacía: una instantánea sin etiqueta es una instantánea que la
    /// biblioteca guardó sin ella —las que escribe Kesto siempre llevan la de su tarea—, y el
    /// resumen del repositorio ya la trae distinguiendo los dos casos. Convertir el `None` en `""`
    /// dejaría al panel con un valor que parece una etiqueta y no lo es.
    label: Option<String>,
    /// Los ficheros que la copia procesó.
    files: u64,
    /// Los bytes que la copia procesó.
    bytes: u64,
    /// El identificador del target sobre el que se restauraría, en su forma canónica; `null` si la
    /// instantánea no casó con ninguna ejecución.
    ///
    /// Es anulable **por contrato** y no por descuido, y lo es junto a los dos de abajo: la
    /// instantánea existe en el repositorio y puede que ninguna ejecución la haya anotado, y en ese
    /// caso el panel tiene que poder decir «de ésta no se sabe el destino» en vez de esconderla del
    /// listado o de inventarse un destino.
    target_id: Option<String>,
    /// El nombre del target, para que el panel lo anuncie sin otra consulta; `null` con el resto.
    target_name: Option<String>,
    /// El tipo del target, con **el mismo texto del cable** que publica `/api/v1/targets`; `null`
    /// con el resto.
    ///
    /// Se publica aunque el target no sea restaurable en esta versión —solo un `directory` lo es—
    /// porque es el dato con el que el panel decide si deshabilitar la acción: saberlo **antes** de
    /// intentarlo es justo lo que evita un `422` que el operador no podía prever.
    target_type: Option<String>,
    /// El nombre de la tarea que copió la instantánea; `null` si no se sabe.
    ///
    /// Anulable por los dos motivos del contrato: la instantánea puede no casar con ninguna tarea
    /// —copia de otra herramienta, o vínculo borrado en cascada— y la tarea puede existir y no tener
    /// nombre. Las dos cosas se publican igual, con `null`, y no hay por qué distinguirlas aquí: en
    /// los dos casos el panel no tiene un nombre que pintar y cae al identificador.
    ///
    /// **No** sustituye a la `label`: ésta sigue siendo el identificador de la tarea, y el nombre
    /// legible viaja en su propio campo para no cambiar lo que ya estaba escrito.
    job_name: Option<String>,
}

impl SnapshotDelCable {
    /// Convierte un resumen del repositorio y su objetivo —si lo tiene— en la vista del cable.
    ///
    /// La fecha se formatea aquí y **puede fallar** —el rango de fechas de `time` es más estrecho
    /// que el de la biblioteca—, y ese fallo es un `500` con la causa al registro y no una fecha
    /// inventada: es el único campo que dice cuándo se copió, y rellenarlo con algo plausible sería
    /// mentir justo en el dato que se publica para no tener que abrir la copia.
    ///
    /// El resto son copias tal cual: el resumen del repositorio ya viene con los tipos del panel
    /// —los dos contadores como `u64` y las rutas como cadenas—, así que aquí no se reinterpreta
    /// ninguno.
    ///
    /// Los tres del target salen del objetivo que resolvió la capa de datos, y su ausencia se
    /// publica como `null` en los tres: no hay un caso en que uno solo falte, porque o hay vínculo
    /// —y entonces están los tres— o no lo hay. El tipo se traduce con
    /// [`texto_del_tipo`], que es la **misma** función que usa el recurso de targets: el vocabulario
    /// del cable es uno, no uno por ruta.
    fn desde(
        resumen: &ResumenDeSnapshot,
        objetivo: Option<&ObjetivoDeRestauracion>,
        nombres: &HashMap<Uuid, String>,
    ) -> Result<Self, AppError> {
        Ok(Self {
            id: resumen.id.clone(),
            time: fecha(resumen.fecha)?,
            paths: resumen.rutas.clone(),
            label: resumen.etiqueta.clone(),
            files: resumen.ficheros,
            bytes: resumen.bytes,
            target_id: objetivo.map(|objetivo| objetivo.target_id.to_string()),
            target_name: objetivo.map(|objetivo| objetivo.target_name.clone()),
            target_type: objetivo.map(|objetivo| texto_del_tipo(objetivo.target_type).to_owned()),
            // El nombre sale del mismo vínculo que los tres de arriba pero **no** del objetivo: el
            // `job_id` del objetivo es la llave con la que se busca en el mapa que resolvió el
            // manejador. Sin objetivo no hay llave —y el `and_then` deja el `null` sin consultar—;
            // con él y sin entrada en el mapa, también `null`.
            job_name: objetivo.and_then(|objetivo| nombres.get(&objetivo.job_id).cloned()),
        })
    }
}

/// Formatea una fecha como RFC3339, que es el formato del cable.
///
/// Es el mismo reparto que en el listado del histórico, y el mensaje **no** lleva el valor que se
/// estaba formateando: una fecha de una fila no es un secreto, pero la regla de la casa es que un
/// motivo que sale al registro diga qué falló y no de qué valor se trataba.
fn fecha(fecha: OffsetDateTime) -> Result<String, AppError> {
    fecha.format(&Rfc3339).map_err(|error| {
        AppError::Other(format!(
            "la fecha de la instantánea no se pudo formatear: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use axum::Router;
    use axum::http::StatusCode;
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::db::entities::{AltaDeEjecucion, AltaDeTarea, AltaDeTarget, Estado, TipoDeTarget};
    use crate::engine::EventoDelMotor;
    use crate::state::{AppState, EstadoDeBase};
    use crate::test_support::{
        DirectorioDePrueba, ETIQUETA_AJENA, ETIQUETA_SEMBRADA, PASSPHRASE_DE_PRUEBA, SEMILLA,
        SEMILLA_CONTENIDO, base_con_clave, body_json, config_con_dsn, directorio_de_prueba,
        estado_de_prueba, get, nombre_de_tarea_de_prueba, post_json, router_desarrollo,
        sembrar_snapshot_ajeno, sembrar_snapshots,
    };

    /// Un identificador bien formado cualquiera, que no corresponde a ninguna fila.
    ///
    /// Bien formado a propósito: así el `404` que se busca lo decide la ausencia de la fila y no el
    /// rechazo del identificador, que es otro caso y tiene su propia prueba.
    const UN_UUID: &str = "00000000-0000-4000-8000-000000000000";

    /// La ventana del listado, la misma que la del histórico de ejecuciones.
    ///
    /// Es la lectura más cara del change —hay que abrir el repositorio y recorrer su índice—, así que
    /// el número no es una comodidad del panel sino una cota del servidor.
    const LA_VENTANA: usize = 200;

    /// El cuerpo del alta de un repositorio cuyo destino es la ruta **local** dada.
    ///
    /// El `endpoint` es la ruta de un directorio de verdad y no una URL de mentira: el alta **habla con
    /// el destino** desde el change `backup-engine`, así que una URL que no existe convertiría esta
    /// siembra en un fallo por destino inalcanzable, que no es lo que se prueba aquí.
    fn cuerpo_de_alta(nombre: &str, ruta: &str) -> serde_json::Value {
        serde_json::json!({
            "name": nombre,
            "endpoint": ruta,
            "bucket": format!("cubo-{nombre}"),
            "region": "eu-west-1",
            "access_key_id": "AKIAIOSFODNN7EXAMPLE",
            "secret_access_key": "AKIAIOSFODNN7EXAMPLE-secreto-del-explorador",
            "passphrase": PASSPHRASE_DE_PRUEBA,
        })
    }

    /// Da de alta el repositorio que describe el destino y devuelve su identificador.
    ///
    /// Se afirma aquí, y no en cada prueba, que el alta contesta `201`: el alta tiene sus propias
    /// pruebas en `repositories` —los dos desenlaces, el destino inalcanzable y la passphrase que no
    /// abre—, y lo que estas pruebas necesitan de ella es la fila, no volver a medirla.
    async fn alta_de_repositorio(router: &Router, nombre: &str, ruta: &str) -> String {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &cuerpo_de_alta(nombre, ruta),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el repositorio del explorador tiene que darse de alta —el listado sale de él y no de la \
         base— y el alta contestó {status} con {cuerpo}"
        );

        cuerpo["data"]["id"]
            .as_str()
            .expect("el alta del repositorio tiene que devolver su identificador")
            .to_owned()
    }

    /// Comprueba que un elemento del listado trae los seis datos del contrato, cada uno con su tipo,
    /// **las tres claves** del target y la del nombre de la tarea.
    ///
    /// Los seis y no uno: el explorador es la única pantalla que enseña qué hay **dentro** del
    /// repositorio, y cada dato que falte deja una columna de la tabla sin poder pintarse. El tipo se
    /// comprueba además del valor porque un número serializado como cadena —o una fecha como número—
    /// pasa un `is_some` y rompe al panel.
    ///
    /// De los tres del target se comprueba aquí **la presencia** y no el valor: el contrato los
    /// publica en todos los elementos —un elemento sin esas claves dejaría al panel sin poder
    /// distinguir «esta copia no tiene target» de «el servidor no lo dice»—, y cuáles llegan a `null`
    /// y cuáles poblados lo miden las pruebas que siembran el vínculo. Lo mismo vale para `job_name`
    /// y con más motivo: una instantánea **sin** vínculo es un caso legítimo y frecuente —una copia
    /// de otra herramienta—, así que su `null` es un dato y no puede desaparecer de la respuesta.
    fn comprobar_elemento(elemento: &serde_json::Value, donde: &str) {
        assert!(
            elemento["id"].as_str().is_some_and(|id| !id.is_empty()),
            "el elemento {donde} no trae su identificador: {elemento}"
        );

        let fecha = elemento["time"]
            .as_str()
            .unwrap_or_else(|| panic!("el elemento {donde} no trae la fecha: {elemento}"));

        OffsetDateTime::parse(fecha, &Rfc3339).unwrap_or_else(|error| {
            panic!("la fecha del elemento {donde} tiene que ser RFC3339 y llegó `{fecha}`: {error}")
        });

        let rutas = elemento["paths"]
            .as_array()
            .unwrap_or_else(|| panic!("el elemento {donde} no trae sus rutas: {elemento}"));

        assert!(
            !rutas.is_empty() && rutas.iter().all(serde_json::Value::is_string),
            "el elemento {donde} tiene que traer al menos una ruta y todas como cadenas: {elemento}"
        );

        assert!(
            elemento["label"]
                .as_str()
                .is_some_and(|etiqueta| !etiqueta.is_empty()),
            "la instantánea del elemento {donde} se sembró con etiqueta y llegó sin ella: {elemento}"
        );

        assert!(
            elemento["files"]
                .as_u64()
                .is_some_and(|ficheros| ficheros >= 1),
            "el resumen del elemento {donde} tiene que contar al menos un fichero: {elemento}"
        );

        assert!(
            elemento["bytes"].as_u64().is_some_and(|bytes| bytes >= 1),
            "el resumen del elemento {donde} tiene que contar los bytes que la copia procesó: \
         {elemento}"
        );

        for campo in CAMPOS_DEL_TARGET {
            assert!(
                elemento.get(campo).is_some(),
                "el elemento {donde} no trae el campo `{campo}` del target, y el contrato lo publica \
             en **todos** los elementos, aunque sea a `null`: {elemento}"
            );
        }

        for campo in CAMPOS_DEL_NOMBRE {
            assert!(
                elemento.get(campo).is_some(),
                "el elemento {donde} no trae el campo `{campo}`, y el contrato lo publica en \
             **todos** los elementos, aunque sea a `null`: {elemento}"
            );
        }
    }

    /// Los tres campos del target que el listado publica en **cada** elemento.
    ///
    /// Son los mismos que necesita el panel para saber si la copia es restaurable y sobre qué: el
    /// identificador, el nombre y el tipo del target, y los tres a `null` cuando la instantánea no
    /// casó con ninguna ejecución.
    const CAMPOS_DEL_TARGET: [&str; 3] = ["target_id", "target_name", "target_type"];

    /// El campo con el nombre de la tarea de origen, que el listado publica en **cada** elemento.
    ///
    /// Es anulable por contrato, y su `null` significa «no se sabe el nombre» —la instantánea no casó
    /// con ninguna tarea, o su tarea no tiene nombre—: la clave tiene que viajar siempre para que el
    /// panel pueda distinguir eso de «el servidor no lo publica». La `label` **no** es esto: sigue
    /// siendo el identificador de la tarea, y por eso el nombre va en su propio campo.
    const CAMPOS_DEL_NOMBRE: [&str; 1] = ["job_name"];

    /// Busca, por su identificador, el elemento del listado que corresponde a una instantánea.
    ///
    /// Se busca por identificador y no por posición porque el orden del listado es el de las fechas
    /// —de la más reciente a la más antigua— y una prueba que se apoyara en la posición estaría
    /// midiendo el orden para hablar del target. Y se falla en vez de devolver `None`: que la
    /// instantánea sembrada no esté en el listado no es un caso a tolerar aquí —eso ya lo afirma la
    /// prueba— sino el motivo por el que la aserción que viene después no significaría nada.
    fn elemento_con_id<'a>(
        listado: &'a [serde_json::Value],
        instantanea: &str,
        cuerpo: &serde_json::Value,
    ) -> &'a serde_json::Value {
        listado
            .iter()
            .find(|elemento| elemento["id"].as_str() == Some(instantanea))
            .unwrap_or_else(|| {
                panic!("la instantánea `{instantanea}` no está en el listado: {cuerpo}")
            })
    }

    /// 1.29a — Sin `repository_id` el listado contesta `400` **nombrando el parámetro**.
    ///
    /// Cubre `api-resources`, escenario «El listado de snapshots exige su parámetro». El repositorio no
    /// viaja en el camino sino en la consulta —un explorador mira **un** repositorio cada vez, y la
    /// ruta no tiene un recurso al que colgarse—, así que lo que falta no es un recurso ausente
    /// (`404`) ni un fallo del servidor (`500`) sino una petición a la que le falta un dato: `400`. Y
    /// nombrarlo es lo que convierte ese `400` en algo que se puede arreglar sin leer el código.
    #[tokio::test]
    async fn sin_el_parametro_el_listado_contesta_400_nombrandolo() {
        let respuesta = get(router_desarrollo().await, "/api/v1/snapshots").await;

        let status = respuesta.status();

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "una petición a la que le falta el parámetro no es una ruta ausente"
        );

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "sin `repository_id` el listado no puede saber qué repositorio mirar y tiene que contestar \
         400; contestó {status}"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 400, "la envoltura del 400: {cuerpo}");

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|motivo| motivo.contains("repository_id")),
            "el motivo del 400 tiene que nombrar el parámetro que falta, y llegó {cuerpo}"
        );
    }

    /// 1.29b — Un `repository_id` que no es un UUID contesta `400`.
    ///
    /// Cubre `api-resources`, escenario «Un repository_id que no es un UUID contesta 400». Es el mismo
    /// reparto que en los tres recursos con identificador en el camino —un identificador mal formado es
    /// una errata de teclado y no un recurso ausente—, y aquí importa además por una razón de coste:
    /// resolver el listado abre el repositorio, que es la operación más cara de la API, así que un
    /// identificador que no puede corresponder a ninguna fila no debe llegar hasta ahí.
    #[tokio::test]
    async fn un_repository_id_que_no_es_un_uuid_contesta_400() {
        let respuesta = get(
            router_desarrollo().await,
            "/api/v1/snapshots?repository_id=no-es-un-uuid",
        )
        .await;

        let status = respuesta.status();

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "un identificador mal formado no es un recurso ausente"
        );

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "un `repository_id` que no es un UUID tiene que contestar 400; contestó {status}"
        );

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 400, "la envoltura del 400: {cuerpo}");

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|motivo| !motivo.is_empty()),
            "el 400 tiene que traer un motivo: {cuerpo}"
        );
    }

    /// 1.29c — Un repositorio que no existe contesta `404`, y uno que existe contesta `200`.
    ///
    /// Cubre `repository`, escenario «El listado identifica el repositorio del que sale».
    ///
    /// Las dos mitades van en la misma prueba y **en este orden** por un motivo medido: un `404` con la
    /// ruta sin montar es el mismo código por un motivo distinto, así que un `assert_eq!(404)` sobre un
    /// repositorio inventado pasaría hoy sin medir nada y seguiría pasando mañana por el mismo azar. La
    /// mitad que sí mide es la de arriba —el listado de un repositorio que existe tiene que contestar
    /// `200`—, y solo después de ella el `404` del que no existe dice lo que dice.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_repositorio_que_no_existe_contesta_404() {
        let router = router_desarrollo().await;

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();

        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let existe = alta_de_repositorio(&router, "explorador-1-29c", &ruta).await;

        let respuesta = get(
            router.clone(),
            &format!("/api/v1/snapshots?repository_id={existe}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado de un repositorio que existe tiene que contestar 200, y contestó {status} con \
         {cuerpo}"
        );

        // Y ahora el que no está: un UUID bien formado, sin fila.
        let respuesta = get(
            router,
            &format!("/api/v1/snapshots?repository_id={UN_UUID}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_ne!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "un repositorio que no existe no es un fallo del servidor: contestó {status} con {cuerpo}"
        );

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "un repositorio que no existe es un recurso ausente y no una petición mal formada; contestó \
         {status} con {cuerpo}"
        );

        assert_eq!(cuerpo["status"], 404, "la envoltura del 404: {cuerpo}");
    }

    /// 1.29d — Cada elemento trae sus seis datos, y el listado **sale del repositorio**.
    ///
    /// Cubre `repository`, escenarios «El listado sale del repositorio» y «El listado identifica el
    /// repositorio del que sale», y `api-resources` en lo que el explorador promete publicar.
    ///
    /// Lo que mide que la lista salga del repositorio no es que haya elementos —una lista deducida de
    /// `job_executions` también los tendría— sino la instantánea **ajena**: la escribió la biblioteca
    /// sin que Kesto interviniera, así que no tiene ninguna fila en el histórico y solo puede aparecer
    /// si alguien ha leído el almacenamiento. Las dos sembradas cubren lo otro: que las copias de Kesto
    /// están, cada una con la etiqueta con la que se identificó su tarea.
    ///
    /// Y dos cosas más, que son las dos mitades que la prueba de la ventana **no** puede medir por
    /// estar en el otro extremo del contrato: el **orden** de la más reciente a la más antigua —lo
    /// único que el explorador garantiza del subconjunto que devuelve—, comprobado sobre la
    /// secuencia entera de fechas, y `truncated: false` con un listado que cabe entero, porque allí
    /// la ventana se llena y el campo es `true`.
    ///
    /// Y una tercera, que es la mitad **sin** vínculo del contrato ampliado: aquí el histórico está
    /// vacío —el repositorio se siembra con la biblioteca y ninguna copia de Kesto ha corrido—, así
    /// que ninguna de las tres instantáneas casa con una ejecución y los tres campos del target
    /// tienen que salir a `null` **sin** que ninguna desaparezca del listado.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cada_elemento_trae_sus_seis_datos_y_el_listado_sale_del_repositorio() {
        let router = router_desarrollo().await;

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();

        let sembradas = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z", "2026-01-02T10:00:00Z"],
        );

        let (ajena, _nodo_ajeno) = sembrar_snapshot_ajeno(destino.ruta(), PASSPHRASE_DE_PRUEBA);

        let repositorio = alta_de_repositorio(&router, "explorador-1-29d", &ruta).await;

        let respuesta = get(
            router,
            &format!("/api/v1/snapshots?repository_id={repositorio}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado de un repositorio sembrado tiene que contestar 200, y contestó {status} con \
         {cuerpo}"
        );

        let listado = cuerpo["data"]["snapshots"].as_array().unwrap_or_else(|| {
            panic!("el listado tiene que traer sus elementos en `data.snapshots`: {cuerpo}")
        });

        assert_eq!(
            listado.len(),
            sembradas.identificadores.len() + 1,
            "el repositorio tiene las dos instantáneas sembradas y la ajena, y el listado trajo {}: \
         {cuerpo}",
            listado.len()
        );

        for (indice, elemento) in listado.iter().enumerate() {
            comprobar_elemento(elemento, &format!("en la posición {indice}"));
        }

        // Y sin ejecuciones en el histórico, los tres campos del target llegan a `null`: es lo que
        // significa «esta copia no casó con ninguna ejecución», y tiene que poder leerse sin que el
        // elemento se caiga del listado. La mitad contraria —los tres poblados— la mide la prueba
        // del vínculo, que sí siembra la cadena.
        for (indice, elemento) in listado.iter().enumerate() {
            for campo in CAMPOS_DEL_TARGET {
                assert!(
                    elemento[campo].is_null(),
                    "sin ejecuciones que anoten la instantánea, el campo `{campo}` del elemento en \
                     la posición {indice} tiene que llegar a `null` y llegó {elemento}"
                );
            }
        }

        // El orden, que es **lo único** que el explorador garantiza del listado: de la más
        // reciente a la más antigua. Se comprueba la secuencia entera de fechas y no la pertenencia
        // de cada elemento —que ya está arriba—, porque un listado que devolviera las mismas
        // instantáneas en cualquier orden pasaría todas las aserciones anteriores, y el panel
        // pintaría la tabla del revés. Las tres fechas son distintas —dos fijas y la de la
        // instantánea ajena, que es el momento de archivarla—, así que el orden tiene que ser
        // **estrictamente** decreciente: con dos fechas iguales, un orden «decreciente» a secas no
        // diría nada.
        let fechas: Vec<OffsetDateTime> = listado
            .iter()
            .map(|elemento| {
                let fecha = elemento["time"]
                    .as_str()
                    .unwrap_or_else(|| panic!("un elemento llegó sin fecha: {elemento}"));

                OffsetDateTime::parse(fecha, &Rfc3339).unwrap_or_else(|error| {
                    panic!("la fecha `{fecha}` del listado no es RFC3339: {error}")
                })
            })
            .collect();

        assert!(
            fechas.windows(2).all(|par| par[0] > par[1]),
            "el explorador promete el orden de la más reciente a la más antigua, y llegó {fechas:?}: \
             un listado desordenado enseña las copias al revés"
        );

        // Y el listado está por debajo de la ventana, así que **no** viene recortado: si el número
        // cabe, decir que hay más sería mentir sobre el contenido del repositorio. Es la mitad que
        // el otro caso —la ventana llena— no puede medir: allí `truncated` es `true` y un campo
        // cableado a `true` pasaría la suite entera.
        assert_eq!(
            cuerpo["data"]["truncated"], false,
            "un listado por debajo de la ventana no puede venir recortado: {cuerpo}"
        );

        let identificadores: Vec<&str> = listado
            .iter()
            .filter_map(|elemento| elemento["id"].as_str())
            .collect();
        let etiquetas: Vec<&str> = listado
            .iter()
            .filter_map(|elemento| elemento["label"].as_str())
            .collect();

        for sembrada in &sembradas.identificadores {
            assert!(
                identificadores.contains(&sembrada.as_str()),
                "la instantánea sembrada `{sembrada}` no está en el listado: {cuerpo}"
            );
        }

        assert!(
            identificadores.contains(&ajena.as_str()),
            "la instantánea ajena `{ajena}` no está en el listado, y el listado tiene que salir del \
         repositorio y no del histórico de ejecuciones: {cuerpo}"
        );

        assert!(
            etiquetas.contains(&ETIQUETA_SEMBRADA) && etiquetas.contains(&ETIQUETA_AJENA),
            "las instantáneas del repositorio tienen que llegar con su etiqueta —`{ETIQUETA_SEMBRADA}` \
         y `{ETIQUETA_AJENA}`— y llegaron {etiquetas:?}"
        );
    }

    /// 1.29f — El listado publica el target de cada instantánea, y `null` si no tiene vínculo.
    ///
    /// Cubre `api-resources`, escenario «El listado trae el target de cada instantánea» (D1). Es la
    /// mitad que la prueba anterior no puede medir: allí el histórico está vacío y todos los
    /// elementos salen sin target; aquí se siembran las dos cosas y se comprueba que la respuesta las
    /// trata **distinto sin perder ninguna**:
    ///
    /// * una instantánea **con** vínculo —target, tarea y ejecución sembrados por la capa de acceso,
    ///   que es la única forma de tener una ejecución, porque las crea una copia que corre—, que sale
    ///   con los tres campos poblados;
    /// * y otra **sin** él, que es la copia que el histórico no anotó —una hecha con otra
    ///   herramienta— y que sigue en el listado con los tres a `null`.
    ///
    /// Se afirma el **tipo del cable** del target y no el de la columna: `directory`, el mismo literal
    /// que publica `/api/v1/targets`, porque es el dato con el que el panel decide si la restauración
    /// es posible **antes** de ofrecerla.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_listado_publica_el_target_de_cada_instantanea_y_null_si_no_lo_tiene() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();

        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z", "2026-01-02T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "explorador-1-29f", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        // Una de las dos se queda sin vínculo: es la ajena al histórico, y tiene que seguir en el
        // listado con su hueco a la vista.
        let con_vinculo = siembra.identificadores[0].clone();
        let sin_vinculo = siembra.identificadores[1].clone();

        let (target, _ejecucion) = sembrar_cadena(
            &estado,
            repositorio_id,
            &con_vinculo,
            TipoDeTarget::Directory,
            Some("/srv/explorador-1-29f".to_owned()),
        )
        .await;

        let respuesta = get(
            router,
            &format!("/api/v1/snapshots?repository_id={repositorio}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado de un repositorio con una instantánea con vínculo y otra sin él tiene que \
             contestar 200, y contestó {status} con {cuerpo}"
        );

        let listado = cuerpo["data"]["snapshots"]
            .as_array()
            .unwrap_or_else(|| panic!("el listado tiene que traer sus elementos: {cuerpo}"));

        assert_eq!(
            listado.len(),
            siembra.identificadores.len(),
            "una instantánea sin vínculo sigue siendo una copia del repositorio y no puede \
             desaparecer del listado: {cuerpo}"
        );

        let del_vinculo = elemento_con_id(listado, &con_vinculo, &cuerpo);

        assert_eq!(
            del_vinculo["target_id"].as_str(),
            Some(target.to_string().as_str()),
            "la instantánea con vínculo tiene que publicar el identificador de su target: {cuerpo}"
        );
        assert_eq!(
            del_vinculo["target_name"].as_str(),
            Some("target-de-la-restauracion"),
            "el listado tiene que publicar el nombre del target, para que el panel lo anuncie sin \
             otra consulta: {cuerpo}"
        );
        assert_eq!(
            del_vinculo["target_type"].as_str(),
            Some("directory"),
            "el tipo del target viaja con el mismo texto del cable que publica `/api/v1/targets`: \
             {cuerpo}"
        );

        let huerfana = elemento_con_id(listado, &sin_vinculo, &cuerpo);

        for campo in CAMPOS_DEL_TARGET {
            assert!(
                huerfana[campo].is_null(),
                "una instantánea sin ejecución que la anote no tiene target y su campo `{campo}` \
                 tiene que llegar a `null`: {cuerpo}"
            );
        }
    }

    /// 1.6 — Cada instantánea publica `job_name`: el nombre de su tarea, o `null`.
    ///
    /// Cubre `api-resources`, escenario «Cada instantánea declara el nombre de su tarea o null». Se
    /// siembran **tres** instantáneas del mismo repositorio para que los dos desenlaces del nombre se
    /// distingan en la misma respuesta:
    ///
    /// * dos cuya tarea tiene nombre —obligatorio desde la `0007`—, y cada una publica el **suyo**;
    /// * y una **sin vínculo** con ninguna ejecución, que publica `null`.
    ///
    /// El otro `null` que el contrato declara —el de una tarea ausente del mapa— no se puede montar
    /// aquí: una instantánea sin vínculo no aporta `job_id` con el que consultar, y una tarea que
    /// exista trae nombre siempre. La huérfana cubre, por tanto, el único `null` alcanzable en este
    /// listado.
    ///
    /// El nombre se **cambia** con SQL directo para medir que el listado sigue al dato: la tarea se
    /// siembra por la capa de acceso —la única forma de tener la cadena `instantánea → ejecución →
    /// tarea`— y lo que esta prueba mide es el **listado**, no el alta de tareas, que tiene las suyas.
    ///
    /// Y se afirma que la `label` **no** cambia: SHALL seguir siendo la que la instantánea trae del
    /// repositorio, para que el nombre nuevo no acabe sustituyéndola por la puerta de atrás.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_listado_publica_el_nombre_de_la_tarea_de_cada_instantanea_o_null() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();

        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );

        let repositorio = alta_de_repositorio(&router, "explorador-1-6", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let primera = siembra.identificadores[0].clone();
        let segunda = siembra.identificadores[1].clone();
        // La tercera se queda **sin** vínculo: es la copia que el histórico no anotó.
        let huerfana = siembra.identificadores[2].clone();

        sembrar_cadena(
            &estado,
            repositorio_id,
            &primera,
            TipoDeTarget::Directory,
            Some("/srv/explorador-1-6-primera".to_owned()),
        )
        .await;

        sembrar_cadena(
            &estado,
            repositorio_id,
            &segunda,
            TipoDeTarget::Directory,
            Some("/srv/explorador-1-6-segunda".to_owned()),
        )
        .await;

        // El nombre de la tarea de la instantánea dada, leído del vínculo `instantánea → ejecución →
        // tarea`. Se lee aquí para poder afirmar que el listado publica **ese** nombre y no otro.
        async fn nombre_de_la_tarea(pool: &sqlx::SqlitePool, instantanea: &str) -> String {
            sqlx::query_scalar(
                "SELECT name FROM backup_jobs WHERE id = \
                 (SELECT job_id FROM job_executions WHERE snapshot_id = ?1)",
            )
            .bind(instantanea.to_owned())
            .fetch_one(pool)
            .await
            .expect("la tarea de la instantánea tiene que estar")
        }

        let nombre_primera = nombre_de_la_tarea(&estado.db.pool, &primera).await;
        let nombre_segunda = nombre_de_la_tarea(&estado.db.pool, &segunda).await;

        // La primera lectura: las tres traen la **clave** `job_name`; las dos con vínculo publican
        // el nombre de su tarea, y solo la huérfana —sin `job_id` con el que consultar— publica
        // `null`.
        let respuesta = get(
            router.clone(),
            &format!("/api/v1/snapshots?repository_id={repositorio}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200, y contestó {status} con {cuerpo}"
        );

        let listado = cuerpo["data"]["snapshots"]
            .as_array()
            .unwrap_or_else(|| panic!("el listado tiene que traer sus elementos: {cuerpo}"));

        for instantanea in [&primera, &segunda, &huerfana] {
            let elemento = elemento_con_id(listado, instantanea, &cuerpo);

            assert!(
                elemento.get("job_name").is_some(),
                "la instantánea `{instantanea}` tiene que traer la **clave** `job_name`: {cuerpo}"
            );
        }

        let elemento = elemento_con_id(listado, &primera, &cuerpo);

        assert_eq!(
            elemento["job_name"], nombre_primera,
            "la instantánea de la primera tarea tiene que publicar el nombre de **su** tarea: {cuerpo}"
        );

        let elemento = elemento_con_id(listado, &segunda, &cuerpo);

        assert_eq!(
            elemento["job_name"], nombre_segunda,
            "cada instantánea publica el nombre de su propia tarea, no el de otra: {cuerpo}"
        );

        let elemento = elemento_con_id(listado, &huerfana, &cuerpo);

        assert!(
            elemento["job_name"].is_null(),
            "una instantánea sin vínculo con ninguna tarea publica `job_name` a `null`: {cuerpo}"
        );

        // Y el nombre sigue al dato: se renombra la tarea de la primera instantánea —y **solo** la
        // suya—, para medir que el listado publica el nuevo sin contagiar a la segunda.
        sqlx::query(
            "UPDATE backup_jobs SET name = ?1 WHERE id = \
             (SELECT job_id FROM job_executions WHERE snapshot_id = ?2)",
        )
        .bind("copia nocturna")
        .bind(&primera)
        .execute(&estado.db.pool)
        .await
        .expect("cambiar el nombre de la tarea es preparación de la prueba y no puede fallar");

        let respuesta = get(
            router,
            &format!("/api/v1/snapshots?repository_id={repositorio}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200, y contestó {status} con {cuerpo}"
        );

        let listado = cuerpo["data"]["snapshots"]
            .as_array()
            .unwrap_or_else(|| panic!("el listado tiene que traer sus elementos: {cuerpo}"));

        let elemento = elemento_con_id(listado, &primera, &cuerpo);

        assert_eq!(
            elemento["job_name"], "copia nocturna",
            "la instantánea de la tarea renombrada tiene que publicar el nombre nuevo: {cuerpo}"
        );

        // La segunda sigue publicando el suyo, y la huérfana, `null`: el renombrado no se contagia.
        let elemento = elemento_con_id(listado, &segunda, &cuerpo);

        assert_eq!(
            elemento["job_name"], nombre_segunda,
            "el nombre de la segunda tarea no puede cambiar porque se renombre la primera: {cuerpo}"
        );

        let elemento = elemento_con_id(listado, &huerfana, &cuerpo);

        assert!(
            elemento["job_name"].is_null(),
            "una instantánea sin vínculo con ninguna tarea publica `job_name` a `null`: {cuerpo}"
        );

        // Y la `label` de las tres sigue siendo la que la instantánea trae del repositorio: el
        // nombre nuevo vive en su propio campo y **no** sustituye a la etiqueta.
        for instantanea in [&primera, &segunda, &huerfana] {
            let elemento = elemento_con_id(listado, instantanea, &cuerpo);

            assert_eq!(
                elemento["label"], ETIQUETA_SEMBRADA,
                "la `label` de la instantánea `{instantanea}` no puede cambiar por este change: \
                 {cuerpo}"
            );
        }
    }

    /// 1.29e — El listado va acotado a la ventana e indica que hay más.
    ///
    /// Cubre `repository`, escenario «El listado va acotado». La ventana no es una comodidad del panel:
    /// listar es la lectura más cara del change —abrir el repositorio y recorrer su índice—, y el
    /// formato del repositorio lo hace además inevitable, porque el identificador de una instantánea es
    /// el nombre de un fichero y no lleva fecha, así que acotar antes de resolver es la única forma de
    /// no recorrerlo entero. Y quien pregunta tiene que poder saber que hay más sin volver a preguntar:
    /// la ventana llena y el silencio serían indistinguibles de «esto es todo».
    ///
    /// Se siembran **una más** que la ventana, y no muchas más, porque con una sola de sobra ya se
    /// distingue «acotado» de «completo», y cada instantánea de más es un fichero más que el índice del
    /// repositorio tiene que escribir y leer en cada corrida del CI.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_listado_esta_acotado_a_la_ventana_e_indica_que_hay_mas() {
        let router = router_desarrollo().await;

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();

        // Una fecha distinta por instantánea, generadas y no escritas a mano: dos instantáneas con la
        // misma fecha son indistinguibles en el orden del listado, y el número de fechas es lo que
        // decide cuántas hay.
        let fechas: Vec<String> = (0..=LA_VENTANA)
            .map(|indice| format!("2026-01-{:02}T{:02}:00:00Z", 1 + indice / 24, indice % 24))
            .collect();
        let referencias: Vec<&str> = fechas.iter().map(String::as_str).collect();

        sembrar_snapshots(destino.ruta(), PASSPHRASE_DE_PRUEBA, &referencias);

        let repositorio = alta_de_repositorio(&router, "explorador-1-29e", &ruta).await;

        let respuesta = get(
            router,
            &format!("/api/v1/snapshots?repository_id={repositorio}"),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "el listado tiene que contestar 200 aunque tenga más instantáneas que la ventana; contestó \
         {status} con {cuerpo}"
        );

        let listado = cuerpo["data"]["snapshots"]
            .as_array()
            .unwrap_or_else(|| panic!("el listado tiene que traer sus elementos: {cuerpo}"));

        assert!(
            listado.len() <= LA_VENTANA,
            "el listado devolvió {} elementos y no puede superar la ventana de {LA_VENTANA}: {cuerpo}",
            listado.len()
        );

        assert_eq!(
            listado.len(),
            LA_VENTANA,
            "el repositorio tiene {} instantáneas y la ventana de {LA_VENTANA} tiene que llenarse: \
         {cuerpo}",
            fechas.len()
        );

        assert_eq!(
            cuerpo["data"]["truncated"], true,
            "con más instantáneas que la ventana, la respuesta tiene que decir que hay más: {cuerpo}"
        );
    }

    /// Cuánto se espera a que el fichero restaurado aparezca bajo el destino.
    ///
    /// La restauración es una operación **en curso** cuando la API contesta: el endpoint devuelve
    /// la ejecución, no su resultado. Sin plazo, una restauración que no escribiera nada dejaría
    /// la prueba esperando para siempre —y un cuelgue no dice qué faltaba—; con él, el desenlace
    /// se lee en el mensaje del fallo.
    const PLAZO_DE_LA_RESTAURACION: Duration = Duration::from_secs(30);

    /// Monta el estado **con su pool y su clave**, para poder sembrar la cadena antes de pedir.
    ///
    /// Las pruebas del listado no lo necesitan —su cadena se siembra por la superficie del §5—,
    /// pero la de la restauración sí: el vínculo `instantánea -> ejecución -> tarea -> target`
    /// solo existe en la base, y **no hay ruta** que dé de alta una ejecución —la crea una copia
    /// que corre—, así que hay que sembrarla por la capa de acceso. Es el mismo patrón que
    /// `executions.rs::estado_con_base`.
    ///
    /// La guarda del directorio viaja en el retorno y tiene que seguir **viva** hasta el final de
    /// la prueba: al soltarse se borra el directorio y la base desaparece bajo los pies del router.
    async fn estado_con_base() -> (AppState, DirectorioDePrueba) {
        let (directorio, pool, clave) = base_con_clave().await;

        let estado = estado_de_prueba(
            config_con_dsn(&directorio.dsn()),
            EstadoDeBase::nuevo(pool, Arc::clone(&clave)),
        );

        (estado, directorio)
    }

    /// Siembra un target, una tarea que lo apunta y una ejecución con la instantánea dada.
    ///
    /// Se siembra por la **capa de acceso** porque es la única forma de tener una ejecución: una
    /// copia la crea cuando corre, y lo que estas pruebas necesitan es el **vínculo** con la
    /// instantánea, no lanzar una copia y esperar a que acabe.
    ///
    /// Devuelve el identificador del target y el de la ejecución, que son dos de los datos que la
    /// respuesta de la restauración tiene que publicar.
    async fn sembrar_cadena(
        estado: &AppState,
        repositorio: Uuid,
        instantanea: &str,
        tipo: TipoDeTarget,
        host_path: Option<String>,
    ) -> (Uuid, Uuid) {
        sembrar_cadena_con_target(
            estado,
            repositorio,
            instantanea,
            AltaDeTarget {
                name: "target-de-la-restauracion".to_owned(),
                target_type: tipo,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path,
                db_path: None,
            },
        )
        .await
    }

    /// Lo mismo que [`sembrar_cadena`], pero con la fila del target **entera** a cargo de quien
    /// llama.
    ///
    /// Existe porque un target de base no se siembra con los cuatro campos del de directorio: su
    /// restauración necesita el contenedor, el usuario y la base, y el comando inverso los exige.
    /// Se pasa un [`AltaDeTarget`] y no nueve parámetros sueltos para que el llamante declare
    /// exactamente lo que la tabla guarda, sin que este ayudante tenga que tener una opinión sobre
    /// cuáles son los campos de cada tipo.
    async fn sembrar_cadena_con_target(
        estado: &AppState,
        repositorio: Uuid,
        instantanea: &str,
        target: AltaDeTarget,
    ) -> (Uuid, Uuid) {
        let target = estado
            .db
            .targets
            .alta(target)
            .await
            .expect("el target de la restauración se da de alta");

        let tarea = estado
            .db
            .tareas
            .alta(AltaDeTarea {
                target_id: target.id,
                repository_id: repositorio,
                cron_expression: "0 3 * * *".to_owned(),
                keep_daily: 7,
                keep_weekly: 4,
                keep_monthly: 6,
                enabled: true,
                name: nombre_de_tarea_de_prueba(),
            })
            .await
            .expect("la tarea de la restauración se da de alta");

        let ejecucion = estado
            .db
            .ejecuciones
            .alta(AltaDeEjecucion {
                job_id: tarea.id,
                status: Estado::Success,
                snapshot_id: Some(instantanea.to_owned()),
                bytes_processed: 1024,
                bytes_uploaded: 512,
                error_message: None,
                started_at: OffsetDateTime::now_utc(),
                finished_at: Some(OffsetDateTime::now_utc()),
            })
            .await
            .expect("la ejecución de la restauración se da de alta");

        (target.id, ejecucion.id)
    }

    /// Comprueba que por el hub **no** ha salido ningún aviso de restauración.
    ///
    /// Se vacía el receptor entero y se mira si había alguno: el receptor se toma **antes** de la
    /// petición que se mide, así que un aviso publicado por ella está dentro. Se lee con las mismas
    /// reglas que la prueba del stream —los avisos de una copia se saltan, que aquí no hay ninguna
    /// corriendo, y un receptor que se hubiera perdido eventos no corta la lectura—.
    fn no_hay_restauraciones_pendientes(
        receptor: &mut tokio::sync::broadcast::Receiver<EventoDelMotor>,
    ) -> bool {
        loop {
            match receptor.try_recv() {
                Ok(EventoDelMotor::Restauracion(_)) => return false,
                Ok(EventoDelMotor::Ejecucion(_)) => continue,
                Err(
                    tokio::sync::broadcast::error::TryRecvError::Empty
                    | tokio::sync::broadcast::error::TryRecvError::Closed,
                ) => return true,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            }
        }
    }

    /// Cuántas filas tiene `restore_executions`, con **SQL directo**.
    ///
    /// Se cuenta contra la tabla y no con la capa de acceso: lo que estas pruebas afirman cuando
    /// dicen «no se creó nada» es qué quedó **escrito**, y un recuento que pasara por la misma capa
    /// que se está probando podría contar otra cosa. Es el mismo patrón que el recuento del módulo
    /// de la entidad.
    async fn restauraciones_persistidas(estado: &AppState) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM restore_executions")
            .fetch_one(&estado.db.pool)
            .await
            .expect("el recuento de `restore_executions` debe poder leerse")
    }

    /// Busca, recursivamente bajo `raiz`, un fichero cuyo contenido sea **exactamente** `contenido`.
    ///
    /// Se busca por contenido y **recursivamente** y no por una ruta construida a mano: rustic
    /// conserva dentro del destino la ruta archivada —la absoluta de la fuente sin su barra
    /// inicial—, así que adivinar dónde acaba el fichero sería medir la suposición de la prueba y
    /// no lo que el motor restauró.
    fn recoge_contenido(raiz: &Path, contenido: &[u8]) -> Option<PathBuf> {
        for entrada in std::fs::read_dir(raiz).ok()?.flatten() {
            let ruta = entrada.path();

            if ruta.is_dir() {
                if let Some(encontrado) = recoge_contenido(&ruta, contenido) {
                    return Some(encontrado);
                }
            } else if std::fs::read(&ruta).is_ok_and(|leido| leido == contenido) {
                return Some(ruta);
            }
        }

        None
    }

    /// Espera, de forma **acotada**, a que el fichero restaurado aparezca bajo `raiz`.
    ///
    /// La restauración corre por detrás de la respuesta, así que el fichero no está en el
    /// instante en que el endpoint contesta. Se sondea el sistema de ficheros —lo que se mide es
    /// que los datos **vuelven**, no la fila que los anuncia— hasta agotar el plazo.
    async fn espera_el_contenido(
        raiz: &Path,
        contenido: &[u8],
        plazo: Duration,
    ) -> Option<PathBuf> {
        let limite = Instant::now() + plazo;

        loop {
            if let Some(encontrado) = recoge_contenido(raiz, contenido) {
                return Some(encontrado);
            }

            if Instant::now() >= limite {
                return None;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// 1.39 — Restaurar una instantánea de un target de directorio devuelve su ejecución y su
    /// target, y **el fichero vuelve a estar**.
    ///
    /// Cubre `api-resources`, escenario «Restaurar un snapshot de un target de directorio».
    ///
    /// La respuesta no basta como medida: un `200` con la ejecución y el target correctos lo
    /// cumple un manejador que resuelva el mapeo y no restaure nada. El criterio de aceptación es
    /// el otro —el fichero sembrado aparece **en la raíz** del `host_path` del target—, y por eso el
    /// destino es un directorio **vacío**: con uno que ya tuviera la fuente no se podría distinguir
    /// «se restauró» de «ya estaba».
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restaurar_una_instantanea_de_un_target_de_directorio_devuelve_su_ejecucion_y_su_target()
     {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        // El destino del repositorio, con una instantánea **de verdad** escrita por la
        // biblioteca: la restauración lee de ahí, así que una instantánea inventada no
        // restauraría nada.
        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-39", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let host = directorio_de_prueba();
        let instantanea = siembra.identificadores[0].clone();

        let (target, _ejecucion) = sembrar_cadena(
            &estado,
            repositorio_id,
            &instantanea,
            TipoDeTarget::Directory,
            Some(host.ruta().to_string_lossy().into_owned()),
        )
        .await;

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            &serde_json::json!({ "repository_id": repositorio }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "restaurar una instantánea de un target de directorio tiene que contestar 200, y \
             contestó {status} con {cuerpo}"
        );

        assert!(
            cuerpo["data"]["execution_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
            "la respuesta tiene que traer el identificador de la ejecución de restauración: {cuerpo}"
        );

        assert_eq!(
            cuerpo["data"]["snapshot_id"].as_str(),
            Some(instantanea.as_str()),
            "la respuesta tiene que identificar la instantánea que se restauró: {cuerpo}"
        );

        let target_publicado = target.to_string();

        assert_eq!(
            cuerpo["data"]["target_id"].as_str(),
            Some(target_publicado.as_str()),
            "la respuesta tiene que identificar el target sobre el que se restauró: {cuerpo}"
        );

        let encontrado =
            espera_el_contenido(host.ruta(), SEMILLA_CONTENIDO, PLAZO_DE_LA_RESTAURACION).await;

        // Y **en la raíz** del `host_path`, no anidado bajo la ruta con la que se montó la fuente
        // —el directorio temporal de la siembra—: la restauración descuenta esa ruta, que es el
        // contrato de la copia leído al revés. Buscar el contenido sólo por su valor lo encontraría
        // también anidado, así que lo que se afirma es dónde quedó.
        assert_eq!(
            encontrado.as_deref(),
            Some(host.ruta().join(SEMILLA).as_path()),
            "el fichero sembrado no reapareció en la raíz de `{}` en {PLAZO_DE_LA_RESTAURACION:?}: \
             una restauración que contesta 200 y no devuelve los datos donde estaban no ha \
             restaurado nada (apareció en {encontrado:?})",
            host.ruta().display()
        );
    }

    /// 1.40 — Una instantánea que no casa con ninguna ejecución contesta `409`.
    ///
    /// Cubre `api-resources`, escenario «Snapshot sin target». El `409` y no un `404`: la
    /// instantánea **existe** en el repositorio —se sembró con la biblioteca—, lo que falta es el
    /// vínculo con un target, y eso no es un recurso ausente sino un conflicto con el estado del
    /// sistema. Se afirma antes el `assert_ne!` del `404` para que la prueba diga lo que mide y no
    /// pase por el azar de una ruta sin montar.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_instantanea_sin_ejecucion_contesta_409() {
        let router = router_desarrollo().await;

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-40", &ruta).await;
        let instantanea = siembra.identificadores[0].clone();

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            &serde_json::json!({ "repository_id": repositorio }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "la instantánea existe en el repositorio, así que lo que falla no es un recurso \
             ausente: contestó {status} con {cuerpo}"
        );

        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "una instantánea sin ejecución ni target no se puede restaurar y la API tiene que \
             contestar 409; contestó {status} con {cuerpo}"
        );

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|motivo| !motivo.is_empty()),
            "el 409 tiene que traer un motivo que explique que falta el vínculo: {cuerpo}"
        );
    }

    /// 1.41 — Un target de base **sin** la confirmación contesta `428`, y no lanza nada.
    ///
    /// Cubre `api-resources`, escenario «Restaurar un target de base sin confirmación». El `428` y
    /// no un `422`: el vínculo existe, la petición está bien formada y lo que falta es una
    /// **condición previa** —que quien pide confirme que se pueden pisar los datos vivos—, que es
    /// exactamente lo que un `Precondition Required` significa. El `422` queda para lo que no se
    /// puede procesar por su forma —un `directory` sin `host_path`—, y el panel tiene que poder
    /// distinguir «confirma esto» de «ese target está mal».
    ///
    /// La instantánea se siembra **de verdad** —la escribe la biblioteca— para que la puerta se
    /// decida en su sitio del orden: la comprobación del almacenamiento va antes, así que con una
    /// copia ausente el desenlace sería el `404` de la prueba siguiente y no este `428`.
    ///
    /// Y se afirma que **no se lanzó nada** por partida doble: `restore_executions` sigue vacía y
    /// por el hub no ha salido ningún `restore_progress`. Un `428` que dejara la fila creada sería
    /// un trabajo que nadie pidió, y el panel lo pintaría como una restauración en curso.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_target_de_base_sin_confirmacion_contesta_428_sin_lanzar_nada() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-41", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let instantanea = siembra.identificadores[0].clone();

        sembrar_cadena_con_target(
            &estado,
            repositorio_id,
            &instantanea,
            AltaDeTarget {
                name: "target-de-base-sin-confirmacion".to_owned(),
                target_type: TipoDeTarget::Postgresql,
                container_name: Some("contenedor-de-la-base-de-la-prueba".to_owned()),
                db_user: Some("kesto".to_owned()),
                db_password: None,
                db_name: None,
                host_path: None,
                db_path: None,
            },
        )
        .await;

        let mut receptor = estado
            .motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            &serde_json::json!({ "repository_id": repositorio }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "la ruta de la restauración tiene que estar montada y la instantánea está: contestó \
             {status} con {cuerpo}"
        );

        assert_ne!(
            status,
            StatusCode::CONFLICT,
            "el vínculo con el target existe, así que no es un conflicto con el estado del \
             sistema: contestó {status} con {cuerpo}"
        );

        assert_ne!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "lo que falta es la confirmación, no un campo del target: contestó {status} con {cuerpo}"
        );

        assert_eq!(
            status,
            StatusCode::PRECONDITION_REQUIRED,
            "un target de base sin `sobrescribir` no se puede restaurar y la API tiene que contestar \
             428 —y no lanzar nada—: contestó {status} con {cuerpo}"
        );

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|motivo| !motivo.is_empty()),
            "el 428 tiene que explicar que hace falta confirmar: {cuerpo}"
        );

        let restauraciones = restauraciones_persistidas(&estado).await;

        assert_eq!(
            restauraciones, 0,
            "un 428 no puede dejar la fila de `restore_executions` creada: el trabajo nunca se \
             lanzó y la fila quedaría en `pending` para siempre"
        );

        assert!(
            no_hay_restauraciones_pendientes(&mut receptor),
            "un 428 no puede publicar ningún `restore_progress`: el panel daría por lanzada una \
             restauración que no existe"
        );
    }

    /// 1.43 — Una instantánea que no está en el repositorio contesta `404`, y no publica nada.
    ///
    /// Cubre `api-resources`, escenario «Snapshot que no está en el repositorio», y es el hallazgo
    /// que la auditoría marcó como bloqueante: el manejador resolvía la fila y el vínculo sin abrir
    /// nunca el almacenamiento, así que una instantánea ausente —una copia purgada, un repositorio
    /// recreado— daba un `200` que prometía un trabajo imposible y el fallo solo salía en el
    /// registro. El vínculo se siembra con un identificador **inventado** para que el `409` del
    /// vínculo no tape el caso: aquí hay fila, hay vínculo y hay target de directorio, y lo que
    /// falta es la copia.
    ///
    /// El escenario dice tres cosas, y se afirman las tres: el `404`, que **no** se publica ningún
    /// `restore_progress` —el receptor del hub se toma antes de pedir y se mira después— y que no se
    /// lanza ninguna restauración, que es lo mismo dicho de otra forma: el manejador contesta antes
    /// de generar el `execution_id`, así que el `data` de la respuesta no existe y no hay trabajo
    /// detrás.
    ///
    /// Y el mensaje se compara con el otro `404` —el del repositorio ausente— para dejar dicho que
    /// son distinguibles: el operador tiene que poder saber si falta el repositorio o la copia.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_instantanea_que_no_esta_en_el_repositorio_contesta_404() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        // El repositorio tiene una instantánea de verdad; la que pide la restauración es otra.
        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-43", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        // Bien formada —64 caracteres hexadecimales— y ausente: así el `404` lo decide el
        // repositorio y no el rechazo de un identificador mal formado, que es otro caso.
        let inventada = "f".repeat(64);

        let host = directorio_de_prueba();
        sembrar_cadena(
            &estado,
            repositorio_id,
            &inventada,
            TipoDeTarget::Directory,
            Some(host.ruta().to_string_lossy().into_owned()),
        )
        .await;

        let mut receptor = estado
            .motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{inventada}/restore"),
            &serde_json::json!({ "repository_id": repositorio }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_ne!(
            status,
            StatusCode::CONFLICT,
            "el vínculo con el target existe, así que lo que falta no es el vínculo: contestó \
             {status} con {cuerpo}"
        );

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "una instantánea que el repositorio no tiene no se puede restaurar y la API tiene que \
             contestar 404 —y no un 200 optimista—: contestó {status} con {cuerpo}"
        );

        let motivo = cuerpo["message"].as_str().unwrap_or_default();

        assert!(
            motivo.contains("instantánea"),
            "el 404 de la instantánea ausente tiene que decir que lo que falta es la copia: {cuerpo}"
        );

        assert_ne!(
            motivo, "no existe ningún repositorio con ese identificador",
            "el 404 de la copia ausente y el del repositorio ausente tienen que ser distinguibles: \
             el operador necesita saber cuál de los dos le falta"
        );

        assert!(
            no_hay_restauraciones_pendientes(&mut receptor),
            "un 404 no puede publicar ningún `restore_progress`: el panel daría por lanzada una \
             restauración que no existe"
        );
    }

    /// 1.44 — Dos restauraciones **simultáneas** sobre el mismo destino no caben: una entra.
    ///
    /// Es la guarda de la tercera corrección. Sin ella, dos `POST` sobre el mismo snapshot generan
    /// cada uno su `execution_id` y escriben a la vez en el **mismo `host_path`**, y lo que queda en
    /// el disco es la mezcla de las dos.
    ///
    /// El orden de los dos códigos es **indeterminado** —las dos peticiones se lanzan a la vez y
    /// cuál reserva primero lo decide el planificador—, así que lo que se afirma es el reparto y no
    /// quién llegó antes. Y no hay dos `409` posibles: la reserva la suelta quien la tomó, así que
    /// una de las dos tiene que haber entrado.
    ///
    /// Por qué esto no es una carrera que pueda salir de otra forma: las dos peticiones empiezan a
    /// la vez y cada una abre el repositorio **antes** de reservar —unos 6,4 s por apertura en la
    /// compilación de pruebas—, así que las dos reservas caen en el mismo tramo de milisegundos,
    /// mientras que la ganadora retiene el destino toda su restauración, que abre el repositorio
    /// **otra vez** con el mismo coste. La ventana en la que la perdedora encontraría el destino
    /// libre es, por tanto, varios segundos más corta que la que necesita para llegar hasta allí.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn dos_restauraciones_simultaneas_sobre_el_mismo_destino_no_caben() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );
        let instantanea = siembra.identificadores[0].clone();

        let repositorio = alta_de_repositorio(&router, "restaurador-1-44", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let host = directorio_de_prueba();
        sembrar_cadena(
            &estado,
            repositorio_id,
            &instantanea,
            TipoDeTarget::Directory,
            Some(host.ruta().to_string_lossy().into_owned()),
        )
        .await;

        let uri = format!("/api/v1/snapshots/{instantanea}/restore");
        let cuerpo = serde_json::json!({ "repository_id": repositorio });

        // Las dos a la vez: `join!` y no dos `await` seguidos, que sería medir el caso en el que una
        // ya ha terminado —y entonces las dos entran, con razón—.
        let (primera, segunda) = tokio::join!(
            post_json(router.clone(), &uri, &cuerpo),
            post_json(router.clone(), &uri, &cuerpo),
        );

        let mut codigos = [primera.status(), segunda.status()];
        codigos.sort_by_key(|codigo| codigo.as_u16());

        assert_eq!(
            codigos,
            [StatusCode::OK, StatusCode::CONFLICT],
            "dos restauraciones simultáneas sobre el mismo destino tienen que dejar una lanzada \
             (200) y la otra rechazada (409): contestaron {codigos:?}"
        );
    }

    /// 1.42 — Un `directory` sin ruta contesta `422`, y no lanza nada.
    ///
    /// Cubre `api-resources`, escenario «Target no restaurable», que es el `422` que **queda** en la
    /// API después de este change: la limitación por tipo se levanta para las bases, pero un target
    /// de directorio sin destino sigue siendo una petición que no se puede procesar. El motivo
    /// **nombra el campo** que falta, que es lo único que permite arreglarlo sin leer el código.
    ///
    /// La fila se siembra por la capa de acceso con `host_path: None` y **no** por el alta: el alta
    /// un `directory` sin ruta lo rechaza con un `400` —lo exige el borde HTTP y tiene su prueba—,
    /// así que la única forma de tener una fila así es una importación o un `UPDATE` a mano, que es
    /// justo el caso que el contrato manda cubrir.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_target_de_directorio_sin_ruta_contesta_422_nombrando_el_campo() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-42", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let instantanea = siembra.identificadores[0].clone();

        sembrar_cadena(
            &estado,
            repositorio_id,
            &instantanea,
            TipoDeTarget::Directory,
            None,
        )
        .await;

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            &serde_json::json!({ "repository_id": repositorio }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "un target de directorio sin ruta no se puede restaurar —no hay dónde escribir— y la API \
             tiene que contestar 422; contestó {status} con {cuerpo}"
        );

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|motivo| motivo.contains("host_path")),
            "el 422 tiene que nombrar el campo que falta: {cuerpo}"
        );

        assert_eq!(
            restauraciones_persistidas(&estado).await,
            0,
            "un 422 no puede dejar fila en `restore_executions`"
        );
    }

    /// 1.45 — Una instantánea ausente contesta `404` **aunque** le falte la confirmación.
    ///
    /// Es el **orden** de las comprobaciones, y el orden es contrato: la existencia real de la
    /// copia en el almacenamiento se decide **antes** de la puerta de la confirmación, así que un
    /// target de base sin `sobrescribir` cuya instantánea ya no está contesta `404` y no `428`. Al
    /// revés, el panel pediría confirmar una sobrescritura sobre una copia que no existe, y el
    /// operador confirmaría una operación imposible.
    ///
    /// El vínculo se siembra con un identificador **inventado** —bien formado y ausente— para que
    /// el `409` del vínculo no tape el caso, y el target es de base para que la puerta esté armada:
    /// si el orden estuviera invertido, esta petición contestaría `428`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn una_instantanea_ausente_de_un_target_de_base_sin_confirmacion_contesta_404() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-45", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        // Bien formada —64 caracteres hexadecimales— y ausente.
        let inventada = "e".repeat(64);

        sembrar_cadena_con_target(
            &estado,
            repositorio_id,
            &inventada,
            AltaDeTarget {
                name: "target-de-base-sin-instantanea".to_owned(),
                target_type: TipoDeTarget::Postgresql,
                container_name: Some("contenedor-de-la-base-de-la-prueba".to_owned()),
                db_user: Some("kesto".to_owned()),
                db_password: None,
                db_name: None,
                host_path: None,
                db_path: None,
            },
        )
        .await;

        let mut receptor = estado
            .motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{inventada}/restore"),
            &serde_json::json!({ "repository_id": repositorio }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_ne!(
            status,
            StatusCode::PRECONDITION_REQUIRED,
            "la existencia de la copia se comprueba antes de la puerta de la confirmación: la API \
             no puede pedir confirmar una sobrescritura sobre una copia que no está; contestó \
             {status} con {cuerpo}"
        );

        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "una instantánea que el repositorio no tiene contesta 404, con o sin confirmación: \
             contestó {status} con {cuerpo}"
        );

        assert_eq!(
            restauraciones_persistidas(&estado).await,
            0,
            "un 404 no puede dejar fila en `restore_executions`"
        );

        assert!(
            no_hay_restauraciones_pendientes(&mut receptor),
            "un 404 no puede publicar ningún `restore_progress`"
        );
    }

    /// 1.46 — Con la confirmación, el `200` devuelve una ejecución **persistida** en `pending`.
    ///
    /// Cubre `api-resources`, escenario «Restaurar un snapshot de un target de base con
    /// confirmación», y la mitad que la fase anterior dejaba sin cerrar: el `execution_id` ya no es
    /// efímero —no hay forma de seguir el desenlace de un identificador que nadie guardó—, así que
    /// lo que se afirma es que **es** una fila de `restore_executions`, con la instantánea y el
    /// target de la petición.
    ///
    /// El estado de la fila se mira con holgura a propósito: el ciclo de vida lo escribe el motor
    /// en segundo plano —pasa a `running` y la cierra con su desenlace—, así que entre el `200` y
    /// esta lectura puede haber avanzado. Lo que nace en `pending` lo fija la capa de acceso, que
    /// tiene su propia prueba; aquí se mide que la fila **existe** y que es la que la respuesta
    /// anunció. Y no se puede cerrar en `success`: el contenedor del target no existe, así que esta
    /// restauración está condenada —y eso es lo que hace la prueba barata: no toca ninguna base
    /// real.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restaurar_un_target_de_base_con_confirmacion_persiste_la_ejecucion() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-46", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let instantanea = siembra.identificadores[0].clone();

        let (target, _ejecucion) = sembrar_cadena_con_target(
            &estado,
            repositorio_id,
            &instantanea,
            AltaDeTarget {
                name: "target-de-base-con-confirmacion".to_owned(),
                target_type: TipoDeTarget::Mariadb,
                container_name: Some("contenedor-de-la-base-de-la-prueba".to_owned()),
                db_user: Some("kesto".to_owned()),
                db_password: None,
                db_name: Some("kesto".to_owned()),
                host_path: None,
                db_path: None,
            },
        )
        .await;

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            &serde_json::json!({ "repository_id": repositorio, "sobrescribir": true }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "con la confirmación, restaurar un target de base tiene que contestar 200; contestó \
             {status} con {cuerpo}"
        );

        let execution_id = cuerpo["data"]["execution_id"]
            .as_str()
            .unwrap_or_else(|| panic!("la respuesta tiene que traer la ejecución: {cuerpo}"));

        let execution_id = Uuid::parse_str(execution_id).unwrap_or_else(|error| {
            panic!("el `execution_id` tiene que ser un UUID en su forma canónica: {error}")
        });

        assert_eq!(
            cuerpo["data"]["snapshot_id"].as_str(),
            Some(instantanea.as_str()),
            "la respuesta tiene que identificar la instantánea que se restauró: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["target_id"].as_str(),
            Some(target.to_string().as_str()),
            "la respuesta tiene que identificar el target sobre el que se restauró: {cuerpo}"
        );
        assert_eq!(
            cuerpo["data"]["target_name"].as_str(),
            Some("target-de-base-con-confirmacion"),
            "la respuesta tiene que poder anunciar el target por su nombre: {cuerpo}"
        );

        let fila = estado
            .db
            .restauraciones
            .leer(&execution_id)
            .await
            .expect("la lectura de la restauración no puede fallar")
            .unwrap_or_else(|| {
                panic!(
                    "el `execution_id` de la respuesta tiene que ser la fila de \
                     `restore_executions` que se acaba de crear: {cuerpo}"
                )
            });

        assert_eq!(
            fila.snapshot_id, instantanea,
            "la fila persistida tiene que apuntar a la instantánea de la petición"
        );
        assert_eq!(
            fila.target_id,
            Some(target),
            "la fila persistida tiene que apuntar al target de la petición"
        );
        assert_ne!(
            fila.status,
            Estado::Success,
            "una restauración contra un contenedor que no existe no puede darse por buena: la fila \
             quedaría mintiendo en el histórico"
        );
    }

    /// 1.47 — Con la confirmación, la restauración de un directorio **poda** el destino.
    ///
    /// Cubre `bateria-e2e-de-copia-y-restauracion`, escenario «Sobrescribir un directorio
    /// existente»: el contenido previo del destino se sustituye por el del snapshot. La bandera del
    /// cuerpo es la que autoriza a retirar del destino lo que la instantánea no contiene —la misma
    /// confirmación que un target de base exige siempre—, y sin ella el directorio solo recibe. La
    /// otra mitad —sin confirmación no se toca— la mide una prueba de `rustic::repositorio`, donde
    /// la bandera se pasa a mano.
    ///
    /// Se siembra un fichero que **no está** en la copia y se espera a que desaparezca, además de
    /// esperar a que llegue el contenido de la copia: un destino vacío cumple «el obsoleto no está»
    /// sin haber restaurado nada, así que las dos mitades se afirman juntas. La poda ocurre **antes**
    /// de escribir los ficheros —la biblioteca la hace al preparar el plan—, así que la aparición del
    /// contenido es también la prueba de que el destino ya se podó.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restaurar_un_directorio_con_la_confirmacion_poda_el_destino() {
        let (estado, _base) = estado_con_base().await;
        let router = api_router(estado.clone());

        let destino = directorio_de_prueba();
        let ruta = destino.ruta().to_string_lossy().into_owned();
        let siembra = sembrar_snapshots(
            destino.ruta(),
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let repositorio = alta_de_repositorio(&router, "restaurador-1-47", &ruta).await;
        let repositorio_id = Uuid::parse_str(&repositorio)
            .expect("el alta del repositorio devuelve un identificador con forma de UUID");

        let host = directorio_de_prueba();
        let instantanea = siembra.identificadores[0].clone();

        // El contenido previo del destino: un fichero que la copia **no** tiene.
        let obsoleto = host.ruta().join("obsoleto.txt");
        std::fs::write(&obsoleto, b"contenido previo del destino\n")
            .expect("el fichero previo del destino se escribe");

        let (_target, _ejecucion) = sembrar_cadena(
            &estado,
            repositorio_id,
            &instantanea,
            TipoDeTarget::Directory,
            Some(host.ruta().to_string_lossy().into_owned()),
        )
        .await;

        let respuesta = post_json(
            router,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            &serde_json::json!({ "repository_id": repositorio, "sobrescribir": true }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::OK,
            "restaurar un directorio con la confirmación tiene que contestar 200; contestó {status} \
             con {cuerpo}"
        );

        let encontrado =
            espera_el_contenido(host.ruta(), SEMILLA_CONTENIDO, PLAZO_DE_LA_RESTAURACION).await;

        assert!(
            encontrado.is_some(),
            "el contenido de la copia no apareció bajo `{}` en {PLAZO_DE_LA_RESTAURACION:?}, así que \
             no hubo restauración que podar",
            host.ruta().display()
        );
        assert!(
            !obsoleto.exists(),
            "con la confirmación de sobrescritura el contenido previo del destino se sustituye por \
             el del snapshot, y `{}` sigue ahí",
            obsoleto.display()
        );
    }
}
