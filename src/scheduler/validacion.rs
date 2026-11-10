//! La validación de la expresión cron, con el analizador del reloj.
//!
//! La expresión de una tarea se valida **antes** de usarse, y con la misma gramática que después
//! la evaluará. Eso no es un adorno: el fallo que esta pieza existe para evitar es una expresión
//! que la validación acepta y el reloj no sabe leer, cuyo síntoma es una tarea que existe, se
//! lista, se pinta como programada y **no corre nunca**.
//!
//! # El analizador es el del reloj, y es más estricto que el del crate
//!
//! `croner` trae dos configuraciones del mismo analizador:
//!
//! - la **por defecto** (`Seconds::Optional`), que acepta cinco campos —inyectando el segundo
//!   `0`— y también seis;
//! - la que construye **la biblioteca del reloj** en sus cinco sitios (`Seconds::Required` con
//!   `dom_and_dow(true)`), que **exige** seis y rechaza la de cinco con `ParseSchedule`.
//!
//! Validar con la primera aceptaría expresiones que el reloj no entiende, así que aquí se usa
//! **la segunda**, sobre la forma normalizada.
//!
//! # Cinco o seis campos: la normalización, y por qué la fila no se reescribe
//!
//! Que las dos formas sean válidas es deliberado: el §3 no impone el número de campos y las filas
//! y pruebas del repositorio escriben cinco (`0 3 * * *`), así que exigir seis habría dejado
//! inválido lo que ya está guardado. Como el analizador del reloj exige seis, la validación
//! **normaliza en vuelo**: antepone `0 ` **solo cuando el número de campos es exactamente 5**, y
//! no toca las de 6 ni las de 7 —una de siete, con año, es válida y no se reescribe—. No es una
//! invención: es literalmente lo que `croner` hace con cinco campos cuando su analizador es
//! `Optional`, reproducido a propósito para que lo que se valida y lo que se le entrega al reloj
//! sean la misma cadena.
//!
//! Y la normalización ocurre **al construir el job, nunca en la fila**: `backup_jobs` guarda lo
//! que tecleó el usuario y el panel lo devuelve tal cual, sin una migración implícita que nadie
//! pidió.
//!
//! # El mensaje del rechazo lo escribe este módulo
//!
//! Y no `croner`, por dos motivos medidos: sus textos no reciben la cadena original
//! (`Invalid pattern: …`), así que el analizador **no puede** nombrar la expresión; y la
//! biblioteca del reloj **descarta** su error al construir el job —`map_err(|_| ParseSchedule)`—
//! cuyo `Display` es su `Debug`, así que al operador le llegaría la palabra `ParseSchedule` y
//! nada más. El mensaje de [`ErrorDeExpresion`] nombra la expresión y dice el motivo, y la
//! cadena vacía se nombra aparte porque es el valor que llega de verdad: `cron_expression` es
//! `TEXT NOT NULL` en el §3, así que un formulario que manda el campo en blanco no produce un
//! error de base sino una fila que no corre nunca.
//!
//! # La agenda se interpreta en UTC
//!
//! La hora de Kesto es una sola: la base guarda UTC, las fechas del cable son RFC3339 con
//! desplazamiento cero y el orden del listado de ejecuciones ya supone que dos marcas del mismo
//! segundo son el mismo instante. Un reloj en la zona local haría que el momento de las copias
//! cambiara con una variable de entorno del contenedor, sin tocar una línea de código y sin que
//! nadie lo relacione con el cambio.

use thiserror::Error;
use time::OffsetDateTime;

/// El motivo con el que se rechaza una expresión vacía.
///
/// Es una constante porque lo dicen dos sitios —el mensaje de [`ErrorDeExpresion::Vacia`] y
/// [`ErrorDeExpresion::motivo`]— y no puede divergir: el requisito obliga a que el motivo diga
/// que la expresión está vacía, y una copia que se quedara atrás lo rompería sin que nada lo
/// delatara.
const MOTIVO_DE_LA_VACIA: &str = "la expresión estaba vacía";

