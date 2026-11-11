//! El stream de eventos: `GET /api/v1/events`.
//!
//! Es la **única** respuesta de la API que no viaja con la envoltura JSON, y este módulo es el
//! sitio donde esa excepción se declara: el cuerpo son líneas `event:` y `data:` —el formato que
//! el §5 publica—, y la excepción **no** alcanza a los errores, que siguen siendo respuestas HTTP
//! normales y siguen saliendo envueltas. En este repositorio la envoltura no es una capa del
//! router sino lo que produce un manejador —[`ApiResponse`](crate::api::response::ApiResponse) o
//! un [`AppError`](crate::error::AppError)—, así que la excepción se consigue sin tocar el router:
//! este manejador devuelve un [`Sse`] y por eso su cuerpo no pasa por la envoltura. Lo que sí
//! recibe, por estar montado en la mitad de negocio como cualquier otra ruta, es la capa de traza
//! y el *gate* de sesión, que es lo que lo protege: un `401` sin sesión lo pone el *gate* **antes**
//! de que este manejador corra, y por eso ese `401` es una respuesta normal con envoltura y **no**
//! deja ningún stream abierto.
//!
//! # El cable vive aquí, no en el hub
//!
//! El nombre del evento —[`NOMBRE_DEL_EVENTO`]—, los siete nombres de campo y el texto del
//! `status` se escriben en este fichero y en ningún otro. El motor entrega un
//! [`EventoDeEjecucion`] con tipos —el `status` es un
//! [`Estado`](crate::db::entities::Estado), no una cadena—, y traducirlo a las líneas del
//! protocolo es cosa de la capa HTTP: si los nombres vivieran en el hub, el hub tendría que saber
//! cómo se llama el cable, y el día que el motor cambie su tipo interno el panel se enteraría por
//! un campo que deja de llegar.
//!
//! # Sin latido, y por qué eso es una decisión y no un olvido
//!
//! El stream puede quedarse **sin enviar un solo byte** durante mucho tiempo —es lo que el §5
//! describe: un evento cuando una ejecución avanza, y nada entre dos avisos—, y aquí **no se manda
//! ningún latido deliberadamente** para no inventar una tercera clase de línea: el contrato dice
//! que el cuerpo está formado por líneas `event:` y `data:`, y una línea de comentario (`:`), que
//! es con lo que se mantiene viva una conexión ociosa, es una clase de línea que el contrato **no**
//! declara. Por eso no se llama a `KeepAlive`: si un despliegue concreto necesita que un
//! intermediario no corte una conexión ociosa, **eso es un cambio que se declara en la
//! especificación**, no algo que se añade en silencio.
//!
//! # El stream termina cuando el hub se cierra, y por eso el apagado no se cuelga
//!
//! El cierre ordenado del proceso cierra el hub —[`Hub::cerrar`](crate::engine::Hub::cerrar), desde
//! [`Motor::cerrar_el_hub`](crate::engine::Motor::cerrar_el_hub)— **antes** de esperar a que las
//! conexiones terminen, y esa mitad no es un adorno: el cierre del servidor no resuelve hasta que
//! todas las conexiones lo hacen, y el cuerpo de un `text/event-stream` no tiene final propio. Su
//! única salida es el `Closed` del canal —ver [`flujo`]—, así que sin cerrar el hub el proceso se
//! quedaría vivo hasta que el período de gracia del orquestador lo matara con `SIGKILL`, con el
//! hilo de trabajo a mitad de archivar: exactamente lo que la parada acotada del motor existe para
//! evitar.
//!
//! El límite que esto **no** cubre, y que hay que saber: un cliente que **deje de leer** —el socket
//! lleno, el portátil cerrado— puede retrasar el cierre de su conexión, porque el cuerpo no se puede
//! vaciar. Eso no está en manos del hub, que es lo que sí lo está: el porqué entero, en el rustdoc
//! de [`Hub::cerrar`](crate::engine::Hub::cerrar).
//!
//! # Un cliente que se queda atrás pierde eventos, y no frena a nadie
//!
//! Cada cliente es un receptor del [`broadcast`] del hub, con su capacidad acotada y su descarte:
//! un receptor que no lee pierde los eventos más viejos y recibe
//! [`Lagged`](tokio::sync::broadcast::error::RecvError::Lagged) diciendo cuántos se dejó. Aquí ese
//! desenlace **no** cierra el stream ni contesta nada: se registra y se sigue leyendo. El contrato
//! no promete reenvío —lo que promete es que el histórico sigue contando la verdad—, así que
//! cerrar la conexión de un cliente lento lo castigaría dos veces: una por perderse eventos y otra
//! por quedarse sin ninguno; y avisarle con un evento propio tampoco vale, porque el nombre del
//! evento está fijado por el §5 y un segundo nombre sería inventarse el cable.
//!
//! # Una sola ruta, y sin parámetros
//!
//! `GET /api/v1/events`. Es un stream **global** —uno que sirve a todos los clientes a la vez— y no
//! uno por ejecución, así que no hay nada que filtrar: lo fija el §5, y de ahí que la ruta no lleve
//! ningún parámetro de consulta.

