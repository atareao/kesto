//! El aparato que comparten las pruebas del planificador.
//!
//! Existe para que [`super::planificacion`] y [`super::disparo`] no dupliquen la base, el motor y
//! las dos capas de acceso. Monta lo que el arranque de verdad monta —la base migrada, el motor ya
//! arrancado, un repositorio y un target para que las tareas tengan sus claves foráneas— y deja a
//! mano lo que las pruebas necesitan mirar: el pool, las dos capas y el motor.
//!
//! El abridor es uno que **no abre nada**: de lo que se ocupa el planificador es de **encolar**, y
//! el encolado no abre repositorio. Que el trabajador que recibe la ejecución falle después es cosa
//! del motor y no de lo que aquí se mide. Pero **tarda** en decir que no abre —[`LO_QUE_TARDA_EN_NO_ABRIR`]— y
//! esa tardanza es lo que hace observable el `pending` de un disparo: con un trabajador ocupado, la
//! ejecución siguiente espera en la cola y se puede leer su estado sin carrera. Es el mismo recurso
//! —y por el mismo motivo— que el `abridor_lento` de las pruebas del motor.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use time::OffsetDateTime;
use uuid::Uuid;

use super::planificacion::{Ajustes as AjustesDelPlanificador, Planificador};

use crate::db::entities::{
    AltaDeEjecucion, AltaDeRepositorio, AltaDeTarea, AltaDeTarget, Ejecucion, Ejecuciones, Estado,
    Repositorios, Tareas, Targets, TipoDeTarget,
};
use crate::engine::{Ajustes as AjustesDelMotor, Dependencias, Motor};
use crate::rustic::{Acceso, BarrasDelMotor, Error, PuestaEnMarcha, Repositorio};
use crate::secret::SecretString;
use crate::test_support::{
    CLAVE_SECRETA_DE_PRUEBA, DirectorioDePrueba, PASSPHRASE_DE_PRUEBA, base_con_clave,
    directorio_de_prueba,
};

/// El socket de un motor de contenedores que no existe.
///
/// El planificador no habla con el motor de contenedores —solo llama a `Motor::encolar`—, así que
/// el socket solo tiene que ser un valor y no una ruta real.
pub(crate) const SOCKET_QUE_NO_EXISTE: &str = "unix:///no/existe/kesto-planificador.sock";

/// A una expresión de cinco campos, la forma que usan las filas del repositorio.
pub(crate) const CRON_DE_CINCO: &str = "0 3 * * *";

/// Cuánto tarda el abridor de prueba en decir que no abre.
///
/// No es un adorno y no se puede acortar a cero: una ejecución recién encolada pasa por `pending`
/// mientras el trabajador de su repositorio está ocupado con la anterior, y sin esta demora el
/// trabajador la toma y la falla en el mismo instante, así que el `pending` no se puede observar
/// sin carrera. Un segundo es lo que dura el `abridor_lento` del motor, y sobra para leer una fila.
const LO_QUE_TARDA_EN_NO_ABRIR: Duration = Duration::from_secs(1);

/// Un abridor que no abre nada, y que tarda un segundo en decirlo.
fn abridor_que_no_abre(
    acceso: &Acceso,
    _barras: BarrasDelMotor,
) -> Result<(Repositorio, PuestaEnMarcha), Error> {
    std::thread::sleep(LO_QUE_TARDA_EN_NO_ABRIR);

    Err(Error::DestinoNoAlcanzable {
        destino: acceso.endpoint.clone(),
        motivo: "el abridor de esta prueba no abre nada".to_owned(),
    })
}

/// La base migrada, el motor arrancado y las dos capas de acceso que las pruebas usan.
pub(crate) struct Aparato {
    /// El directorio de la base: al soltarse se borra, y con él la base.
    _base: DirectorioDePrueba,
    /// El pool migrado, tal y como se le pasa al planificador.
    pub(crate) pool: SqlitePool,
    /// El motor, ya arrancado, como en el arranque de verdad.
    pub(crate) motor: Motor,
    /// La capa de acceso de `backup_jobs`.
    pub(crate) tareas: Tareas,
    /// La capa de acceso de `job_executions`.
    pub(crate) ejecuciones: Ejecuciones,
    /// El repositorio de las tareas de prueba.
    pub(crate) repositorio: Uuid,
    /// El target de las tareas de prueba.
    pub(crate) target: Uuid,
    /// El directorio del repositorio local: se retiene para no borrarlo.
    _destino: DirectorioDePrueba,
}

