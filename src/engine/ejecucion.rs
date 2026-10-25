//! El ciclo de vida de una ejecución: de `pending` a `success` o a `failed`, y **por qué la
//! condición de éxito no se hereda de nadie**.
//!
//! El motor es el **único** componente que escribe en `job_executions`, y lo hace aquí: este
//! módulo recorre una ejecución de principio a fin —leer la tarea, abrir el repositorio, copiar,
//! retener, escribir la fila— y decide si acabó bien.
//!
//! # La condición de éxito se compone aquí, y el `Ok` del archivado no forma parte de ella
//!
//! Es el hallazgo más grave del change y está medido: el archivado de la biblioteca
//! **silencia** el error del lector, sigue y devuelve un resultado correcto **guardando la
//! instantánea sin el volcado dentro**, y en el peor caso medido sin una sola línea de aviso.
//! Heredar el éxito de ese `Ok` es exactamente el fallo que este módulo existe para no cometer:
//! daría por buena una copia que existe, ocupa sitio y no se puede restaurar.
//!
//! Las tres comprobaciones las hace [`el_volcado_se_entrego`], y **ninguna es el `Ok` del
//! archivado**: el volcado se entregó entero
//! ([`crate::rustic::EntregaDelVolcado::completa`]), el proceso de extracción salió con cero, y la
//! instantánea tiene el volcado dentro (`ficheros >= 1`). La tercera es la que sirve de los dos
//! números que el archivado cuenta —`bytes` vale `0` tanto en una copia truncada como en una
//! legítimamente vacía, así que no distingue un fallo de un éxito, y `ficheros` sí, medido en las
//! dos direcciones—.
//!
//! # Y un directorio tiene un límite conocido, que va escrito en vez de fingido
//!
//! Para un target de tipo `directory` no hay puente ni señal de entrega: la copia es la del
//! origen local de la biblioteca, y la condición es que `archivar_directorio` devuelva `Ok`. Lo
//! que eso **no** cubre: el archivado silencia con un aviso el fallo de lectura de un fichero
//! concreto y sigue, así que una copia de directorio puede no llevar un fichero que se volvió
//! ilegible a mitad. Para un volcado eso lo caza la señal de la entrega y para un directorio no
//! hay señal equivalente: queda declarado aquí y en `docs/FEATURE-005-backup-engine.md`, en vez de
//! fingir una comprobación que no existe.
//!
//! # El pánico del runtime está medido, y no puede llevarse al trabajador
//!
//! Llamar a Rustic desde dentro de un runtime entra en **pánico** —«Cannot start a runtime from
//! within a runtime.»—, así que el repositorio se abre **fuera de todo `block_on`**: el hilo del
//! trabajador no tiene runtime propio, entra en el de la aplicación **solo** para la base
//! ([`Contexto::en_la_base`]) y llama a la biblioteca fuera de él. El candado de esa regla es una
//! prueba de este módulo; el cierre, por si alguien la rompe, está en el bucle del trabajador
//! (`super::cola`): un pánico que llegara a subir deja la ejecución en `failed` en vez de quedarse
//! en `running` para siempre.
//!
//! # El motivo que se guarda: recortado y sin credenciales
//!
//! `error_message` lo muestra el panel y se guarda en la base, así que es texto que viene de
//! fuera del proceso —el `stderr` de un `exec`, un error de la biblioteca— y pasa por la red que
//! ya existe en [`super::extraccion`]: la redacción de los valores del entorno y el recorte. No se
//! reimplementa aquí ninguna de las dos, porque dos redes para lo mismo es una que se queda atrás
//! el día que la otra se amplía.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use time::OffsetDateTime;
use uuid::Uuid;

use super::extraccion::{
    CAPACIDAD_DEL_CANAL, ErrorDeExtraccion, INACTIVIDAD_MAXIMA, Objetivo, Resultado, Ruta,
    motivo_para_guardar, recortar, ruta, volcar,
};
use super::hub::{EventoDeEjecucion, Hub};
use super::{Ajustes, Contexto};
use crate::db::entities::{Ejecucion, Estado};
use crate::docker::{Cliente, Orden};
use crate::rustic::{
    Acceso, AvanceDelVolcado, AvisoDeProgreso, BarrasDelMotor, Copia, CopiaHecha,
    ProductorDelVolcado, PuestaEnMarcha, Retencion, TunelDelVolcado, tunel_del_volcado,
};

// ── Las acciones que el panel pinta ──────────────────────────────────────────
//
// Son texto de pantalla, así que van en español, y son **constantes de este módulo** para que no
// haya literales sueltos: el panel se escribe contra estas frases y una de ellas escrita a mano en
// otro sitio es una frase distinta que nadie ve hasta que está en producción. Ninguna lleva nada
// que venga del contenedor ni de una fila del §3.

/// Acción de una ejecución recién encolada.
pub(crate) const ACCION_ENCOLADA: &str = "encolada";
/// Acción de la ejecución que está buscando su tarea, su target y su repositorio.
const ACCION_LEYENDO_LA_FILA: &str = "leyendo la tarea, el target y el repositorio";
/// Acción de la ejecución que está abriendo el repositorio de copias.
const ACCION_ABRIENDO_EL_REPOSITORIO: &str = "abriendo el repositorio";
/// Acción de la ejecución que está sacando el volcado del contenedor.
const ACCION_EXTRAYENDO_EL_VOLCADO: &str = "extrayendo el volcado";
/// Acción de la ejecución que está archivando —un volcado o un directorio—.
const ACCION_ARCHIVANDO: &str = "archivando";
/// Acción de la ejecución que está aplicando la retención.
const ACCION_RETENIENDO: &str = "aplicando la retención";
/// Acción de la ejecución que está escribiendo su final.
pub(crate) const ACCION_TERMINANDO: &str = "terminando";

/// El nombre del equipo que la instantánea graba.
///
/// Es una constante y **no** el nombre de la máquina: la retención agrupa por host, etiqueta y
/// rutas, y el host que aquí se graba es el mismo corra donde corra el contenedor. Con el nombre
/// de la máquina, mover el contenedor de servidor partiría el historial en dos grupos de retención
/// y ninguna de las dos reglas vería las copias de la otra.
const HOST_DEL_MOTOR: &str = "kesto";

/// El nombre que se le da al volcado cuando el del target no deja nada utilizable.
///
/// Un nombre de volcado no puede quedar vacío —la biblioteca entra en **pánico** con un camino
/// vacío, no devuelve un error—, así que un target cuyo nombre no sobreviva al saneado se copia
/// igualmente, con este nombre.
const NOMBRE_DE_VOLCADO_POR_DEFECTO: &str = "volcado";