use std::convert::Infallible;
use std::pin::Pin;

use axum::Router;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::routing::get;
use futures_util::stream::{self, Stream};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

use crate::engine::EventoDeEjecucion;
use crate::state::AppState;

/// El nombre del evento del §5: el único que este stream emite.
///
/// Es una constante, y una sola, porque el nombre es **contrato**: el panel se escribe contra él
/// —`event: execution_progress`— y un cambio de literal aquí es un cambio de protocolo, no un
/// detalle de implementación. Vive al lado del manejador que lo escribe y no en el hub, por lo
/// dicho arriba: el hub reparte un tipo, el cable lo pone la capa HTTP.
pub const NOMBRE_DEL_EVENTO: &str = "execution_progress";

/// El flujo de eventos de un cliente, ya en el formato del protocolo.
///
/// Es un `Box<dyn Stream>` y no un `impl Stream` porque el tipo que devuelve
/// [`futures_util::stream::unfold`] no se puede nombrar, y nombrar el flujo permite que la firma
/// del manejador diga lo que devuelve —un `Sse` de eventos— en vez de un `Response` opaco. La caja
/// se paga **una vez por cliente conectado**, no por evento: es el precio de que el tipo del
/// cuerpo se lea, y comparado con lo que dura una conexión de estas es ruido.
type FlujoDelStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

/// Las rutas del stream: una, `GET /api/v1/events`.
///
/// Se declara como las otras cuatro de recurso —un `router()` que `api_router` mezcla— para que el
/// día que haya una segunda ruta de eventos tenga su sitio, y para que este módulo no tenga que
/// conocer la partición del router: dónde se cuelga —la mitad de negocio— lo decide quien lo llama.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/events", get(stream))
}

/// `GET /api/v1/events` — el stream de eventos en vivo del §5.
///
/// Se suscribe al hub del motor **al atender la petición** y no antes: lo que abre la conexión es
/// este manejador corriendo, así que todo lo que el motor publique a partir de aquí llega a este
/// cliente, y lo que se publicó antes no —el hub no promete historia, y la historia es
/// `/api/v1/executions`—.
///
/// El `Sse` sale sin latido a propósito; el porqué está en la documentación del módulo.
///
/// # Un hub ya cerrado no abre un stream
///
/// El hub se cierra durante el apagado ordenado —[`Hub::cerrar`](crate::engine::Hub::cerrar)— para que los clientes conectados
/// terminen y el proceso pueda acabar; ver [`flujo`]. Una petición que ya estaba dentro cuando eso
/// pasa llamaría aquí **después** del cierre, y entonces no hay nada que servir: el cuerpo sale
/// vacío y termina en el acto, igual que le pasa al cliente que ya estaba conectado. El estado está
/// en el tipo —[`Hub::suscribir`](crate::engine::Hub::suscribir) devuelve `Option`— y no se disfraza con una suscripción a un canal
/// que nadie va a cerrar: eso dejaría una conexión abierta sin final esperando eventos que ya no
/// existen, que es justo el cuelgue que el cierre viene a evitar.
pub async fn stream(State(estado): State<AppState>) -> Sse<FlujoDelStream> {
    match estado.motor.hub().suscribir() {
        Some(receptor) => Sse::new(Box::pin(flujo(receptor))),
        None => {
            tracing::info!(
                "el stream se pide con el hub ya cerrado: se contesta un cuerpo vacío, que termina \
                 en el acto, en vez de dejar la conexión abierta esperando eventos que no van a \
                 llegar"
            );

            Sse::new(Box::pin(stream::empty::<Result<Event, Infallible>>()))
        }
    }
}

