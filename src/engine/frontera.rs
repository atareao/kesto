//! La frontera de rutas: qué puede alcanzar la ruta de un target y qué no.
//!
//! Este módulo es **puro**: no habla con Docker, ni con la red, ni con la base, y del sistema de
//! ficheros solo toca lo imprescindible para canonicalizar. Se prueba sin motor de contenedores.
//! Y es el **único** sitio donde se decide qué es una ruta aceptable: la API comprueba la forma y
//! el motor la canonicaliza, pero la regla —absoluta, sin `..`, bajo la raíz, por componentes— se
//! escribe una sola vez aquí.
//!
//! # Dos comprobaciones, dos momentos
//!
//! - [`forma_de_host_path`] y [`forma_de_db_path`] son la comprobación **textual**: no tocan el
//!   disco ni exigen que la ruta exista, y por eso son las que puede usar el borde HTTP, donde dar
//!   de alta un directorio que todavía no está montado es legítimo.
//! - [`confinar_host_path`] es la comprobación **canónica** del motor: resuelve los enlaces
//!   simbólicos de **todos** los componentes y compara el resultado contra la raíz **también
//!   canonicalizada**. Es la que de verdad cierra el agujero, porque una ruta textualmente
//!   correcta puede escapar por un enlace en un directorio **padre**.
//!
//! # La comparación es por componentes, nunca por prefijo de cadena
//!
//! `/backup_sources_x` **empieza por** `/backup_sources` como texto y **no** está dentro de él. La
//! comparación se hace con [`Path::starts_with`], que compara componentes enteros: un confinamiento
//! por prefijo de cadena deja pasar justo lo que quiere cerrar.
//!
//! # El mensaje nombra el campo, y nunca una ruta
//!
//! Cada error lleva el [`Campo`] que lo provocó y su `Display` nombra `host_path` o `db_path`. El
//! texto no interpola ninguna ruta: lo único que se lee en el mensaje es el nombre del campo que
//! el operador escribió.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// El campo de un target cuya ruta se está comprobando.
///
/// Existe para que el error nombre el campo sin que cada variante tenga que llevar un `&str`
/// suelto: el vocabulario del cable —`host_path`, `db_path`— vive en un solo sitio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Campo {
    /// La ruta de un directorio del host: el `host_path` de un target `directory`.
    HostPath,
    /// La ruta del `.db` dentro de su contenedor: el `db_path` de un target `sqlite`.
    DbPath,
}

impl Campo {
    /// El nombre del campo tal y como lo escribe el operador.
    pub fn nombre(self) -> &'static str {
        match self {
            Self::HostPath => "host_path",
            Self::DbPath => "db_path",
        }
    }
}

/// Una ruta que no cabe en su alcance.
///
/// Las variantes llevan el [`Campo`] y nada más: el `Display` nombra `host_path` o `db_path`, y el
/// valor que escribió el operador no se interpola en ningún caso.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FronteraError {
    /// La ruta no es absoluta.
    #[error("el campo `{}` ha de ser una ruta absoluta", .campo.nombre())]
    NoAbsoluta {
        /// El campo que traía la ruta.
        campo: Campo,
    },

    /// La ruta lleva un `..` en la cadena.
    #[error("el campo `{}` no admite `..`", .campo.nombre())]
    ConPuntoDoble {
        /// El campo que traía la ruta.
        campo: Campo,
    },

    /// La ruta lleva un segmento vacío: un separador de más.
    #[error("el campo `{}` no admite un segmento vacío", .campo.nombre())]
    ConSegmentoVacio {
        /// El campo que traía la ruta.
        campo: Campo,
    },

    /// La ruta queda fuera de la raíz de copias.
    #[error("el campo `{}` ha de quedar bajo la raíz de copias", .campo.nombre())]
    FueraDeLaRaiz {
        /// El campo que traía la ruta.
        campo: Campo,
    },

    /// El valor declarado no coincide con su propia forma canónica dentro del contenedor.
    ///
    /// Es el desenlace de [`coincide_con_su_forma_canonica`] cuando `readlink -f` resuelve la
    /// ruta a otra cosa —un `..`, un enlace en el último componente o un enlace en un
    /// directorio **padre**— o cuando no devuelve exactamente una línea. El `Display` nombra el
    /// campo y **no** interpola ninguna ruta: lo que el operador escribió no se publica.
    #[error(
        "el campo `{}` no coincide con su forma canónica dentro del contenedor",
        .campo.nombre()
    )]
    NoCoincideConSuFormaCanonica {
        /// El campo que traía la ruta.
        campo: Campo,
    },
}

