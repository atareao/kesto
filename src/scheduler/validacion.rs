//! La validación de la expresión cron, con el analizador nativo.
//!
//! La expresión de una tarea se valida **antes** de usarse, y con la misma gramática que después
//! la evaluará. Eso no es un adorno: el fallo que esta pieza existe para evitar es una expresión
//! que la validación acepta y el reloj no sabe leer, cuyo síntoma es una tarea que existe, se
//! lista, se pinta como programada y **no corre nunca**.
//!
//! # Analizador nativo
//!
//! Este módulo implementa su propio analizador de cron, eliminando las bibliotecas externas de
//! fechas del árbol de dependencias.
//!
//! # Cinco o seis campos: la normalización
//!
//! Anteponemos `0 ` solo a las expresiones de exactamente cinco campos, para convertirlas en su
//! forma de seis campos con segundos.
//!
//! # La agenda se interpreta en UTC
//!
//! La hora de Kesto es una sola: UTC. Un instante fijo de entrada da siempre el mismo instante de
//! salida, en cualquier máquina y con cualquier `TZ`.

use thiserror::Error;
use time::OffsetDateTime;

/// El motivo con el que se rechaza una expresión vacía.
const MOTIVO_DE_LA_VACIA: &str = "la expresión estaba vacía";

/// La expresión cron ya validada, con su forma normalizada y el analizador que la evaluará.
#[derive(Debug)]
pub struct Agenda {
    /// La forma de seis campos —la que entiende el reloj—.
    normalizada: String,
    /// El analizador ya construido.
    parsed: ParsedCron,
}

/// ParsedCron contiene los campos de una expresión cron ya analizada.
///
/// Cada campo es un `Vec<u8>` con todos los valores permitidos (para `*` son todos los del rango).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCron {
    /// Segundos (0-59)
    segundos: Vec<u8>,
    /// Minutos (0-59)
    minutos: Vec<u8>,
    /// Horas (0-23)
    horas: Vec<u8>,
    /// Día del mes (1-31)
    dia_mes: Vec<u8>,
    /// Mes (1-12)
    mes: Vec<u8>,
    /// Día de la semana (0=domingo..6=sábado)
    dia_semana: Vec<u8>,
    /// Año opcional
    anno: Option<i32>,
    /// day-of-month Y day-of-week son OR si es true, AND si es false
    dom_and_dow: bool,
}

impl Agenda {
    /// La forma de seis campos que sí entiende el reloj.
    pub fn normalizada(&self) -> &str {
        &self.normalizada
    }

    /// La próxima ejecución **estrictamente posterior** a `desde`, sobre el calendario en UTC.
    ///
    /// Devuelve `None` cuando la expresión no tiene una ocurrencia siguiente. La búsqueda salta
    /// directamente al año pedido cuando el campo de año está fijado, y explora por días —con un
    /// horizonte de 28 años, un ciclo completo de bisiestos y días de la semana— cuando no lo está.
    pub fn proxima_tras(&self, desde: OffsetDateTime) -> Option<OffsetDateTime> {
        let base = desde.to_offset(time::UtcOffset::UTC);
        let p = &self.parsed;

        const SEGUNDOS_POR_DIA: i64 = 86_400;

        let (inicio, fin) = match p.anno {
            Some(anno) => {
                if base.year() > anno {
                    return None;
                }
                let primer_dia = if base.year() < anno {
                    time::Date::from_calendar_date(anno, time::Month::January, 1).ok()?
                } else {
                    base.date()
                };
                let ultimo_dia =
                    time::Date::from_calendar_date(anno + 1, time::Month::January, 1).ok()?;
                (
                    primer_dia
                        .with_hms(0, 0, 0)
                        .ok()?
                        .assume_utc()
                        .unix_timestamp(),
                    ultimo_dia
                        .with_hms(0, 0, 0)
                        .ok()?
                        .assume_utc()
                        .unix_timestamp(),
                )
            }
            None => {
                let primer_dia = base.date().with_hms(0, 0, 0).ok()?.assume_utc();
                let inicio = primer_dia.unix_timestamp();
                (inicio, inicio + 366 * 28 * SEGUNDOS_POR_DIA)
            }
        };

        for dia_ts in (inicio..fin).step_by(SEGUNDOS_POR_DIA as usize) {
            let dia = OffsetDateTime::from_unix_timestamp(dia_ts).ok()?;
            if !self.coincide_con_el_dia(dia) {
                continue;
            }
            for &hora in &p.horas {
                for &minuto in &p.minutos {
                    for &segundo in &p.segundos {
                        let candidato = dia
                            .date()
                            .with_hms(hora, minuto, segundo)
                            .ok()?
                            .assume_utc();
                        if candidato > base {
                            return Some(candidato);
                        }
                    }
                }
            }
        }
        None
    }