/// El flujo de eventos de **un** cliente, leído del hub hasta que el canal se cierre.
///
/// El bucle de dentro existe por los dos desenlaces que **no** son un evento y que tampoco son el
/// final del stream: un receptor que se quedó atrás —`Lagged`, que se registra y se salta— y un
/// aviso que no se pudo serializar, que se descarta por el mismo camino. Los dos se saltan en vez
/// de cerrar, porque un cliente conectado vale más que la perfección de un aviso.
///
/// `Closed` sí termina el flujo, y es la forma correcta de terminar: significa que el extremo de
/// envío del hub ya no existe, así que no va a llegar nada más y dejar la conexión abierta sería
/// dejar a un cliente esperando algo que ya no puede ocurrir. Y es también **cómo termina el apagado
/// ordenado**: el proceso cierra el hub —[`Hub::cerrar`](crate::engine::Hub::cerrar)— justo antes de esperar a que las
/// conexiones acaben, y este `Closed` es lo que hace que la de este cliente acabe. Sin él, el
/// cierre del servidor esperaría para siempre a un cuerpo que no tiene final.
fn flujo(
    receptor: broadcast::Receiver<EventoDeEjecucion>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold(receptor, |mut receptor| async move {
        loop {
            match receptor.recv().await {
                Ok(evento) => {
                    if let Some(evento) = linea(&evento) {
                        return Some((Ok(evento), receptor));
                    }
                }
                Err(RecvError::Lagged(eventos_perdidos)) => {
                    tracing::warn!(
                        eventos_perdidos,
                        "un cliente del stream se quedó atrás y perdió eventos: sigue leyendo, \
                         que el contrato no promete reenvío"
                    );
                }
                Err(RecvError::Closed) => return None,
            }
        }
    })
}

/// Traduce un aviso del motor a la línea de evento que viaja por el cable.
///
/// Devuelve `None` —y lo registra— si el `data` no se puede serializar. No debería ocurrir: los
/// siete campos son dos cadenas, tres números y un `Option<f64>`, y con eso `serde_json` no falla.
/// Pero un manejador no aborta, y aquí además no hay ninguna respuesta donde poner un `500`: el
/// stream ya está abierto, así que el desenlace honesto es perder ese aviso y seguir sirviendo los
/// siguientes, en vez de cerrar la conexión de todos los clientes por un evento que no se pudo
/// escribir.
fn linea(evento: &EventoDeEjecucion) -> Option<Event> {
    match serde_json::to_string(&VistaDelEvento::desde(evento)) {
        Ok(datos) => Some(Event::default().event(NOMBRE_DEL_EVENTO).data(datos)),
        Err(error) => {
            tracing::error!(
                %error,
                "un aviso de ejecución no se pudo serializar: se descarta y el stream sigue"
            );

            None
        }
    }
}

/// El `data` del evento: los siete campos del §5, con sus nombres literales.
///
/// Es un tipo propio de la API y **no** el [`EventoDeEjecucion`] del motor, y no por ceremonia: los
/// nombres de los campos son contrato —el panel los lee por su nombre—, y serializar el tipo del
/// motor los ataría a su estructura interna, que existe para otra cosa. Aquí los nombres están
/// escritos una sola vez, y son los del §5.
#[derive(Debug, Serialize)]
struct VistaDelEvento<'a> {
    /// La ejecución de la que habla el aviso, en su forma canónica con guiones.
    ///
    /// Como texto y no como `Uuid` —que `serde` serializaría igual— por coherencia con el resto del
    /// §5: el panel compara este identificador con los que recibe de `/api/v1/executions`, y una
    /// representación distinta en un solo sitio sería una comparación que falla.
    execution_id: String,
    /// La tarea a la que pertenece esa ejecución.
    job_id: String,
    /// El estado de la ejecución, en el texto que el `CHECK` del §3 admite.
    ///
    /// El motor manda un `Estado`, que es un tipo cerrado, y el texto lo pone
    /// [`Estado::texto`](crate::db::entities::Estado::texto) —la misma función que usa el listado
    /// del histórico y el filtro de la consulta—, así que el `status` de un evento y el de su fila
    /// no pueden discrepar: son el mismo `match`.
    status: &'static str,
    /// Los bytes que han salido de la extracción.
    bytes_processed: u64,
    /// Los bytes que el archivador ha ido leyendo.
    bytes_uploaded: u64,
    /// El porcentaje, o `null` cuando el total no se conoce: ver el contrato del §5.
    percentage: Option<f64>,
    /// Qué está haciendo la ejecución ahora mismo.
    current_action: &'a str,
}