/// Bytes que tienen que pasar entre dos avisos de progreso, como mínimo, para publicar otro.
///
/// La unidad importa: un volcado de 20 MB llega en unos doce mil trozos, así que publicar uno por
/// trozo serían doce mil eventos para pintar una barra. Con un mebibyte, un volcado grande publica
/// unas decenas de avisos: suficiente para que la barra avance con suavidad y despreciable frente
/// al trabajo de copiar.
const UMBRAL_DE_AVISO: u64 = 1024 * 1024;

/// Tiempo máximo entre dos avisos, aunque el umbral de bytes no se haya alcanzado.
///
/// Es la mitad que impide un panel congelado: un volcado de poco caudal puede tardar minutos en
/// acumular un mebibyte, y sin esta segunda condición no publicaría nada en todo ese rato. Con
/// ella, lo que se publica es el avance real de cada segundo, que es lo que el operador necesita
/// para distinguir «va despacio» de «se ha colgado».
const INTERVALO_MINIMO_DE_AVISO: Duration = Duration::from_secs(1);

/// Ejecuta una ejecución **en el hilo del trabajador**, de principio a fin.
///
/// Es el único punto de entrada de este módulo, y **nunca deja la fila en `running`**: cada camino
/// de error escribe `failed` con su motivo y su `finished_at`. El único desenlace que se sale de
/// esa regla es no poder hablar con la base, y entonces no hay nada que escribir en ninguna parte
/// —se registra y se sigue, que es todo lo que se puede hacer—.
pub(crate) fn ejecutar(contexto: &Contexto, fila: &Ejecucion) {
    // 1. La ejecución pasa a `running`, entrando en el runtime de la aplicación **solo** para la
    //    base. Y se publica la transición: el panel tiene que ver el cambio de estado aunque el
    //    proceso muera en el paso siguiente.
    let en_curso = match marcar_en_curso(contexto, fila) {
        Ok(en_curso) => en_curso,
        Err(causa) => {
            tracing::error!(
                execution_id = %fila.id,
                %causa,
                "no se ha podido pasar la ejecución a `running`: se queda como estaba"
            );
            return;
        }
    };

    let en_vivo = Arc::new(EnVivo::nueva(contexto, &en_curso));
    en_vivo.accion(ACCION_LEYENDO_LA_FILA);
    en_vivo.publicar();

    // 2 a 7: la copia entera.
    let cierre = correr(contexto, &en_curso, &en_vivo);

    // 8. La fila, con lo que se haya medido: los contadores se escriben también cuando la copia
    //    falla, porque lo que hay que contar es lo que llegó a pasar. `started_at` no se toca —es
    //    la fecha de nacimiento de la fila y la capa de acceso no la acepta en una actualización—.
    en_vivo.accion(ACCION_TERMINANDO);
    escribir_el_final(contexto, &en_curso, &cierre, en_vivo.cifras());

    // 9. Y el evento final, con el estado que acaba de escribirse.
    contexto
        .hub
        .publicar(en_vivo.evento(cierre.estado, ACCION_TERMINANDO));
}

/// Escribe el final de una ejecución en su fila.
///
/// Va aparte para que el camino del pánico —que vive en el bucle del trabajador— escriba lo mismo
/// sin duplicar la conversión de contadores ni la fecha.
fn escribir_el_final(contexto: &Contexto, fila: &Ejecucion, cierre: &Cierre, cifras: Cifras) {
    let mut final_de_la_fila = fila.clone();
    final_de_la_fila.status = cierre.estado;
    final_de_la_fila.snapshot_id = cierre.snapshot_id.clone();
    final_de_la_fila.error_message = cierre.motivo.clone();
    final_de_la_fila.bytes_processed = como_entero(cifras.procesados);
    final_de_la_fila.bytes_uploaded = como_entero(cifras.subidos);
    final_de_la_fila.finished_at = Some(OffsetDateTime::now_utc());

    match contexto.en_la_base(
        contexto
            .dependencias
            .ejecuciones
            .actualizar(&final_de_la_fila),
    ) {
        Ok(true) => {}
        // La fila ya no está: la borró alguien entre el encolado y el final. No es un fallo de la
        // copia —la copia ocurrió— y desde luego no se inventa una fila para contarla.
        Ok(false) => tracing::warn!(
            execution_id = %final_de_la_fila.id,
            "la ejecución ha terminado y su fila ya no estaba: no hay dónde escribir el final"
        ),
        Err(causa) => tracing::error!(
            execution_id = %final_de_la_fila.id,
            %causa,
            "no se ha podido escribir el final de la ejecución"
        ),
    }
}

/// Marca una ejecución como **fallida** si no ha terminado ya.
///
/// La usan los dos caminos en los que el motor descubre un fallo **fuera** del ciclo normal de la
/// copia: el encolado que no llegó a su cola —una fila `pending` que nadie va a ejecutar es peor
/// que un error— y el pánico del trabajador. El «si no ha terminado ya» no es una precaución de
/// más: el pánico puede llegar **después** de que la fila se haya escrito —un fallo en un `Drop`,
/// por ejemplo— y reescribir un `success` convertiría una copia buena en un fallo sin motivo.
pub(crate) fn fallar_si_no_termino(contexto: &Contexto, id: &Uuid, motivo: &str) {
    let fila = match contexto.en_la_base(contexto.dependencias.ejecuciones.leer(id)) {
        Ok(Some(fila)) => fila,
        Ok(None) => {
            tracing::warn!(%id, "la ejecución que se quería marcar como fallida ya no está");
            return;
        }
        Err(causa) => {
            tracing::error!(
                %id,
                %causa,
                "no se ha podido leer la ejecución para marcarla como fallida"
            );
            return;
        }
    };

    if matches!(fila.status, Estado::Success | Estado::Failed) {
        return;
    }

    escribir_el_final(
        contexto,
        &fila,
        &Cierre::fallida(motivo.to_owned()),
        Cifras::nada(),
    );
}

/// Pasa una ejecución a `running` en la base y devuelve la fila tal y como queda.
///
/// Devuelve la fila y no la escribe a medias para que el resto del ciclo trabaje sobre lo que
/// **quedó escrito**: lo que se lee de aquí es lo que la base tiene, no lo que se creía tener.
fn marcar_en_curso(contexto: &Contexto, fila: &Ejecucion) -> Result<Ejecucion, crate::db::DbError> {
    let mut en_curso = fila.clone();
    en_curso.status = Estado::Running;

    contexto.en_la_base(contexto.dependencias.ejecuciones.actualizar(&en_curso))?;

    Ok(en_curso)
}

