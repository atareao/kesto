//! Pruebas de verificación de la migración de `chrono`/`chrono-tz` a `time`/`jiff`.
//!
//! Estas pruebas FALLARÁN mientras `chrono` siga siendo una dependencia directa del crate.
//! Pasarán (GREEN) una vez completada la migración completa (issue #185).

use std::fs;
use std::path::Path;

/// 1.1 — El `Cargo.toml` del paquete `kesto` **no** declara `chrono` como dependencia directa.
///
/// `chrono = "0.4"` (línea 291 en el estado actual) debe desaparecer de `[dependencies]`.
#[test]
fn cargo_toml_no_declara_chrono_como_dependencia_directa() {
    let cargo_toml_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");

    let contenido = fs::read_to_string(&cargo_toml_path).expect("Cargo.toml debe poder leerse");

    // Buscar la línea exacta que declara la dependencia directa: `chrono = "0.4"`
    // (ignorar comentarios y otras apariciones).
    let tiene_chrono_directa = contenido.lines().any(|linea| {
        let l = linea.trim();
        l.starts_with("chrono =") && !l.starts_with("#")
    });

    assert!(
        !tiene_chrono_directa,
        "Cargo.toml todavía declara 'chrono' como dependencia directa; la migración a time/jiff no está completa"
    );
}

/// 1.2 — El `Cargo.toml` **no** declara `tokio-cron-scheduler` ni `croner` como dependencias directas.
///
/// Ambas deben eliminarse cuando se reemplace el programador por alternativas nativas de `time`/`jiff`.
#[test]
fn cargo_toml_no_declara_croner_ni_tokio_cron_scheduler() {
    let cargo_toml_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");

    let contenido = fs::read_to_string(&cargo_toml_path).expect("Cargo.toml debe poder leerse");

    let tiene_croner = contenido.lines().any(|linea| {
        let l = linea.trim();
        l.starts_with("croner =") && !l.starts_with("#")
    });
    let tiene_tokio_cron_scheduler = contenido.lines().any(|linea| {
        let l = linea.trim();
        l.starts_with("tokio-cron-scheduler =") && !l.starts_with("#")
    });

    assert!(
        !tiene_croner,
        "Cargo.toml todavía declara 'croner' como dependencia directa; la migración a time/jiff no está completa"
    );
    assert!(
        !tiene_tokio_cron_scheduler,
        "Cargo.toml todavía declara 'tokio-cron-scheduler' como dependencia directa; la migración a time/jiff no está completa"
    );
}

/// 1.3 — `deny.toml` solo admite la excepción `RUSTSEC-2026-0543` (chrono) por la cadena FTP.
///
/// `chrono-tz` (`RUSTSEC-2026-0555`) desaparece del árbol y no puede quedar eximida.
/// `chrono` ya no es dependencia directa, pero `rustic_backend 0.7.0` fija la ruta
/// `suppaftp → opendal-service-ftp → opendal → rustic_backend` y las features de Cargo
/// son aditivas, así que esa única excepción debe documentar dicha cadena y el hecho
/// de que `chrono` ya no se declara directamente.
#[test]
fn deny_toml_solo_excepcion_chrono_por_cadena_ftp() {
    let deny_toml_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("deny.toml");

    let contenido = fs::read_to_string(&deny_toml_path).expect("deny.toml debe poder leerse");

    let contiene_0543 = contenido.contains("RUSTSEC-2026-0543");
    let contiene_0555 = contenido.contains("RUSTSEC-2026-0555");

    assert!(
        !contiene_0555,
        "deny.toml aún contiene la excepción RUSTSEC-2026-0555 (chrono-tz); la migración no está completa"
    );

    if contiene_0543 {
        assert!(
            contenido.contains("suppaftp")
                && contenido.contains("opendal")
                && contenido.contains("rustic_backend"),
            "RUSTSEC-2026-0543 debe documentar la cadena FTP suppaftp -> opendal-service-ftp -> opendal -> rustic_backend"
        );
        assert!(
            contenido.contains("ya no es dependencia directa"),
            "RUSTSEC-2026-0543 debe documentar que chrono ya no es dependencia directa"
        );
    }
}

/// 1.4 — El código del scheduler (`validacion.rs`) **no** menciona `chrono`.
///
/// El único cruce de familias de fechas debe eliminarse al migrar el programador.
#[test]
fn validacion_rs_no_menciona_chrono() {
    let validacion_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("scheduler")
        .join("validacion.rs");

    let contenido = fs::read_to_string(&validacion_path)
        .expect("src/scheduler/validacion.rs debe poder leerse");

    // Ignorar comentarios: solo comprobar que la palabra "chrono" no aparece en código
    // (pero los comentarios actuales la mencionan, así que este test FALLARÁ hasta la migración).
    let menciona_chrono = contenido.contains("chrono");

    assert!(
        !menciona_chrono,
        "src/scheduler/validacion.rs aún menciona 'chrono' (en comentarios o código); la migración a time/jiff no está completa"
    );
}

/// 1.5 — El código de conversiones (`conversiones.rs`) **no** menciona `chrono`.
///
/// Este módulo existe solo para cruzar entre `time` y `chrono`. Debe desaparecer o quedar vacío
/// tras la migración completa.
#[test]
fn conversiones_rs_no_menciona_chrono() {
    let conversiones_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("scheduler")
        .join("conversiones.rs");

    let contenido = fs::read_to_string(&conversiones_path)
        .expect("src/scheduler/conversiones.rs debe poder leerse");

    let menciona_chrono = contenido.contains("chrono");

    assert!(
        !menciona_chrono,
        "src/scheduler/conversiones.rs aún menciona 'chrono'; la migración a time/jiff no está completa"
    );
}
