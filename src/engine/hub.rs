//! El hub de eventos: por dónde sale el progreso de una ejecución hacia los clientes del panel.
//!
//! Es la mitad del motor que **no** copia nada: recibe avisos y los reparte. Lo que reparte es
//! un [`EventoDeEjecucion`], con los siete campos del §5, y quien los pinta es el panel, que los
//! recibe por el stream de `GET /api/v1/events` —que vive en la capa HTTP, no aquí—.
//!
//! # Un `broadcast` acotado, y por qué no un canal por cliente
//!
//! El hub es un [`tokio::sync::broadcast`] con capacidad acotada, y la elección resuelve las dos
//! cosas que el contrato pide **sin código nuestro**:
//!
//! - **tiene tope**, así que no crece. Un hub sin tope es una fuga de memoria con forma de
//!   funcionalidad: el panel abre una pestaña, alguien cierra el portátil, y el motor sigue
//!   guardando eventos para un lector que ya no está;
//! - y **descarta** lo que no cabe en vez de bloquear, así que un cliente que se queda atrás
//!   pierde eventos y no frena al motor. Un hub que bloqueara convertiría a un cliente lento en
//!   un cliente que **para las copias**, que es el peor trueque posible: lo que hay que proteger
//!   es la copia, no el progreso. Y el cliente que se pierde eventos no se queda a ciegas,
//!   porque el histórico sigue contando la verdad.
//!
//! La capacidad es una constante del módulo —[`CAPACIDAD_DEL_HUB`]— y no entra por parámetro:
//! quien la eligiera tendría que saber cuántos eventos se aguantan sin lector, y eso no lo sabe
//! nadie mejor que este módulo. Lo que sí cambia según el tamaño del panel es el cliente, y el
//! cliente no decide esto.
//!
//! # El cierre, y por qué el emisor es **uno solo**
//!
//! El hub se puede cerrar —[`Hub::cerrar`]—, y eso es lo que hace que el apagado ordenado del
//! proceso no se cuelgue con un cliente conectado al stream: el cierre de `axum` no resuelve hasta
//! que todas las conexiones terminan, y el cuerpo de un `text/event-stream` solo termina cuando el
//! canal se cierra. Sin cerrarlo, el proceso se queda vivo hasta que el período de gracia del
//! orquestador lo mata con `SIGKILL`, con el hilo de trabajo a mitad de archivar: exactamente lo
//! que la parada acotada existe para evitar.
//!
//! Y por eso el emisor **no** puede ser un `Sender` por clon, que es lo que parecería natural en un
//! tipo `Clone`. Los clones del hub no son todos de un momento: `EnVivo` —el estado de una
//! ejecución en curso— guarda el suyo mientras su copia corre, así que cerrar desde uno dejaría el
//! otro en pie, el canal seguiría abierto y el apagado volvería a colgarse igual. Con un único
//! extremo de envío compartido —`Arc<Mutex<Option<…>>>`— soltarlo lo cierra para todos, incluido el
//! de una copia en vuelo.
//!
//! Cerrar **no** es una tercera vía de terminación del flujo: es la que ya estaba. [`Hub::cerrar`]
//! suelta el emisor, los receptores reciben su `Closed` y el stream termina por el camino que su
//! rustdoc ya explica.
//!
//! # Aquí no vive el cable
//!
//! El nombre del evento —`execution_progress`— y los nombres de los campos **no** están en este
//! módulo: son el cable, y el cable es de la API, que es quien mapea este tipo a su DTO. Aquí los
//! campos se llaman como el §5 para que ese mapeo sea de siete líneas y se lea de un vistazo.
//!
//! Y una decisión que sí es de aquí: el `status` es un [`Estado`] y no una cadena, porque el tipo
//! del §3 ya garantiza que son los cuatro del `CHECK` —`pending`, `running`, `success` y
//! `failed`—, así que el texto del cable lo pone quien publica el evento, no el evento. Con una
//! cadena, un `status` inventado no sería un error de compilación sino un evento que el panel
//! pinta como pueda.

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
use uuid::Uuid;

use crate::db::entities::Estado;

