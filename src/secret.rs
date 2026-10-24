//! Secretos que no se filtran al formatearse ni al registro.

use std::fmt;

use zeroize::Zeroizing;

/// Cadena que guarda una credencial.
///
/// Redacta su contenido en `Debug` y en `Display`, lo expone únicamente por
/// [`SecretString::expose`] y borra la memoria que ocupaba cuando el valor cae.
#[derive(Clone)]
pub struct SecretString {
    /// El valor, puesto a cero al caer por `zeroize`.
    inner: Zeroizing<String>,
}

/// Marca que sustituye al valor en cualquier formato.
pub const REDACTED: &str = "<redactado>";

impl SecretString {
    /// Envuelve `value` para que no pueda filtrarse al formatearse.
    pub fn new(value: String) -> Self {
        Self {
            inner: Zeroizing::new(value),
        }
    }

    /// Devuelve el valor de la credencial.
    ///
    /// Es el único camino por el que el contenido sale del tipo: que sea un
    /// método con nombre es lo que hace visible cada sitio que lo usa.
    pub fn expose(&self) -> &str {
        self.inner.as_str()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SecretString").field(&REDACTED).finish()
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::SecretString;

    /// Valor distinguible: si aparece en una salida, la salida es el fallo.
    const VALOR: &str = "s3cr3t-muy-largo-y-distinguible";

    #[test]
    fn debug_no_revela_el_secreto() {
        let secreto = SecretString::new(VALOR.to_owned());

        let salida = format!("{secreto:?}");

        assert!(!salida.contains(VALOR), "Debug publicó el valor: {salida}");
        assert!(
            salida.to_lowercase().contains("redact"),
            "Debug no indica que el valor está redactado: {salida}"
        );
    }

    #[test]
    fn display_no_revela_el_secreto() {
        let secreto = SecretString::new(VALOR.to_owned());

        let salida = format!("{secreto}");

        assert!(
            !salida.contains(VALOR),
            "Display publicó el valor: {salida}"
        );
    }

    #[test]
    fn el_valor_solo_sale_por_el_metodo_dedicado() {
        let secreto = SecretString::new(VALOR.to_owned());

        assert_eq!(secreto.expose(), VALOR);
    }
}