impl Aparato {
    /// Monta el aparato: la base migrada, las cuatro capas, el motor y las dos filas que las tareas
    /// necesitan para no ser huérfanas.
    pub(crate) async fn nuevo() -> Self {
        let (base, pool, clave) = base_con_clave().await;
        let destino_guardado = directorio_de_prueba();
        let destino = destino_guardado.ruta().to_string_lossy().into_owned();

        let repositorios = Repositorios::nuevo(pool.clone(), clave.clone());
        let targets = Targets::nuevo(pool.clone(), clave);

        let repositorio = repositorios
            .alta(AltaDeRepositorio {
                name: "repositorio-del-planificador".to_owned(),
                endpoint: destino.clone(),
                bucket: "cubo-de-prueba".to_owned(),
                region: "region-de-prueba".to_owned(),
                access_key_id: "clave-de-acceso-de-prueba".to_owned(),
                secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
                passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
            })
            .await
            .expect("el alta del repositorio no puede fallar")
            .id;

        let target = targets
            .alta(AltaDeTarget {
                name: "target-del-planificador".to_owned(),
                target_type: TipoDeTarget::Directory,
                container_name: None,
                db_user: None,
                db_password: None,
                db_name: None,
                host_path: Some(destino),
                db_path: None,
            })
            .await
            .expect("el alta del target no puede fallar")
            .id;

        let motor = Motor::arrancar(
            AjustesDelMotor::nuevos(SOCKET_QUE_NO_EXISTE).con_abridor(abridor_que_no_abre),
            Dependencias {
                repositorios: Arc::new(repositorios),
                targets: Arc::new(targets),
                tareas: Arc::new(Tareas::nuevo(pool.clone())),
                ejecuciones: Arc::new(Ejecuciones::nuevo(pool.clone())),
            },
        )
        .expect("el motor se monta desde dentro del runtime de la prueba");

        Self {
            _base: base,
            pool: pool.clone(),
            motor,
            tareas: Tareas::nuevo(pool.clone()),
            ejecuciones: Ejecuciones::nuevo(pool),
            repositorio,
            target,
            _destino: destino_guardado,
        }
    }

    /// Arranca el planificador sobre este aparato, como lo arranca el proceso.
    ///
    /// Es síncrono con el arranque de verdad —motor ya en marcha, mismo pool— y existe para que las
    /// dos pruebas que lo necesitan no repitan la llamada ni la elección de ajustes. El período es
    /// el de producción: estas pruebas **nunca** esperan a que transcurra, fuerzan la reconciliación
    /// a mano con [`Planificador::reconciliar`].
    pub(crate) async fn planificador(&self) -> Planificador {
        Planificador::arrancar(
            self.motor.clone(),
            self.pool.clone(),
            AjustesDelPlanificador::nuevos(),
        )
        .await
        .expect("el arranque del planificador no puede fallar en una prueba")
    }

    /// Da de alta una tarea con la expresión y el interruptor que se le digan.
    pub(crate) async fn tarea(&self, cron: &str, enabled: bool) -> Uuid {
        self.tareas
            .alta(AltaDeTarea {
                target_id: self.target,
                repository_id: self.repositorio,
                cron_expression: cron.to_owned(),
                keep_daily: 7,
                keep_weekly: 4,
                keep_monthly: 12,
                enabled,
            })
            .await
            .expect("el alta de la tarea no puede fallar")
            .id
    }