/// El recorrido de la copia: de la fila a un [`Cierre`].
///
/// Va aparte de [`ejecutar`] porque lo que pasa **antes** del veredicto —leer, abrir, copiar,
/// retener— no necesita saber cómo se escribe un final, y al revés: el que escribe no necesita
/// saber por dónde salió la copia.
fn correr(contexto: &Contexto, fila: &Ejecucion, en_vivo: &Arc<EnVivo>) -> Cierre {
    let ajustes = &contexto.ajustes;

    // 2. La tarea, su target y su repositorio. Cualquiera de las tres puede haber desaparecido
    //    entre el encolado y la ejecución —una fila borrada a mano, con su `ON DELETE CASCADE`
    //    llevándose lo de detrás—, así que el motivo tiene que decir **cuál** falta: «no se pudo
    //    leer» manda al operador a mirar la base sin decirle qué buscar.
    let tarea = match leer_la_tarea(contexto, &fila.job_id) {
        Ok(tarea) => tarea,
        Err(motivo) => return Cierre::fallida(motivo),
    };

    let target = match leer_el_target(contexto, &tarea.target_id) {
        Ok(target) => target,
        Err(motivo) => return Cierre::fallida(motivo),
    };

    let repositorio = match leer_el_repositorio(contexto, &tarea.repository_id) {
        Ok(repositorio) => repositorio,
        Err(motivo) => return Cierre::fallida(motivo),
    };

    // 3. Por dónde sale la copia. Un objetivo incoherente —un directorio sin ruta, una base sin
    //    usuario— se cuenta aquí y no se intenta arreglar.
    let objetivo = Objetivo::from(&target);
    let por_donde = match ruta(&objetivo) {
        Ok(por_donde) => por_donde,
        Err(causa) => return Cierre::fallida(recortar(&causa.to_string())),
    };

    // El acceso, con los dos secretos **ya descifrados** por la capa de acceso. Este módulo no
    // recibe la clave maestra: tenerla sería un segundo sitio del programa capaz de descifrar
    // credenciales, que es justo lo que el cifrado en reposo del §3 quiere evitar.
    let acceso = Acceso {
        endpoint: repositorio.endpoint.clone(),
        bucket: repositorio.bucket.clone(),
        region: repositorio.region.clone(),
        access_key_id: repositorio.access_key_id.clone(),
        secret_access_key: repositorio.secret_access_key.clone(),
        passphrase: repositorio.passphrase.clone(),
    };

    // El nombre del nodo y el de la copia son **la misma cadena** a partir de aquí. El puente lo
    // comprueba antes de archivar y rechaza la copia si no coinciden: la instantánea declara las
    // rutas que le da la copia y el nodo lo nombra el puente, así que dos nombres distintos dejan
    // una copia que dice contener lo que no tiene.
    let nombre = nombre_del_volcado(&target.name);
    let etiqueta = fila.job_id.hyphenated().to_string();
    let retencion = Retencion {
        keep_daily: tarea.keep_daily,
        keep_weekly: tarea.keep_weekly,
        keep_monthly: tarea.keep_monthly,
    };

    // 4. El repositorio, **fuera de todo `block_on`** y **por ejecución**: medido, reutilizarlo
    //    entre dos copias desactiva la deduplicación por completo —el índice en memoria no
    //    contiene el árbol que la copia anterior acaba de escribir—, así que un repositorio de
    //    larga vida no ahorra trabajo: lo esconde.
    //
    //    Y fuera de `block_on` porque llamar a Rustic desde dentro de un runtime entra en pánico
    //    —«Cannot start a runtime from within a runtime.»—. Aquí no se está dentro de ninguno:
    //    este hilo no tiene runtime propio y solo pisa el de la aplicación para la base.
    en_vivo.accion(ACCION_ABRIENDO_EL_REPOSITORIO);
    en_vivo.publicar();

    let barras = en_vivo.barras_de_esta_ejecucion();

    let (copia_del_repositorio, puesta_en_marcha) = match (ajustes.abridor)(&acceso, barras) {
        Ok(abierto) => abierto,
        Err(causa) => {
            return Cierre::fallida(motivo_de_la_biblioteca(&causa.to_string(), &acceso));
        }
    };

    // El desenlace se registra con su traza: «no existía, lo he creado» y «existía y he conectado»
    // no se leen igual —un destino recién creado está vacío y la primera copia que llegue será la
    // primera del repositorio; uno que ya existía tiene un historial que esta copia viene a
    // continuar—.
    match puesta_en_marcha {
        PuestaEnMarcha::Inicializado => tracing::info!(
            endpoint = %acceso.endpoint,
            "el destino no tenía repositorio y se ha creado con esta copia"
        ),
        PuestaEnMarcha::Conectado => tracing::info!(
            endpoint = %acceso.endpoint,
            "el destino ya tenía repositorio y se ha conectado con él"
        ),
    }

    // 5. La copia, según por dónde salga.
    let hecho = match por_donde {
        Ruta::PorFichero { ruta } => copiar_el_directorio(
            &copia_del_repositorio,
            &acceso,
            &ruta,
            &nombre,
            &etiqueta,
            en_vivo,
        ),
        Ruta::PorContenedor { contenedor, orden } => copiar_el_volcado(
            &copia_del_repositorio,
            &acceso,
            &contenedor,
            orden,
            &nombre,
            &etiqueta,
            en_vivo,
            ajustes,
        ),
    };

    let Ok(hecho) = hecho else {
        return hecho.unwrap_err();
    };

    // 7. La retención, con los tres `keep_*` de la tarea. Un fallo **no cambia el resultado de la
    //    copia**: la copia ya está hecha y es correcta, y lo que hay que contar es que la copia
    //    está y la retención no. Por eso se registra y se sigue.
    en_vivo.accion(ACCION_RETENIENDO);
    en_vivo.publicar();

    if let Err(causa) = copia_del_repositorio.aplicar_retencion(&retencion) {
        tracing::warn!(
            %etiqueta,
            %causa,
            "la copia ha terminado bien y la retención ha fallado: lo que hay que mirar es la \
             retención, no la copia"
        );
    }

    Cierre::hecha(&hecho)
}

/// Lee la tarea de la ejecución, o el motivo por el que no está.
fn leer_la_tarea(contexto: &Contexto, job_id: &Uuid) -> Result<crate::db::entities::Tarea, String> {
    match contexto.en_la_base(contexto.dependencias.tareas.leer(job_id)) {
        Ok(Some(tarea)) => Ok(tarea),
        Ok(None) => Err(format!(
            "la tarea `{job_id}` ya no está: la ejecución se encoló para una fila que se ha \
             borrado entre el encolado y la ejecución"
        )),
        Err(causa) => Err(format!("la tarea `{job_id}` no se ha podido leer: {causa}")),
    }
}

/// Lee el target de la tarea, o el motivo por el que no está.
fn leer_el_target(
    contexto: &Contexto,
    target_id: &Uuid,
) -> Result<crate::db::entities::Target, String> {
    match contexto.en_la_base(contexto.dependencias.targets.leer(target_id)) {
        Ok(Some(target)) => Ok(target),
        Ok(None) => Err(format!(
            "el target `{target_id}` de la tarea ya no está: sin él no hay nada que copiar"
        )),
        Err(causa) => Err(format!(
            "el target `{target_id}` no se ha podido leer: {causa}"
        )),
    }
}