/// Capacidad del hub, en eventos.
///
/// La unidad es el evento, y el número sale de comparar lo que se retiene con lo que hace falta:
/// un cliente que se engancha tarde tiene que poder pintar los últimos avisos de las copias en
/// marcha —que son unos pocos por ejecución—, y 256 eventos los cubren con holgura, mientras que
/// la memoria retenida sigue siendo despreciable —un evento son dos identificadores, dos enteros,
/// un `Option<f64>` y una frase corta—.
///
/// Más grande no compra nada: un cliente que se pierda 256 eventos ya se enterará por el
/// histórico, que es lo que el contrato manda. Más pequeño, en cambio, empieza a descartar
/// eventos de una ejecución normal en cuanto el panel se descuida un segundo.
pub const CAPACIDAD_DEL_HUB: usize = 256;

/// Un aviso de progreso de una ejecución, con los siete campos del §5.
///
/// Es lo que viaja del motor al panel: lo que el motor sabe de una ejecución en un instante. Ni
/// un byte del volcado, ni una ruta, ni un motivo de error —el campo de la acción lleva una frase
/// **constante** del motor, no texto que venga de fuera—.
///
/// # Los dos contadores, y lo que **no** cuentan los dos
///
/// Medido en el archivador de la biblioteca: su `inc` de bytes se ejecuta **fuera** del `if` de
/// la deduplicación, así que cuenta bytes **leídos**, no escritos. De ahí que, para un volcado,
/// [`bytes_uploaded`](Self::bytes_uploaded) vaya pegado a
/// [`bytes_processed`](Self::bytes_processed) —los dos cuentan lo leído del `exec`, uno a la
/// entrada del archivador y otro a su salida— y de que **ninguna de las dos cifras sea un
/// recuento de los bytes que han llegado a S3**: eso no lo observa el motor, y presentarlo como
/// si lo observara sería describir un hecho que nadie ha medido.
#[derive(Debug, Clone, PartialEq)]
pub struct EventoDeEjecucion {
    /// La ejecución de la que habla este aviso.
    pub execution_id: Uuid,
    /// La tarea a la que pertenece esa ejecución.
    pub job_id: Uuid,
    /// El estado de la ejecución en el momento del aviso.
    ///
    /// Es [`Estado`] y no una cadena a propósito: el tipo del §3 garantiza que es uno de los
    /// cuatro del `CHECK`, así que aquí no puede entrar un estado que la tabla no admitiría.
    /// El texto lo pone quien publica el cable, que es la capa HTTP.
    pub status: Estado,
    /// Los bytes que han **salido** de la extracción: para un volcado, lo que el productor ha
    /// entregado al canal; para un directorio, el total que su origen declara.
    pub bytes_processed: u64,
    /// Los bytes que el archivador ha ido leyendo, según su gancho de progreso.
    pub bytes_uploaded: u64,
    /// El porcentaje, entre `0` y `100`, **solo cuando hay un total conocido y mayor que cero**.
    ///
    /// Medido: un volcado por `stdout` **no recibe nunca** `set_length` en su barra de bytes —el
    /// `set_length(0)` que aparece en el registro es de las barras de tipo `Counter`, que el
    /// gancho descarta a propósito—, así que su porcentaje es nulo **por construcción** y no por
    /// comodidad. Un `total == 0` —un directorio vacío— también da `None`: no hay nada que
    /// proporcionar, y un `0 %` o un `100 %` inventados serían una barra pintada sobre una cifra
    /// falsa.
    pub percentage: Option<f64>,
    /// Qué está haciendo la ejecución ahora mismo.
    ///
    /// Es **texto de pantalla**, así que va en español, y lo construye el motor desde constantes
    /// suyas: nunca lleva nada que venga del contenedor ni de una fila del §3 —una ruta del
    /// volcado dentro de una frase que el panel pinta sería un dato de la base fuera de su sitio—.
    pub current_action: String,
}

