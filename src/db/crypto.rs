//! El cifrado en reposo de los tres secretos del §3.
//!
//! Un secreto se guarda como un sobre versionado,
//!
//! ```text
//! v1:<nonce en base64>:<texto cifrado en base64>
//! ```
//!
//! con AES-256-GCM y una clave derivada de `KESTO_MASTER_KEY` por HKDF-SHA256. El
//! prefijo de versión forma parte **del valor** y no de la configuración: es lo que
//! permite que un esquema de cifrado posterior conviva con las filas ya escritas
//! —la capa de descifrado elige por el prefijo— sin un `UPDATE` masivo que exigiría
//! la clave vieja a mano.
//!
//! El nonce es aleatorio **por valor**, nunca fijo ni derivado de la clave:
//! repetirlo con la misma clave rompe el AEAD —revela que dos secretos son iguales
//! y abre la puerta a ataques con texto claro conocido—, así que guardar dos veces
//! el mismo secreto produce dos sobres distintos.
//!
//! **No hay *associated data***: el AEAD no autentica ni la columna ni la fila. Los
//! tres secretos comparten una sola clave y nada ata un sobre a su sitio, así que
//! quien pueda **escribir** en el fichero no puede fabricar un sobre —no tiene la
//! clave— pero **sí copiar uno válido** de una columna o de una fila a otra, y el
//! AEAD lo autenticará: la aplicación leería un secreto legítimo en el sitio
//! equivocado, sin señal. No es divulgación —quien escribe el fichero ya controla el
//! plano de datos— sino **integridad**, y se descarta a sabiendas: atar la columna
//! obligaría a que el `aad` entrara **por parámetro** en `cifrar` y `descifrar`, y
//! decidirlo **después** invalidaría las filas ya escritas. Queda escrito para que
//! quien lo quiera sepa que es un cambio de contrato, no un detalle.
//!
//! La etiqueta es, además, el **dominio de separación**: si otro módulo llegara a
//! cifrar con esta misma clave, tiene que estrenar etiqueta propia.
//!
//! El descifrado **falla en alto**: un prefijo desconocido, un nonce de longitud
//! equivocada, un base64 ilegible, un texto cifrado que el AEAD no autentica o un
//! valor en claro son [`ErrorDeCifrado`], nunca un [`SecretString`] vacío.
//! Devolver la cadena vacía convertiría un fichero corrupto, una clave rotada o una
//! fila manipulada en un repositorio configurado **sin credenciales**, que
//! fallaría más tarde, en el motor, con un error que habla de S3 y no de la base.
//!
//! Ninguna variante del error publica el valor cifrado, el nonce ni el claro: una
//! **longitud** sí, porque no revela el secreto y ayuda al diagnóstico. El sobre lo
//! construye y lo parte **este** módulo: la capa de acceso no arma cadenas con
//! `format!` ni conoce la forma del valor almacenado.

use aes_gcm::aead::Aead;
use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::secret::SecretString;

/// Longitud, en bytes, del nonce de AES-GCM.
const LONGITUD_DEL_NONCE: usize = 12;

/// Prefijo de versión del sobre: es la primera de sus tres partes.
pub const PREFIJO: &str = "v1";

/// Etiqueta `info` de la derivación, constante en el código y NO configurable.
///
/// Cambiarla cambia todas las claves derivadas, y una etiqueta que se toca por
/// variable de entorno es una forma de perder los secretos sin haber rotado nada,
/// así que va aquí y no en la configuración. Termina en [`PREFIJO`] a propósito:
/// la versión del sobre y la de la derivación son la misma, y el test 1.45 la fija
/// **literalmente** —no basta con que «acabe en `v1`»—.
pub const ETIQUETA: &[u8] = b"kesto/secrets/v1";

/// Error del cifrado en reposo.
///
/// Ninguna variante publica el valor cifrado, el nonce ni el valor en claro: una
/// **longitud** sí, porque no revela el secreto y ayuda al diagnóstico.
#[derive(Debug, thiserror::Error)]
pub enum ErrorDeCifrado {
    /// El valor almacenado no tiene la forma `v1:<nonce>:<cifrado>`.
    #[error("el valor almacenado no es un sobre cifrado: no tiene las tres partes esperadas")]
    SobreMalFormado,

