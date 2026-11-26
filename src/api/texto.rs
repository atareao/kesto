//! El recorte y la cota de los campos de texto del borde.
//!
//! La tabla del §3 no acota sus columnas `TEXT`, así que sin esta regla un `bucket` de miles de
//! caracteres o un `name` de solo espacios entran en la base y reaparecen más tarde —en un listado,
//! en una etiqueta de instantánea o en una orden del motor—. El recorte va **antes** de medir: los
//! espacios de los extremos no son parte del valor, así que el tope se mide sobre lo que de verdad
//! se guarda.
//!
//! Lo usan las dos puertas de escritura de `/targets` y `/repositories`, y el nombre de las tareas
//! de `/jobs`: son las tres superficies del §5 donde el operador escribe texto que acaba en la
//! base, y una segunda copia de la validación en cada una sería una segunda boca que el día que se
//! separara dejaría pasar por una lo que se rechaza por la otra.
//!
//! Tres reglas, ni una más:
//!
//! * un campo **obligatorio** que falta, o que tras recortar queda vacío, es un `400` que **nombra
//!   el campo** —es la misma regla que la falta del campo—;
//! * un campo **opcional** que se informa vacío o de solo espacios se guarda como `NULL`, que es lo
//!   que el §3 significa para «ausente»; nunca la cadena vacía;
//! * un campo que tras recortar **supera su cota** es un `400` que nombra el campo y **no repite el
//!   valor**: la regla de la casa es que un rechazo diga qué campo está mal, nunca lo que traía.
//!
//! La cota se mide en **caracteres** y no en bytes: contar bytes rechazaría un nombre con acentos
//! que el formulario deja escribir. Los campos de **secreto** (`secret_access_key`, `passphrase`,
//! `db_password`) no pasan por aquí: un secreto no se recorta ni se acota, porque recortarlo lo
//! cambiaría y la cota de un secreto es una cota que la tabla no tiene.

use crate::error::AppError;

/// La cota de `name`, en caracteres.
pub const COTA_NOMBRE: usize = 120;

/// La cota de `container_name`, en caracteres.
pub const COTA_CONTENEDOR: usize = 128;

/// La cota de `db_user`, en caracteres.
pub const COTA_DB_USER: usize = 63;

/// La cota de `db_name`, en caracteres.
pub const COTA_DB_NAME: usize = 63;

/// La cota de `bucket`, en caracteres.
pub const COTA_BUCKET: usize = 63;

/// La cota de `region`, en caracteres.
pub const COTA_REGION: usize = 64;

/// La cota de `access_key_id`, en caracteres.
pub const COTA_ACCESS_KEY: usize = 128;

/// La cota de `endpoint`, en caracteres.
pub const COTA_ENDPOINT: usize = 2048;

/// La cota de `db_path`, en caracteres.
pub const COTA_DB_PATH: usize = 1024;

/// La cota de `host_path`, en caracteres.
pub const COTA_HOST_PATH: usize = 4096;

/// El `400` de un campo que falta o que queda vacío tras recortar.
fn falta(campo: &str) -> AppError {
    AppError::InvalidInput(format!("el campo `{campo}` es obligatorio"))
}

/// El `400` de un campo que tras recortar supera su cota, sin repetir el valor.
fn sobrepasa(campo: &str, maximo: usize) -> AppError {
    AppError::InvalidInput(format!(
        "el campo `{campo}` no puede superar los {maximo} caracteres"
    ))
}

/// Recorta un campo de texto obligatorio y comprueba su cota.
///
/// El valor se recorta **antes** de medirlo, así que un campo de solo espacios es tan ausente como
/// uno que no venga, y el tope se cuenta sobre lo que de verdad se va a guardar. La medida es de
/// **caracteres** y no de bytes.
///
/// # Errors
///
/// Devuelve [`AppError::InvalidInput`] —el `400` del §5— si el campo falta, si tras recortar queda
/// vacío o si supera `maximo` caracteres. El motivo **nombra el campo** y nunca repite el valor.
pub fn obligatorio(campo: &str, valor: Option<&str>, maximo: usize) -> Result<String, AppError> {
    let Some(recortado) = valor.map(str::trim).filter(|texto| !texto.is_empty()) else {
        return Err(falta(campo));
    };

    if recortado.chars().count() > maximo {
        return Err(sobrepasa(campo, maximo));
    }

    Ok(recortado.to_owned())
}

