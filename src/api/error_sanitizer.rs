//! Saneado y validación de mensajes de error antes de enviarlos al cliente.
//!
//! Los errores que genera el motor pueden contener información sensible: rutas absolutas del
//! sistema, credenciales embebidas en cadenas de texto o trazas de pila con nombres de módulos.
//! Esta capa filtra esos datos **antes** de serializar la respuesta JSON, sin tocar la base de
//! datos ni las entidades.
//!
//! El spec `executions-error_message` define dos reglas:
//! 1. Si el mensaje contiene datos sensibles (credenciales, rutas ocultas, stack traces), se
//!    devuelve `400 InvalidInput` nombrando el campo.
//! 2. Si el mensaje está vacío se trata como `null`.

use crate::error::AppError;

/// Valida y sana un mensaje de error devuelto por el motor.
///
/// Reglas del spec:
/// - `None` → `Ok(None)` (sin error).
/// - Vacío o solo espacios → `Ok(None)` (se considera sin error).
/// - Contiene patrones sensibles → `Err(InvalidInput)` con mensaje indicativo.
/// - Tiene contenido útil pero contiene fragmentos reveladores → saneado agresivo:
///   se eliminan las credenciales y rutas, dejando solo lo inocuo.
pub fn sanitize_error_message(msg: Option<&str>) -> Result<Option<String>, AppError> {
    // None → None.
    if msg.is_none() {
        return Ok(None);
    }

    let Some(texto) = msg else {
        return Ok(None);
    };

    if texto.trim().is_empty() {
        return Ok(None);
    }

    // Verificar patrones prohibidos que hacen inválido el mensaje completo.
    verificar_datos_sensibles(texto)?;

    // Sanear lo que queda: eliminar credenciales y rutas aún presentes.
    let mut resultado = texto.to_owned();
    censurar_credenciales_residual(&mut resultado);
    censurar_rutas_residual(&mut resultado);

    // Si tras el saneado quedó vacío, devolver None.
    if resultado.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(resultado))
}

// ── Verificación de patrones prohibidos ────────────────────────────────────

/// Comprueba si el mensaje contiene datos sensibles prohibidos.
fn verificar_datos_sensibles(texto: &str) -> Result<(), AppError> {
    if tiene_credenciales_prohibidas(texto) {
        return Err(AppError::InvalidInput(
            "error_message contiene datos sensibles".to_owned(),
        ));
    }

    if tiene_rutas_ocultas(texto) {
        return Err(AppError::InvalidInput(
            "error_message contiene rutas sensibles".to_owned(),
        ));
    }

    if tiene_stack_trace(texto) {
        return Err(AppError::InvalidInput(
            "error_message contiene una traza de pila".to_owned(),
        ));
    }

    Ok(())
}