    /// Comprueba si el día de `cuando` coincide con año, mes, día del mes y día de la semana.
    fn coincide_con_el_dia(&self, cuando: OffsetDateTime) -> bool {
        let p = &self.parsed;

        if p.anno.is_some_and(|anno| cuando.year() != anno) {
            return false;
        }

        if !p.mes.contains(&(cuando.month() as u8)) {
            return false;
        }

        let dow_candidato = match cuando.weekday() {
            time::Weekday::Sunday => 0,
            time::Weekday::Monday => 1,
            time::Weekday::Tuesday => 2,
            time::Weekday::Wednesday => 3,
            time::Weekday::Thursday => 4,
            time::Weekday::Friday => 5,
            time::Weekday::Saturday => 6,
        };

        let dom_ok = p.dia_mes.contains(&cuando.day());
        let dow_ok = p.dia_semana.contains(&dow_candidato);

        if p.dom_and_dow {
            dom_ok || dow_ok
        } else {
            dom_ok && dow_ok
        }
    }
}

/// Normaliza de cinco a seis campos. Solo toca las de exactamente cinco.
pub fn normalizar(expresion: &str) -> String {
    let campos: Vec<&str> = expresion.split_whitespace().collect();
    if campos.len() == 5 {
        format!("0 {}", campos.join(" "))
    } else {
        expresion.to_owned()
    }
}

/// Valida una expresión con el analizador nativo, sobre su forma normalizada.
pub fn validar(expresion: &str) -> Result<Agenda, ErrorDeExpresion> {
    if expresion.trim().is_empty() {
        return Err(ErrorDeExpresion::Vacia);
    }

    let normalizada = normalizar(expresion);
    let parsed = parse_cron(&normalizada).map_err(|motivo| ErrorDeExpresion::NoParseable {
        expresion: expresion.to_owned(),
        motivo,
    })?;

    // Un campo vacío daría una agenda que nunca dispara: mejor rechazarla en el borde que
    // guardar una tarea que aparece como programada y no corre nunca.
    if [
        &parsed.segundos,
        &parsed.minutos,
        &parsed.horas,
        &parsed.dia_mes,
        &parsed.mes,
        &parsed.dia_semana,
    ]
    .iter()
    .any(|campo| campo.is_empty())
    {
        return Err(ErrorDeExpresion::NoParseable {
            expresion: expresion.to_owned(),
            motivo: "algún campo de la expresión no produce valores".to_owned(),
        });
    }

    Ok(Agenda {
        normalizada,
        parsed,
    })
}

/// Analiza una expresión cron de 6 o 7 campos.
fn parse_cron(expr: &str) -> Result<ParsedCron, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 6 && parts.len() != 7 {
        return Err(format!(
            "se esperaban 6 o 7 campos, encontrados {}",
            parts.len()
        ));
    }

    let segundos = parse_campo(parts[0], 0, 59, "segundos")?;
    let minutos = parse_campo(parts[1], 0, 59, "minutos")?;
    let horas = parse_campo(parts[2], 0, 23, "horas")?;
    let dia_mes = parse_campo(parts[3], 1, 31, "día del mes")?;
    let mes = parse_campo_mes(parts[4])?;
    let dia_semana = parse_campo_dow(parts[5])?;

    let anno = if parts.len() == 7 {
        let anio: i32 = parts[6]
            .parse()
            .map_err(|_| format!("año inválido: '{}'", parts[6]))?;
        if !(1970..=2099).contains(&anio) {
            return Err(format!("año {} fuera de rango (1970-2099)", anio));
        }
        Some(anio)
    } else {
        None
    };

    Ok(ParsedCron {
        segundos,
        minutos,
        horas,
        dia_mes,
        mes,
        dia_semana,
        anno,
        dom_and_dow: true,
    })
}