impl<'a> VistaDelEvento<'a> {
    /// Convierte un aviso del motor en la vista que viaja por el cable.
    fn desde(evento: &'a EventoDeEjecucion) -> Self {
        Self {
            execution_id: evento.execution_id.hyphenated().to_string(),
            job_id: evento.job_id.hyphenated().to_string(),
            status: evento.status.texto(),
            bytes_processed: evento.bytes_processed,
            bytes_uploaded: evento.bytes_uploaded,
            percentage: evento.percentage,
            current_action: &evento.current_action,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Las pruebas del stream de eventos: `GET /api/v1/events`.
    //!
    //! Son las que fijan las tres cosas que la excepción del delta `api-shell` significa: que el
    //! stream contesta `200` con `text/event-stream` y **sin** ningún parámetro, que su cuerpo son
    //! líneas `event:` y `data:` y **no** un objeto envuelto, y que lo que publica es el payload
    //! del §5 —el nombre `execution_progress` y sus siete campos con sus nombres literales en
    //! inglés—, más el `401` de un error anterior a que el stream se abra.
    //!
    //! # El camino para provocar eventos de verdad, sin contenedores y sin red
    //!
    //! El stream solo habla cuando el motor publica, y el motor publica cuando una ejecución
    //! avanza. La forma barata de que avance una es un target de tipo **`directory`**: un
    //! directorio no pasa por Docker —Kesto lo lee con el `LocalSource` de la biblioteca desde su
    //! `host_path`—, así que un disparo manual sobre una tarea con un target de directorio y un
    //! repositorio **local** recorre toda la cadena —cola, hilo, archivado, avisos— sin un
    //! contenedor y sin salir de la máquina. Es el mismo camino que usan las pruebas del motor, y
    //! por eso este módulo no tiene ninguna prueba marcada `#[ignore]`.
    //!
    //! Todo se pide por el **router de producción** y por la superficie del §5 —las altas de target,
    //! repositorio y tarea, el disparo y el propio stream—: así lo que se mide es lo que el panel
    //! ve, y no una ruta montada a mano para la prueba.
    use std::path::Path;
    use std::time::{Duration, Instant};

    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::http::header;
    use uuid::Uuid;

    use crate::api::api_router;
    use crate::test_support::{
        DirectorioDePrueba, datos_del_evento, directorio_de_prueba, estado_desarrollo, get,
        header_text, post, post_json, router_oidc, siguiente_evento,
    };

    /// Lo que se espera a que llegue un evento del stream, en cada espera.
    ///
    /// Treinta segundos, y el número está **medido**: el primer aviso de una ejecución lo publica el
    /// encolado **antes** de contestar el `202`, así que para cuando la prueba se pone a leer ya está
    /// dentro del hub y lo normal es que llegue en microsegundos. Lo que este plazo acota es el otro
    /// desenlace —que el aviso no llegue—, y ahí lo que hay que evitar es un cuelgue: un stream sin
    /// eventos no se cierra nunca, así que sin plazo la prueba esperaría para siempre y no diría qué
    /// faltaba.
    const PLAZO_DEL_EVENTO: Duration = Duration::from_secs(30);

    /// Los siete campos del §5, con sus nombres literales.
    ///
    /// Se declaran como lista y no se escriben uno a uno en la aserción porque el requisito los fija
    /// **todos**, y una lista es lo único que hace que añadir un octavo o perder uno se lea en el
    /// mensaje del fallo. El campo que el evento no debe llevar de más se comprueba aparte, contándolos.
    const LOS_SIETE_CAMPOS_DEL_SRS: [&str; 7] = [
        "execution_id",
        "job_id",
        "status",
        "bytes_processed",
        "bytes_uploaded",
        "percentage",
        "current_action",
    ];

    /// El aparato del stream: el router de producción sobre una base sembrada **por la API**.
    ///
    /// Siembra un target de directorio, un repositorio local y una tarea, y nada más: lo que se mide
    /// aquí es el stream, no el motor, que ya tiene sus pruebas. Los dos directorios se retienen
    /// —la fuente que se copia y el destino del repositorio— porque la prueba termina antes de que la
    /// copia acabe: soltarlos borraría la ruta bajo los pies de un hilo que sigue trabajando, y el
    /// fallo se leería como un fallo del stream.
    struct AparatoDelStream {
        /// La fuente del target de directorio, que es lo que se copia.
        _fuente: DirectorioDePrueba,
        /// El destino local del repositorio, donde el motor archiva.
        _destino: DirectorioDePrueba,
        /// El router de producción, con el que se piden las cinco cosas.
        router: Router,
        /// La tarea que se dispara.
        tarea: Uuid,
    }

    impl AparatoDelStream {
        /// Siembra la cadena entera —target, repositorio y tarea— por la superficie del §5.
        async fn nuevo() -> Self {
            let router = api_router(estado_desarrollo().await);

            let fuente = directorio_de_prueba();
            std::fs::write(
                fuente.ruta().join("semilla.txt"),
                b"contenido de la fuente\n",
            )
            .expect("el fichero de la fuente se escribe");

            let destino = directorio_de_prueba();

            let target = alta_de_target(&router, fuente.ruta()).await;
            let repositorio = alta_de_repositorio(&router, destino.ruta()).await;
            let tarea = alta_de_tarea(&router, &target, &repositorio).await;

            Self {
                _fuente: fuente,
                _destino: destino,
                router,
                tarea,
            }
        }

        /// Dispara la tarea y devuelve el identificador de la ejecución que el `202` publica.
        ///
        /// Se afirma aquí, y no en cada prueba, que el disparo contesta `202`: el disparo tiene sus
        /// propias pruebas en `jobs` —el `404` de una tarea que no está, el `409` de una que ya corre y
        /// el `400` de un identificador mal formado—, y lo que estas pruebas necesitan de él es la
        /// ejecución que arranca, no volver a medirlo.
        async fn disparar(&self) -> String {
            let uri = format!("/api/v1/jobs/{}/run", self.tarea);
            let respuesta = post(self.router.clone(), &uri).await;

            let status = respuesta.status();
            let cuerpo = crate::test_support::body_json(respuesta).await;

            assert_eq!(
                status,
                StatusCode::ACCEPTED,
                "el disparo tiene que contestar 202 para que haya algo que seguir por el stream, y \
                 contestó {status} con {cuerpo}"
            );

            cuerpo["data"]["id"]
                .as_str()
                .expect("el disparo tiene que devolver el identificador de la ejecución")
                .to_owned()
        }
    }

    /// Da de alta un target de **directorio** sobre la ruta dada y devuelve su identificador.
    ///
    /// El tipo `directory` es lo que hace que la copia no pase por Docker: la ruta es la que Kesto ve
    /// dentro de su propio contenedor, y aquí es un directorio temporal de la prueba.
    async fn alta_de_target(router: &Router, ruta: &Path) -> String {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/targets",
            &serde_json::json!({
                "name": "fuente-del-stream",
                "target_type": "directory",
                "host_path": ruta.to_string_lossy(),
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = crate::test_support::body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el target de directorio de la siembra tiene que darse de alta para que el disparo tenga \
             algo que copiar, y contestó {status} con {cuerpo}"
        );

        cuerpo["data"]["id"]
            .as_str()
            .expect("el alta del target debe devolver su identificador")
            .to_owned()
    }

    /// Da de alta el repositorio sobre el destino **local** dado y devuelve su identificador.
    ///
    /// El destino es un directorio del sistema de ficheros y no un cubo de mentira: el alta **habla con
    /// el destino** —lo inicializa si no existe—, así que con una URL que no responde el alta fallaría
    /// y esta siembra no sembraría nada.
    async fn alta_de_repositorio(router: &Router, destino: &Path) -> String {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/repositories",
            &serde_json::json!({
                "name": "repositorio-del-stream",
                "endpoint": destino.to_string_lossy(),
                "bucket": "cubo-del-stream",
                "region": "region-de-prueba",
                "access_key_id": "clave-de-acceso-de-prueba",
                "secret_access_key": "AKIAIOSFODNN7EXAMPLE-secreto-del-stream",
                "passphrase": crate::test_support::PASSPHRASE_DE_PRUEBA,
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = crate::test_support::body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "el repositorio de la siembra tiene que darse de alta para que el motor tenga dónde \
             archivar, y contestó {status} con {cuerpo}"
        );

        cuerpo["data"]["id"]
            .as_str()
            .expect("el alta del repositorio debe devolver su identificador")
            .to_owned()
    }

    /// Da de alta la tarea que apunta al target y al repositorio dados.
    async fn alta_de_tarea(router: &Router, target_id: &str, repository_id: &str) -> Uuid {
        let respuesta = post_json(
            router.clone(),
            "/api/v1/jobs",
            &serde_json::json!({
                "target_id": target_id,
                "repository_id": repository_id,
                "cron_expression": "0 3 * * *",
                "keep_daily": 7,
                "keep_weekly": 4,
                "keep_monthly": 6,
                "enabled": true,
            }),
        )
        .await;

        let status = respuesta.status();
        let cuerpo = crate::test_support::body_json(respuesta).await;

        assert_eq!(
            status,
            StatusCode::CREATED,
            "la tarea de la siembra tiene que darse de alta para poder dispararla, y contestó {status} \
             con {cuerpo}"
        );

        let id = cuerpo["data"]["id"]
            .as_str()
            .expect("el alta de la tarea debe devolver su identificador");

        Uuid::parse_str(id)
            .expect("el alta de la tarea devuelve un identificador con forma de UUID")
    }

    /// 1.24 — El stream contesta `200` con `text/event-stream`, y no exige ningún parámetro.
    ///
    /// Cubre `api-resources`, escenario «El stream contesta su tipo de contenido», y la mitad del
    /// requisito que dice que la ruta **no** lleva parámetros: es un stream **global** —uno que sirve a
    /// todos los clientes y no uno por ejecución—, así que no hay nada que filtrar y la petición se
    /// atiende sin nada en la consulta. Se pide tal cual, sin `?`, que es lo que comprueba esa mitad.
    ///
    /// No se lee el cuerpo: un `text/event-stream` no termina, y lo que esta prueba afirma es la
    /// cabecera y el código. Lo que viaja dentro lo miden las dos siguientes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_stream_contesta_su_tipo_de_contenido_y_no_exige_parametros() {
        let respuesta = get(router_desarrollo_del_stream().await, "/api/v1/events").await;

        let status = respuesta.status();
        let tipo = header_text(&respuesta, header::CONTENT_TYPE);

        assert_eq!(
            status,
            StatusCode::OK,
            "el stream tiene que atender una petición sin parámetros, y contestó {status} con el tipo \
             {tipo:?}"
        );

        assert!(
            tipo.as_deref()
                .is_some_and(|tipo| tipo.starts_with("text/event-stream")),
            "el tipo de contenido del stream tiene que ser `text/event-stream`, y llegó {tipo:?}"
        );
    }

    /// El router del stream en el modo de desarrollo, que es el que no exige sesión.
    ///
    /// Existe para no repetir la construcción en las tres pruebas que lo necesitan, y para decir una
    /// vez por qué es ese modo: en desarrollo no hay proveedor de identidad y no se monta capa, así que
    /// las rutas de negocio se piden sin cookie, que es lo que permite ejercitar el motor sin levantar
    /// un proveedor falso.
    async fn router_desarrollo_del_stream() -> Router {
        api_router(estado_desarrollo().await)
    }

    /// 1.24 y 1.26 — El evento del stream es el del §5, con sus siete campos, y **sin** la envoltura.
    ///
    /// Cubre el escenario «El evento llega con el payload del §5» del requisito del motor y el
    /// escenario «El stream no lleva la envoltura» del delta `api-shell`, que son la misma lectura: lo
    /// que el cuerpo del stream trae.
    ///
    /// Las tres afirmaciones y por qué ninguna sobra:
    ///
    /// - el **nombre** del evento, porque el §5 publica `execution_progress` y una API que emitiera
    ///   otro nombre sería una API que nadie puede consumir sin leer el código;
    /// - los **siete campos**, y **exactamente** siete: uno de menos deja al panel sin poder pintar la
    ///   barra, y uno de más es un juego de campos que el §5 no publica;
    /// - la **ausencia de la envoltura dentro del `data`**: `{status, message, data}` dentro de un
    ///   `data:` no es un `text/event-stream`, es un objeto JSON que el cliente tendría que desenvolver.
    ///
    /// La comprobación de la envoltura se hace por sus dos claves de cuerpo —`message` y `data`— y no
    /// por `status`, porque el `status` del evento **sí** existe: es suyo, y es una cadena con uno de
    /// los cuatro estados del §3, mientras el de la envoltura es un número.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_evento_del_stream_trae_los_siete_campos_del_srs_y_no_lleva_la_envoltura() {
        let aparato = AparatoDelStream::nuevo().await;

        let respuesta = get(aparato.router.clone(), "/api/v1/events").await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::OK,
            "el stream tiene que abrirse antes de que el motor publique nada, y contestó {status}"
        );

        let mut cuerpo = respuesta.into_body();

        let ejecucion = aparato.disparar().await;

        let evento = siguiente_evento(&mut cuerpo, PLAZO_DEL_EVENTO)
            .await
            .expect(
                "el stream no publicó ni un evento después de encolar una ejecución: o el aviso no \
                 llega al cliente o el cliente no estaba suscrito cuando el motor lo publicó",
            );

        let nombre = evento
            .lines()
            .find_map(|linea| linea.strip_prefix("event:"))
            .map(str::trim);

        assert_eq!(
            nombre,
            Some("execution_progress"),
            "el §5 publica un único nombre de evento, `execution_progress`, y llegó {evento:?}"
        );

        let datos = datos_del_evento(&evento);

        for campo in LOS_SIETE_CAMPOS_DEL_SRS {
            assert!(
                datos.get(campo).is_some(),
                "el `data` del evento no trae el campo `{campo}` del §5: {evento:?}"
            );
        }

        let cuantos = datos
            .as_object()
            .map(serde_json::Map::len)
            .unwrap_or_default();

        assert_eq!(
            cuantos,
            LOS_SIETE_CAMPOS_DEL_SRS.len(),
            "el `data` del evento tiene que traer exactamente los siete campos del §5, y trajo \
             {cuantos}: {evento:?}"
        );

        assert!(
            datos.get("message").is_none() && datos.get("data").is_none(),
            "el cuerpo del stream no puede llevar la envoltura `{{status, message, data}}` dentro del \
             `data`: un `text/event-stream` envuelto no es un `text/event-stream`, y llegó {evento:?}"
        );

        assert_eq!(
            datos["execution_id"], ejecucion,
            "el aviso tiene que hablar de la ejecución que se acaba de encolar: {evento:?}"
        );
    }

    /// 1.24 — Dos clientes conectados a la vez reciben **el mismo** evento.
    ///
    /// Cubre el escenario «Dos clientes ven el mismo evento», que es la mitad que distingue un stream
    /// **global** de un stream por ejecución: el panel abre una pestaña por operador, y ninguna puede
    /// ver algo que la otra no vea. Los dos clientes se conectan **antes** del disparo, que es lo que
    /// hace que el aviso del encolado esté en el búfer de los dos.
    ///
    /// Lo que se compara es el evento entero —su `data`— y no solo que los dos reciban algo: dos
    /// clientes que recibieran dos avisos distintos del mismo momento estarían viendo dos verdades
    /// distintas de la misma ejecución.
    ///
    /// Y además se comprueban dos cosas sobre el primer cliente, con el otro como testigo: que su
    /// primer evento hable de la ejecución que se acaba de encolar —su `execution_id`, que no depende
    /// de ningún orden— y que el **nacimiento** de esa ejecución —su `pending`— llegue a la conexión.
    /// Sin lo primero, un evento constante con los siete campos y cualquier valor pasaría la prueba
    /// —la igualdad entre los dos clientes la cumpliría igual—; sin lo segundo, la prueba diría que
    /// el motor anuncia *algo*, no que anuncia el arranque.
    ///
    /// El `pending` **no** se exige como el **primer** evento, y esa es la diferencia con lo que la
    /// prueba hacía antes: el motor publica el aviso del encolado **después** de entregar la ejecución
    /// a la cola, y a partir de ahí el trabajador es otro hilo que, en cuanto la recibe, la escribe
    /// como `running` y publica su aviso. Un cliente desprogramado en esa ventana ve `running` de
    /// primero, así que lo único afirmable es que el nacimiento **llegue**, con plazo: el porqué
    /// entero, en `espera_el_nacimiento`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dos_clientes_del_stream_ven_el_mismo_evento() {
        let aparato = AparatoDelStream::nuevo().await;

        let primer_cliente = get(aparato.router.clone(), "/api/v1/events").await;
        let segundo_cliente = get(aparato.router.clone(), "/api/v1/events").await;

        assert_eq!(
            (primer_cliente.status(), segundo_cliente.status()),
            (StatusCode::OK, StatusCode::OK),
            "los dos clientes tienen que poder abrir el stream antes del disparo"
        );

        let mut primero = primer_cliente.into_body();
        let mut segundo = segundo_cliente.into_body();

        let ejecucion = aparato.disparar().await;

        let de_uno = siguiente_evento(&mut primero, PLAZO_DEL_EVENTO)
            .await
            .expect("el primer cliente no recibió ningún evento");
        let de_otro = siguiente_evento(&mut segundo, PLAZO_DEL_EVENTO)
            .await
            .expect("el segundo cliente no recibió ningún evento");

        let datos_uno = datos_del_evento(&de_uno);
        let datos_otro = datos_del_evento(&de_otro);

        assert_eq!(
            datos_uno, datos_otro,
            "dos clientes conectados a la vez recibieron eventos distintos: el primero {de_uno:?} y el \
             segundo {de_otro:?}"
        );

        // El identificador sí se afirma sobre el primer evento, y ese aserto no compite con el
        // trabajador: el aviso de `running` es de **la misma** ejecución, así que dé igual cuál de
        // los dos llegue antes.
        assert_eq!(
            datos_uno["execution_id"], ejecucion,
            "el primer evento de los dos clientes no habla de la ejecución que se acaba de encolar: \
             {de_uno:?}"
        );

        assert!(
            espera_el_nacimiento(datos_uno, &mut primero, &ejecucion, PLAZO_DEL_EVENTO).await,
            "el stream no anunció el nacimiento de {ejecucion} en {PLAZO_DEL_EVENTO:?}: la ejecución \
             que se acaba de encolar no se vio nunca en `pending`, así que el panel no podría pintar \
             que ha nacido"
        );
    }