/// Lee el repositorio de la tarea, o el motivo por el que no está.
fn leer_el_repositorio(
    contexto: &Contexto,
    repository_id: &Uuid,
) -> Result<crate::db::entities::Repositorio, String> {
    match contexto.en_la_base(contexto.dependencias.repositorios.leer(repository_id)) {
        Ok(Some(repositorio)) => Ok(repositorio),
        Ok(None) => Err(format!(
            "el repositorio `{repository_id}` de la tarea ya no está: sin él no hay dónde copiar"
        )),
        Err(causa) => Err(format!(
            "el repositorio `{repository_id}` no se ha podido leer: {causa}"
        )),
    }
}

/// Copia un directorio del sistema de ficheros: **sin Docker, sin túnel y sin productor**.
///
/// Es el camino de un target de tipo `directory`, que va contra la ruta que el `compose` del §8
/// monta dentro del contenedor de Kesto. Que este camino no pase por Docker es lo que hace que un
/// directorio se copie aunque el socket del motor esté caído.
///
/// Su límite conocido —el archivado silencia con un aviso el fallo de lectura de un fichero
/// concreto y sigue— está escrito en el encabezado del módulo.
fn copiar_el_directorio(
    repositorio: &crate::rustic::Repositorio,
    acceso: &Acceso,
    ruta: &Path,
    nombre: &str,
    etiqueta: &str,
    en_vivo: &Arc<EnVivo>,
) -> Result<CopiaHecha, Cierre> {
    // Las rutas de la copia son las del `host_path` **tal cual**: son lo que el origen local lee y
    // lo que la instantánea declara contener. El nombre se informa igual aunque este camino no lo
    // use como ruta —`archivar_directorio` no archiva un nodo con él, archiva el árbol del
    // directorio—: la instantánea es la misma clase de cosa, y el nombre es el del volcado que esa
    // tarea produce, que es lo que hace que dos copias de la misma tarea se lean igual.
    let copia = Copia {
        rutas: vec![ruta.to_path_buf()],
        nombre: nombre.to_owned(),
        etiqueta: etiqueta.to_owned(),
        host: HOST_DEL_MOTOR.to_owned(),
        // La procedencia: una frase que dice qué se copió. **Nunca** el `argv` de Kesto, que es lo
        // que la biblioteca graba por su cuenta cuando el campo no se informa.
        comando: format!("copia del directorio {}", ruta.display()),
    };

    en_vivo.accion(ACCION_ARCHIVANDO);
    en_vivo.publicar();

    repositorio
        .archivar_directorio(&copia)
        .map_err(|causa| Cierre::fallida(motivo_de_la_biblioteca(&causa.to_string(), acceso)))
}

/// Copia un volcado: el `exec`, el túnel, el archivado y **las tres comprobaciones**.
///
/// El orden no es negociable y es el que fija el puente: el productor en su hilo, el archivado en
/// éste, la unión al final y, **después**, la consulta de la entrega. Un productor que corriera en
/// el hilo del archivado se bloquearía en su primer `enviar` en cuanto el canal se llenara,
/// esperando a un consumidor que estaría esperándole.
#[allow(clippy::too_many_arguments)]
fn copiar_el_volcado(
    repositorio: &crate::rustic::Repositorio,
    acceso: &Acceso,
    contenedor: &str,
    orden: Orden,
    nombre: &str,
    etiqueta: &str,
    en_vivo: &Arc<EnVivo>,
    ajustes: &Ajustes,
) -> Result<CopiaHecha, Cierre> {
    // El túnel, antes de archivar nada: es lo que da el avance del productor, que el motor publica
    // en vivo.
    let tunel = tunel_del_volcado(nombre.to_owned(), CAPACIDAD_DEL_CANAL, INACTIVIDAD_MAXIMA)
        .map_err(|causa| Cierre::fallida(recortar(&causa.to_string())))?;

    let TunelDelVolcado {
        origen,
        productor,
        entrega,
        avance,
    } = tunel;

    // El avance del productor se conecta aquí: hasta esta línea no había ningún productor del que
    // leer, y el panel se enteró igual porque las transiciones de estado se publican siempre.
    en_vivo.conectar_el_volcado(avance);

    let copia = Copia {
        // Un volcado es **un solo** nodo, y su nombre es el que el puente fabrica dentro del
        // árbol: declarar aquí otra cosa dejaría una instantánea que dice contener lo que no tiene.
        rutas: vec![PathBuf::from(nombre)],
        nombre: nombre.to_owned(),
        etiqueta: etiqueta.to_owned(),
        host: HOST_DEL_MOTOR.to_owned(),
        // La procedencia es la orden: lo que hace la copia. **No** lleva la contraseña —va por el
        // entorno del `exec`, que no se toca— y **no** es el `argv` de Kesto.
        comando: orden.cmd.join(" "),
    };

    en_vivo.accion(ACCION_EXTRAYENDO_EL_VOLCADO);
    en_vivo.publicar();

    // El productor, a su hilo, con **su** runtime: el `exec` y su drenaje tienen que compartir el
    // runtime del hilo que los lanza —medido: si ese runtime muere mientras se drena, el drenaje
    // falla con «A Tokio 1.x context was found, but it is being shutdown»— y con **dos** hilos de
    // trabajo, porque con uno solo cualquier otra tarea de ese runtime —empezando por el plazo que
    // el transporte pone alrededor de cada trozo— se queda sin poll mientras el cierre bloquea, y
    // entonces el plazo no puede vencer.
    let hilo_del_productor = lanzar_el_productor(
        ajustes.socket_del_motor.clone(),
        ajustes.espera_del_motor,
        contenedor.to_owned(),
        orden,
        productor,
    )
    .map_err(|motivo| Cierre::fallida(recortar(&motivo)))?;

    en_vivo.accion(ACCION_ARCHIVANDO);
    en_vivo.publicar();

    let archivado = repositorio.archivar_volcado(&copia, &origen);

    // La unión **antes** de decidir nada: la entrega completa la marca el lector dentro del
    // archivado, y el código de salida solo lo tiene el hilo del productor.
    let vuelta = hilo_del_productor.join();

    let hecho = match archivado {
        Ok(hecho) => hecho,
        Err(causa) => {
            return Err(Cierre::fallida(motivo_de_la_biblioteca(
                &causa.to_string(),
                acceso,
            )));
        }
    };

    let resultado = match vuelta {
        Ok(Ok(resultado)) => resultado,
        // El productor llegó a su término con un error propio de la extracción: no hay hechos que
        // contar y su motivo —ya redactado y recortado en el hilo del productor, que es el que
        // tiene la orden— es el mejor que se puede guardar.
        Ok(Err(motivo)) => return Err(Cierre::fallida(motivo)),
        // El hilo del productor se cayó con un pánico. No se publica su contenido —el texto de un
        // pánico es arbitrario y podría llevar cualquier cosa—, pero tampoco se calla.
        Err(_) => {
            return Err(Cierre::fallida(recortar(
                "el hilo que extraía el volcado se ha caído sin dejar hechos: la copia no se \
                 puede dar por buena",
            )));
        }
    };

    // ⚠️ Aquí está la condición de éxito, y **no** es el `Ok` del archivado —que ya se tiene y no
    // se mira—: el archivado silencia el error del lector, sigue y guarda la instantánea sin el
    // volcado dentro, y en el peor caso medido sin una sola línea de aviso. Heredar ese `Ok` da
    // por buena una copia que existe, ocupa sitio y no se puede restaurar.
    match el_volcado_se_entrego(entrega.completa(), resultado.codigo, hecho.ficheros) {
        Ok(()) => Ok(hecho),
        Err(fallo) => Err(Cierre::fallida(motivo_del_fallo(
            fallo,
            &resultado,
            hecho.ficheros,
        ))),
    }
}