/// Analiza un campo numérico: `*`, lista `1,15`, rango `1-10`, paso `*/5`.
///
/// El paso se aplica **desde el inicio de cada segmento** —el comienzo del rango, el valor suelto
/// o el mínimo del campo en `*`—, no desde el mínimo global del campo. Así `1-10/2` son
/// `1,3,5,7,9` y `5/10` son `5,15,25,35,45,55`.
fn parse_campo(field: &str, min: u8, max: u8, name: &str) -> Result<Vec<u8>, String> {
    let (base, paso) = match field.split_once('/') {
        Some((base, paso)) => (base, Some(paso)),
        None => (field, None),
    };
    let con_paso = paso.is_some();
    let paso: u8 = match paso {
        Some(paso) => {
            let paso = paso
                .parse()
                .map_err(|_| format!("paso inválido en campo {}: '{}'", name, field))?;
            if paso == 0 {
                return Err(format!(
                    "paso debe ser mayor que cero en campo {}: '{}'",
                    name, field
                ));
            }
            paso
        }
        None => 1,
    };
    let paso_usize = usize::from(paso);

    if base == "*" {
        return Ok((min..=max).step_by(paso_usize).collect());
    }

    let mut valores = Vec::new();
    for parte in base.split(',') {
        if let Some((ini, fin)) = parte.split_once('-') {
            let ini: u8 = ini
                .parse()
                .map_err(|_| format!("rango inválido en {}: '{}'", name, parte))?;
            let fin: u8 = fin
                .parse()
                .map_err(|_| format!("rango inválido en {}: '{}'", name, parte))?;
            if ini < min || fin > max || ini > fin {
                return Err(format!("rango {}-{} fuera de rango en {}", ini, fin, name));
            }
            valores.extend((ini..=fin).step_by(paso_usize));
        } else {
            let v: u8 = parte
                .parse()
                .map_err(|_| format!("valor inválido en {}: '{}'", name, parte))?;
            if v < min || v > max {
                return Err(format!(
                    "valor {} fuera de rango {}-{} en {}",
                    v, min, max, name
                ));
            }
            if con_paso {
                valores.extend((v..=max).step_by(paso_usize));
            } else {
                valores.push(v);
            }
        }
    }

    if valores.is_empty() {
        return Err(format!("el campo {} no produce valores: '{}'", name, field));
    }
    valores.sort_unstable();
    valores.dedup();
    Ok(valores)
}

/// Analiza el campo mes: números 1-12 o abreviaturas de tres letras en inglés.
fn parse_campo_mes(field: &str) -> Result<Vec<u8>, String> {
    let nombres = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let rango = || (1..=12).collect();
    if field == "*" {
        return Ok(rango());
    }
    if let Ok(v) = field.parse::<u8>() {
        return if (1..=12).contains(&v) {
            Ok(vec![v])
        } else {
            Err(format!("mes fuera de rango: {}", v))
        };
    }
    let lower = field.to_ascii_lowercase();
    for (i, nombre) in nombres.iter().enumerate() {
        if lower == *nombre {
            return Ok(vec![i as u8 + 1]);
        }
    }
    Err(format!("mes inválido: '{}'", field))
}

/// Analiza el campo día de la semana: 0=domingo..6=sábado o nombres en inglés.
fn parse_campo_dow(field: &str) -> Result<Vec<u8>, String> {
    let nombres = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
    if field == "*" || field == "?" {
        return Ok((0..=6).collect());
    }
    if let Ok(v) = field.parse::<u8>() {
        return if v <= 6 {
            Ok(vec![v])
        } else {
            Err(format!("día de la semana fuera de rango: {}", v))
        };
    }
    let lower = field.to_ascii_lowercase();
    for (i, nombre) in nombres.iter().enumerate() {
        if lower == *nombre {
            return Ok(vec![i as u8]);
        }
    }
    Err(format!("día de la semana inválido: '{}'", field))
}

/// Por qué se rechazó una expresión cron.
#[derive(Debug, Error)]
pub enum ErrorDeExpresion {
    /// La expresión está vacía.
    #[error("{}", MOTIVO_DE_LA_VACIA)]
    Vacia,

    /// La expresión no parsea con el analizador nativo.
    #[error("la expresión cron «{expresion}» no se puede interpretar: {motivo}")]
    NoParseable {
        /// La expresión tal y como llegó, sin normalizar.
        expresion: String,
        /// El motivo del analizador.
        motivo: String,
    },
}