    /// Espera, de forma **acotada**, al evento de nacimiento de una ejecución, ya leído el primero.
    ///
    /// Nace de una reserva de la auditoría del change: el aserto del `status` exigía que `pending`
    /// fuera el **primer** evento del stream, y eso es un orden que el motor no garantiza. El aviso
    /// del encolado lo publica el motor **después** de entregar la ejecución a la cola —el orden que
    /// evita anunciar un nacimiento que luego acabe en `failed` por una cola llena, así que el arreglo
    /// no está aquí sino en el aserto—, y el trabajador es otro hilo: entre el `try_send` y su
    /// `publicar` hay un despertar, una lectura y una escritura de base, así que un cliente que se
    /// quede desprogramado en esa ventana ve `running` de primero. Lo que protegía el aserto viejo era
    /// un **margen de tiempo**, y «casi siempre» no es «siempre».
    ///
    /// Aquí se recorre el stream de ese cliente —el primero, el que ya se leyó— hasta dar con un
    /// evento que sea **de esa** ejecución y diga `pending`, o hasta agotar el plazo. El identificador
    /// se comprueba en cada evento y no solo en el primero: un `pending` de otra ejecución no vale,
    /// por el mismo motivo por el que existe la igualdad entre los dos clientes.
    async fn espera_el_nacimiento(
        primer_evento: serde_json::Value,
        cuerpo: &mut Body,
        ejecucion: &str,
        plazo: Duration,
    ) -> bool {
        let limite = Instant::now() + plazo;
        let mut datos = primer_evento;

        loop {
            let es_el_nacimiento =
                datos["execution_id"].as_str() == Some(ejecucion) && datos["status"] == "pending";

            if es_el_nacimiento {
                return true;
            }

            let restante = limite.saturating_duration_since(Instant::now());

            match siguiente_evento(cuerpo, restante).await {
                Some(evento) => datos = datos_del_evento(&evento),
                None => return false,
            }
        }
    }