/// Lanza el hilo del productor, que es quien conduce el `exec`.
///
/// Devuelve el `JoinHandle` o el motivo por el que no se pudo lanzar. El hilo se lleva la orden,
/// el nombre del contenedor, el socket y el productor del puente: **nada** de eso se queda aquí.
///
/// # El runtime es de este hilo, y por eso se construye dentro
///
/// El productor necesita un runtime para drenar el `stream` del `exec`, y **no** puede usar el de
/// la aplicación: lo que hace es bloquearse esperando hueco en un canal acotado —la contrapresión
/// del puente—, y un `block_on` sobre el runtime que atiende las peticiones HTTP detendría el hilo
/// que despacha y, con él, el resto de la aplicación. Así que el hilo construye el suyo, lo usa
/// para todo el volcado y muere con él.
fn lanzar_el_productor(
    socket: String,
    espera: Duration,
    contenedor: String,
    orden: Orden,
    productor: ProductorDelVolcado,
) -> Result<std::thread::JoinHandle<VueltaDelProductor>, String> {
    std::thread::Builder::new()
        .name(format!("kesto-volcado-{contenedor}"))
        .spawn(move || producir(socket, espera, contenedor, orden, productor))
        .map_err(|causa| format!("no se ha podido lanzar el hilo del volcado: {causa}"))
}

/// Lo que devuelve el hilo del productor: los hechos, o el motivo por el que no los hay.
///
/// El motivo es un [`String`] y no el error de la extracción porque cruza un `join`, que no admite
/// otra cosa; y se construye con la red de siempre —recorte y redacción— para que lo que se guarde
/// en la fila no lleve credenciales ni texto de más.
type VueltaDelProductor = Result<Resultado, String>;

/// Conduce el volcado: **aquí** se construye el runtime del productor y aquí se drena el `exec`.
///
/// El cliente se construye **dentro** de este runtime y **por ejecución**, y las dos cosas son
/// deliberadas: el `exec` y su drenaje tienen que compartir el runtime del hilo que los lanza
/// —está medido—, y este runtime es de la ejecución y muere con ella. El precio, que hay que
/// saber: no hay agrupador de conexiones compartido entre copias. Lo que se gana: que una copia no
/// pueda dejar el runtime de la aplicación esperando a nada.
fn producir(
    socket: String,
    espera: Duration,
    contenedor: String,
    orden: Orden,
    productor: ProductorDelVolcado,
) -> VueltaDelProductor {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(causa) => {
            return Err(recortar(&format!(
                "no se ha podido crear el runtime del volcado: {causa}"
            )));
        }
    };

    runtime.block_on(async move {
        let cliente =
            Cliente::conectar(&socket, espera).map_err(|causa| recortar(&causa.to_string()))?;

        volcar(&cliente, &contenedor, &orden, productor, INACTIVIDAD_MAXIMA)
            .await
            .map_err(|causa| motivo_del_volcado(&causa, &orden))
    })
}

/// El techo de los contadores que se escriben en la fila.
///
/// Las columnas del §3 son `INTEGER` y lo que se cuenta son bytes, así que la conversión desde
/// `u64` es real y no formal: un `as i64` sobre un valor por encima del máximo daría un número
/// **negativo** —un contador que dice que la copia procesó bytes al revés—, que es peor que un
/// techo. El techo es inalcanzable en la práctica —nueve exabytes— y aun así está escrito, porque
/// el día que se alcance el problema no es el número grande: es el signo.
fn como_entero(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

/// El nombre del nodo y del fichero de un volcado, saneado a partir del nombre del target.
///
/// # Por qué se sanea
///
/// El nombre hace de vez de **dos** cosas a la vez: es el `path` de la entrada que se le entrega a
/// la biblioteca y el nombre del nodo dentro del árbol de la instantánea. Y un nombre de target lo
/// escribe un operador, así que puede traer un espacio, un paréntesis o una barra. Con un
/// separador dentro, el nombre deja de ser un nombre y pasa a ser un camino —la entrada se
/// partiría en trozos y el nodo llevaría un separador dentro de su propio nombre—, de modo que la
/// instantánea declararía contener algo que no tiene: el mismo fallo que el resto del módulo evita,
/// en versión de metadatos.
///
/// Lo que no sea letra, dígito, `.`, `-` o `_` se sustituye por `_`, y se le añade la extensión
/// `.sql`. Solo ASCII: el nombre acaba dentro de una instantánea que leen otras herramientas, y un
/// nombre en ASCII no plantea ninguna pregunta de codificación.
///
/// Un nombre que no deje ni un carácter —o que sea de solo espacios— no puede quedarse vacío: un
/// camino vacío es un **pánico** dentro de la biblioteca, no un error devuelto. En ese caso se usa
/// [`NOMBRE_DE_VOLCADO_POR_DEFECTO`]. No se recorta por dentro nada más: recortar el nombre de un
/// target sería cambiar en silencio lo que el operador escribió.
fn nombre_del_volcado(del_target: &str) -> String {
    let saneado: String = del_target
        .chars()
        .map(|caracter| {
            if caracter.is_ascii_alphanumeric() || matches!(caracter, '.' | '-' | '_') {
                caracter
            } else {
                '_'
            }
        })
        .collect();

    let base = if saneado.is_empty() {
        NOMBRE_DE_VOLCADO_POR_DEFECTO
    } else {
        &saneado
    };

    format!("{base}.sql")
}

/// Por qué **no** se dio por bueno un volcado.
///
/// Es un enumerado y no un `bool` porque cada desenlace nombra una cosa distinta y el motivo que
/// acaba en la fila tiene que decir cuál: «no se entregó entero» manda a mirar el contenedor,
/// «salió con 3» a mirar el comando, y «no llegó al árbol» a mirar el archivado. Un `false` a
/// secas obligaría al operador a adivinar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FalloDelVolcado {
    /// El productor cerró el canal sin declarar el final, o el lector no llegó a él.
    NoSeEntregoEntero,
    /// El proceso de extracción salió con un código distinto de cero.
    ElProcesoFallo {
        /// El código con el que salió.
        codigo: i32,
    },
    /// El archivado terminó sin error y la instantánea **no tiene** el volcado dentro.
    SinElVolcadoDentro,
}