/// La forma textual del `host_path` de un target `directory`.
///
/// Exige que sea **absoluto**, **sin `..`** y que quede **bajo** `raiz`, comparando por componentes.
/// El rechazo de `..` no es cosmético: [`Path::starts_with`] compara componentes, y `..` es un
/// componente válido, así que `/backup_sources/../etc` **pasa** el filtro por prefijo y alcanza lo
/// que no se declara. Sin esta comprobación el alta pasaría y el fallo aparecería en la primera
/// copia; con ella, el operador lo ve en el formulario.
///
/// No toca el disco: una ruta que todavía no existe es una ruta válida —al dar de alta un directorio
/// puede no estar montado aún—, y por eso esta es la comprobación que puede usar el borde HTTP.
///
/// La raíz misma cuenta como «bajo la raíz» —[`Path::starts_with`] compara el prefijo entero—, y
/// es deliberado: copiar la raíz entera es una configuración legítima.
///
/// **No** se exige el segmento vacío: en un `host_path` de directorio una barra final
/// —`/backup_sources/var_www/`— es legítima. Es la diferencia con [`forma_de_db_path`], donde sí se
/// exige porque el valor ha de ser el nombre exacto de un fichero.
pub fn forma_de_host_path(raiz: &Path, declarado: &str) -> Result<(), FronteraError> {
    exigir_absoluta(Campo::HostPath, declarado)?;
    exigir_sin_punto_doble(Campo::HostPath, declarado)?;

    if !Path::new(declarado).starts_with(raiz) {
        return Err(FronteraError::FueraDeLaRaiz {
            campo: Campo::HostPath,
        });
    }

    Ok(())
}

/// La forma textual del `db_path` de un target `sqlite`.
///
/// Exige que sea **absoluto**, sin `..` y sin segmento vacío. La frontera de este campo no es una
/// raíz de Kesto —el `.db` vive en el sistema de ficheros de otro contenedor—, sino el contenedor
/// declarado fuente de copia; aquí solo se cierra la forma, y la coincidencia con su propia forma
/// canónica la comprueba el motor dentro del contenedor.
pub fn forma_de_db_path(declarado: &str) -> Result<(), FronteraError> {
    exigir_absoluta(Campo::DbPath, declarado)?;
    exigir_sin_punto_doble(Campo::DbPath, declarado)?;
    exigir_sin_segmento_vacio(Campo::DbPath, declarado)
}

/// Confina un `host_path` **canónico**: resuelve los enlaces simbólicos de todos sus componentes y
/// exige que el resultado quede bajo `raiz` **también canonicalizada**, por componentes.
///
/// Es la comprobación del motor y la que de verdad cierra el agujero: una ruta textualmente
/// correcta puede escapar por un enlace en un directorio **padre**. Se aplica en las dos
/// direcciones —leer y escribir— y también a las filas anteriores a la regla, porque la API no es
/// el único camino al motor.
///
/// La ruta puede **no existir** —el destino de una restauración es justo lo que todavía no está—:
/// cuando no existe se canonicaliza el anteproyecto existente más profundo y se recompone el resto
/// tal cual. Es el patrón que el motor ya usa en `normalizar_el_destino` para la clave de reserva
/// de una restauración, y aquí vive para que el motor no tenga que reimplementarlo.
///
/// Devuelve la ruta **canónica** ya comprobada —la que usan los tests, que miden justamente la
/// resolución—. El motor, en cambio, la **descarta** a propósito y opera sobre la ruta **declarada**:
/// la instantánea de un directorio conserva «la ruta con la que se montó» —el `compose` del §8 monta
/// el `host_path` dentro de Kesto—, así que leer o escribir con la canónica archivarían o pisarían
/// otra cosa. Los dos llamantes de producción —extracción y restauración— ignoran el valor devuelto
/// por ese mismo motivo.
///
/// Es **más estricta** que la forma del borde HTTP a propósito: aquí `..` se rechaza en vez de
/// resolverse. Resolverlo sería aceptar una ruta que no es la que el operador escribió, y el
/// anteproyecto se recompone textualmente, así que un `..` que sobreviva a la composición podría
/// leerse fuera de la raíz. Un `..` en el motor no tiene nada que hacer: la ruta declarada es la
/// que se copia.
pub fn confinar_host_path(raiz: &Path, declarado: &str) -> Result<PathBuf, FronteraError> {
    exigir_absoluta(Campo::HostPath, declarado)?;
    exigir_sin_punto_doble(Campo::HostPath, declarado)?;

    let canonica = anteproyecto_canonico(Path::new(declarado));

    if !canonica.starts_with(raiz_canonica(raiz)) {
        return Err(FronteraError::FueraDeLaRaiz {
            campo: Campo::HostPath,
        });
    }

    Ok(canonica)
}