    /// El prefijo de versión del sobre no es el que este módulo sabe leer.
    ///
    /// No se interpola lo que se encontró: el mensaje se queda en «no es de esta
    /// versión», que es lo que hace falta para el diagnóstico, y nada del valor
    /// sale del proceso.
    #[error("el sobre cifrado no lleva un prefijo de versión conocido")]
    PrefijoDesconocido,

    /// El nonce no decodifica a la longitud que exige el algoritmo.
    #[error("el nonce del sobre no tiene la longitud esperada: {longitud} bytes")]
    NonceDeLongitudEquivocada {
        /// Longitud, en bytes, del nonce leído. Es una longitud, no el nonce.
        longitud: usize,
    },

    /// Una de las partes del sobre no es base64 válido.
    ///
    /// La causa **sí se interpola**, y es la única del módulo que lo hace: el estilo
    /// de registro del repositorio imprime solo el `Display` —`error = %causa` en
    /// `src/error.rs`—, así que sin interpolarla el operador leería el mensaje
    /// genérico y perdería el desplazamiento, que es lo único que conserva este
    /// error para el registro del servidor.
    ///
    /// No filtra nada: el `Display` de `base64::DecodeError` imprime un valor de
    /// byte y un desplazamiento (`Invalid symbol 33, offset 0.`), **nunca** la cadena
    /// de entrada; y el nonce y la versión del sobre no son secretos —están en la
    /// misma columna, a la vista de quien lea la base—, así que citar un byte no
    /// revela nada que no estuviera ya ahí. El cifrado y el claro, que **sí** son el
    /// secreto, no se interpolan en ninguna variante.
    #[error("una de las partes del sobre no es base64 válido: {causa}")]
    Base64Ilegible {
        /// Motivo, tal y como lo informa `base64`.
        #[source]
        causa: base64::DecodeError,
    },

    /// El texto cifrado no autentica con esta clave.
    ///
    /// No se conserva el error de `aes-gcm` como fuente: es un tipo opaco
    /// —`aead::Error` no lleva mensaje ni longitud—, así que no añadiría nada al
    /// registro. Lo que sí se conserva es el **veredicto** del AEAD: no se traga,
    /// que es la regla del requisito. Un cifrado **truncado** —más corto que el tag
    /// de 16 bytes— cae también aquí.
    #[error(
        "el texto cifrado no autentica: la clave no es la que se usó, el dato se manipuló o está \
         truncado"
    )]
    NoAutentica,

    /// El AEAD no pudo cifrar el secreto, o no hubo entropía para el nonce.
    ///
    /// Ya **no** es una rama formal: la generación del nonce usa `try_fill_bytes`, así
    /// que el fallo del `getrandom` subyacente —un syscall bloqueado por `seccomp`,
    /// un kernel sin él— llega aquí y se devuelve como error tipado en vez de entrar
    /// en pánico y cortar la conexión sin la envoltura JSON. El otro modo de fallo,
    /// el límite de tamaño del AEAD, tampoco es alcanzable con lo que aquí se cifra.
    #[error("no se pudo cifrar el secreto")]
    CifradoFallido,

    /// El texto autenticado no era UTF-8 válido.
    ///
    /// Es una formalidad y no un caso esperado: si el AEAD ha autenticado el
    /// texto, es el que ciframos nosotros —y lo que se cifró fue un `&str`—, así
    /// que solo se llega aquí si el sobre no es de este proceso. Existe para **no**
    /// pasar por `String::from_utf8_lossy`, que devolvería un secreto manipulado y
    /// silencioso; no lleva el valor dentro.
    #[error("el valor descifrado no es texto válido")]
    ClaroNoEsTexto,
}

/// La clave de cifrado, derivada de `KESTO_MASTER_KEY`.
///
/// No publica su material: su [`Debug`] dice que está redactada y no hay ningún
/// camino que lo exponga. La clave no acompaña al dato —no se guarda en la base ni
/// en una variable global— y se deriva una vez por proceso. El material vive en un
/// [`Zeroizing`], así que se borra al caer, como el de [`SecretString`]: es la
/// misma política del crate para todo lo que es secreto.
pub struct Clave(Zeroizing<[u8; 32]>);