/// La expresión cron ya validada, con su forma normalizada y el analizador que la evaluará.
///
/// Nace solo de [`validar`], que es la única puerta: un `Agenda` construida a mano sería una
/// expresión que nadie ha comprobado contra la gramática del reloj, que es justo lo que este tipo
/// existe para impedir.
#[derive(Debug)]
pub struct Agenda {
    /// La forma de seis campos —la que entiende el reloj—, y la que hay que darle al construir el
    /// job. No es la expresión que tecleó el usuario: ver [`Agenda::normalizada`].
    normalizada: String,
    /// El analizador ya construido, para no volver a interpretar la expresión cada vez que se
    /// pregunta por su próxima ejecución. Es el mismo objeto con el que se evaluará, no otro
    /// construido con la misma configuración.
    analizada: croner::Cron,
}

impl Agenda {
    /// La forma de seis campos que sí entiende el reloj.
    ///
    /// Es la cadena que hay que entregarle al construir el job —`Job::new_async(agenda.normalizada(),
    /// …)`—, y **no** la que se guarda en `backup_jobs.cron_expression`, que sigue siendo la que
    /// tecleó el usuario. Devolver una y no la otra es lo que hace que el panel no reescriba nada.
    pub fn normalizada(&self) -> &str {
        &self.normalizada
    }

    /// La próxima ejecución **estrictamente posterior** a `desde`, sobre el calendario en UTC.
    ///
    /// Recibe la hora base por parámetro —en vez de leer el reloj de pared— porque es lo único
    /// que hace determinista a quien la llama, y porque así la zona del proceso **no puede**
    /// entrar en la cuenta: no se lee en ningún sitio. Un instante fijo de entrada da siempre el
    /// mismo instante de salida, en cualquier máquina y con cualquier `TZ`.
    ///
    /// Devuelve `None` cuando la expresión no tiene una ocurrencia siguiente —una que solo nombre
    /// años ya pasados, por ejemplo—, que es un desenlace y no un fallo.
    pub fn proxima_tras(&self, desde: OffsetDateTime) -> Option<OffsetDateTime> {
        // El **único** cruce de familias de fechas del módulo —`time` → `chrono` → `croner` →
        // `time`—, y por eso vive aquí y no repartido: `croner` mide el calendario en `chrono` y
        // la superficie de Kesto lo mide en `time`. El instante entra por `time`, se traduce una
        // sola vez a `chrono` para preguntarle a la agenda ya analizada, y su respuesta vuelve a
        // `time`. Ninguna otra firma pública de este módulo nombra una fecha de `chrono`.
        let instante =
            chrono::DateTime::from_timestamp(desde.unix_timestamp(), desde.nanosecond())?;

        let siguiente = self.analizada.find_next_occurrence(&instante, false).ok()?;

        OffsetDateTime::from_unix_timestamp(siguiente.timestamp()).ok()
    }
}

/// Normaliza de cinco a seis campos. Solo toca las de exactamente cinco.
///
/// Antepone `0 ` —el mismo `parts.insert(0, "0")` que hace el analizador por defecto de
/// `croner`— a una expresión con cinco campos separados por espacios, que es la forma que el
/// analizador del reloj no entiende; y devuelve **tal cual** las de seis y las de siete: una de
/// siete, con año, es válida y no hay nada que reescribir en ella. Cualquier otro número de
/// campos se deja intacto también, para que sea el analizador quien lo rechace y el motivo del
/// rechazo sea suyo.
pub fn normalizar(expresion: &str) -> String {
    let campos: Vec<&str> = expresion.split_whitespace().collect();

    // Solo las de **exactamente** cinco: las de seis ya traen su segundo y las de siete —con
    // año— son válidas tal cual, así que reescribirlas sería cambiar lo que el operador escribió.
    // Cualquier otro número de campos se deja intacto para que sea el analizador quien lo rechace.
    if campos.len() == 5 {
        format!("0 {}", campos.join(" "))
    } else {
        expresion.to_owned()
    }
}