/// Comprueba que el `db_path` declarado coincide con su propia forma canónica **dentro del
/// contenedor**.
///
/// `salida` es el `stdout` crudo de `readlink -f <declarado>` ejecutado en el contenedor del
/// target —lo lee el motor con el cliente del daemon, sin `sh -c`—. La comparación es **exacta**:
/// se admite el salto de línea final de `readlink` y nada más.
///
/// Es la comprobación de alcance de este campo, y su frontera **no** es una raíz de Kesto —el
/// `.db` vive en el sistema de ficheros de otro contenedor, del que Kesto no conoce la
/// disposición—, sino el contenedor declarado fuente de copia. Dentro de él, `readlink -f` cierra
/// los tres vectores de una vez: un `..` en la cadena, un enlace en el **último** componente y
/// —el peligroso— un enlace en un directorio **padre**, que un `rm -f` seguiría hasta borrar un
/// fichero ajeno.
///
/// # Por qué una salida vacía o de varias líneas también falla
///
/// Un `stdout` vacío no es «la ruta coincide»: es que no se pudo leer una forma canónica. Y más
/// de una línea tampoco es una ruta —una ruta no lleva un salto de línea dentro—, así que
/// cualquiera de las dos se rechaza en vez de compararse contra el declarado y dar un veredicto
/// que no se puede sostener.
///
/// # Errors
///
/// Devuelve [`FronteraError::NoCoincideConSuFormaCanonica`] —que nombra `db_path`— cuando la
/// salida no es exactamente el valor declarado.
pub fn coincide_con_su_forma_canonica(declarado: &str, salida: &[u8]) -> Result<(), FronteraError> {
    // El `readlink` de GNU y el de busybox terminan su línea con `\n`; en un `\r\n` se admite
    // también el retorno de carro. Solo **uno**: lo que venga detrás ya no es el salto final.
    let sin_salto = salida.strip_suffix(b"\n").unwrap_or(salida);
    let forma = sin_salto.strip_suffix(b"\r").unwrap_or(sin_salto);

    if forma.is_empty() || forma.contains(&b'\n') || forma != declarado.as_bytes() {
        return Err(FronteraError::NoCoincideConSuFormaCanonica {
            campo: Campo::DbPath,
        });
    }

    Ok(())
}

/// Exige que el valor sea una ruta absoluta.
///
/// No se recorta el valor: una ruta que empiece por un espacio no es absoluta y falla, que es lo
/// que quiere un operador que pegó la variable con un espacio de más.
fn exigir_absoluta(campo: Campo, declarado: &str) -> Result<(), FronteraError> {
    if !declarado.starts_with('/') {
        return Err(FronteraError::NoAbsoluta { campo });
    }

    Ok(())
}

/// Exige que el valor no lleve `..` en la cadena.
///
/// Es textual, y a propósito: `..` es lo que resuelve un `rm` o un `open` al recorrer la ruta, y
/// resolverlo aquí a mano —contando niveles— daría una ruta que no es la declarada.
fn exigir_sin_punto_doble(campo: Campo, declarado: &str) -> Result<(), FronteraError> {
    if declarado.split('/').any(|segmento| segmento == "..") {
        return Err(FronteraError::ConPuntoDoble { campo });
    }

    Ok(())
}

/// Exige que el valor no lleve ningún segmento vacío.
///
/// Un segmento vacío es un separador de más, y el `Path` de la biblioteca los absorbe sin decirlo:
/// `Path::new("/datos//base.db")` y `Path::new("/datos/")` se leen como si no estuvieran, así que
/// la comprobación se hace sobre el **texto** —quitando la barra inicial, que sí es la raíz— y no
/// sobre los componentes.
fn exigir_sin_segmento_vacio(campo: Campo, declarado: &str) -> Result<(), FronteraError> {
    let resto = declarado.strip_prefix('/').unwrap_or(declarado);

    if resto.split('/').any(str::is_empty) {
        return Err(FronteraError::ConSegmentoVacio { campo });
    }

    Ok(())
}