impl Clave {
    /// Deriva la clave de la maestra con [`ETIQUETA`].
    ///
    /// No devuelve `Result` a propósito: HKDF-SHA256 con cualquier entrada
    /// funciona, así que no hay ningún fallo que informar.
    pub fn desde_maestra(maestra: &SecretString) -> Self {
        Clave(Zeroizing::new(derivar(maestra, ETIQUETA)))
    }

    /// Cifra un secreto en el sobre `v1:<nonce>:<cifrado>`.
    pub fn cifrar(&self, claro: &SecretString) -> Result<String, ErrorDeCifrado> {
        // Nonce **aleatorio por valor**, del `OsRng` del propio `aead` —su feature
        // `getrandom`, activa por defecto, trae `rand_core`—: ni fijo ni derivado de
        // la clave, que es lo que rompería el AEAD al repetirse. `try_fill_bytes` y
        // no `fill_bytes`: el `fill_bytes` de `rand_core` **entra en pánico** si el
        // `getrandom` subyacente falla —un syscall bloqueado por `seccomp`, un kernel
        // sin él—, y un pánico aquí corta la conexión sin la envoltura JSON.
        let mut nonce = [0u8; LONGITUD_DEL_NONCE];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| ErrorDeCifrado::CifradoFallido)?;

        let cifrado = self
            .cifrador()
            .encrypt(Nonce::from_slice(&nonce), claro.expose().as_bytes())
            .map_err(|_| ErrorDeCifrado::CifradoFallido)?;

        // El sobre se arma aquí y solo aquí; base64 **estándar**, que es la forma
        // que la capa de acceso no necesita conocer.
        Ok(format!(
            "{PREFIJO}:{}:{}",
            STANDARD.encode(nonce),
            STANDARD.encode(cifrado)
        ))
    }

    /// Descifra un sobre `v1:<nonce>:<cifrado>` y devuelve el valor original.
    pub fn descifrar(&self, almacenado: &str) -> Result<SecretString, ErrorDeCifrado> {
        // 1. Exactamente tres partes. Una de menos o de más no es un sobre.
        let mut partes = almacenado.split(':');
        let (Some(prefijo), Some(nonce_en_base64), Some(cifrado_en_base64), None) =
            (partes.next(), partes.next(), partes.next(), partes.next())
        else {
            return Err(ErrorDeCifrado::SobreMalFormado);
        };

        // 2. El prefijo, comparado entero y sin interpolarlo en el error.
        if prefijo != PREFIJO {
            return Err(ErrorDeCifrado::PrefijoDesconocido);
        }

        // 3. El nonce: base64 legible y de la longitud del algoritmo.
        let nonce = STANDARD
            .decode(nonce_en_base64)
            .map_err(|causa| ErrorDeCifrado::Base64Ilegible { causa })?;

        let nonce: [u8; LONGITUD_DEL_NONCE] =
            nonce
                .as_slice()
                .try_into()
                .map_err(|_| ErrorDeCifrado::NonceDeLongitudEquivocada {
                    longitud: nonce.len(),
                })?;

        // 4. El texto cifrado, en base64.
        let cifrado = STANDARD
            .decode(cifrado_en_base64)
            .map_err(|causa| ErrorDeCifrado::Base64Ilegible { causa })?;

        // 5. El AEAD autentica o no: su veredicto no se traga, es el requisito
        //    central del módulo. `from_slice` no puede fallar aquí: el tipo
        //    `[u8; LONGITUD_DEL_NONCE]` fija la longitud que el algoritmo exige.
        let claro = self
            .cifrador()
            .decrypt(Nonce::from_slice(&nonce), cifrado.as_ref())
            .map_err(|_| ErrorDeCifrado::NoAutentica)?;

        // 6. El claro, a `SecretString`. El AEAD ha autenticado el texto y lo que se
        //    cifró fue un `&str`, así que este camino es una formalidad; aun así no
        //    se pasa por `from_utf8_lossy`, que devolvería un secreto manipulado y
        //    silencioso en vez de fallar en alto.
        String::from_utf8(claro)
            .map(SecretString::new)
            .map_err(|_| ErrorDeCifrado::ClaroNoEsTexto)
    }

    /// El cifrador AES-256-GCM con la clave ya derivada.
    ///
    /// `Key::from_slice` **no puede fallar** aquí: la clave es `[u8; 32]` y el tipo
    /// fija la longitud que el algoritmo exige, así que la construcción es
    /// infalible y no hay ningún `Result` que contestar.
    fn cifrador(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.0[..]))
    }
}