    /// Cambia el `enabled` de una tarea, como lo haría `PUT /api/v1/jobs/:id`.
    pub(crate) async fn cambiar_enabled(&self, tarea: &Uuid, enabled: bool) {
        let mut fila = self
            .tareas
            .leer(tarea)
            .await
            .expect("la lectura de la tarea no puede fallar")
            .expect("la tarea de la prueba tiene que estar");
        fila.enabled = enabled;

        assert!(
            self.tareas
                .actualizar(&fila)
                .await
                .expect("la edición de la tarea no puede fallar"),
            "el cambio de `enabled` tenía que quedar escrito"
        );
    }

    /// Cambia la `cron_expression` de una tarea, sin pasar por la API.
    pub(crate) async fn cambiar_cron(&self, tarea: &Uuid, cron: &str) {
        let mut fila = self
            .tareas
            .leer(tarea)
            .await
            .expect("la lectura de la tarea no puede fallar")
            .expect("la tarea de la prueba tiene que estar");
        fila.cron_expression = cron.to_owned();

        assert!(
            self.tareas
                .actualizar(&fila)
                .await
                .expect("la edición de la tarea no puede fallar"),
            "el cambio de expresión tenía que quedar escrito"
        );
    }

    /// Deja una ejecución `pending` de una tarea, como si ya estuviera en curso.
    ///
    /// Es lo que hace falta para provocar el `409` sin contenedor: la guarda de `encolar` mira las
    /// filas que ya hay para esa tarea, con independencia de que nadie las esté ejecutando.
    pub(crate) async fn dejar_una_en_curso(&self, tarea: &Uuid) {
        self.ejecuciones
            .alta(AltaDeEjecucion {
                job_id: *tarea,
                status: Estado::Pending,
                snapshot_id: None,
                bytes_processed: 0,
                bytes_uploaded: 0,
                error_message: None,
                started_at: OffsetDateTime::now_utc(),
                finished_at: None,
            })
            .await
            .expect("el alta de la ejecución de prueba no puede fallar");
    }

    /// Cuántas ejecuciones tiene una tarea, con SQL directo.
    pub(crate) async fn ejecuciones_de(&self, tarea: &Uuid) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM job_executions WHERE job_id = ?1")
            .bind(tarea.hyphenated().to_string())
            .fetch_one(&self.pool)
            .await
            .expect("el recuento de ejecuciones debe poder leerse")
    }

    /// El estado de la única ejecución de una tarea, con SQL directo.
    pub(crate) async fn estado_de_la_ejecucion(&self, tarea: &Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM job_executions WHERE job_id = ?1")
            .bind(tarea.hyphenated().to_string())
            .fetch_one(&self.pool)
            .await
            .expect("la fila de la ejecución tiene que existir")
    }

    /// La única ejecución de una tarea, leída por el camino normal de la capa de acceso.
    ///
    /// Es lo que hace falta para comparar la fila que nace de un disparo del reloj con la que nace
    /// de uno manual **campo a campo y con sus tipos**, sin componer la consulta dentro de la
    /// prueba. Se localiza por la tarea —de la que solo hay una ejecución en estas pruebas— y se
    /// lee con [`Ejecuciones::leer`], que es la misma puerta que usa el resto del crate.
    pub(crate) async fn ejecucion_de(&self, tarea: &Uuid) -> Ejecucion {
        let id: String = sqlx::query_scalar("SELECT id FROM job_executions WHERE job_id = ?1")
            .bind(tarea.hyphenated().to_string())
            .fetch_one(&self.pool)
            .await
            .expect("la fila de la ejecución tiene que existir");

        let id = Uuid::parse_str(&id).expect("el identificador guardado es un UUID con guiones");

        self.ejecuciones
            .leer(&id)
            .await
            .expect("la lectura de la ejecución no puede fallar")
            .expect("la ejecución que se acaba de localizar tiene que seguir ahí")
    }

    /// Los nombres de las columnas de `job_executions`, en el orden del §3.
    pub(crate) async fn columnas_de_ejecuciones(&self) -> Vec<String> {
        sqlx::query_scalar("SELECT name FROM pragma_table_info('job_executions')")
            .fetch_all(&self.pool)
            .await
            .expect("las columnas de job_executions deben poder leerse")
    }
}