/// El hub: el reparto de eventos a los clientes del stream.
///
/// Se construye vacío con [`Hub::nuevo`] y se comparte —es `Clone`, y clonarlo no copia ni un
/// evento: por dentro hay un `Arc`—. Publicar nunca falla y nunca bloquea: ver
/// [`Hub::publicar`]. Y se puede **cerrar** —[`Hub::cerrar`]—, que es lo que deja terminar a los
/// clientes del stream cuando el proceso se apaga.
#[derive(Clone)]
pub struct Hub {
    /// El extremo de envío del `broadcast`: **uno solo** para todos los clones del hub.
    ///
    /// Los receptores los crea [`Hub::suscribir`], y el canal se queda con el último extremo de
    /// envío aunque no quede ningún receptor, de modo que suscribirse después de que todos se hayan
    /// ido sigue funcionando.
    ///
    /// El `Option` es el cierre: [`Hub::cerrar`] lo vacía, y al soltar el `Sender` el canal queda
    /// cerrado de golpe para todos los receptores. El `Arc` es lo que hace que el cierre alcance a
    /// **todos** los clones —el de `EnVivo`, que vive lo que dura una copia—; el porqué, en el
    /// encabezado del módulo.
    ///
    /// El candado es de `std` y no de Tokio a propósito: protege tres operaciones instantáneas
    /// —mirar el `Option`, suscribir y publicar—, no se mantiene nunca a lo largo de un `await`, y
    /// [`Hub::publicar`] tiene que poder llamarse desde los hilos del motor, que no están dentro de
    /// un runtime.
    emisor: Arc<Mutex<Option<broadcast::Sender<EventoDeEjecucion>>>>,
}

impl Hub {
    /// Un hub vacío, sin ningún suscriptor.
    pub fn nuevo() -> Self {
        // El `Receiver` que devuelve `channel` se suelta aquí mismo: lo que se conserva es el
        // extremo de envío, y cada cliente pide el suyo con `suscribir`.
        let (emisor, _) = broadcast::channel(CAPACIDAD_DEL_HUB);

        Self {
            emisor: Arc::new(Mutex::new(Some(emisor))),
        }
    }

    /// Un receptor para un cliente del stream, o `None` si el hub ya está cerrado.
    ///
    /// Cada cliente tiene el suyo: el `broadcast` reparte una copia a cada uno, así que dos
    /// clientes conectados a la vez ven los mismos eventos y ninguno ve algo que el otro no vea.
    /// Un cliente que se queda atrás pierde eventos —su receptor devuelve
    /// [`Lagged`](tokio::sync::broadcast::error::RecvError::Lagged) con cuántos se dejó— y **no**
    /// frena a nadie: eso es exactamente lo que la capacidad acotada compra.
    ///
    /// # Por qué devuelve un `Option`, y no un receptor siempre
    ///
    /// Porque el hub se puede cerrar **a la vez** que llega una petición: una que ya estaba dentro
    /// cuando llegó el `SIGTERM` puede llamar aquí justo después de [`Hub::cerrar`], y con un
    /// `Receiver` a secas habría que devolver algo inventado —un canal recién hecho, cuyo emisor
    /// habría que soltar en el acto para que la primera lectura diera `Closed`—. El `Option` pone
    /// ese estado en el **tipo**: quien llama sabe que no hay stream que servir, y la capa HTTP
    /// contesta un cuerpo que termina en el acto en vez de fingir una suscripción.
    ///
    /// El candado envenenado devuelve `None` por el mismo motivo que [`Hub::cerrar`] no entra en
    /// pánico: un cierre a medias no puede dejar a un cliente esperando eventos que ya no van a
    /// llegar.
    pub fn suscribir(&self) -> Option<broadcast::Receiver<EventoDeEjecucion>> {
        let emisor = self
            .emisor
            .lock()
            .unwrap_or_else(|envenenado| envenenado.into_inner());

        emisor.as_ref().map(broadcast::Sender::subscribe)
    }