/// La forma canónica de una ruta que **puede no existir**: canonicaliza el anteproyecto existente
/// más profundo y recompone el resto tal cual.
///
/// Existe por lo mismo que `normalizar_el_destino` en el motor: [`std::fs::canonicalize`] solo
/// habla de lo que existe, y el destino de una restauración es justamente lo que todavía no está.
/// Se resuelve lo que sí existe —los enlaces simbólicos de los padres incluidos, que son por donde
/// escapa una ruta textualmente correcta— y se deja el resto en su forma textual.
fn anteproyecto_canonico(declarado: &Path) -> PathBuf {
    if let Ok(canonica) = std::fs::canonicalize(declarado) {
        return canonica;
    }

    // Los nombres que no se pudieron resolver, del más profundo al menos: se apilan para
    // recomponerlos en orden inverso sobre el anteproyecto que sí exista.
    let mut pendientes: Vec<OsString> = Vec::new();
    let mut actual = declarado.to_path_buf();

    while let Some(padre) = actual.parent().map(Path::to_path_buf) {
        let Some(nombre) = actual.file_name().map(OsString::from) else {
            break;
        };

        pendientes.push(nombre);

        if let Ok(canonica) = std::fs::canonicalize(&padre) {
            let mut compuesta = canonica;
            compuesta.extend(pendientes.iter().rev());

            return compuesta;
        }

        actual = padre;
    }

    // Sin ningún anteproyecto existente no hay nada que resolver: la ruta se queda como está y la
    // comparación de componentes de quien llama decide.
    declarado.to_path_buf()
}

/// La raíz **canonicalizada**, o la raíz tal cual si no existe todavía.
///
/// Si la raíz no existe —un arranque antes de que el montaje esté—, no hay nada que resolver y se
/// compara contra su forma textual: negarse a confinar por una raíz ausente convertiría un montaje
/// que falta en un motor que no copia nada.
fn raiz_canonica(raiz: &Path) -> PathBuf {
    std::fs::canonicalize(raiz).unwrap_or_else(|_| raiz.to_path_buf())
}

