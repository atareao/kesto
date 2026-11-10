//! El cuerpo del disparo y su tabla de desenlaces.
//!
//! Un disparo es **una llamada a [`Motor::encolar`](crate::engine::Motor::encolar)**, la misma puerta que
//! `POST /api/v1/jobs/:id/run`. No hay una segunda ruta de escritura: el planificador no conoce la
//! cola, no conoce la capa de acceso de `job_executions` y no escribe ni una fila por su cuenta. De
//! ahí hereda tres cosas que este módulo no reproduce:
//!
//! - la **guarda atómica** del `409`, que es una escritura condicional y no una lectura previa, así
//!   que reproducirla mal daría dos ejecuciones `pending` para la misma tarea;
//! - la **publicación del nacimiento** de la ejecución en el hub, así que un disparo del reloj se ve
//!   en el panel exactamente como uno del botón;
//! - y los **desenlaces ya tipados**, que son los que gobiernan la tabla de abajo.
//!
//! # El cuerpo es un método, no el cierre
//!
//! La lógica vive en [`Planificador::disparar`] y no dentro del cierre que la biblioteca llama, por
//! un motivo de prueba y no de estilo: un tick de calendario real no se puede esperar en una prueba,
//! así que lo que se prueba es el cuerpo —al que la prueba llama directamente— y el cierre queda
//! como una sola línea que lo invoca.
//!
//! # La tabla de desenlaces
//!
//! `encolar` puede devolver seis errores, y cada uno pide una reacción distinta. El criterio que
//! ordena la tabla es **dónde está la causa**: si está en la tarea —ya está corriendo, ya no
//! existe— el planificador sigue vivo y lo que cambia es la planificación de **esa** tarea; si está
//! en el proceso —el motor ha dejado de aceptar trabajo— el planificador se para, porque seguir
//! planificando no encolaría nada y encolaría «casi», que es una ventana de carrera con la parada.
//! Y todo lo demás se registra y se sigue: un reloj que se cae por el primer error se convierte en
//! la ausencia de todas las copias programadas, que es el peor desenlace posible.
//!
//! Nada de esto se propaga como un pánico: un fallo de un disparo no puede matar al planificador ni
//! al proceso.

use uuid::Uuid;

use crate::db::entities::Ejecucion;
use crate::engine::ErrorDelMotor;

use super::planificacion::Planificador;

/// Qué hace el planificador con el desenlace de un encolado.
///
/// Es la tabla del requisito 5 hecha tipo, para poder probar cada variante **sin contenedor**: las
/// que no se alcanzan sin él —una cola llena, una cola sin quien la atienda— se cubren aquí
/// llamando a [`decidir`] con el error construido a mano.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Se registra y **se sigue**: el disparo no cambia la planificación.
    Seguir,
    /// Se registra y **se salta**: la tarea no se reintenta en el acto, pero sigue planificada y el
    /// siguiente tick volverá a intentarlo.
    Saltar,
    /// Se **retira** la tarea de la planificación en el momento en que se sabe, sin esperar a la
    /// siguiente reconciliación.
    Retirar,
    /// Se **para** el planificador: el motor ha dejado de aceptar trabajo.
    Parar,
}