    /// Publica un aviso. **Nunca bloquea y nunca falla**: sin suscriptores se descarta.
    ///
    /// Las dos mitades son el contrato, y las da el propio `broadcast`: su `send` es síncrono
    /// —así que se puede llamar desde los hilos del motor, que no están en un runtime— y con el
    /// búfer lleno tira el evento más viejo en vez de esperar. De ahí que el `Result` que
    /// devuelve —error solo cuando no queda ningún receptor vivo— se descarte: «no hay nadie
    /// escuchando» no es un fallo del motor, es una pestaña cerrada, y tratarlo como un fallo
    /// obligaría a decidir qué hacer con una copia por el hecho de que nadie mire.
    ///
    /// Después de [`Hub::cerrar`] esto es un **no-op silencioso**, y esa es la tercera mitad del
    /// contrato durante el apagado: no hay a quién publicar —los clientes ya tienen su `Closed`— y
    /// un trabajador que esté a mitad de una copia sigue avisando hasta que su ejecución acabe.
    /// Negarse aquí obligaría a decidir qué hacer con una copia en curso por el hecho de que el
    /// proceso se esté apagando, que es justo lo que `publicar` existe para no hacer.
    ///
    /// El candado se mantiene **durante el `send`**, y no es un descuido: clonar el `Sender` para
    /// soltarlo antes de enviar dejaría un extremo de envío vivo un instante más, y un
    /// [`Hub::cerrar`] que cayera en ese hueco no cerraría nada —el clon mantendría el canal
    /// abierto y los receptores no verían su `Closed`, que es el cuelgue que esto viene a
    /// arreglar—. Publicar dura microsegundos y ningún `await` cabe dentro.
    pub fn publicar(&self, evento: EventoDeEjecucion) {
        let emisor = self
            .emisor
            .lock()
            .unwrap_or_else(|envenenado| envenenado.into_inner());

        if let Some(emisor) = emisor.as_ref() {
            let _ = emisor.send(evento);
        }
    }

    /// Cierra el hub: los clientes del stream terminan, y publicar deja de hacer nada.
    ///
    /// Es lo que el apagado ordenado llama **antes** de esperar a que las conexiones terminen, y
    /// existe porque ese cierre no resuelve hasta que todas lo hacen: con un cliente conectado al
    /// stream, su cuerpo no termina nunca —la única salida del flujo es el `Closed` del canal— y
    /// el proceso se quedaría vivo hasta que el orquestador lo matara con `SIGKILL`, con el hilo
    /// de trabajo a mitad de archivar. Lo llama [`Motor::cerrar_el_hub`](crate::engine::Motor::cerrar_el_hub),
    /// una vez, dentro del futuro de apagado del servidor.
    ///
    /// Es **idempotente** —la segunda vez no queda nada que soltar— y no entra en pánico si el
    /// candado quedó envenenado: el trabajo se hace igual, con el candado recogido, porque un
    /// cierre que se cae no cierra nada.
    ///
    /// # El límite que esto **no** cubre, y que hay que saber
    ///
    /// Un cliente que además **deje de leer** —el socket lleno, el portátil cerrado— puede seguir
    /// retrasando el cierre: con el búfer de escritura lleno, el cuerpo no se puede vaciar y esa
    /// conexión no termina. Eso no está en manos del hub: aquí se cierra el canal, que es lo que sí
    /// lo está. Poner un plazo de escritura sería inventarse una segunda vía de terminación —y, con
    /// ella, un cliente al que se corta sin que nadie lo haya decidido—; el contrato pide que el
    /// apagado no se cuelgue por culpa del motor, no que un cliente roto se pueda ignorar.
    pub fn cerrar(&self) {
        let mut emisor = self
            .emisor
            .lock()
            .unwrap_or_else(|envenenado| envenenado.into_inner());

        // El `take` **es** el cierre: al soltar el último extremo de envío del `broadcast` el canal
        // se cierra y todos los receptores —los conectados y los que lleguen tarde— reciben
        // `Closed`. Y es idempotente por construcción: la segunda vez devuelve `None`.
        let _ = emisor.take();
    }

    /// La capacidad del hub, en eventos.
    ///
    /// Devuelve el valor de [`CAPACIDAD_DEL_HUB`], que es el que se le pidió al canal al
    /// construirlo: el `Sender` del `broadcast` no vuelve a publicar la capacidad con la que se
    /// creó, así que la constante es la fuente. Existe porque el tope es contrato —el contrato
    /// exige que el hub no crezca— y una propiedad del contrato tiene que poder comprobarse.
    pub fn capacidad(&self) -> usize {
        CAPACIDAD_DEL_HUB
    }
}