/// Valida una expresión con el analizador del reloj, sobre su forma normalizada.
///
/// Es la única puerta que fabrica una [`Agenda`]: normaliza primero, y después interpreta con la
/// misma configuración del analizador que usa la biblioteca del reloj, para que lo que aquí se
/// acepta sea exactamente lo que el reloj sabe leer.
///
/// # Errors
///
/// [`ErrorDeExpresion::Vacia`] si la expresión está vacía —o solo tiene espacios—, y
/// [`ErrorDeExpresion::NoParseable`] con la expresión y el motivo del analizador si no parsea.
pub fn validar(expresion: &str) -> Result<Agenda, ErrorDeExpresion> {
    // El vacío —o el que solo tiene espacios— no es una expresión mal escrita: es el valor que
    // llega de un formulario en blanco, y merece su propio motivo en vez de un «expresión
    // inválida» genérico.
    if expresion.trim().is_empty() {
        return Err(ErrorDeExpresion::Vacia);
    }

    let normalizada = normalizar(expresion);

    // **El mismo analizador que el reloj**, sobre la forma normalizada: `Seconds::Required` con
    // `dom_and_dow(true)` es literalmente lo que `tokio-cron-scheduler` construye al montar el
    // job. Con el analizador por defecto se aceptarían cinco campos que el reloj rechaza, y el
    // síntoma sería una tarea que existe y no corre nunca.
    let analizador = croner::parser::CronParser::builder()
        .seconds(croner::parser::Seconds::Required)
        .dom_and_dow(true)
        .build();

    let analizada =
        analizador
            .parse(&normalizada)
            .map_err(|error| ErrorDeExpresion::NoParseable {
                // La expresión original, la que tecleó el operador: el analizador no puede nombrarla
                // —su motivo no recibe la cadena— y por eso el mensaje la lleva delante.
                expresion: expresion.to_owned(),
                motivo: error.to_string(),
            })?;

    Ok(Agenda {
        normalizada,
        analizada,
    })
}

/// Por qué se rechazó una expresión cron.
///
/// Los dos desenlaces están separados porque el vacío no es una expresión mal escrita: es el valor
/// que llega de verdad desde un formulario, y merece un motivo que se entienda en vez de un
/// «expresión inválida» genérico. En los dos casos el mensaje **nombra la expresión** salvo
/// cuando no hay nada que nombrar: el de la vacía dice que está vacía.
#[derive(Debug, Error)]
pub enum ErrorDeExpresion {
    /// La expresión está vacía.
    ///
    /// `cron_expression` es `TEXT NOT NULL` en el §3, así que un formulario que manda el campo en
    /// blanco no produce un error de base: produce una fila con una cadena vacía —una tarea que
    /// existe, se lista, se pinta como programada y no corre nunca—.
    #[error("{}", MOTIVO_DE_LA_VACIA)]
    Vacia,

    /// La expresión no parsea con el analizador del reloj.
    ///
    /// El motivo es el texto del analizador, tal cual, porque describe el defecto del patrón; lo
    /// que el analizador no puede dar es la expresión original, y por eso el mensaje la lleva
    /// delante.
    #[error("la expresión cron «{expresion}» no se puede interpretar: {motivo}")]
    NoParseable {
        /// La expresión tal y como llegó, sin normalizar: es lo que el operador escribió.
        expresion: String,
        /// El motivo del analizador.
        motivo: String,
    },
}

