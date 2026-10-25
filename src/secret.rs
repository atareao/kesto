//! Secretos que no se filtran al formatearse ni al registro.

use std::fmt;

use serde::{Deserialize, Deserializer};
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

/// El `Deserialize` es **manual**, y el `Serialize` **no** existe. Las dos mitades
/// tienen motivo, y ninguna se deduce leyendo el código.
///
/// El `Deserialize` no se deriva porque [`SecretString`] guarda su valor en un
/// [`Zeroizing<String>`], y el `Deserialize` de `Zeroizing` vive tras la feature
/// **`serde`** de `zeroize` —una dependencia **opcional**, hoy apagada—, así que un
/// `#[derive(Deserialize)]` sobre el tipo del cuerpo no compila tal cual. Encender esa
/// feature resolvería el `derive`, pero activaría la feature para **todo** el grafo que
/// usa `zeroize` —las features de Cargo son aditivas y se unifican por crate—:
/// `crypto-bigint`, `curve25519-dalek`, `der`, `ed25519-dalek`, `elliptic-curve`,
/// `generic-array`, `jsonwebtoken`, `num-bigint-dig`, `rsa`, `rustls`,
/// `rustls-pki-types`, `sec1` y, por encima, `p256`, `p384`, `ecdsa` y `primeorder`.
/// Dejaría una invariante de seguridad de este crate colgando de una feature de
/// terceros por una necesidad local; la implementación son unas pocas líneas y pasa por
/// [`SecretString::new`], de modo que el envoltorio `Zeroizing` no se puede saltar.
///
/// Y **`Serialize` no se implementa**, ni derivado ni a mano: es lo que hace que un
/// tipo de respuesta que llevara una credencial **no compile**. La regla «un secreto no
/// vuelve al cliente» deja así de depender de que alguien se acuerde al escribir el DTO
/// y pasa a ser imposible de escribir. Implementar `Serialize` aquí —aunque fuera para
/// redactar— abriría esa puerta: bastaría con que un campo llegara a un DTO de
/// respuesta para que el valor saliera por el cable.
impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let valor = String::deserialize(deserializer)?;

        Ok(Self::new(valor))
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