/// **La condición de éxito de un volcado**, que no se hereda del archivado.
///
/// Las tres comprobaciones, y el porqué de cada una:
///
/// 1. **`entrega_completa`**: el único sitio del sistema que sabe si el volcado llegó entero es la
///    señal del puente. Medido: el error del lector **no** sirve de señal —la biblioteca lo
///    descarta con un aviso y `archive()` devuelve `Ok` igualmente—, así que sin esto un volcado
///    que murió a mitad y una copia buena son indistinguibles desde fuera.
/// 2. **`codigo == 0`**: el proceso de extracción tiene que haber salido bien. Es lo que ata la
///    condición al éxito del comando y no a «el archivado no se quejó».
/// 3. **`ficheros >= 1`**: la instantánea tiene que tener el volcado dentro. De los dos números
///    que el archivado cuenta, este es el que sirve, y está medido en las dos direcciones: un
///    volcado truncado deja `0` ficheros —el archivado descarta la entrada que falló— y uno
///    legítimamente vacío deja `1` con `bytes == 0`. `bytes` **no** sirve: vale `0` en los dos
///    casos, así que no distingue un fallo de un éxito, y una base vacía produce un volcado vacío
///    perfectamente bueno.
///
/// Y **ninguna de las tres es el `Ok` del archivado**, que es el fallo que este módulo existe para
/// no cometer.
fn el_volcado_se_entrego(
    entrega_completa: bool,
    codigo: i32,
    ficheros: u64,
) -> Result<(), FalloDelVolcado> {
    if !entrega_completa {
        return Err(FalloDelVolcado::NoSeEntregoEntero);
    }

    if codigo != 0 {
        return Err(FalloDelVolcado::ElProcesoFallo { codigo });
    }

    if ficheros < 1 {
        return Err(FalloDelVolcado::SinElVolcadoDentro);
    }

    Ok(())
}

/// El motivo que se guarda por un volcado que no se dio por bueno.
///
/// El motivo del contenedor entra tal cual **porque ya viene depurado**: lo construye
/// [`volcar`] con `motivo_para_guardar`, que quita los valores del entorno antes de recortar. Aquí
/// solo queda recortar el conjunto, y por eso no hace falta una orden con la que redactar —la
/// única orden de esta copia viajó al hilo del productor con el volcado—.
fn motivo_del_fallo(fallo: FalloDelVolcado, resultado: &Resultado, ficheros: u64) -> String {
    let que_paso = match fallo {
        // El código de salida entra aquí cuando no es cero, y no es un adorno: el desenlace más
        // frecuente de un volcado que no se entrega entero es justo ése —el proceso falla, el
        // productor se suelta sin declarar el final y el puente lee el cierre como truncado—, y
        // decir «no se entregó entero» sin decir con qué código salió obligaría al operador a
        // mirar el contenedor para averiguar algo que el motor ya sabe.
        FalloDelVolcado::NoSeEntregoEntero if resultado.codigo != 0 => format!(
            "el volcado no se entregó entero —el comando de extracción salió con el código {}—: \
             puede estar truncado, y archivarlo daría por buena una copia incompleta",
            resultado.codigo
        ),
        FalloDelVolcado::NoSeEntregoEntero => {
            "el volcado no se entregó entero: puede estar truncado, y archivarlo daría por buena \
             una copia incompleta"
                .to_owned()
        }
        FalloDelVolcado::ElProcesoFallo { codigo } => {
            format!("el comando de extracción salió con el código {codigo}")
        }
        FalloDelVolcado::SinElVolcadoDentro => format!(
            "la instantánea no tiene el volcado dentro: el archivado contó {ficheros} ficheros"
        ),
    };

    // El motivo del contenedor es la última frase que escribió antes de fallar y la que más ayuda
    // a diagnosticar. Vacío no deja un «el contenedor dijo:» colgando: eso se leería como que sí
    // dijo algo.
    let lo_que_dijo = resultado.motivo_del_contenedor.trim();
    if lo_que_dijo.is_empty() {
        return recortar(&que_paso);
    }

    recortar(&format!("{que_paso}; el contenedor dijo: {lo_que_dijo}"))
}

/// El motivo de un fallo de la extracción, listo para guardarse.
///
/// Se pasa por la red completa —redacción y recorte— y no solo por el recorte porque aquí **sí**
/// hay una orden con la que redactar: el `stderr` que la extracción acumuló ya viene depurado,
/// pero el error del transporte puede llevar dentro el nombre del socket y el contenedor, así que
/// una segunda pasada por la red es lo que evita que el día que algo cambie en la extracción el
/// valor acabe en la fila.
fn motivo_del_volcado(causa: &ErrorDeExtraccion, orden: &Orden) -> String {
    motivo_para_guardar(causa.to_string().into_bytes(), orden)
}

/// El motivo de un error de la biblioteca, listo para guardarse.
///
/// La razón de pasarlo por la red no es teórica: medido en el módulo del repositorio, el motivo de
/// un fallo del destino es el `display_log()` de la biblioteca, y un error de un backend puede
/// llevar dentro la URL del destino con su credencial. Se guarda en una columna que el panel
/// muestra, así que es texto que viene de fuera y se trata como tal.
///
/// El [`Orden`] que se construye aquí **no se ejecuta**: es la forma que tiene este crate de
/// nombrar un conjunto de valores que no pueden aparecer en un texto, y los dos valores que hay
/// que quitar de un motivo del repositorio son exactamente sus dos secretos. Su `Debug` publica
/// los nombres y nunca los valores, así que ni un registro de esta línea los saca.
fn motivo_de_la_biblioteca(texto: &str, acceso: &Acceso) -> String {
    let a_quitar = Orden {
        cmd: Vec::new(),
        env: vec![
            format!("secret_access_key={}", acceso.secret_access_key.expose()),
            format!("passphrase={}", acceso.passphrase.expose()),
        ],
    };

    motivo_para_guardar(texto.as_bytes().to_vec(), &a_quitar)
}

/// Cómo acabó una ejecución, listo para escribirse en su fila.
struct Cierre {
    /// El estado final: `success` o `failed`, nunca otro.
    estado: Estado,
    /// El identificador de la instantánea, cuando la hubo.
    snapshot_id: Option<String>,
    /// El motivo, cuando falló. Ya **recortado y sin credenciales**.
    motivo: Option<String>,
}

impl Cierre {
    /// Una copia que se dio por buena.
    fn hecha(hecho: &CopiaHecha) -> Self {
        Self {
            estado: Estado::Success,
            snapshot_id: Some(hecho.id.clone()),
            motivo: None,
        }
    }