impl Default for Hub {
    /// Un hub vacío: lo mismo que [`Hub::nuevo`].
    ///
    /// Está porque un `pub fn nuevo()` sin `Default` es un aviso de `clippy`, y con razón: quien
    /// tenga un tipo que se construye sin argumentos espera poder decir `Hub::default()` en el
    /// sitio donde el tipo lo elige otro.
    fn default() -> Self {
        Self::nuevo()
    }
}

impl std::fmt::Debug for Hub {
    /// Publica la capacidad y **nada** del canal ni de sus receptores.
    ///
    /// Un `Debug` derivado imprimiría el `Debug` del `Sender` —que no dice más que esto— y, si
    /// alguna versión lo cambiara, publicaría el interior del tipo: aquí no hay nada que publicar
    /// salvo el tope, que es la única propiedad de un hub que un diagnóstico necesita.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("capacidad", &self.capacidad())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    use uuid::Uuid;

    use super::{EventoDeEjecucion, Hub};
    use crate::db::entities::Estado;

    /// Un evento distinguible, con la acción que se le pida.
    ///
    /// Los identificadores se generan aquí y no se fijan: lo que estas pruebas miden es el
    /// reparto, no la identidad de nadie.
    fn evento(accion: &str) -> EventoDeEjecucion {
        EventoDeEjecucion {
            execution_id: Uuid::new_v4(),
            job_id: Uuid::new_v4(),
            status: Estado::Running,
            bytes_processed: 1024,
            bytes_uploaded: 512,
            percentage: None,
            current_action: accion.to_owned(),
        }
    }

    /// **13.a** Dos suscriptores reciben **el mismo** evento, y publicar sin nadie escuchando no
    /// falla.
    ///
    /// Es el requisito del stream global —«dos clientes ven el mismo evento»— en su forma mínima:
    /// el hub reparte una copia a cada uno. Y la segunda mitad es la que impide que un panel
    /// cerrado se convierta en un fallo del motor: publicar sin suscriptores se descarta.
    #[test]
    fn dos_suscriptores_reciben_el_mismo_evento() {
        let hub = Hub::nuevo();

        // Sin nadie escuchando: no falla, se descarta.
        hub.publicar(evento("antes de que nadie escuche"));

        let mut uno = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");
        let mut otro = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        let publicado = evento("archivando");
        hub.publicar(publicado.clone());

        let del_primero = uno
            .try_recv()
            .expect("el primer suscriptor tiene que recibir el evento");
        let del_segundo = otro
            .try_recv()
            .expect("el segundo suscriptor tiene que recibir el mismo evento");

        assert_eq!(
            del_primero, publicado,
            "el primer suscriptor recibió un evento que no es el publicado"
        );
        assert_eq!(
            del_segundo, publicado,
            "los dos suscriptores no recibieron el mismo evento"
        );
    }