impl std::fmt::Debug for Clave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Clave(<redactada>)")
    }
}

/// Deriva una clave de 32 bytes de la maestra con la etiqueta dada.
///
/// Privada y con la etiqueta **por parámetro**: es lo que permite al test 1.45
/// derivar con otra etiqueta y comprobar que la clave cambia, sin exponer la
/// etiqueta a la configuración.
fn derivar(maestra: &SecretString, etiqueta: &[u8]) -> [u8; 32] {
    // La sal va como `None`: la del propio HKDF, la por defecto del algoritmo.
    // Añadir aquí una sal fija no aportaría nada —el IKM ya es secreto— y crearía
    // un segundo sitio donde versionar, cuando lo que versiona es la etiqueta.
    let hkdf = Hkdf::<Sha256>::new(None, maestra.expose().as_bytes());

    let mut material = [0u8; 32];

    // `expand` solo falla si la salida excede 255·32 bytes; con 32 no puede. No se
    // usa `expect` —no hay pánicos en producción— y el resultado se descarta a
    // sabiendas de que, con esta longitud, es siempre `Ok`.
    let _ = hkdf.expand(etiqueta, &mut material);

    material
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Clave, ETIQUETA, ErrorDeCifrado, PREFIJO};
    use crate::secret::SecretString;
    use crate::test_support::capturar_log;

    /// Clave maestra de las pruebas: larga y distinguible, para que un `contains`
    /// sobre el registro no dé un falso positivo.
    const MAESTRA: &str = "clave-maestra-crypto-distinguible-000000000000000000000000";
    /// Otra clave maestra, distinguible de [`MAESTRA`]: es la del límite aceptado
    /// de la rotación (test 1.46).
    const OTRA_MAESTRA: &str = "otra-clave-maestra-crypto-distinguible-11111111111111111111";

    /// Un `SecretString` con el valor dado.
    fn secreto(valor: &str) -> SecretString {
        SecretString::new(valor.to_owned())
    }

    /// La clave de cifrado derivada de la maestra dada.
    fn clave_de(maestra: &str) -> Clave {
        Clave::desde_maestra(&secreto(maestra))
    }

    /// Cambia un carácter del **texto cifrado** del sobre, dejando prefijo y nonce
    /// intactos: el primer carácter del alfabeto base64, que cambia el primer byte
    /// del cifrado sin romper el base64 —la longitud y el relleno no se tocan—.
    fn cambiar_un_byte_del_cifrado(sobre: &str) -> String {
        let partes: Vec<&str> = sobre.split(':').collect();
        let mut cifrado: Vec<char> = partes[2].chars().collect();

        cifrado[0] = if cifrado[0] == 'A' { 'B' } else { 'A' };

        format!(
            "{}:{}:{}",
            partes[0],
            partes[1],
            cifrado.into_iter().collect::<String>()
        )
    }

    /// 1.37 — El valor guardado empieza por `v1:` y tiene tres partes.
    #[test]
    fn el_valor_guardado_lleva_prefijo_y_tres_partes() {
        let clave = clave_de(MAESTRA);

        let almacenado = clave
            .cifrar(&secreto("AKIAIOSFODNN7EXAMPLE-1-37"))
            .expect("cifrar un secreto no puede fallar");

        assert!(
            almacenado.starts_with(&format!("{PREFIJO}:")),
            "el sobre no empieza por `{PREFIJO}:`"
        );

        let partes: Vec<&str> = almacenado.split(':').collect();
        assert_eq!(
            partes.len(),
            3,
            "el sobre debe tener tres partes separadas por ':'"
        );
    }

    /// 1.38 — Dos cifrados del mismo secreto son distintos: el nonce es aleatorio
    /// **por valor**, no fijo ni derivado de la clave. Se afirma sobre el nonce y
    /// sobre el **texto cifrado**, que es lo que pide el escenario.
    #[test]
    fn dos_cifrados_del_mismo_secreto_son_distintos() {
        let clave = clave_de(MAESTRA);
        let valor = secreto("el-mismo-secreto-para-los-dos-cifrados");

        let primero = clave
            .cifrar(&valor)
            .expect("el primer cifrado no puede fallar");
        let segundo = clave
            .cifrar(&valor)
            .expect("el segundo cifrado no puede fallar");

        assert_ne!(
            primero, segundo,
            "los dos sobres del mismo secreto son iguales: el nonce no es aleatorio"
        );

        let parte_de = |sobre: &str, indice: usize| -> String {
            sobre
                .split(':')
                .nth(indice)
                .expect("el sobre tiene el prefijo, el nonce y el cifrado")
                .to_owned()
        };

        assert_ne!(
            parte_de(&primero, 1),
            parte_de(&segundo, 1),
            "los dos nonces son iguales: repetir un nonce con la misma clave rompe el AEAD"
        );
        assert_ne!(
            parte_de(&primero, 2),
            parte_de(&segundo, 2),
            "los dos textos cifrados son iguales"
        );

        // Una pareja no distingue un generador bueno de uno de entropía baja: 64
        // cifrados del mismo secreto tienen que dar 64 nonces distintos.
        let nonces: HashSet<String> = (0..64)
            .map(|_| {
                let sobre = clave
                    .cifrar(&valor)
                    .expect("cifrar no puede fallar con una clave derivada en pie");

                parte_de(&sobre, 1)
            })
            .collect();

        assert_eq!(
            nonces.len(),
            64,
            "64 cifrados del mismo secreto no dieron 64 nonces distintos: \
             el nonce no es aleatorio por valor"
        );
    }

    /// 1.39 — `descifrar(cifrar(x))` devuelve el original **sin** el prefijo.
    #[test]
    fn la_ida_y_vuelta_devuelve_el_valor_original_sin_prefijo() {
        let clave = clave_de(MAESTRA);
        let original = "AKIAIOSFODNN7EXAMPLE-ida-y-vuelta";

        let almacenado = clave
            .cifrar(&secreto(original))
            .expect("cifrar no puede fallar");

        let recuperado = clave
            .descifrar(&almacenado)
            .expect("descifrar lo que se acaba de cifrar no puede fallar");

        assert_eq!(
            recuperado.expose(),
            original,
            "el valor devuelto no es el original"
        );
        assert!(
            !recuperado.expose().contains(&format!("{PREFIJO}:")),
            "el prefijo de versión no debe aparecer en el valor devuelto"
        );
    }

    /// 1.40 — Un prefijo desconocido devuelve error y **no** un secreto vacío.
    #[test]
    fn un_prefijo_desconocido_no_se_descifra() {
        let clave = clave_de(MAESTRA);

        for sobre in [
            "v2:AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB",
            "x:AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB",
            "v10:AAAAAAAAAAAAAAAAAAAAAA==:BBBBBBBBBBBBBBBBBBBBBB",
        ] {
            let error = clave
                .descifrar(sobre)
                .expect_err("un prefijo desconocido no puede dar un secreto");

            assert!(
                matches!(error, ErrorDeCifrado::PrefijoDesconocido),
                "el fallo debe ser el prefijo desconocido, no otro: {error}"
            );
        }
    }

    /// 1.41 — Un nonce de longitud equivocada devuelve error y no un secreto vacío.
    ///
    /// El sobre se fabrica a mano: el nonce va en base64 y decodifica a 8 y a 16
    /// bytes, ninguna de las dos la longitud de AES-GCM. Se afirma también la
    /// longitud que el error dice, que es el dato que la variante existe para llevar.
    #[test]
    fn un_nonce_de_longitud_equivocada_no_se_descifra() {
        let clave = clave_de(MAESTRA);

        for (nonce, esperada) in [("AAAAAAAAAAA=", 8), ("AAAAAAAAAAAAAAAAAAAAAA==", 16)] {
            let sobre = format!("{PREFIJO}:{nonce}:{nonce}");

            let error = clave
                .descifrar(&sobre)
                .expect_err("un nonce de longitud equivocada no puede dar un secreto");

            assert!(
                matches!(
                    error,
                    ErrorDeCifrado::NonceDeLongitudEquivocada { longitud }
                        if longitud == esperada
                ),
                "el fallo debe ser el nonce de longitud equivocada y decir cuál ({esperada}): {error}"
            );
        }
    }

    /// 1.42 — Un texto cifrado con **un byte cambiado** no autentica —es el AEAD—
    /// y no devuelve un secreto parcial.
    #[test]
    fn un_texto_cifrado_manipulado_no_se_descifra() {
        let clave = clave_de(MAESTRA);

        let almacenado = clave
            .cifrar(&secreto("secreto-manipulable-1-42"))
            .expect("cifrar no puede fallar");
        let manipulado = cambiar_un_byte_del_cifrado(&almacenado);

        assert_ne!(
            manipulado, almacenado,
            "el aparato de la prueba no cambió nada del sobre"
        );

        let error = clave
            .descifrar(&manipulado)
            .expect_err("un texto cifrado manipulado no puede dar un secreto");

        assert!(
            matches!(error, ErrorDeCifrado::NoAutentica),
            "el fallo debe ser el AEAD que no autentica, no otro: {error}"
        );
    }

    /// 1.43 — Un valor **en claro**, sin sobre, devuelve error y **no** ese valor
    /// como si fuera un secreto legítimo.
    #[test]
    fn un_valor_en_claro_no_se_descifra() {
        let clave = clave_de(MAESTRA);
        let en_claro = "AKIAIOSFODNN7EXAMPLE-en-claro-sin-sobre";

        let error = clave
            .descifrar(en_claro)
            .expect_err("un valor en claro no puede darse por descifrado");

        assert!(
            matches!(
                error,
                ErrorDeCifrado::SobreMalFormado | ErrorDeCifrado::PrefijoDesconocido
            ),
            "el fallo debe ser un sobre que no lo es, no otro: {error}"
        );
    }

    /// 1.44 — El mensaje del error **no** contiene el valor cifrado, ni el nonce,
    /// ni el valor en claro: los tres se buscan literalmente.
    #[test]
    fn el_mensaje_del_error_no_publica_el_cifrado_ni_el_nonce_ni_el_claro() {
        let clave = clave_de(MAESTRA);
        let en_claro = "AKIAIOSFODNN7EXAMPLE-no-debe-salir-1-44";

        let almacenado = clave
            .cifrar(&secreto(en_claro))
            .expect("cifrar no puede fallar");

        let partes: Vec<&str> = almacenado.split(':').collect();
        let nonce = partes[1].to_owned();

        let manipulado = cambiar_un_byte_del_cifrado(&almacenado);
        let cifrado_de_la_entrada = manipulado.split(':').nth(2).unwrap_or_default().to_owned();

        let error = clave
            .descifrar(&manipulado)
            .expect_err("el cifrado manipulado no puede descifrarse");
        let mensaje = error.to_string();

        assert!(
            !mensaje.contains(&cifrado_de_la_entrada),
            "el mensaje publica el texto cifrado: {mensaje}"
        );
        assert!(
            !mensaje.contains(partes[2]),
            "el mensaje publica el texto cifrado original: {mensaje}"
        );
        assert!(
            !mensaje.contains(&nonce),
            "el mensaje publica el nonce: {mensaje}"
        );
        assert!(
            !mensaje.contains(en_claro),
            "el mensaje publica el valor en claro: {mensaje}"
        );
    }

    /// Un sobre cuya parte no es base64 válido: la variante que conserva la causa y
    /// que hasta ahora no ejercía ningún test.
    ///
    /// Es un test **extra**, más allá de los once de las tareas: cubre la variante
    /// `Base64Ilegible`, que ninguno de los once tocaba, y comprueba a la vez que
    /// interpolar la causa **no** publica la parte ilegible.
    #[test]
    fn la_parte_que_no_es_base64_no_se_descifra_ni_publica_la_parte() {
        let clave = clave_de(MAESTRA);
        let almacenado = format!("{PREFIJO}:no-es-base64:cualquiera");

        let error = clave
            .descifrar(&almacenado)
            .expect_err("una parte que no es base64 no puede dar un secreto");

        assert!(
            matches!(error, ErrorDeCifrado::Base64Ilegible { .. }),
            "el fallo debe ser la parte ilegible, no otro: {error}"
        );

        let mensaje = error.to_string();
        assert!(
            !mensaje.contains("no-es-base64"),
            "el mensaje publica la parte ilegible: {mensaje}"
        );
    }

    /// 1.45 — La derivación es determinista, **cambiar la etiqueta la cambia**, y
    /// [`ETIQUETA`] termina en [`PREFIJO`].
    ///
    /// Es la invariante que impide que la versión del sobre y la de la derivación
    /// se separen: si divergen, un cambio de esquema deja de ser un cambio de
    /// versión y pasa a ser una pérdida de secretos.
    #[test]
    fn la_derivacion_es_determinista_y_la_etiqueta_la_cambia() {
        let maestra = secreto(MAESTRA);

        let primera = super::derivar(&maestra, ETIQUETA);
        let segunda = super::derivar(&maestra, ETIQUETA);

        assert_eq!(
            primera, segunda,
            "dos derivaciones con la misma maestra y la misma etiqueta difieren"
        );

        let con_otra_etiqueta = super::derivar(&maestra, b"kesto/secrets/otra");

        assert_ne!(
            primera, con_otra_etiqueta,
            "cambiar la etiqueta no cambió la clave derivada"
        );

        // La etiqueta es **literal**: no basta con que «acabe en `v1`». Cambiarla
        // aquí tiene que doler, porque cambiarla en producción deja ilegibles todos
        // los secretos ya guardados y el síntoma aparece más tarde, en el motor,
        // hablando de S3.
        assert_eq!(
            ETIQUETA, b"kesto/secrets/v1",
            "la etiqueta de derivación cambió: los secretos ya guardados quedarían ilegibles"
        );
    }

    /// El material derivado, contra un valor conocido y calculado aparte (RFC 5869).
    ///
    /// Es un **known-answer test**: fija de una vez el algoritmo (HKDF-SHA256), la
    /// sal (la del propio HKDF, 32 ceros) y la etiqueta. Si alguien cambia
    /// cualquiera de los tres, esto se pone rojo aquí y no en producción.
    #[test]
    fn el_material_derivado_es_el_de_hkdf_sha256_con_esta_sal() {
        let material = super::derivar(&secreto(MAESTRA), ETIQUETA);

        assert_eq!(
            material,
            [
                0x74, 0xf0, 0x86, 0x11, 0x88, 0xf1, 0xb4, 0x23, 0xdd, 0x12, 0x7e, 0x34, 0x5d, 0xde,
                0x32, 0xac, 0xca, 0x70, 0x4f, 0x9d, 0xb0, 0x1e, 0x77, 0x10, 0x6e, 0x47, 0xa3, 0x56,
                0xf8, 0x1b, 0x47, 0x38,
            ],
            "el material derivado no es el de HKDF-SHA256 con sal por defecto y esta etiqueta"
        );
    }

    /// 1.46 — Con **otra** `KESTO_MASTER_KEY` el secreto no se descifra.
    ///
    /// El límite es **aceptado**: no hay clave de datos envuelta por una clave
    /// maestra rotable, así que rotar la maestra deja ilegible lo ya guardado. Lo
    /// que este test fija es que ese límite **falla en alto** —error de descifrado—
    /// y no devuelve un secreto vacío, que sería un repositorio configurado sin
    /// credenciales que revienta más tarde, en el motor, hablando de S3.
    #[test]
    fn otra_clave_maestra_deja_el_secreto_ilegible() {
        let en_claro = "secreto-que-no-sobrevive-a-la-rotacion-1-46";

        let almacenado = clave_de(MAESTRA)
            .cifrar(&secreto(en_claro))
            .expect("cifrar con la maestra original no puede fallar");

        let error = clave_de(OTRA_MAESTRA)
            .descifrar(&almacenado)
            .expect_err("con otra maestra el secreto no puede leerse");

        assert!(
            matches!(error, ErrorDeCifrado::NoAutentica),
            "el fallo debe ser el AEAD que no autentica con otra clave: {error}"
        );
    }

    /// 1.50 — Con `RUST_LOG=trace` no aparece ni el valor de `KESTO_MASTER_KEY` ni
    /// el de la clave derivada, y las operaciones de alta y lectura con secreto no
    /// filtran ni el claro ni la forma cifrada.
    ///
    /// El escenario es «`RUST_LOG=trace` **y una operación de alta y otra de lectura
    /// con secreto**», así que se cifra y se descifra de verdad: un
    /// `tracing::trace!` metido **dentro** de `cifrar` —el fallo realista— lo caza
    /// esta prueba, y sin ejecutar las operaciones no lo cazaría.
    ///
    /// Usa el arnés de captura **por hilo** de `src/test_support.rs`, nunca
    /// `set_default` ni `with_default`: un despachante de hilo deja un callsite
    /// cacheado como `Interest::never` para todo el proceso y el registro
    /// desaparece **en silencio**.
    #[test]
    fn el_registro_no_publica_la_maestra_ni_la_clave_derivada() {
        let (buffer, _guard) = capturar_log();

        let maestra = secreto(MAESTRA);
        let clave = Clave::desde_maestra(&maestra);
        // El material esperado, para buscar su forma impresa en el registro: si el
        // `Debug` de `Clave` dejara de redactar, el `?clave` de abajo lo publicaría.
        let derivada = super::derivar(&maestra, ETIQUETA);

        // Alta y lectura con secreto, dentro de la captura.
        let en_claro = "AKIAIOSFODNN7EXAMPLE-no-debe-salir-1-50";
        let sobre = clave
            .cifrar(&secreto(en_claro))
            .expect("cifrar no puede fallar");
        let leido = clave
            .descifrar(&sobre)
            .expect("descifrar lo que se acaba de cifrar no puede fallar");

        tracing::trace!(?clave, "clave de cifrado derivada de la maestra");
        tracing::trace!(?maestra, "clave maestra cargada");

        let log = buffer.texto();

        assert_eq!(
            leido.expose(),
            en_claro,
            "la ida y vuelta debe devolver el mismo valor"
        );

        // El candado de ausencia solo vale si la captura está viva: sin esta
        // aserción, un registro que no llegara al buffer pasaría por bueno.
        assert!(
            log.contains("clave de cifrado derivada de la maestra"),
            "la captura por hilo no recibió el registro, así que no prueba nada: {log}"
        );

        // Y el `Debug` de la clave **se formateó**, redactado: sin esta aserción, el
        // candado probaría que el evento llegó, pero no que el valor no salió.
        assert!(
            log.contains("Clave(<redactada>)"),
            "el `Debug` de la clave no aparece redactado en el registro: {log}"
        );

        assert!(
            !log.contains(MAESTRA),
            "el registro publica el valor de la clave maestra: {log}"
        );

        assert!(
            !log.contains(en_claro),
            "el registro publica el valor en claro del secreto: {log}"
        );

        assert!(
            !log.contains(&sobre),
            "el registro publica el sobre cifrado: {log}"
        );

        let huella = format!("{derivada:?}");
        assert!(
            !log.contains(&huella),
            "el registro publica el material de la clave derivada: {log}"
        );
    }

    /// La clave se comparte entre tareas: el módulo que la consuma la tendrá en un
    /// `Arc`, así que `Send + Sync` es parte del contrato.
    ///
    /// Es un candado de contrato para el módulo 2 —la capa de acceso—: si alguien
    /// metiera un `Rc<RefCell<…>>` dentro, el fallo aparecería al montar el estado y
    /// no aquí.
    #[test]
    fn la_clave_es_compartible_entre_hilos() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<Clave>();
    }
}