    /// Una copia que no se dio por buena, con su motivo.
    fn fallida(motivo: String) -> Self {
        Self {
            estado: Estado::Failed,
            snapshot_id: None,
            motivo: Some(motivo),
        }
    }
}

/// Los tres números que un aviso de progreso lleva, ya resueltos.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cifras {
    /// Los bytes que han salido de la extracción.
    procesados: u64,
    /// Los bytes que el archivador ha leído.
    subidos: u64,
    /// El porcentaje, solo cuando hay un total conocido y mayor que cero.
    percentage: Option<f64>,
}

impl Cifras {
    /// Las cifras de una ejecución de la que no se ha medido nada.
    const fn nada() -> Self {
        Self {
            procesados: 0,
            subidos: 0,
            percentage: None,
        }
    }
}

/// El progreso acumulado de una ejecución, tal y como lo va contando el gancho.
#[derive(Debug)]
struct Avance {
    /// El total que el gancho declaró, si lo declaró.
    ///
    /// Medido: un volcado por `stdout` **no recibe nunca** un `set_length` en su barra de bytes, así
    /// que aquí se queda en `None` para todo un volcado, y eso es lo correcto y no un hueco: un
    /// porcentaje sin denominador es una cifra inventada pintada como una barra de progreso.
    total: Option<u64>,
    /// Los bytes que el gancho ha ido sumando.
    subidos: u64,
    /// Cuándo se publicó el último aviso.
    ultimo_aviso: Instant,
    /// Cuántos bytes se habían sumado en el último aviso.
    ///
    /// Se guarda el valor y no un contador de bytes desde el aviso: la condición del umbral es
    /// `subidos - bytes_del_ultimo_aviso >= UMBRAL_DE_AVISO`, y llevarlo así no puede desviarse por
    /// un aviso que no llegara.
    bytes_del_ultimo_aviso: u64,
}

/// El estado de una ejecución **en curso**: lo que hace falta para publicar su avance.
///
/// Vive dentro de un [`Arc`] porque lo comparten el hilo del trabajador —que cambia la acción y
/// publica las transiciones— y el archivado, que llama al gancho **en el mismo hilo** y desde ahí
/// publica el avance. Los candados son de grano fino y **ninguno se mantiene mientras se publica**:
/// publicar vuelve a entrar a leer las cifras, y hacerlo con un candado tomado sería un bloqueo
/// consigo mismo.
struct EnVivo {
    /// El hub por el que sale todo.
    hub: Hub,
    /// La ejecución de la que habla cada aviso.
    execution_id: Uuid,
    /// Su tarea.
    job_id: Uuid,
    /// El avance del productor, cuando esta copia es un volcado de flujo.
    ///
    /// Nace en `None` y lo conecta el camino del contenedor en cuanto construye su túnel. Para un
    /// directorio no habrá ninguno —ahí los bytes procesados son el total que el origen declara—, y
    /// antes de que el túnel exista tampoco: las barras del motor se construyen al abrir el
    /// repositorio, que es antes de que haya productor.
    volcado: Mutex<Option<AvanceDelVolcado>>,
    /// Qué está haciendo la ejecución ahora mismo.
    accion: Mutex<&'static str>,
    /// Lo que el gancho ha contado hasta ahora.
    avance: Mutex<Avance>,
}

impl EnVivo {
    /// El estado de una ejecución recién puesta en `running`.
    fn nueva(contexto: &Contexto, fila: &Ejecucion) -> Self {
        Self {
            hub: contexto.hub.clone(),
            execution_id: fila.id,
            job_id: fila.job_id,
            volcado: Mutex::new(None),
            accion: Mutex::new(ACCION_ENCOLADA),
            avance: Mutex::new(Avance {
                total: None,
                subidos: 0,
                ultimo_aviso: Instant::now(),
                bytes_del_ultimo_aviso: 0,
            }),
        }
    }

    /// Las barras del motor que informan a **esta** ejecución.
    ///
    /// El cierre captura el `Arc` del estado en vivo, que es lo que permite que el gancho —que la
    /// biblioteca llama desde el hilo del archivado— publique sin saber nada del motor.
    fn barras_de_esta_ejecucion(self: &Arc<Self>) -> BarrasDelMotor {
        let en_vivo = Arc::clone(self);

        BarrasDelMotor::nuevas(Arc::new(move |aviso: AvisoDeProgreso| {
            en_vivo.al_avisar(aviso);
        }))
    }

    /// Conecta el avance del productor del volcado.
    fn conectar_el_volcado(&self, avance: AvanceDelVolcado) {
        *candado(&self.volcado) = Some(avance);
    }

    /// Cambia la acción que se está publicando.
    fn accion(&self, accion: &'static str) {
        *candado(&self.accion) = accion;
    }

    /// Los tres números del aviso, tal y como están ahora.
    ///
    /// `bytes_processed` sale de donde de verdad se mide, y no es el mismo sitio para las dos
    /// clases de origen: para un volcado es la cuenta del **productor** —lo que ha salido del
    /// `exec`— y para un directorio es el **total** que su origen declara, que es lo que el §5 mide.
    /// Con un `inc` sin `set_length` el total no se conoce y no se inventa: se cuenta lo leído.
    fn cifras(&self) -> Cifras {
        let avance = candado(&self.avance);
        let del_volcado = candado(&self.volcado).clone();

        let subidos = avance.subidos;
        let total = avance.total;
        let procesados = match del_volcado {
            Some(volcado) => volcado.bytes(),
            None => total.unwrap_or(subidos),
        };

        // Nulo cuando no hay total, y nulo cuando el total es cero: un directorio vacío no tiene
        // nada que proporcionar, y un `0 %` o un `100 %` inventados serían una barra sobre una
        // cifra falsa.
        let percentage = match total {
            Some(total) if total > 0 => Some(100.0 * subidos as f64 / total as f64),
            _ => None,
        };

        Cifras {
            procesados,
            subidos,
            percentage,
        }
    }

    /// El evento del estado que se le pida, con las cifras de ahora mismo.
    fn evento(&self, estado: Estado, accion: &str) -> EventoDeEjecucion {
        let cifras = self.cifras();

        EventoDeEjecucion {
            execution_id: self.execution_id,
            job_id: self.job_id,
            status: estado,
            bytes_processed: cifras.procesados,
            bytes_uploaded: cifras.subidos,
            percentage: cifras.percentage,
            current_action: accion.to_owned(),
        }
    }

    /// Publica un avance de la ejecución **en curso**.
    fn publicar(&self) {
        let accion = *candado(&self.accion);

        self.hub.publicar(self.evento(Estado::Running, accion));
    }