/// Detecta credenciales explícitas tipo `clave=valor` o mención de palabra clave + valor entre comillas.
fn tiene_credenciales_prohibidas(texto: &str) -> bool {
    // Patrón A: `password=value`, `secret: value`, etc. — formato clave=valor o clave: valor
    const PALABRAS_PROHIBIDAS: &[&str] = &[
        "password=",
        "passwd=",
        "secret=",
        "token=",
        "api_key=",
        "api-key=",
        "access_key=",
        "access-key=",
        "auth_token=",
        "auth-token=",
        "private_key=",
        "private-key=",
        "passphrase=",
        "credential=",
    ];

    for &palabra in PALABRAS_PROHIBIDAS {
        if busqueda_case_insensitive(texto, palabra).is_some() {
            return true;
        }
    }

    // Patrón B: palabras clave seguidas de valor entre comillas simples o dobles.
    // Ejemplo: `Password 'admin'`, `secret = "abc123"`, `token ="xyz"`
    const PALABRAS_CLAVE_COMILLAS: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "token",
        "api.?key",
        "access.?key",
        "passphrase",
        "auth.?token",
        "private.?key",
        "user.?name",
        "credential",
    ];

    for &palabra in PALABRAS_CLAVE_COMILLAS {
        let variantes = expandir_wildcard(palabra);
        for variante in variantes {
            let lower_texto = texto.to_ascii_lowercase();
            if let Some(pos) = lower_texto.find(variante.as_str()) {
                let despues = pos + variante.len();
                // Buscar comilla después de la palabra (con espacios opcionales)
                let despues_texto = &texto[despues..];
                let no_blanco = despues_texto.find(|c: char| !c.is_whitespace());
                if let Some(idx_no_blanco) = no_blanco {
                    let c = match despues_texto[idx_no_blanco..].chars().next() {
                        Some(ch) => ch,
                        None => continue,
                    };
                    if matches!(c, '"' | '\'') {
                        // Hay una comilla → buscar el cierre
                        let despues_c = &despues_texto[idx_no_blanco..];
                        let Some(cierre) = despues_c[1..].find(c) else {
                            continue;
                        };
                        // Hay contenido entre comillas → es un valor de credencial
                        if cierre > 0 {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// Busca una subcadena ignorando mayúsculas/minúsculas.
fn busqueda_case_insensitive(texto: &str, patron: &str) -> Option<usize> {
    texto.to_ascii_lowercase().find(patron)
}

/// Detecta rutas absolutas sensibles no permitidas.
fn tiene_rutas_ocultas(texto: &str) -> bool {
    const DIRECTORIOS_PROHIBIDOS: &[&str] = &[
        "/var/", "/usr/", "/etc/", "/home/", "/root/", "/opt/", "/tmp/",
    ];

    for &dir in DIRECTORIOS_PROHIBIDOS {
        if busqueda_case_insensitive(texto, dir).is_some() {
            // Verificar que no sea una ruta permitida
            if !es_ruta_permidada(texto, dir) {
                return true;
            }
        }
    }
    false
}

/// Determina si una coincidencia de directorio corresponde a una ruta permitida.
fn es_ruta_permidada(texto: &str, directorio: &str) -> bool {
    // Buscar desde el '/' más cercano hacia atrás y construir la ruta
    let inicio_opt = busqueda_case_insitive_exact_slash(texto, directorio);

    if let Some(idx) = inicio_opt {
        let fin_ruta = extraer_fin_ruta_absoluta(texto, idx);
        let ruta_completa = &texto[idx..fin_ruta.min(texto.len())];

        RUTAS_PERMITIDAS
            .iter()
            .any(|permitida| ruta_completa.starts_with(permitida))
    } else {
        false
    }
}

/// Busca el índice exacto de la coincidencia (case-sensitive para paths).
fn busqueda_case_insitive_exact_slash(texto: &str, directorio: &str) -> Option<usize> {
    texto.find(directorio)
}

const RUTAS_PERMITIDAS: &[&str] = &[
    "/backup_sources",
    "/proc/version",
    "/dev/null",
    "/dev/stdin",
    "/dev/stdout",
    "/proc/sys",
    "/sys/",
];

/// Extrae el índice de fin de una ruta absoluta empezando en `inicio`.
fn extraer_fin_ruta_absoluta(texto: &str, inicio: usize) -> usize {
    let mut fin = inicio;

    // Saltar el '/' inicial si lo hay
    if fin < texto.len() && texto.as_bytes()[fin] == b'/' {
        fin += 1;
    }

    // Continuar mientras haya caracteres válidos de path
    while fin < texto.len() {
        let c = texto.as_bytes()[fin];
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'/' | b'~' | b'+') {
            fin += 1;
        } else {
            break;
        }
    }

    fin
}

/// Detecta trazas de pila con nombres de módulo internos.
fn tiene_stack_trace(texto: &str) -> bool {
    // Patrón 1: `thread 'name' panicked at ...`
    if texto.contains("' panicked at ") {
        return true;
    }

    // Patrón 2: ` at something.rs:digit:` — típico de panics de Rust
    if tiene_at_con_rust_location(texto) {
        return true;
    }

    false
}

/// Verifica si existe un patrón ` at module.rs:X:Y` suelto.
fn tiene_at_con_rust_location(texto: &str) -> bool {
    if let Some(pos_at) = texto.rfind(" at ") {
        let despues_at = &texto[pos_at + 4..];
        es_rust_location(despues_at)
    } else {
        false
    }
}

/// Determina si una cadena parece una ubicación de Rust (`lib.rs:X:Y`).
fn es_rust_location(s: &str) -> bool {
    let trimed = s.trim_start();
    if trimed.is_empty() {
        return false;
    }

    match trimed.find(".rs:") {
        Some(idx) => {
            let after = &trimed[idx + 4..];
            matches!(after.chars().next(), Some(c) if c.is_ascii_digit())
        }
        None => false,
    }
}

// ── Saneado residual (post-validación) ─────────────────────────────────────

fn censurar_credenciales_residual(texto: &mut String) {
    const PALABRAS_CLAVE: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "token",
        "api.?key",
        "credential",
        "access.?key",
        "passphrase",
        "auth.?token",
        "private.?key",
    ];

    for &palabra in PALABRAS_CLAVE {
        let variantes = expandir_wildcard(palabra);
        for variante in &variantes {
            if let Some(pos) = texto.to_ascii_lowercase().find(variante.as_str()) {
                intentar_reemplazar_en_pos(texto, pos, variante.len());
                break; // Una vez ya basta
            }
        }
    }
}

fn expandir_wildcard(patron: &str) -> Vec<String> {
    let tiene_wildcard = patron.contains(".?");
    if !tiene_wildcard {
        return vec![patron.to_owned()];
    }

    let con_guion = patron.replace(".?", "_");
    let sin_caracter = patron.replace(".?", "");

    let mut resultados = Vec::new();
    if !resultados.contains(&con_guion) {
        resultados.push(con_guion);
    }
    if !resultados.contains(&sin_caracter) {
        resultados.push(sin_caracter);
    }
    if resultados.is_empty() {
        resultados.push(patron.to_owned());
    }
    resultados
}

fn intentar_reemplazar_en_pos(texto: &mut String, pos_palabra: usize, len_palabra: usize) -> bool {
    let despues = pos_palabra + len_palabra;

    // Saltar espacios
    let Some(despues_espacio) = texto[despues..].find(|c: char| !c.is_whitespace()) else {
        return false;
    };
    let pos_signo = despues + despues_espacio;

    // Verificar '=' o ':'
    if pos_signo >= texto.len() {
        return false;
    }
    let signo = texto.as_bytes()[pos_signo];
    if signo != b'=' && signo != b':' {
        return false;
    }

    // Saltar espacios después del signo
    let Some(despues_signo) = texto[pos_signo + 1..].find(|c: char| !c.is_whitespace()) else {
        return false;
    };
    let pos_valor = pos_signo + 1 + despues_signo;

    if pos_valor >= texto.len() {
        return false;
    }

    // Valor termina en espacio siguiente o fin de cadena
    let fin_valor = texto[pos_valor..]
        .find(char::is_whitespace)
        .map(|i| pos_valor + i)
        .unwrap_or(texto.len());

    let valor_len = fin_valor - pos_valor;
    if valor_len == 0 || valor_len > 512 {
        return false;
    }

    texto.replace_range(pos_valor..fin_valor, "[CENSORED]");
    true
}

fn censurar_rutas_residual(texto: &mut String) {
    const DIRECTORIOS_SENSIBLES: &[&str] = &[
        "/var/", "/usr/", "/etc/", "/home/", "/root/", "/opt/", "/tmp/",
    ];

    for &dir in DIRECTORIOS_SENSIBLES {
        while let Some(inicio) = texto.find(dir) {
            let fin_ruta = extraer_fin_ruta_absoluta(texto, inicio);
            let ruta_completa = &texto[inicio..fin_ruta.min(texto.len())];

            let es_permidada = RUTAS_PERMITIDAS
                .iter()
                .any(|perm| ruta_completa.starts_with(perm));
            if es_permidada {
                texto.drain(inicio..fin_ruta.min(texto.len()));
                continue;
            }

            texto.replace_range(inicio..fin_ruta.min(texto.len()), "<ruta_censurada>");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_devuelve_none_ok() {
        assert!(sanitize_error_message(None).unwrap().is_none());
    }

    #[test]
    fn vacio_devuelve_none_ok() {
        assert!(sanitize_error_message(Some("")).unwrap().is_none());
    }

    #[test]
    fn whitespace_devuelve_none_ok() {
        assert!(sanitize_error_message(Some("  \t\n  ")).unwrap().is_none());
    }

    #[test]
    fn sin_senso_devuelve_texto() {
        let input = "Backup completed successfully";
        let result = sanitize_error_message(Some(input)).expect("no debería fallar");
        assert_eq!(result, Some(input.to_owned()));
    }

    #[test]
    fn password_con_igual_es_invalido() {
        let result = sanitize_error_message(Some("Password=admin failed"));
        assert!(result.is_err(), "debe fallar con password=valor");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("sensibles"),
            "mensaje debe mencionar datos sensibles"
        );
    }

    #[test]
    fn secret_con_valor_es_invalido() {
        let result = sanitize_error_message(Some("Connection failed: secret=my_key_abc"));
        assert!(result.is_err(), "secret=valor debe ser invalido");
    }

    #[test]
    fn token_jwt_es_invalido() {
        let result = sanitize_error_message(Some("Auth failed: token=eyJhbGciOiJ"));
        assert!(result.is_err(), "token=valor debe ser invalido");
    }

    #[test]
    fn api_key_es_invalido() {
        let result = sanitize_error_message(Some("API call failed: api_key=AKIAIOSFODNN7"));
        assert!(result.is_err(), "api_key=valor debe ser invalido");
    }

    #[test]
    fn ruta_var_es_invalida() {
        let result = sanitize_error_message(Some("Can't read /var/data/secrets/config.yaml"));
        assert!(result.is_err(), "ruta /var/ debe ser invalida");
    }

    #[test]
    fn ruta_home_es_invalida() {
        let result = sanitize_error_message(Some("Permission denied for /home/user/.ssh/id_rsa"));
        assert!(result.is_err(), "ruta /home/ debe ser invalida");
    }

    #[test]
    fn backup_sources_no_es_invalida() {
        let result = sanitize_error_message(Some("Reading from /backup_sources/machine-1/db.dump"));
        assert!(result.is_ok(), "backup_sources no debe ser invalido");
    }

    #[test]
    fn stack_trace_thread_panicked_es_invalido() {
        let input = "thread ''main'' panicked at ''index out of bounds'', src/main.rs:42:5";
        let result = sanitize_error_message(Some(input));
        assert!(result.is_err(), "thread panicked debe ser invalido");
    }

    #[test]
    fn stack_trace_at_mod_linenumber_es_invalido() {
        let input = "Stack trace at module.rs:42:5 in main";
        let result = sanitize_error_message(Some(input));
        assert!(result.is_err(), "at module.rs:line debe ser invalido");
    }

    #[test]
    fn password_comilla_contiene_credencial() {
        // "Password 'admin'" — pattern B detecta password + comilla + valor
        let result = sanitize_error_message(Some("Password 'admin' failed for db user"));
        assert!(
            result.is_err(),
            "Password 'admin' debe contener credenciales y ser invalido"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("sensibles"));
    }

    #[test]
    fn credentials_en_medio_del_mensaje_se_censuran() {
        // Un mensaje que pasa la validación (sin patterns prohibidos) pero tiene
        // credenciales residual como word=value dentro de una oracion mas larga
        // que no fue detectada por la validacion (porque no es standalone).
        // En este caso, el saneado las reemplaza.
        let input = "Error: password=testval, continuing...";
        // Este SI es detectado por Pattern A (password=), asi que falla en validacion.
        let result = sanitize_error_message(Some(input));
        assert!(result.is_err());
    }
}