    /// **13.b** El hub **no crece sin tope**: un cliente que no lee pierde eventos y no recibe
    /// más que la capacidad.
    ///
    /// Publicar `capacidad + 50` eventos con un receptor que no lee nada es el escenario que el
    /// contrato pide —«un cliente lento no frena al motor»— y se mide en las dos direcciones: el
    /// receptor encuentra `Lagged(n)` con `n > 0` —o sea que sí se perdieron, y el hub lo dice en
    /// vez de mentir—, y lo que llega a entregar no pasa de la capacidad, que es el tope. Si el
    /// hub no tuviera tope, `publicar` no devolvería `Lagged` nunca y el bucle entregaría los
    /// `capacidad + 50`.
    #[test]
    fn el_hub_no_crece_sin_tope_y_el_cliente_lento_no_frena_al_motor() {
        let hub = Hub::nuevo();
        let capacidad = hub.capacidad();
        let mut lento = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        // El cronómetro va aquí solo para dejar constancia de la otra mitad del contrato: con un
        // búfer acotado y descartando, publicar no puede quedarse esperando a que alguien lea.
        let empezado = std::time::Instant::now();

        for publicado in 0..(capacidad + 50) {
            hub.publicar(evento(&format!("aviso {publicado}")));
        }

        let tardado = empezado.elapsed();

        let mut entregados = 0usize;
        let mut perdidos = 0usize;

        loop {
            match lento.try_recv() {
                Ok(_) => entregados += 1,
                Err(TryRecvError::Lagged(cuantos)) => {
                    perdidos = perdidos.max(cuantos as usize);
                }
                // El canal vacío es el final de la lectura, y `Closed` no puede ocurrir mientras
                // el hub viva —el extremo de envío lo sigue teniendo `hub`—.
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
            }
        }

        assert!(
            perdidos > 0,
            "un cliente que no lee no perdió ni un evento con {capacidad} de capacidad y \
             {} publicados: el hub no tiene tope",
            capacidad + 50
        );
        assert!(
            entregados <= capacidad,
            "el receptor entregó {entregados} eventos con {capacidad} de capacidad: el hub creció"
        );
        assert!(
            entregados > 0,
            "el receptor no recibió nada: el reparto está roto y la medición no dice nada"
        );
        assert!(
            tardado < std::time::Duration::from_secs(5),
            "publicar {} eventos tardó {tardado:?}: el hub está esperando a alguien, y eso es \
             un cliente que frena a las copias",
            capacidad + 50
        );
    }

    /// El receptor del stream **no se queda sin el evento por publicar dos veces**: dos avisos
    /// seguidos llegan los dos, en orden.
    ///
    /// Es la mitad que el tope no puede romper: descartar solo cuando no cabe. Con dos eventos y
    /// capacidad de sobra, los dos tienen que llegar, y en el orden en que se publicaron —el
    /// panel pinta el último que recibe, así que un reparto que reordenara mostraría una acción
    /// vieja como si fuera la de ahora—.
    #[test]
    fn dos_avisos_seguidos_llegan_los_dos_y_en_orden() {
        let hub = Hub::nuevo();
        let mut cliente = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        hub.publicar(evento("extrayendo el volcado"));
        hub.publicar(evento("archivando"));

        let primero = cliente
            .try_recv()
            .expect("el primer aviso tiene que llegar");
        let segundo = cliente
            .try_recv()
            .expect("el segundo aviso tiene que llegar");

        assert_eq!(primero.current_action, "extrayendo el volcado");
        assert_eq!(
            segundo.current_action, "archivando",
            "los avisos llegaron desordenados: el panel pintaría una acción vieja como la de ahora"
        );
    }

    /// `publicar` no falla aunque **todos** los clientes se hayan ido.
    ///
    /// Es el caso que el contrato no puede permitirse que sea un error: un panel que cierra la
    /// pestaña no puede hacer que una copia falle. Se comprueba con un receptor que se suelta
    /// —con lo que el canal se queda sin nadie— y publicando después.
    #[test]
    fn publicar_sin_suscriptores_vivos_no_falla() {
        let hub = Hub::nuevo();

        {
            let cliente = hub
                .suscribir()
                .expect("el hub no está cerrado en esta prueba");
            drop(cliente);
        }

        hub.publicar(evento("sin nadie escuchando"));

        // Y el que llegue después sigue recibiendo: el hub no se cierra porque se vayan todos.
        let mut tardio = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");
        hub.publicar(evento("el que llega tarde"));

        assert!(
            tardio.try_recv().is_ok(),
            "el hub dejó de repartir después de quedarse sin suscriptores"
        );
    }