    /// 1.26 — Un `401` **antes** de abrir el stream sí lleva la envoltura.
    ///
    /// Cubre el escenario «Un error antes de abrir el stream sí lleva la envoltura»: la excepción del
    /// delta `api-shell` es del **cuerpo del stream ya abierto**, no de los errores, que siguen siendo
    /// respuestas HTTP normales y siguen viajando envueltas. El modo es `Oidc` con la capa puesta y la
    /// petición va sin cookie.
    ///
    /// El código se afirma **antes** de leer el cuerpo, y eso no es orden de lectura sino una
    /// precaución: si la ruta quedara del lado público, esta petición recibiría un stream abierto —un
    /// cuerpo que no termina— y leerlo colgaría la prueba. Comprobado el `401`, leer el cuerpo es leer
    /// una respuesta que ya está cerrada.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sin_sesion_el_stream_contesta_401_con_la_envoltura() {
        let respuesta = get(router_oidc().await, "/api/v1/events").await;
        let status = respuesta.status();

        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "el stream es superficie de negocio y en modo OIDC tiene que exigir sesión: contestó \
             {status}"
        );

        let texto = crate::test_support::body_text(respuesta).await;
        let cuerpo: serde_json::Value = serde_json::from_str(&texto)
            .expect("el 401 del stream tiene que salir por la envoltura JSON de la API");

        assert_eq!(cuerpo["status"], 401, "la envoltura del 401: {texto}");

        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 401 tiene que traer un mensaje: {texto}"
        );
    }
}