impl ErrorDeExpresion {
    /// El motivo del rechazo, sin la expresión delante.
    ///
    /// Existe para que quien tenga que envolverlo —el `400` de la capa de recursos, el error del
    /// arranque— no tenga que recortar el mensaje del `Display` para quedarse con la razón.
    pub fn motivo(&self) -> &str {
        match self {
            Self::Vacia => MOTIVO_DE_LA_VACIA,
            Self::NoParseable { motivo, .. } => motivo,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use time::macros::datetime;
    use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError, JobToRunAsync};
    use uuid::Uuid;

    use super::{ErrorDeExpresion, normalizar, validar};

    /// 1.1 — La normalización toca las de cinco y **solo** las de cinco.
    ///
    /// Es la regla que hace compatible la forma que ya está guardada con el analizador del reloj,
    /// y su borde es lo que hay que sujetar: una de seis o de siete reescrita sería una expresión
    /// distinta de la que el operador escribió.
    #[test]
    fn la_normalizacion_antepone_el_segundo_solo_a_las_de_exactamente_cinco_campos() {
        assert_eq!(
            normalizar("0 3 * * *"),
            "0 0 3 * * *",
            "una expresión de cinco campos tiene que salir con el segundo delante"
        );
        assert_eq!(
            normalizar("0 0 3 * * *"),
            "0 0 3 * * *",
            "una de seis campos no se toca: ya trae su segundo"
        );
        assert_eq!(
            normalizar("0 0 3 * * * 2027"),
            "0 0 3 * * * 2027",
            "una de siete campos —con año— es válida y no se reescribe"
        );
    }

    /// 1.1 — Escenario 1: una expresión de cinco campos es válida y su próxima ejecución se
    /// calcula en UTC.
    ///
    /// El instante de partida es **fijo y explícito**, y llega por parámetro: la zona del proceso
    /// no puede entrar en la cuenta porque no se lee en ningún sitio, así que el valor esperado
    /// —las tres de la mañana, hora UTC, del día siguiente— vale en cualquier máquina y con
    /// cualquier `TZ`. Lo que se comprueba, por tanto, no es solo que la expresión se acepte, sino
    /// que la agenda está en UTC y no en la hora local de quien la esté mirando.
    #[test]
    fn una_expresion_de_cinco_campos_es_valida_y_calcula_su_proxima_ejecucion_en_utc() {
        let agenda = validar("0 3 * * *").expect("la forma de cinco campos tiene que aceptarse");

        // 2023-11-14T22:13:20Z, un instante concreto y ya pasado: la base no importa.
        let base = datetime!(2023-11-14 22:13:20 UTC);
        // Y las 03:00:00 UTC del 15, que es la primera ocurrencia de `0 3 * * *` después de él.
        let esperada = datetime!(2023-11-15 03:00:00 UTC);

        assert_eq!(
            agenda.proxima_tras(base),
            Some(esperada),
            "la próxima ejecución no es la de la agenda en UTC"
        );
    }

    /// 1.1 — Escenario 2: la forma de cinco campos que se acepta **es la que el reloj sabe leer**.
    ///
    /// Es la prueba que cazó el spike y la más importante de todas: sin ella, la validación podría
    /// aceptar con una gramática que el reloj no entiende —las cinco piezas que `croner` acepta
    /// por defecto y `Seconds::Required` rechaza—, y el síntoma sería una tarea que no corre
    /// nunca, sin un solo error a la vista. Por eso no basta con validar: hay que entregarle al
    /// reloj la forma validada y comprobar que no contesta `ParseSchedule`.
    #[test]
    fn la_forma_de_cinco_campos_que_se_acepta_es_la_que_el_reloj_sabe_leer() {
        let agenda = validar("0 3 * * *").expect("la forma de cinco campos tiene que aceptarse");

        // El cierre de disparo con la firma literal del alias `JobToRunAsync`; su cuerpo da igual
        // aquí: lo que se está midiendo es el analizador, no el disparo.
        let disparo: Box<JobToRunAsync> = Box::new(
            |_job_id: Uuid, _reloj: JobScheduler| -> Pin<Box<dyn Future<Output = ()> + Send>> {
                Box::pin(async move {})
            },
        );

        match Job::new_async(agenda.normalizada(), disparo) {
            Ok(_) => {}
            Err(JobSchedulerError::ParseSchedule) => panic!(
                "el reloj no acepta la forma que la validación dio por buena: esa tarea no \
                 correría nunca"
            ),
            Err(otro) => panic!("el reloj rechazó la forma validada por otro motivo: {otro:?}"),
        }
    }

    /// 1.1 — Escenario 3: una expresión de seis campos también es válida, y da la misma próxima
    /// ejecución que la de cinco.
    ///
    /// Las dos formas son la misma agenda escrita de dos maneras, así que tienen que coincidir en
    /// el instante, no solo en el hecho de aceptarse: si divergieran, la normalización estaría
    /// cambiando el significado de lo que el operador escribió.
    #[test]
    fn una_expresion_de_seis_campos_tambien_es_valida_y_da_la_misma_proxima_ejecucion() {
        let de_cinco = validar("0 3 * * *").expect("la forma de cinco campos es válida");
        let de_seis = validar("0 0 3 * * *").expect("la forma de seis campos también es válida");

        let base = datetime!(2023-11-14 22:13:20 UTC);

        assert_eq!(
            de_cinco.proxima_tras(base),
            de_seis.proxima_tras(base),
            "las dos formas de la misma agenda no dan la misma próxima ejecución"
        );
    }

    /// 1.1 — Escenario 4: una expresión que no parsea se rechaza nombrando la expresión **y** el
    /// motivo.
    ///
    /// Las dos cosas se comprueban por separado porque son dos exigencias distintas del
    /// requisito, y ninguna se deduce de la otra: el motivo lo escribe el módulo precisamente
    /// porque el analizador **no** nombra la expresión, así que un mensaje que dijera solo una de
    /// las dos estaría a medias.
    #[test]
    fn una_expresion_que_no_parsea_se_rechaza_nombrando_la_expresion_y_el_motivo() {
        let expresion = "a b c d e";

        let error = validar(expresion)
            .expect_err("una expresión que el analizador no entiende tiene que rechazarse");

        assert!(
            matches!(error, ErrorDeExpresion::NoParseable { .. }),
            "el rechazo de una expresión ilegible no es el de «no parsea»: {error}"
        );

        let mensaje = error.to_string();

        assert!(
            mensaje.contains(expresion),
            "el rechazo no nombra la expresión original: {mensaje}"
        );
        assert!(
            !error.motivo().is_empty(),
            "el rechazo no dice el motivo del analizador: {mensaje}"
        );
        assert!(
            mensaje.contains(error.motivo()),
            "el motivo no aparece en el mensaje del rechazo: {mensaje}"
        );
    }

    /// 1.1 — Escenario 5: la cadena vacía se rechaza, y el motivo dice que está vacía.
    ///
    /// Se comprueba que el rechazo del vacío es **el suyo** y no el de una expresión ilegible:
    /// `cron_expression` es `TEXT NOT NULL`, así que el vacío llega desde un formulario en blanco
    /// y es el único caso en el que no hay nada que nombrar; confundirlo con «no parsea» dejaría
    /// al operador buscando una errata que no existe.
    #[test]
    fn la_cadena_vacia_se_rechaza_con_el_motivo_de_que_esta_vacia() {
        let error = validar("").expect_err("una expresión vacía tiene que rechazarse");

        assert!(
            matches!(error, ErrorDeExpresion::Vacia),
            "el rechazo del vacío no es el del vacío: {error}"
        );
        assert!(
            error.motivo().contains("vacía"),
            "el motivo no dice que la expresión está vacía: {error}"
        );
        assert!(
            error.to_string().contains("vacía"),
            "el mensaje no dice que la expresión está vacía: {error}"
        );
    }
}