    /// Un `recv` sobre un receptor vivo con el hub vacío espera, no falla: es lo que hace el
    /// stream real entre dos avisos.
    ///
    /// Queda fijado aquí porque es la propiedad de la que depende la capa HTTP: un cliente
    /// conectado y una ejecución ociosa no es un error, es una espera.
    #[tokio::test]
    async fn un_receptor_sin_nada_que_leer_espera() {
        let hub = Hub::nuevo();
        let mut cliente = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), cliente.recv())
                .await
                .is_err(),
            "un receptor sin eventos devolvió algo, cuando lo que tiene que hacer es esperar"
        );

        // Y el canal sigue vivo: lo que caducó fue la espera de la prueba, no el hub.
        assert!(
            !matches!(cliente.try_recv(), Err(TryRecvError::Closed)),
            "el canal se cerró por una espera: el hub no puede morir porque nadie publique"
        );

        hub.publicar(evento("después de la espera"));

        assert!(
            cliente.recv().await.is_ok(),
            "el aviso no llegó después de la espera"
        );
        assert!(matches!(cliente.try_recv(), Err(TryRecvError::Empty)));
    }

    /// **El cierre suelta el canal**: el cliente que estaba esperando recibe `Closed` sin que nadie
    /// publique nada, y el que llega después ya no se puede suscribir.
    ///
    /// Es la mitad del arreglo del apagado que se puede medir aquí: `Closed` es la **única** salida
    /// del flujo de la capa HTTP, así que un receptor que lo recibe es un stream que termina y una
    /// conexión que se puede cerrar. Se mide con un `recv` de verdad —no con un `try_recv`— porque
    /// el flujo real está esperando en ese `recv`: lo que hay que comprobar es que el cierre
    /// **despierta** a quien espera, no solo que el estado cambie.
    #[tokio::test]
    async fn el_cierre_despierta_al_cliente_que_esperaba_y_no_deja_suscribir_a_nadie_mas() {
        let hub = Hub::nuevo();
        let mut cliente = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        hub.cerrar();

        let desenlace =
            tokio::time::timeout(std::time::Duration::from_secs(5), cliente.recv()).await;

        assert!(
            matches!(desenlace, Ok(Err(RecvError::Closed))),
            "el cierre no despertó al cliente con su `Closed`: el cuerpo del stream no terminaría y \
             el apagado ordenado se quedaría esperándolo ({desenlace:?})"
        );

        assert!(
            hub.suscribir().is_none(),
            "un hub cerrado sigue dando receptores: una petición que llegue durante el apagado \
             tendría un stream abierto que ya no puede recibir nada"
        );
    }

    /// `cerrar` es **idempotente** y, después, publicar es un no-op silencioso.
    ///
    /// El apagado puede pasar por aquí más de una vez y ninguna de esas veces puede ser un error ni
    /// un pánico: lo que cierra el proceso no es un camino que se pueda permitir fallar. Y publicar
    /// después del cierre tiene que seguir sin fallar, porque un trabajador a mitad de una copia
    /// sigue avisando hasta que su ejecución acabe.
    #[test]
    fn cerrar_es_idempotente_y_publicar_despues_no_falla() {
        let hub = Hub::nuevo();
        let mut receptor = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        hub.cerrar();
        hub.cerrar();

        // Publicar después del cierre no entra en pánico y no le llega a nadie: el receptor solo
        // puede ver el `Closed` del cierre.
        hub.publicar(evento("después del cierre"));

        assert!(matches!(receptor.try_recv(), Err(TryRecvError::Closed)));
    }

    /// El cierre alcanza también a los **clones** del hub, que es lo que `EnVivo` guarda.
    ///
    /// Es la mitad del arreglo que un hub de un solo dueño no necesitaría, y la que hace que el
    /// apagado no se cuelgue con una copia en curso: el estado en vivo de una ejecución guarda su
    /// propio clon del hub y publica por él, así que con un extremo de envío por clon ese clon
    /// mantendría el canal abierto. Aquí se sujeta con un clon explícito que se conserva vivo
    /// después del cierre.
    #[tokio::test]
    async fn el_cierre_alcanza_a_los_clones_del_hub() {
        let hub = Hub::nuevo();
        let en_vivo = hub.clone();
        let mut cliente = hub
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        hub.cerrar();

        // El clon sigue ahí, y sigue publicando: es lo que hace `EnVivo` mientras su copia corre.
        en_vivo.publicar(evento("el aviso de una copia en vuelo"));

        let desenlace =
            tokio::time::timeout(std::time::Duration::from_secs(5), cliente.recv()).await;

        assert!(
            matches!(desenlace, Ok(Err(RecvError::Closed))),
            "un clon del hub mantuvo el canal abierto después del cierre: es exactamente el caso de \
             `EnVivo`, y con él el apagado se cuelga aunque el motor cierre su hub ({desenlace:?})"
        );
    }
}