    /// Recibe un aviso del gancho del archivador y decide si merece publicarse.
    ///
    /// El ritmo es el del contrato y está en dos constantes de arriba: ni un evento por trozo —un
    /// volcado de 20 MB son doce mil trozos— ni un evento por minuto. Se publica cuando han pasado
    /// [`UMBRAL_DE_AVISO`] bytes desde el último aviso **o** cuando ha pasado
    /// [`INTERVALO_MINIMO_DE_AVISO`] con algo nuevo; lo segundo, porque un volcado de poco caudal
    /// dejaría el panel congelado. Las transiciones de estado se publican aparte y siempre.
    fn al_avisar(&self, aviso: AvisoDeProgreso) {
        let toca = {
            let mut avance = candado(&self.avance);

            match aviso {
                AvisoDeProgreso::TotalDeBytes(total) => avance.total = Some(total),
                AvisoDeProgreso::Bytes(bytes) => {
                    avance.subidos = avance.subidos.saturating_add(bytes);
                }
                // El final de la operación de bytes no cambia ninguna cifra: lo que se publica al
                // final de una copia lo publica el cierre de la ejecución, con su estado.
                AvisoDeProgreso::FinDeBytes => {}
            }

            let nuevos = avance.subidos > avance.bytes_del_ultimo_aviso;
            let por_bytes =
                avance.subidos.saturating_sub(avance.bytes_del_ultimo_aviso) >= UMBRAL_DE_AVISO;
            let por_tiempo = nuevos && avance.ultimo_aviso.elapsed() >= INTERVALO_MINIMO_DE_AVISO;

            if por_bytes || por_tiempo {
                avance.ultimo_aviso = Instant::now();
                avance.bytes_del_ultimo_aviso = avance.subidos;
            }

            por_bytes || por_tiempo
        };

        // Publicar **fuera** del candado: `publicar` vuelve a entrar en `cifras`.
        if toca {
            self.publicar();
        }
    }
}

/// Toma un candado **sin entrar en pánico** si otro hilo se cayó teniéndolo tomado.
///
/// Es la misma decisión que la del puente de `src/rustic`: el dato de dentro sigue siendo tan
/// válido como era —un `Option` y tres contadores—, y un motor no se cae por el pánico de otro.
fn candado<T>(candado: &Mutex<T>) -> MutexGuard<'_, T> {
    match candado.lock() {
        Ok(guardado) => guardado,
        Err(veneno) => veneno.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::{FalloDelVolcado, el_volcado_se_entrego, nombre_del_volcado};

    /// **La condición de éxito de un volcado son las tres cosas, y ninguna es el `Ok` del
    /// archivado.**
    ///
    /// Se miden los cuatro desenlaces, y el primero es el que importa: un archivado que **no dio
    /// error** —de ahí que no aparezca por ninguna parte en esta llamada— con el volcado entregado
    /// a medias tiene que ser un fallo. Es la prueba que muerde contra la mutación obvia: quitar
    /// `entrega_completa` de la condición la deja caer con «un volcado que no se entregó entero se
    /// dio por bueno», que es exactamente el fallo medido que este módulo existe para no cometer.
    #[test]
    fn la_condicion_de_exito_del_volcado_no_se_hereda_del_archivado() {
        assert_eq!(
            el_volcado_se_entrego(true, 0, 1),
            Ok(()),
            "un volcado entregado entero, con el proceso en cero y con su nodo en la instantánea \
             es una copia buena"
        );

        assert_eq!(
            el_volcado_se_entrego(false, 0, 1),
            Err(FalloDelVolcado::NoSeEntregoEntero),
            "un volcado que no se entregó entero se dio por bueno: el archivado silencia el error \
             del lector y devuelve `Ok`, así que un `Ok` no puede ser la condición de éxito"
        );

        assert_eq!(
            el_volcado_se_entrego(true, 3, 1),
            Err(FalloDelVolcado::ElProcesoFallo { codigo: 3 }),
            "un proceso de extracción que sale con 3 no puede dar una copia buena"
        );

        assert_eq!(
            el_volcado_se_entrego(true, 0, 0),
            Err(FalloDelVolcado::SinElVolcadoDentro),
            "una instantánea sin el volcado dentro no es una copia: el archivado descartó la \
             entrada que falló y guardó el árbol sin ella"
        );
    }

    /// **Un volcado vacío es un éxito, y por eso la tercera condición es `ficheros` y no `bytes`.**
    ///
    /// Es la mitad que impide «arreglar» la condición con los bytes: medido, un volcado
    /// legítimamente vacío —un comando que no escribe nada y sale con cero— deja `ficheros == 1` y
    /// `bytes == 0`, así que exigir bytes convertiría una base vacía en una copia fallida.
    #[test]
    fn un_volcado_vacio_con_su_nodo_dentro_es_una_copia_buena() {
        assert_eq!(
            el_volcado_se_entrego(true, 0, 1),
            Ok(()),
            "un volcado de cero bytes sigue siendo un nodo dentro de la instantánea, y `bytes` \
             vale cero igual que en el truncado: por eso no sirve para decidir"
        );
    }

    /// **El nombre del volcado sale saneado del nombre del target.**
    ///
    /// Un nombre con un separador dentro dejaría la instantánea mintiendo sobre su contenido —el
    /// nodo llevaría un camino en vez de un nombre— y un nombre vacío sería un pánico dentro de la
    /// biblioteca, no un error devuelto. Se miden los tres casos: el normal, el que trae lo que no
    /// puede ir, y el que no deja nada.
    #[test]
    fn el_nombre_del_volcado_se_sanea_a_partir_del_target() {
        assert_eq!(
            nombre_del_volcado("tienda"),
            "tienda.sql",
            "un nombre normal no se toca más allá de la extensión"
        );
        assert_eq!(
            nombre_del_volcado("tienda-de-ropa_2026.v2"),
            "tienda-de-ropa_2026.v2.sql",
            "los caracteres que sí valen en un nombre de nodo no se sustituyen"
        );
        assert_eq!(
            nombre_del_volcado("volcados/tienda"),
            "volcados_tienda.sql",
            "un separador dentro del nombre partiría la entrada en trozos"
        );
        assert_eq!(
            nombre_del_volcado("la tienda (madrid)"),
            "la_tienda__madrid_.sql",
            "lo que no es letra, dígito, punto, guion o subrayado se sustituye"
        );
        assert_eq!(
            nombre_del_volcado(""),
            "volcado.sql",
            "un nombre vacío sería un pánico dentro de la biblioteca, no un error devuelto"
        );
        assert_eq!(
            nombre_del_volcado("   "),
            "___.sql",
            "un nombre de solo espacios sobrevive como subrayados: no queda vacío —un camino vacío \
             es un pánico dentro de la biblioteca— y no se recorta, que sería cambiar en silencio \
             lo que el operador escribió"
        );
        assert_eq!(
            nombre_del_volcado("años"),
            "a_os.sql",
            "el saneado es ASCII: el nombre acaba en una instantánea que leen otras herramientas"
        );
    }
}