/// Traduce el desenlace de `encolar` a lo que el planificador tiene que hacer.
///
/// Es una función **pura** —no toca el reloj ni la base— precisamente para que las seis variantes
/// del error se prueben una a una, incluidas las dos que no se alcanzan sin contenedor.
pub(crate) fn decidir(desenlace: &Result<Ejecucion, ErrorDelMotor>) -> Decision {
    match desenlace {
        // Encoló: el disparo hizo su trabajo y nada cambia en la planificación.
        Ok(_) => Decision::Seguir,

        // La causa está en **la tarea** o en su cola, no en el planificador: se registra y se
        // salta, pero la tarea sigue planificada y el siguiente tick volverá a intentarlo.
        Err(ErrorDelMotor::YaTieneUnaEnCurso { .. })
        | Err(ErrorDelMotor::ColaDelRepositorioLlena { .. })
        | Err(ErrorDelMotor::SinQuienAtiendaLaCola { .. }) => Decision::Saltar,

        // La tarea ya no existe: seguir disparándola cada tick sería una tarea fantasma, así que se
        // retira de la planificación en el acto, sin esperar a la siguiente reconciliación.
        Err(ErrorDelMotor::TareaQueNoEsta { .. }) => Decision::Retirar,

        // La causa está en **el proceso**: el motor ha dejado de aceptar trabajo, y seguir
        // planificando no encolaría nada —y encolaría «casi», que es una carrera con la parada—.
        Err(ErrorDelMotor::MotorParado) => Decision::Parar,

        // Un fallo de base es pasajero —otro escritor, un instante—: se registra y se sigue, que el
        // siguiente disparo se intente.
        Err(ErrorDelMotor::Base { .. }) => Decision::Seguir,

        // El resto de variantes de [`ErrorDelMotor`] no las devuelve `encolar` —son de otras
        // operaciones del motor—, así que no llegan aquí; si alguna llegara, el desenlace prudente
        // es registrar y seguir, que es lo que nunca se cae por el primer error.
        Err(_) => Decision::Seguir,
    }
}