#[cfg(all(test, not(feature = "motor")))]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use super::{
        Campo, FronteraError, coincide_con_su_forma_canonica, confinar_host_path, forma_de_db_path,
        forma_de_host_path,
    };
    use crate::test_support::directorio_de_prueba;

    /// La raíz de copias de las pruebas de forma textual.
    ///
    /// No hace falta que exista: la comparación por componentes es de texto, y lo que se mide es
    /// justo que `/backup_sources_x` no cuele por empezar igual que ella.
    const RAIZ: &str = "/backup_sources";

    #[test]
    fn un_host_path_absoluto_bajo_la_raiz_se_acepta() {
        let resultado = forma_de_host_path(Path::new(RAIZ), "/backup_sources/var_www");

        assert_eq!(
            resultado,
            Ok(()),
            "una ruta bajo la raíz es una ruta válida"
        );
    }

    #[test]
    fn una_raiz_que_solo_coincide_como_prefijo_de_cadena_se_rechaza() {
        let resultado = forma_de_host_path(Path::new(RAIZ), "/backup_sources_x");

        assert_eq!(
            resultado,
            Err(FronteraError::FueraDeLaRaiz {
                campo: Campo::HostPath
            }),
            "`/backup_sources_x` empieza por la raíz como texto y no está dentro de ella"
        );
    }

    #[test]
    fn un_host_path_relativo_se_rechaza() {
        let resultado = forma_de_host_path(Path::new(RAIZ), "backup_sources/var_www");

        assert_eq!(
            resultado,
            Err(FronteraError::NoAbsoluta {
                campo: Campo::HostPath
            }),
            "una ruta de host relativa no dice de qué directorio del host se habla"
        );
    }

    #[test]
    fn un_host_path_con_punto_doble_se_rechaza() {
        let resultado = forma_de_host_path(Path::new(RAIZ), "/backup_sources/../etc");

        assert_eq!(
            resultado,
            Err(FronteraError::ConPuntoDoble {
                campo: Campo::HostPath
            }),
            "`..` es un componente válido para `starts_with`, así que sin rechazarlo el alta pasaría \
             y el fallo aparecería en la primera copia"
        );
    }

    #[test]
    fn la_raiz_misma_es_un_host_path_valido() {
        let resultado = forma_de_host_path(Path::new(RAIZ), RAIZ);

        assert_eq!(
            resultado,
            Ok(()),
            "copiar la raíz entera es una configuración legítima"
        );
    }

    #[test]
    fn un_host_path_con_barra_final_se_acepta() {
        let resultado = forma_de_host_path(Path::new(RAIZ), "/backup_sources/var_www/");

        assert_eq!(
            resultado,
            Ok(()),
            "en un directorio una barra final es legítima: el segmento vacío solo se exige en \
             `db_path`"
        );
    }

    #[test]
    fn un_db_path_con_punto_doble_se_rechaza() {
        let resultado = forma_de_db_path("/datos/../etc/base.db");

        assert_eq!(
            resultado,
            Err(FronteraError::ConPuntoDoble {
                campo: Campo::DbPath
            }),
            "`..` en la cadena es la forma de alcanzar lo que no se declara"
        );
    }

    #[test]
    fn un_db_path_relativo_se_rechaza() {
        let resultado = forma_de_db_path("datos/base.db");

        assert_eq!(
            resultado,
            Err(FronteraError::NoAbsoluta {
                campo: Campo::DbPath
            }),
            "el `.db` se declara con su ruta absoluta dentro del contenedor"
        );
    }

    #[test]
    fn un_db_path_con_barra_final_se_rechaza() {
        let resultado = forma_de_db_path("/datos/");

        assert_eq!(
            resultado,
            Err(FronteraError::ConSegmentoVacio {
                campo: Campo::DbPath
            }),
            "una barra de más es un segmento vacío, no un `.db`"
        );
    }

    #[test]
    fn un_db_path_con_doble_barra_se_rechaza() {
        let resultado = forma_de_db_path("/datos//base.db");

        assert_eq!(
            resultado,
            Err(FronteraError::ConSegmentoVacio {
                campo: Campo::DbPath
            }),
            "una barra doble es un segmento vacío en medio de la ruta"
        );
    }

    #[test]
    fn un_db_path_absoluto_y_sin_ambiguedad_se_acepta() {
        let resultado = forma_de_db_path("/datos/base.db");

        assert_eq!(resultado, Ok(()), "una ruta absoluta y limpia es válida");
    }

    #[test]
    fn un_enlace_que_escapa_de_la_raiz_se_rechaza() {
        let raiz = directorio_de_prueba();
        let fuera = directorio_de_prueba();
        symlink(fuera.ruta(), raiz.ruta().join("enlace")).expect("el enlace de la prueba se crea");

        let declarado = raiz.ruta().join("enlace/fichero");

        let resultado = confinar_host_path(raiz.ruta(), texto(&declarado));

        assert_eq!(
            resultado,
            Err(FronteraError::FueraDeLaRaiz {
                campo: Campo::HostPath
            }),
            "un enlace que apunta fuera de la raíz escapa aunque el texto empiece por ella"
        );
    }

    #[test]
    fn un_enlace_que_apunta_dentro_de_la_raiz_se_acepta() {
        let raiz = directorio_de_prueba();
        std::fs::create_dir(raiz.ruta().join("real")).expect("el directorio real se crea");
        symlink(raiz.ruta().join("real"), raiz.ruta().join("enlace"))
            .expect("el enlace de la prueba se crea");

        let declarado = raiz.ruta().join("enlace/sub");

        let resultado = confinar_host_path(raiz.ruta(), texto(&declarado));

        let esperado = std::fs::canonicalize(raiz.ruta())
            .expect("la raíz de la prueba existe")
            .join("real")
            .join("sub");

        assert_eq!(
            resultado,
            Ok(esperado),
            "un enlace que no sale de la raíz se acepta, y el motor usa la ruta canónica"
        );
    }

    #[test]
    fn una_ruta_que_no_existe_bajo_la_raiz_se_acepta() {
        let raiz = directorio_de_prueba();
        let declarado = raiz.ruta().join("nueva/cosa");

        let resultado = confinar_host_path(raiz.ruta(), texto(&declarado));

        let esperado = std::fs::canonicalize(raiz.ruta())
            .expect("la raíz de la prueba existe")
            .join("nueva")
            .join("cosa");

        assert_eq!(
            resultado,
            Ok(esperado),
            "el destino de una restauración no existe todavía y aun así se confina"
        );
    }

    #[test]
    fn una_ruta_que_no_existe_fuera_de_la_raiz_se_rechaza() {
        let raiz = directorio_de_prueba();
        let fuera = directorio_de_prueba();
        let declarado = fuera.ruta().join("nueva/cosa");

        let resultado = confinar_host_path(raiz.ruta(), texto(&declarado));

        assert_eq!(
            resultado,
            Err(FronteraError::FueraDeLaRaiz {
                campo: Campo::HostPath
            }),
            "que la ruta no exista no la hace admisible: fuera de la raíz es fuera de la raíz"
        );
    }

    #[test]
    fn un_host_path_canonico_con_punto_doble_se_rechaza() {
        let raiz = directorio_de_prueba();
        let declarado = format!("{}/../otra", texto(raiz.ruta()));

        let resultado = confinar_host_path(raiz.ruta(), &declarado);

        assert_eq!(
            resultado,
            Err(FronteraError::ConPuntoDoble {
                campo: Campo::HostPath
            }),
            "`..` en el motor no se resuelve a ciegas: se rechaza"
        );
    }

    #[test]
    fn la_raiz_misma_se_confina_como_bajo_la_raiz() {
        let raiz = directorio_de_prueba();

        let resultado = confinar_host_path(raiz.ruta(), texto(raiz.ruta()));

        let esperado = std::fs::canonicalize(raiz.ruta()).expect("la raíz de la prueba existe");

        assert_eq!(
            resultado,
            Ok(esperado),
            "la raíz misma cuenta como «bajo la raíz»: copiarla entera es una configuración \
             legítima, y la canónica se devuelve resuelta"
        );
    }

    /// El texto de una ruta de prueba, que el arnés construye siempre en UTF-8.
    fn texto(ruta: &Path) -> &str {
        ruta.to_str().expect("las rutas de prueba son UTF-8")
    }

    #[test]
    fn una_forma_canonica_igual_al_declarado_se_acepta() {
        let resultado = coincide_con_su_forma_canonica("/datos/base.db", b"/datos/base.db\n");

        assert_eq!(
            resultado,
            Ok(()),
            "la salida de `readlink -f` coincide con el declarado: es su forma canónica"
        );
    }

    #[test]
    fn el_salto_de_linea_final_no_cuenta() {
        // Sin salto, con `\n` y con `\r\n`: las tres formas que un `readlink` puede dejar.
        for salida in [
            &b"/datos/base.db"[..],
            b"/datos/base.db\n",
            b"/datos/base.db\r\n",
        ] {
            assert_eq!(
                coincide_con_su_forma_canonica("/datos/base.db", salida),
                Ok(()),
                "el salto de línea final de `readlink` no forma parte de la ruta"
            );
        }
    }

    #[test]
    fn una_forma_canonica_distinta_se_rechaza() {
        let resultado = coincide_con_su_forma_canonica("/datos/base.db", b"/otra/base.db\n");

        assert_eq!(
            resultado,
            Err(FronteraError::NoCoincideConSuFormaCanonica {
                campo: Campo::DbPath
            }),
            "un enlace o un `..` cambian la forma canónica: la ruta declarada no es la que se lee"
        );
    }

    #[test]
    fn una_salida_vacia_se_rechaza() {
        let resultado = coincide_con_su_forma_canonica("/datos/base.db", b"");

        assert_eq!(
            resultado,
            Err(FronteraError::NoCoincideConSuFormaCanonica {
                campo: Campo::DbPath
            }),
            "una salida vacía no es una forma canónica que coincida"
        );
    }

    #[test]
    fn una_salida_de_varias_lineas_se_rechaza() {
        let resultado =
            coincide_con_su_forma_canonica("/datos/base.db", b"/datos/base.db\n/otra\n");

        assert_eq!(
            resultado,
            Err(FronteraError::NoCoincideConSuFormaCanonica {
                campo: Campo::DbPath
            }),
            "una ruta no lleva un salto de línea dentro: dos líneas no son una ruta"
        );
    }

    #[test]
    fn el_motivo_de_la_forma_canonica_nombra_el_campo_sin_publicar_la_ruta() {
        let error = coincide_con_su_forma_canonica("/datos/base.db", b"/otra/base.db\n")
            .expect_err("difiere");

        let mensaje = error.to_string();

        assert!(
            mensaje.contains("db_path"),
            "el motivo tiene que nombrar el campo: {mensaje}"
        );
        assert!(
            !mensaje.contains("/datos/base.db") && !mensaje.contains("/otra"),
            "el motivo no puede publicar ninguna ruta: {mensaje}"
        );
    }
}
