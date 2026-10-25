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
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::api::extract::Query;
use crate::api::response::ApiResponse;
use crate::api::{contenido, del_motor};
use crate::db::entities::Repositorio;
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

/// Las rutas del explorador: el listado de un repositorio, y solo él.
///
/// Se declara como las rutas de los otros recursos —un `router()` que
/// [`api_router`](crate::api::api_router) mezcla— para que dónde se cuelga lo decida quien lo llama
/// y este módulo no tenga que conocer la partición del router. Lo que sí se decide aquí es lo que
/// **no** se hace: la ruta no entra en la lista blanca de rutas públicas, porque es superficie de
/// negocio —publica qué hay en el almacenamiento— y le toca el *gate* de sesión como al resto.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/snapshots", get(listado))
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
/// # Errors
///
/// Un `400` de [`AppError::InvalidInput`] para el parámetro que falta o que no es un UUID, un `404`
/// de [`AppError::NotFound`] para la fila que no está, lo que devuelva la traducción del motor si el
/// repositorio no se puede listar, y un `500` de [`AppError::Database`] si la fila no se puede leer.
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

    // La vista se compone antes de envolverla porque formatear una fecha puede fallar: ese fallo es
    // un `500` con la causa al registro, y no una respuesta a la que le falte un elemento.
    let vista = ListadoDelCable::desde(&instantaneas, truncated)?;

    Ok(ApiResponse::new(
        StatusCode::OK,
        "instantáneas listadas",
        contenido(&vista)?,
    ))
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
    /// Vuelca las instantáneas del motor en la vista del cable.
    ///
    /// Puede fallar porque formatear una fecha es falible: ver [`SnapshotDelCable::desde`].
    fn desde(instantaneas: &[ResumenDeSnapshot], truncated: bool) -> Result<Self, AppError> {
        let snapshots = instantaneas
            .iter()
            .map(SnapshotDelCable::desde)
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
}

impl SnapshotDelCable {
    /// Convierte un resumen del repositorio en la vista del cable.
    ///
    /// La fecha se formatea aquí y **puede fallar** —el rango de fechas de `time` es más estrecho
    /// que el de la biblioteca—, y ese fallo es un `500` con la causa al registro y no una fecha
    /// inventada: es el único campo que dice cuándo se copió, y rellenarlo con algo plausible sería
    /// mentir justo en el dato que se publica para no tener que abrir la copia.
    ///
    /// El resto son copias tal cual: el resumen del repositorio ya viene con los tipos del panel
    /// —los dos contadores como `u64` y las rutas como cadenas—, así que aquí no se reinterpreta
    /// ninguno.
    fn desde(resumen: &ResumenDeSnapshot) -> Result<Self, AppError> {
        Ok(Self {
            id: resumen.id.clone(),
            time: fecha(resumen.fecha)?,
            paths: resumen.rutas.clone(),
            label: resumen.etiqueta.clone(),
            files: resumen.ficheros,
            bytes: resumen.bytes,
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
    use axum::Router;
    use axum::http::StatusCode;
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    use crate::test_support::{
        ETIQUETA_AJENA, ETIQUETA_SEMBRADA, PASSPHRASE_DE_PRUEBA, body_json, directorio_de_prueba,
        get, post_json, router_desarrollo, sembrar_snapshot_ajeno, sembrar_snapshots,
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

    /// Comprueba que un elemento del listado trae los seis datos del contrato, cada uno con su tipo.
    ///
    /// Los seis y no uno: el explorador es la única pantalla que enseña qué hay **dentro** del
    /// repositorio, y cada dato que falte deja una columna de la tabla sin poder pintarse. El tipo se
    /// comprueba además del valor porque un número serializado como cadena —o una fecha como número—
    /// pasa un `is_some` y rompe al panel.
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
}