impl Planificador {
    /// Dispara una tarea: la encola en el motor, con los desenlaces de [`decidir`].
    ///
    /// Es **lo que el cierre de la biblioteca llama**, y también lo que una prueba llama para no
    /// esperar un tick de calendario real. Consulta la puerta de parada y encola **bajo el mismo
    /// candado**, que es la garantía de que un encolado o termina antes de que la puerta cierre o no
    /// empieza —y eso cubre también el tick que la biblioteca dispare después de la parada—.
    ///
    /// Con la puerta cerrada no hace nada: no se planifica ni se encola trabajo nuevo.
    pub async fn disparar(&self, job_id: Uuid) {
        // La puerta y el encolado **bajo el mismo candado**: la sección crítica va de aquí hasta
        // que el encolado termina, así que un encolado o termina antes de que la puerta cierre o no
        // empieza —y eso cubre también el tick que la biblioteca dispare después de la parada—.
        let mut estado = self.0.estado.lock().await;

        if !estado.abierta {
            return;
        }

        let desenlace = self.0.motor.encolar(job_id).await;

        match decidir(&desenlace) {
            Decision::Seguir => {
                if let Err(error) = &desenlace {
                    tracing::warn!(
                        job_id = %job_id,
                        %error,
                        "el disparo no ha encolado, pero se sigue: el siguiente tick volverá a intentarlo"
                    );
                }
            }
            Decision::Saltar => {
                if let Err(error) = &desenlace {
                    tracing::warn!(
                        job_id = %job_id,
                        %error,
                        "el disparo se salta este tick"
                    );
                }
            }
            Decision::Retirar => {
                tracing::warn!(
                    job_id = %job_id,
                    "la tarea del disparo ya no existe: se retira de la planificación"
                );

                if let Some(mango) = estado.planificadas.remove(&job_id) {
                    estado.expresiones.remove(&job_id);

                    // El reloj, tras el de estado: es el orden de cerrojo que declara
                    // [`super::planificacion::Interior`].
                    let reloj = self.0.reloj.lock().await;

                    if let Err(error) = reloj.remove(&mango).await {
                        tracing::warn!(
                            job_id = %job_id,
                            %error,
                            "el mango de la tarea retirada no se ha podido quitar del reloj"
                        );
                    }
                }
            }
            Decision::Parar => {
                tracing::error!(
                    job_id = %job_id,
                    "el motor ha dejado de aceptar trabajo: el planificador se para"
                );

                // Un solo sentido: la puerta se cierra y no se reabre.
                estado.abierta = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use time::OffsetDateTime;
    use tokio::sync::broadcast::error::TryRecvError;
    use uuid::Uuid;

    use super::{Decision, decidir};
    use crate::db::DbError;
    use crate::db::entities::{Ejecucion, Estado};
    use crate::engine::{ErrorDelMotor, EventoDeEjecucion};
    use crate::scheduler::pruebas::{Aparato, CRON_DE_CINCO};

    /// El plazo, acotado y con nombre, con el que una prueba espera a que una ejecución acabe.
    ///
    /// El abridor de este aparato tarda un segundo en decir que no abre, así que lo que mide este
    /// plazo es un cuelgue y no una prueba lenta. Existe porque los trabajadores del motor corren en
    /// hilos aparte que escriben en la base a través del runtime: soltar el runtime con una
    /// ejecución a medias hace saltar al hilo que escribe, así que se espera antes de acabar.
    const PLAZO_DE_ESPERA: Duration = Duration::from_secs(30);

    /// Espera, acotada, a que la única ejecución de una tarea llegue a un estado terminal.
    async fn esperar_el_final(aparato: &Aparato, tarea: &Uuid) {
        let limite = Instant::now() + PLAZO_DE_ESPERA;

        loop {
            let estado = aparato.estado_de_la_ejecucion(tarea).await;

            if estado == "success" || estado == "failed" {
                return;
            }

            assert!(
                Instant::now() < limite,
                "la ejecución de la prueba no llegó a un estado terminal en {PLAZO_DE_ESPERA:?}: \
                 {estado}"
            );

            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Vacía el receptor del hub y devuelve los eventos que había.
    fn recoger(
        receptor: &mut tokio::sync::broadcast::Receiver<EventoDeEjecucion>,
    ) -> Vec<EventoDeEjecucion> {
        let mut eventos = Vec::new();

        loop {
            match receptor.try_recv() {
                Ok(evento) => eventos.push(evento),
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                // Un cliente que se pierde eventos no se corta: sigue leyendo. Aquí no puede pasar
                // —se lee enseguida y son pocos—, pero el camino se escribe igual.
                Err(TryRecvError::Lagged(_)) => continue,
            }
        }

        eventos
    }

    /// El nacimiento de una tarea dentro de un lote de eventos: el que llega en `pending`.
    fn nacimiento<'a>(eventos: &'a [EventoDeEjecucion], tarea: &Uuid) -> &'a EventoDeEjecucion {
        match eventos
            .iter()
            .find(|evento| evento.job_id == *tarea && evento.status == Estado::Pending)
        {
            Some(evento) => evento,
            None => panic!("no se publicó el nacimiento de la tarea {tarea}: llegaron {eventos:?}"),
        }
    }

    /// 5 — La tabla de desenlaces reparte las seis salidas de `encolar`, incluidas las dos que no se
    /// alcanzan sin un contenedor.
    ///
    /// Es una prueba de la función pura y por eso cubre lo que ninguna otra puede:
    /// `ColaDelRepositorioLlena` —haría falta llenar la cola de sesenta y cuatro ejecuciones— y
    /// `SinQuienAtiendaLaCola` —el canal desconectado es inalcanzable por construcción— se
    /// construyen aquí a mano y se comprueba a qué reacción obliga cada uno. Y el `Ok`, que no está
    /// en la tabla del requisito porque no es un desenlace de error: un disparo que encola no cambia
    /// la planificación.
    #[test]
    fn la_tabla_de_desenlaces_reparte_las_salidas_de_encolar() {
        let tarea = Uuid::new_v4();
        let repositorio = Uuid::new_v4();

        let nacida = Ejecucion {
            id: Uuid::new_v4(),
            job_id: tarea,
            status: Estado::Pending,
            snapshot_id: None,
            bytes_processed: 0,
            bytes_uploaded: 0,
            error_message: None,
            started_at: OffsetDateTime::now_utc(),
            finished_at: None,
        };

        let tabla = [
            (Ok(nacida), Decision::Seguir),
            (
                Err(ErrorDelMotor::YaTieneUnaEnCurso { job_id: tarea }),
                Decision::Saltar,
            ),
            (
                Err(ErrorDelMotor::ColaDelRepositorioLlena {
                    repository_id: repositorio,
                }),
                Decision::Saltar,
            ),
            (
                Err(ErrorDelMotor::SinQuienAtiendaLaCola {
                    repository_id: repositorio,
                    motivo: "el hilo de la cola no se pudo crear".to_owned(),
                }),
                Decision::Saltar,
            ),
            (
                Err(ErrorDelMotor::TareaQueNoEsta { job_id: tarea }),
                Decision::Retirar,
            ),
            (Err(ErrorDelMotor::MotorParado), Decision::Parar),
            (
                Err(ErrorDelMotor::Base {
                    causa: DbError::DirectorioAusente {
                        ruta: PathBuf::from("/no/existe/kesto"),
                    },
                }),
                Decision::Seguir,
            ),
        ];

        for (desenlace, esperada) in &tabla {
            assert_eq!(
                decidir(desenlace),
                *esperada,
                "el desenlace {desenlace:?} no se reparte como manda el requisito 5"
            );
        }
    }

    /// 5 — Escenario 1: una tarea ya en curso se salta y no crea una segunda ejecución.
    ///
    /// La ejecución que ya estaba se deja en la base, no en la cola: lo que bloquea el segundo
    /// encolado es la guarda atómica del `409`, que mira las filas, no el trabajador. Así que no
    /// hace falta ningún contenedor para provocarlo, y el planificador tiene que seguir vivo y con
    /// la tarea planificada —el siguiente tick volverá a intentarlo—.
    #[tokio::test]
    async fn una_tarea_ya_en_curso_se_salta_y_no_crea_una_segunda_ejecucion() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        aparato.dejar_una_en_curso(&tarea).await;

        planificador.disparar(tarea).await;

        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            1,
            "un disparo sobre una tarea que ya está en curso creó una segunda ejecución"
        );
        assert!(
            !planificador.esta_parado(),
            "un `409` no puede parar al planificador"
        );
        assert!(
            planificador.planificadas().contains(&tarea),
            "una tarea saltada dejó de estar planificada: el siguiente tick no podría reintentarla"
        );
    }

    /// 5 — Escenario 2: una tarea borrada deja de estar planificada en el mismo disparo.
    ///
    /// No se espera a la siguiente reconciliación: seguir disparando cada tick contra un `404` es
    /// una tarea fantasma. El borrado no pasa por la API —es un `DELETE` sobre la tabla—, que es
    /// justo el caso que la reconciliación cubriría demasiado tarde.
    #[tokio::test]
    async fn una_tarea_borrada_se_retira_de_la_planificacion_al_disparar() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        assert!(
            aparato
                .tareas
                .borrar(&tarea)
                .await
                .expect("el borrado de la tarea no puede fallar"),
            "la tarea de la prueba tenía que existir para borrarse"
        );

        planificador.disparar(tarea).await;

        assert!(
            !planificador.planificadas().contains(&tarea),
            "una tarea borrada siguió planificada tras el disparo: {:?}",
            planificador.planificadas()
        );
        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            0,
            "el disparo de una tarea borrada creó una ejecución"
        );
        assert!(
            !planificador.esta_parado(),
            "un `404` no puede parar al planificador"
        );
    }

    /// 5 — Escenario 3: un motor parado detiene el planificador.
    ///
    /// El motor se para de verdad —el mismo `Motor::parar` de la parada ordenada— y el disparo
    /// encuentra su puerta cerrada. El planificador tiene que pararse él también: seguir planificando
    /// no encolaría nada, y encolaría «casi», que es una ventana de carrera con la parada. No basta
    /// con registrar el fallo de ese tick.
    #[tokio::test]
    async fn un_motor_parado_detiene_el_planificador() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        aparato.motor.parar(PLAZO_DE_ESPERA);

        planificador.disparar(tarea).await;

        assert!(
            planificador.esta_parado(),
            "un motor parado no detuvo al planificador: seguiría intentando encolar en cada tick"
        );
        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            0,
            "un disparo contra un motor parado creó una ejecución"
        );
    }

    /// 5 — Escenario 4: un error de base en un disparo no para el planificador.
    ///
    /// La base se cierra de verdad, así que `encolar` falla con su desenlace de base. Parar por esto
    /// dejaría al reloj caído por un bloqueo pasajero de la tabla —otro escritor, un instante—, y lo
    /// que el requisito pide es justo lo contrario: registrar y seguir, que el siguiente disparo se
    /// intente.
    #[tokio::test]
    async fn un_error_de_base_en_un_disparo_no_para_el_planificador() {
        let aparato = Aparato::nuevo().await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        aparato.pool.close().await;

        planificador.disparar(tarea).await;

        assert!(
            !planificador.esta_parado(),
            "un fallo de base en un tick paró al planificador: un bloqueo pasajero dejaría el reloj \
             caído"
        );
    }

    /// 4 — Escenario 1: llegada la hora, la tarea queda con una ejecución en `pending`.
    ///
    /// El tick no se espera —no se puede—: se llama a `disparar`, que es el cuerpo que el cierre de
    /// la biblioteca invoca. Y el `pending` se hace **observable** ocupando antes al trabajador del
    /// repositorio con una ejecución que tarda un segundo en fallar: la del disparo espera detrás en
    /// la cola y se puede leer su estado sin carrera.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn llegada_la_hora_la_tarea_queda_con_una_ejecucion_en_pending() {
        let aparato = Aparato::nuevo().await;
        let ocupante = aparato.tarea(CRON_DE_CINCO, true).await;
        let tarea = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        aparato
            .motor
            .encolar(ocupante)
            .await
            .expect("el ocupante tiene que entrar en su cola");

        planificador.disparar(tarea).await;

        assert_eq!(
            aparato.ejecuciones_de(&tarea).await,
            1,
            "el disparo no dejó una fila nueva en `job_executions`"
        );
        assert_eq!(
            aparato.estado_de_la_ejecucion(&tarea).await,
            "pending",
            "la ejecución del disparo no nació en `pending`"
        );

        // Se deja que las dos acaben antes de soltar el runtime.
        esperar_el_final(&aparato, &ocupante).await;
        esperar_el_final(&aparato, &tarea).await;
    }

    /// 4 — Escenario 2: el origen de la ejecución no es distinguible en la base.
    ///
    /// Dos mitades. La primera es el esquema: la tabla tiene exactamente las nueve columnas del §3
    /// —ninguna se llama de origen, disparador ni fuente—, así que en la base no hay dónde escribir
    /// de dónde vino una ejecución. La segunda es la fila: la que nace de un disparo del reloj y la
    /// que nace de uno manual, leídas al nacer, traen los mismos campos, y lo único que las distingue
    /// es de quién son y cuándo empezaron. Se ocupa antes al trabajador para poder leer las dos en
    /// `pending` sin carrera.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_origen_de_la_ejecucion_no_es_distinguible_en_la_base() {
        let aparato = Aparato::nuevo().await;
        let ocupante = aparato.tarea(CRON_DE_CINCO, true).await;
        let del_reloj = aparato.tarea(CRON_DE_CINCO, true).await;
        let a_mano = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        aparato
            .motor
            .encolar(ocupante)
            .await
            .expect("el ocupante tiene que entrar en su cola");

        planificador.disparar(del_reloj).await;
        aparato
            .motor
            .encolar(a_mano)
            .await
            .expect("el disparo manual tiene que entrar");

        let columnas = aparato.columnas_de_ejecuciones().await;
        assert_eq!(
            columnas,
            [
                "id",
                "job_id",
                "status",
                "snapshot_id",
                "bytes_processed",
                "bytes_uploaded",
                "error_message",
                "started_at",
                "finished_at",
            ],
            "la tabla `job_executions` no tiene las columnas del §3: una de más podría estar \
             diciendo de dónde vino la ejecución"
        );

        let del_reloj_fila = aparato.ejecucion_de(&del_reloj).await;
        let a_mano_fila = aparato.ejecucion_de(&a_mano).await;

        assert_eq!(
            del_reloj_fila.status,
            Estado::Pending,
            "la ejecución del reloj no nació en `pending`"
        );

        // Las dos filas, campo a campo, fuera de lo que las identifica. Si una llevara una marca de
        // origen, esta comparación sería lo primero que se rompería.
        assert_eq!(del_reloj_fila.status, a_mano_fila.status);
        assert_eq!(del_reloj_fila.snapshot_id, a_mano_fila.snapshot_id);
        assert_eq!(del_reloj_fila.bytes_processed, a_mano_fila.bytes_processed);
        assert_eq!(del_reloj_fila.bytes_uploaded, a_mano_fila.bytes_uploaded);
        assert_eq!(del_reloj_fila.error_message, a_mano_fila.error_message);
        assert_eq!(del_reloj_fila.finished_at, a_mano_fila.finished_at);
        assert_ne!(
            del_reloj_fila.id, a_mano_fila.id,
            "las dos ejecuciones de la prueba son la misma fila"
        );

        esperar_el_final(&aparato, &ocupante).await;
        esperar_el_final(&aparato, &del_reloj).await;
        esperar_el_final(&aparato, &a_mano).await;
    }

    /// 4 — Escenario 3: el nacimiento se publica en el hub como el de un disparo manual.
    ///
    /// El nacimiento lo publica `encolar`, no el planificador, así que un disparo del reloj se ve en
    /// el panel igual que uno del botón y no hace falta ningún evento nuevo. Se compara el evento del
    /// reloj con el de un disparo manual hecho en la misma prueba: los dos tienen que traer el mismo
    /// nombre y los mismos campos, con la única diferencia de a quién pertenecen.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_nacimiento_se_publica_en_el_hub_como_el_de_un_disparo_manual() {
        let aparato = Aparato::nuevo().await;
        let del_reloj = aparato.tarea(CRON_DE_CINCO, true).await;
        let a_mano = aparato.tarea(CRON_DE_CINCO, true).await;
        let planificador = aparato.planificador().await;

        let mut receptor = aparato
            .motor
            .hub()
            .suscribir()
            .expect("el hub no está cerrado en esta prueba");

        planificador.disparar(del_reloj).await;
        let eventos_del_reloj = recoger(&mut receptor);

        let manual = aparato
            .motor
            .encolar(a_mano)
            .await
            .expect("el disparo manual tiene que entrar");
        let eventos_a_mano = recoger(&mut receptor);

        let nacimiento_del_reloj = nacimiento(&eventos_del_reloj, &del_reloj).clone();
        let nacimiento_a_mano = nacimiento(&eventos_a_mano, &a_mano).clone();

        assert_eq!(
            nacimiento_a_mano.execution_id, manual.id,
            "el evento del disparo manual no es el de la ejecución que se encoló"
        );
        assert_eq!(
            nacimiento_del_reloj.status,
            Estado::Pending,
            "el nacimiento del disparo del reloj no llegó en `pending`"
        );
        assert!(
            !nacimiento_del_reloj.current_action.is_empty(),
            "el nacimiento del disparo del reloj no dice qué está haciendo la ejecución"
        );

        assert_eq!(
            nacimiento_del_reloj,
            EventoDeEjecucion {
                execution_id: nacimiento_del_reloj.execution_id,
                job_id: nacimiento_del_reloj.job_id,
                ..nacimiento_a_mano
            },
            "el nacimiento del disparo del reloj no trae los campos de uno manual: haría falta un \
             evento nuevo, y el requisito lo prohíbe"
        );

        esperar_el_final(&aparato, &del_reloj).await;
        esperar_el_final(&aparato, &a_mano).await;
    }
}