impl ErrorDeExpresion {
    /// El motivo del rechazo, sin la expresión delante.
    pub fn motivo(&self) -> &str {
        match self {
            Self::Vacia => MOTIVO_DE_LA_VACIA,
            Self::NoParseable { motivo, .. } => motivo,
        }
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::{ErrorDeExpresion, normalizar, validar};

    #[test]
    fn la_normalizacion_antepone_el_segundo_solo_a_las_de_exactamente_cinco_campos() {
        assert_eq!(normalizar("0 3 * * *"), "0 0 3 * * *");
        assert_eq!(normalizar("0 0 3 * * *"), "0 0 3 * * *");
        assert_eq!(normalizar("0 0 3 * * * 2027"), "0 0 3 * * * 2027");
    }

    #[test]
    fn una_expresion_de_cinco_campos_es_valida_y_calcula_su_proxima_ejecucion_en_utc() {
        let agenda = validar("0 3 * * *").expect("la forma de cinco campos tiene que aceptarse");
        let base = datetime!(2023-11-14 22:13:20 UTC);
        let esperada = datetime!(2023-11-15 03:00:00 UTC);
        assert_eq!(agenda.proxima_tras(base), Some(esperada));
    }

    #[test]
    fn la_forma_de_cinco_campos_que_se_acepta_es_la_que_el_reloj_sabe_leer() {
        let agenda = validar("0 3 * * *").expect("la forma de cinco campos es válida");
        assert_eq!(agenda.normalizada(), "0 0 3 * * *");
    }

    #[test]
    fn una_expresion_de_seis_campos_tambien_es_valida_y_da_la_misma_proxima_ejecucion() {
        let de_cinco = validar("0 3 * * *").expect("la forma de cinco campos es válida");
        let de_seis = validar("0 0 3 * * *").expect("la forma de seis campos también es válida");
        let base = datetime!(2023-11-14 22:13:20 UTC);
        assert_eq!(de_cinco.proxima_tras(base), de_seis.proxima_tras(base));
    }

    #[test]
    fn una_expresion_que_no_parsea_se_rechaza_nombrando_la_expresion_y_el_motivo() {
        let expresion = "a b c d e";
        let error = validar(expresion).expect_err("tiene que rechazarse");
        assert!(matches!(error, ErrorDeExpresion::NoParseable { .. }));
        let mensaje = error.to_string();
        assert!(mensaje.contains(expresion));
        assert!(!error.motivo().is_empty());
        assert!(mensaje.contains(error.motivo()));
    }

    #[test]
    fn la_cadena_vacia_se_rechaza_con_el_motivo_de_que_esta_vacia() {
        let error = validar("").expect_err("una expresión vacía tiene que rechazarse");
        assert!(matches!(error, ErrorDeExpresion::Vacia));
        assert!(error.motivo().contains("vacía"));
        assert!(error.to_string().contains("vacía"));
    }

    #[test]
    fn el_paso_de_un_rango_empieza_en_el_inicio_del_rango() {
        let agenda = validar("0 1-10/2 * * * *").expect("el rango con paso es válido");
        assert_eq!(agenda.parsed.minutos, vec![1, 3, 5, 7, 9]);
    }

    #[test]
    fn el_paso_sin_rango_empieza_en_el_valor_dado_y_no_da_un_campo_vacio() {
        let agenda = validar("0 5/10 * * * *").expect("el valor con paso es válido");
        assert!(
            !agenda.parsed.minutos.is_empty(),
            "un campo con paso no puede quedarse sin valores"
        );
        assert_eq!(agenda.parsed.minutos, vec![5, 15, 25, 35, 45, 55]);
        assert_eq!(
            agenda.proxima_tras(datetime!(2023-11-14 22:13:20 UTC)),
            Some(datetime!(2023-11-14 22:15:00 UTC))
        );
    }

    #[test]
    fn un_paso_cero_se_rechaza_nombrando_el_motivo() {
        let error = validar("*/0 * * * * *").expect_err("un paso cero tiene que rechazarse");
        assert!(
            error.motivo().contains("paso debe ser mayor que cero"),
            "el motivo no dice que el paso tiene que ser mayor que cero: {}",
            error.motivo()
        );
    }

    #[test]
    fn un_campo_de_ano_lejano_no_se_pierde_por_el_horizonte_de_busqueda() {
        let agenda = validar("0 0 3 * * * 2028").expect("el año 2028 está dentro del rango");
        let base = datetime!(2026-11-26 12:00:00 UTC);
        assert_eq!(
            agenda.proxima_tras(base),
            Some(datetime!(2028-01-01 03:00:00 UTC))
        );
        assert_eq!(
            agenda.proxima_tras(datetime!(2029-01-01 00:00:00 UTC)),
            None,
            "un año ya pasado no tiene ocurrencias siguientes"
        );
    }

    #[test]
    fn la_proxima_ejecucion_es_determinista_con_un_instante_con_offset() {
        let agenda = validar("0 3 * * *").expect("la forma de cinco campos es válida");
        let base = datetime!(2023-07-15 23:30:00 +02:00);
        assert_eq!(
            agenda.proxima_tras(base),
            Some(datetime!(2023-07-16 03:00:00 UTC)),
            "un instante con offset explícito tiene que dar siempre el mismo instante en UTC"
        );
    }
}