/// Recorta un campo de texto opcional y comprueba su cota.
///
/// Ausente, vacío o de solo espacios se traduce a `None` —que es lo que la columna del §3 guarda
/// como `NULL`—, y presente con contenido se recorta. Es la distinción que hace que «no lo
/// informes» y «lo informo en blanco» acaben en el mismo sitio: el nulo, y nunca la cadena vacía.
///
/// # Errors
///
/// Devuelve [`AppError::InvalidInput`] si el valor recortado supera `maximo` caracteres. El motivo
/// **nombra el campo** y nunca repite el valor.
pub fn opcional(
    campo: &str,
    valor: Option<&str>,
    maximo: usize,
) -> Result<Option<String>, AppError> {
    let Some(recortado) = valor.map(str::trim).filter(|texto| !texto.is_empty()) else {
        return Ok(None);
    };

    if recortado.chars().count() > maximo {
        return Err(sobrepasa(campo, maximo));
    }

    Ok(Some(recortado.to_owned()))
}

/// Recorta y valida un `container_name` contra el juego de caracteres de Docker.
///
/// El nombre se recorta como cualquier otro campo de texto y luego se exige el juego con el que
/// Docker nombra y localiza contenedores: una primera letra alfanumérica seguida de letras, dígitos,
/// `_`, `.` o `-` (`[A-Za-z0-9][A-Za-z0-9_.-]*`).
///
/// El porqué no es de estilo: el cliente de la API de Docker interpola el nombre **sin codificar** en
/// la ruta de la petición (`/containers/{container_name}/exec`), así que un nombre con `/`, `..` o
/// `?` no da un error de nombre —mueve la petición a otro punto de la API de Docker—. Ninguno de
/// esos caracteres está en el juego que Docker admite, y exigirlo cierra la inyección de ruta y, de
/// paso, hace que el nombre sea uno que un contenedor puede tener de verdad. Cubre también los
/// identificadores —largos o cortos—, que son hexadecimales.
///
/// # Errors
///
/// Devuelve [`AppError::InvalidInput`] si el valor recortado supera [`COTA_CONTENEDOR`] caracteres
/// o si no cumple el juego de Docker. El motivo **nombra `container_name`** y nunca repite el valor.
pub fn nombre_de_contenedor(valor: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(recortado) = opcional("container_name", valor, COTA_CONTENEDOR)? else {
        return Ok(None);
    };

    let valido = {
        let mut caracteres = recortado.chars();
        match caracteres.next() {
            Some(primero) if primero.is_ascii_alphanumeric() => caracteres.all(|caracter| {
                caracter.is_ascii_alphanumeric() || matches!(caracter, '_' | '.' | '-')
            }),
            _ => false,
        }
    };

    if !valido {
        return Err(AppError::InvalidInput(
            "el campo `container_name` no es un nombre de contenedor válido".to_owned(),
        ));
    }

    Ok(Some(recortado))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extrae el motivo de un `InvalidInput`, o falla si el error no es ése.
    fn motivo(error: AppError) -> String {
        match error {
            AppError::InvalidInput(motivo) => motivo,
            otro => panic!("se esperaba un `400` de entrada y llegó {otro:?}"),
        }
    }

    #[test]
    fn obligatorio_recorta_y_devuelve_el_texto_limpio() {
        assert_eq!(
            obligatorio("name", Some("  objetivo  "), COTA_NOMBRE).expect("un nombre válido"),
            "objetivo"
        );
    }

    #[test]
    fn obligatorio_rechaza_ausente_vacio_y_blanco_nombrando_el_campo() {
        for valor in [None, Some(""), Some("   "), Some("\t\n")] {
            let mensaje = motivo(
                obligatorio("name", valor, COTA_NOMBRE).expect_err("tiene que faltar el campo"),
            );
            assert!(
                mensaje.contains("name"),
                "el motivo tiene que nombrar el campo: {mensaje:?}"
            );
        }
    }

    #[test]
    fn obligatorio_acepta_justo_en_la_cota() {
        let justo = "a".repeat(COTA_NOMBRE);
        assert_eq!(
            obligatorio("name", Some(&justo), COTA_NOMBRE).expect("cabe justo"),
            justo
        );
    }

    #[test]
    fn obligatorio_rechaza_lo_que_supera_la_cota_sin_repetir_el_valor() {
        let largo = "z".repeat(COTA_NOMBRE + 1);
        let mensaje =
            motivo(obligatorio("name", Some(&largo), COTA_NOMBRE).expect_err("sobra un carácter"));
        assert!(
            mensaje.contains("name"),
            "el motivo tiene que nombrar el campo: {mensaje:?}"
        );
        assert!(
            !mensaje.contains(&largo),
            "el motivo no puede repetir el valor recibido: {mensaje:?}"
        );
    }

    #[test]
    fn la_cota_se_mide_en_caracteres_y_no_en_bytes() {
        let con_acentos = "á".repeat(COTA_DB_USER);
        assert!(
            con_acentos.len() > COTA_DB_USER,
            "cada acentuada ocupa más de un byte en UTF-8"
        );
        assert!(
            obligatorio("db_user", Some(&con_acentos), COTA_DB_USER).is_ok(),
            "una cadena con acentos que cabe en caracteres no puede rechazarse por sus bytes"
        );

        let uno_mas = "á".repeat(COTA_DB_USER + 1);
        assert!(obligatorio("db_user", Some(&uno_mas), COTA_DB_USER).is_err());
    }

    #[test]
    fn opcional_ausente_vacio_o_blanco_es_none() {
        for valor in [None, Some(""), Some("   ")] {
            assert_eq!(
                opcional("db_name", valor, COTA_DB_NAME).expect("no falla"),
                None
            );
        }
    }

    #[test]
    fn opcional_recorta_lo_que_viene() {
        assert_eq!(
            opcional("db_name", Some("  base  "), COTA_DB_NAME).expect("válido"),
            Some("base".to_owned())
        );
    }

    #[test]
    fn opcional_rechaza_lo_que_supera_la_cota() {
        let largo = "b".repeat(COTA_DB_NAME + 1);
        let mensaje =
            motivo(opcional("db_name", Some(&largo), COTA_DB_NAME).expect_err("sobra un carácter"));
        assert!(
            mensaje.contains("db_name"),
            "el motivo tiene que nombrar el campo: {mensaje:?}"
        );
        assert!(
            !mensaje.contains(&largo),
            "el motivo no puede repetir el valor recibido: {mensaje:?}"
        );
    }

    #[test]
    fn nombre_de_contenedor_acepta_el_juego_de_docker() {
        for valor in [
            "contenedor_de-la.base-01",
            "contenedor",
            "a1b2c3d4e5f6",
            "a",
            "0",
        ] {
            assert_eq!(
                nombre_de_contenedor(Some(valor)).expect("válido"),
                Some(valor.to_owned()),
                "`{valor}` es un nombre de contenedor válido"
            );
        }
    }

    #[test]
    fn nombre_de_contenedor_recorta_los_extremos() {
        assert_eq!(
            nombre_de_contenedor(Some("  contenedor  ")).expect("válido"),
            Some("contenedor".to_owned())
        );
    }

    #[test]
    fn nombre_de_contenedor_ausente_o_blanco_es_none() {
        assert_eq!(nombre_de_contenedor(None).expect("no falla"), None);
        assert_eq!(nombre_de_contenedor(Some("   ")).expect("no falla"), None);
    }

    #[test]
    fn nombre_de_contenedor_rechaza_la_inyeccion_de_ruta_sin_repetir_el_valor() {
        for valor in [
            "../../v1.40/containers/x",
            "a/b",
            "..",
            "a?b",
            "-abc",
            ".abc",
            "_abc",
            "a b",
            "ñ",
        ] {
            let mensaje =
                motivo(nombre_de_contenedor(Some(valor)).expect_err("tiene que rechazarse"));
            assert!(
                mensaje.contains("container_name"),
                "el motivo tiene que nombrar el campo: {mensaje:?}"
            );
            assert!(
                !mensaje.contains(valor),
                "el motivo no puede repetir el valor `{valor}`: {mensaje:?}"
            );
        }
    }

    #[test]
    fn nombre_de_contenedor_rechaza_lo_que_supera_la_cota() {
        let largo = "c".repeat(COTA_CONTENEDOR + 1);
        let mensaje = motivo(nombre_de_contenedor(Some(&largo)).expect_err("sobra un carácter"));
        assert!(
            mensaje.contains("container_name"),
            "el motivo tiene que nombrar el campo: {mensaje:?}"
        );
        assert!(
            !mensaje.contains(&largo),
            "el motivo no puede repetir el valor recibido: {mensaje:?}"
        );
    }
}
