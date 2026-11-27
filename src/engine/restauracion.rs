//! La restauración: por dónde vuelve la copia y con qué orden.
//!
//! Este módulo es el **espejo** de [`crate::engine::extraccion`] y responde a la misma pregunta
//! desde el otro lado: si allí [`ruta`](crate::engine::extraccion::ruta) decidía **por dónde sale**
//! el volcado y con qué comando, aquí [`comando`] decide **por dónde vuelve** y con qué orden.
//!
//! Tiene las **tres** piezas del espejo, y en el mismo orden que su hermano: [`comando`] compone
//! el camino —y [`destino_del_objetivo`] la clave del destino que dos restauraciones no pueden
//! compartir—, el puente ([`tunel_de_restauracion`]) une el `Write` bloqueante de la biblioteca con
//! el `stdin` asíncrono del `exec`, y [`ejecutar_las_ordenes`] conduce los `exec` contra ese
//! `stdin`. Lo que **no** hace ninguna de las tres: leer el repositorio ni decidir el destino del
//! sistema de ficheros —eso es el motor—, ni escribir nada en disco. Que el contenedor exista o
//! que el cliente de la base esté dentro se averigua al ejecutar, no aquí.
//!
//! # El reverso no es simétrico
//!
//! Cada volcado se deshace con el cliente que lo produjo, sobre el alcance que le dio su
//! `db_name`:
//!
//! - `mariadb-dump --all-databases` se deshace con `mariadb` **sin base nombrada** —el propio
//!   volcado lleva los `CREATE DATABASE`— y con `mariadb <db_name>` cuando el volcado fue de una
//!   sola base.
//! - `pg_dumpall` se deshace con `psql` sobre una base **a la que conectarse**, y `postgres` es la
//!   que siempre existe en un servidor PostgreSQL. `pg_dump -d base` se deshace con `psql -d base`.
//! - `sqlite3 .dump` se deshace **reconstruyendo el fichero**: el `.dump` emite `CREATE TABLE`
//!   sin `DROP TABLE`, así que aplicarlo sobre la base viva chocaría con las tablas existentes.
//!   Por eso son **dos** órdenes y en ese orden: [`retirar`](CaminoDeRestauracion::PorContenedor)
//!   primero y reimportar después.
//!
//! # La credencial, en el entorno y nunca en la línea de comandos
//!
//! La contraseña viaja en el **entorno** del `exec` —`MYSQL_PWD` para MariaDB y `PGPASSWORD` para
//! PostgreSQL—, igual que en la extracción, y por la misma razón: en un contenedor el listado de
//! procesos es público. La construcción del entorno **no se reimplementa aquí**: es
//! [`entorno_del_exec`](crate::engine::extraccion::entorno_del_exec), que es la única boca que
//! decide el nombre de la variable y que una contraseña vacía no entra. `sqlite` no usa
//! credenciales —un fichero SQLite no tiene usuario ni contraseña— y su entorno va vacío.

use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, BufReader};

use crate::db::entities::{ObjetivoDeRestauracion, TipoDeTarget};
use crate::docker::{Canal, Cliente, Orden, Trozo};
use crate::engine::extraccion::{
    acumular_el_motivo, comprobar_la_forma_canonica_del_db_path, entorno_del_exec,
    lo_que_dijo_el_contenedor, motivo_para_guardar, nombre_del_tipo, recortar,
};
use crate::engine::frontera::{self, FronteraError};
use crate::engine::troceador::{Instruccion, Troceador};

use super::ErrorDelMotor;

/// Por dónde vuelve la copia.
///
/// Es el espejo de [`Ruta`](crate::engine::extraccion::Ruta) y conserva sus dos desenlaces: un
/// destino que vive **dentro** de un contenedor —y entonces hay órdenes que ejecutar— y un
/// directorio del sistema de ficheros, que el motor escribe por su cuenta y en el que **no** se
/// ejecuta ningún comando.
///
/// El [`Debug`] se deriva y es seguro: la variante de contenedor lleva [`Orden`]es, cuyo `Debug`
/// publica el comando y los **nombres** de las variables de entorno, nunca sus valores —la
/// credencial va en el valor—. Eso está fijado en el transporte y aquí se hereda.
///
/// No se deriva `Clone`, por lo mismo que [`Ruta`](crate::engine::extraccion::Ruta): una orden
/// copiada es una credencial copiada, y un camino se construye una vez por restauración.
#[derive(Debug)]
pub enum CaminoDeRestauracion {
    /// Un proceso —o varios— dentro del contenedor del target.
    PorContenedor {
        /// Nombre del contenedor en el que hay que ejecutar las órdenes.
        contenedor: String,
        /// Las órdenes **en el orden en que hay que ejecutarlas**.
        ///
        /// Hoy son una para las dos bases y dos para un `sqlite` —retirar el fichero y
        /// reimportar—. El orden es contrato y no una comodidad: reimportar antes de retirar
        /// choca con las tablas vivas, y el `.dump` no lleva `DROP TABLE` que lo salve.
        ordenes: Vec<Orden>,
        /// El `db_path` declarado cuando el tipo es `sqlite`, para comprobar su forma canónica
        /// dentro del contenedor **antes** de ejecutar la primera orden —la que **retira** el
        /// fichero y sus hermanos—; `None` en los demás tipos.
        ///
        /// Es lo que hace que un `db_path` que no sea su propia forma canónica **no borre nada**:
        /// la retirada es un `rm -f`, y un `..` o un enlace en un directorio padre la llevarían a
        /// borrar un fichero ajeno. La comprobación —un `readlink -f` dentro del contenedor, sin
        /// `sh -c`— la conduce [`ejecutar_las_ordenes`].
        db_path: Option<String>,
        /// Cómo se reparte el volcado entre las órdenes: entero a la última —MariaDB y SQLite— o
        /// troceado por base —PostgreSQL—. Ver [`Reparto`] para el porqué de la diferencia.
        reparto: Reparto,
    },

    /// Un directorio del sistema de ficheros, tal como lo ve este proceso.
    ///
    /// Un target de tipo `directory` **no** pasa por Docker: no hay proceso que ejecutar. La
    /// ruta es la de [`ObjetivoDeRestauracion::host_path`], que es el destino que el `compose`
    /// del §8 monta dentro de Kesto, y la escribe el motor con el origen local de la biblioteca.
    PorDirectorio {
        /// Ruta del directorio de destino, en el espacio de ficheros de este proceso.
        ruta: PathBuf,
    },
}

/// Cómo se reparte el volcado entre las órdenes de una restauración por contenedor.
///
/// Es la diferencia entre los dos desenlaces de [`CaminoDeRestauracion::PorContenedor`]: una base
/// de servidor que se reimporta con **un solo** cliente recibe el volcado entero por el `stdin` de
/// su única orden —la última—, y un PostgreSQL, cuyo volcado global separa las bases con
/// `\connect`, se reparte en **secciones** —una por base— porque el único modo de `psql` que
/// rechaza metacomandos, `\restrict`, **bloquea `\connect`**: anteponerlo a un volcado global
/// desviaría los datos a la base equivocada. Trocear por base permite ignorar los metacomandos de
/// cada sección sin perder el `\connect`.
///
/// El [`Debug`] se deriva y es seguro por la misma razón que el de [`CaminoDeRestauracion`]: lo que
/// lleva dentro son [`Orden`]es, cuyo `Debug` publica el comando y los **nombres** de las variables
/// de entorno, nunca sus valores —la credencial va en el valor—. Eso está fijado en el transporte y
/// aquí se hereda.
#[derive(Debug)]
pub enum Reparto {
    /// El volcado entero va al `stdin` de la **última** orden; las de antes corren con la entrada
    /// cerrada. Es el desenlace de MariaDB y de SQLite.
    ALaUltimaOrden,

    /// El volcado se trocea por base y cada sección va al `stdin` de su propia orden, compuesta a
    /// partir de `plantilla`.
    ///
    /// **Precondición obligatoria:** el cliente del contenedor ha de honrar `\restrict` —`psql`
    /// 17.6, o el mínimo de su rama: 16.10, 15.14, 14.19, 13.22—. Sin él, un volcado hostil
    /// conseguiría ejecutar como `root` en el contenedor destino, así que la restauración **falla**
    /// en vez de degradarse: [`ejecutar_por_secciones`] comprueba la versión con el mismo binario
    /// antes de ejecutar nada.
    PorSeccionesDePostgres {
        /// La orden de `psql` **sin** la base: su `cmd` es `["psql", "-X", "-U", <usuario>, "-d"]` y
        /// cada sección le añade su base como **valor** de `-d`, nunca como argumento suelto —un
        /// nombre que empiece por `-` no puede leerse como una opción—. Su `env` lleva la
        /// credencial, que es lo que hace que el reparto se guarde como [`Orden`] y no como una
        /// lista de cadenas.
        plantilla: Orden,
        /// La base de la **primera** sección: el `db_name` del target o, si es nulo, `postgres`.
        ///
        /// La primera sección del volcado no viene precedida de ningún `\connect`, así que hay que
        /// saber a qué base entregarla.
        base_inicial: String,
    },
}

/// Elige el camino y compone las órdenes de la restauración.
///
/// Del sistema de ficheros solo toca lo imprescindible para canonicalizar el `host_path` de un
/// directorio, y no habla con nadie: decide **por dónde** vuelve la copia y con **qué** órdenes, y
/// nada más. Ver el encabezado del módulo para el comando de cada tipo y el porqué de cada uno.
///
/// `raiz_de_copias` es la frontera del `host_path` de un target de directorio: la ruta se
/// **canonicaliza** —resolviendo los enlaces simbólicos de todos sus componentes, también los de
/// un directorio padre— y se rechaza si el resultado no queda bajo esa raíz. Es lo que impide que
/// una restauración escriba —o, con la confirmación de sobrescritura, **pode**— fuera de la raíz
/// de copias: el `host_path` es destino de escritura y de poda, así que una ruta fuera de la raíz
/// podría sobrescribir el propio estado de Kesto. Se comprueba con la ruta canónica y se opera con
/// la **declarada**. Ver [`crate::engine::frontera`] para la regla entera.
///
/// # Errors
///
/// Devuelve [`ErrorDeRestauracion::CampoQueFalta`] si el objetivo no trae un campo que su tipo
/// necesita —`container_name` y `db_user` para una base, `container_name` y `db_path` para un
/// `sqlite`, `host_path` para un directorio—, [`ErrorDeRestauracion::CampoEnBlanco`] si lo trae
/// informado y no sirve, y [`ErrorDeRestauracion::ValorQuePareceBandera`] si el valor de un campo
/// que acaba en la línea de comandos empieza por `-`. En los tres casos **no se construye nada**:
/// un objetivo incoherente no es una orden a medias.
///
/// Y devuelve [`ErrorDeRestauracion::FueraDeLaFrontera`] cuando el `host_path` de un directorio
/// queda fuera de `raiz_de_copias` —canonicalizado y comparado por componentes—: el alcance de la
/// ruta se aplica también a las filas anteriores a la regla, porque la API no es el único camino
/// al motor.
pub fn comando(
    raiz_de_copias: &Path,
    objetivo: &ObjetivoDeRestauracion,
) -> Result<CaminoDeRestauracion, ErrorDeRestauracion> {
    match objetivo.target_type {
        // Un target de directorio **no pasa por Docker**: no hay ningún proceso que ejecutar. La
        // restauración escribe ficheros con el origen local de la biblioteca, y el destino es la
        // ruta del host —el `compose` del §8 la monta dentro de Kesto—, que es la única lectura
        // con datos detrás: la otra necesitaría un mapeo host→contenedor que nadie ha dado.
        TipoDeTarget::Directory => {
            let ruta = exigido(objetivo, "host_path", objetivo.host_path.as_deref())?;

            // La comprobación es **canónica** y vive en `frontera`, que es el único sitio donde se
            // decide qué es una ruta aceptable. Va **antes** de construir el camino: una ruta fuera
            // de la raíz no llega a escribir ni a podar nada, que es lo que la restauración haría
            // con ella. Se comprueba con la canónica y se opera con la **declarada**.
            frontera::confinar_host_path(raiz_de_copias, ruta)?;

            Ok(CaminoDeRestauracion::PorDirectorio {
                ruta: PathBuf::from(ruta),
            })
        }

        // El reverso de `mariadb-dump --all-databases` es el cliente **sin base nombrada**: el
        // volcado global lleva sus `CREATE DATABASE`, y nombrar una base aquí la elegiría como
        // destino por defecto de las sentencias que traigan tabla sin base.
        TipoDeTarget::Mariadb => {
            let (contenedor, usuario, base) = datos_de_una_base(objetivo)?;

            let mut cmd = vec![
                "mariadb".to_owned(),
                // El modo binario es lo que hace que el cliente **no interprete** sus
                // metacomandos: el volcado lo escribe quien controla el repositorio, así que se
                // trata como contenido no confiable.
                "--binary-mode".to_owned(),
                format!("-u{usuario}"),
            ];

            // Y aquí `db_name` decide el alcance, igual que decidió el del volcado: `NULL` es el
            // servidor entero y un nombre es esa sola base.
            if let Some(base) = base {
                cmd.push(base.to_owned());
            }

            Ok(CaminoDeRestauracion::PorContenedor {
                contenedor: contenedor.to_owned(),
                ordenes: vec![Orden {
                    cmd,
                    env: entorno_del_exec(TipoDeTarget::Mariadb, objetivo.db_password.as_ref()),
                }],
                // Una base de servidor no lleva ninguna ruta de fichero en su comando, así que no
                // hay forma canónica que comprobar.
                db_path: None,
                reparto: Reparto::ALaUltimaOrden,
            })
        }

        // El reverso de `pg_dumpall` es `psql` sobre una base **a la que conectarse**, y
        // `postgres` es la que existe en cualquier servidor: `psql` sin `-d` caería en la base
        // por defecto del usuario, que no es la que el operador cree.
        //
        // Pero un volcado global **no** se reimporta con un solo `psql`: separa las bases con
        // `\connect`, y el único modo de `psql` que rechaza metacomandos —`\restrict`— **bloquea
        // `\connect`**, así que anteponerlo a un volcado global desviaría los datos a la base
        // equivocada. Por eso el camino **no** trae órdenes fijas: trae una plantilla de `psql` y
        // la base de la primera sección, y el motor compone una orden por sección conforme lee los
        // `\connect` del volcado. Ver [`Reparto`] y [`ejecutar_por_secciones`].
        TipoDeTarget::Postgresql => {
            let (contenedor, usuario, base) = datos_de_una_base(objetivo)?;

            // La plantilla **sin** la base: cada sección le añade la suya como valor de `-d`. El
            // `-X` es lo que hace que `psql` ignore su fichero de arranque `~/.psqlrc`, que podría
            // traer metacomandos de fuera del volcado.
            let plantilla = Orden {
                cmd: vec![
                    "psql".to_owned(),
                    "-X".to_owned(),
                    "-U".to_owned(),
                    usuario.to_owned(),
                    "-d".to_owned(),
                ],
                env: entorno_del_exec(TipoDeTarget::Postgresql, objetivo.db_password.as_ref()),
            };

            Ok(CaminoDeRestauracion::PorContenedor {
                contenedor: contenedor.to_owned(),
                // Sin órdenes fijas: el motor compone una por sección.
                ordenes: Vec::new(),
                // Una base de servidor no lleva ninguna ruta de fichero en su comando.
                db_path: None,
                reparto: Reparto::PorSeccionesDePostgres {
                    plantilla,
                    // La primera sección del volcado no viene precedida de ningún `\connect`, así
                    // que su base es la que declaró el operador, o `postgres` si no declaró ninguna.
                    base_inicial: base.unwrap_or("postgres").to_owned(),
                },
            })
        }

        // Un `sqlite` **reconstruye el fichero**, y por eso son dos órdenes y en este orden: el
        // `.dump` emite `CREATE TABLE` sin `DROP TABLE`, así que `sqlite3 <db_path>` sobre el
        // fichero vivo chocaría con las tablas que ya están. Y los hermanos `-wal` y `-shm` se
        // retiran con él: con el modo WAL activo, un diario viejo aplicado sobre un fichero nuevo
        // reinyecta páginas que ya no son de nadie.
        //
        // Ni usuario ni contraseña ni nombre de base: un fichero SQLite no tiene ninguno de los
        // tres, así que su entorno va vacío.
        TipoDeTarget::Sqlite => {
            let contenedor = exigido(
                objetivo,
                "container_name",
                objetivo.container_name.as_deref(),
            )?;
            let db_path = exigido(objetivo, "db_path", objetivo.db_path.as_deref())?;

            // La ruta acaba en la línea de comandos como argumento posicional de las dos
            // órdenes, y un `db_path` que empezara por `-` lo leería `sqlite3` —o `rm`— como una
            // opción. La guarda es la misma que la del usuario y el nombre de base.
            let db_path = sin_ser_bandera(objetivo, "db_path", db_path)?;

            let retirada = Orden {
                cmd: vec![
                    "rm".to_owned(),
                    "-f".to_owned(),
                    db_path.to_owned(),
                    format!("{db_path}-wal"),
                    format!("{db_path}-shm"),
                ],
                env: Vec::new(),
            };

            let reimportacion = Orden {
                // `-safe` es lo que hace que `sqlite3` **no interprete** sus metacomandos
                // —`.shell`, `.read`—: el `.dump` lo escribe quien controla el repositorio, así que
                // se trata como contenido no confiable. Requiere SQLite ≥ 3.27; si el contenedor no
                // lo trae, la orden falla en voz alta y la restauración no se da por buena.
                cmd: vec!["sqlite3".to_owned(), "-safe".to_owned(), db_path.to_owned()],
                env: Vec::new(),
            };

            Ok(CaminoDeRestauracion::PorContenedor {
                contenedor: contenedor.to_owned(),
                ordenes: vec![retirada, reimportacion],
                // El `db_path` viaja con el camino para que el flujo compruebe su forma canónica
                // dentro del contenedor **antes** de la retirada: un `..` o un enlace —en el último
                // componente o en un directorio padre— cambiarían a qué fichero llega el `rm -f`, y
                // un `db_path` que no sea su propia forma canónica no puede borrar nada.
                db_path: Some(db_path.to_owned()),
                reparto: Reparto::ALaUltimaOrden,
            })
        }
    }
}

/// La **clave de destino** de un objetivo: lo que dos restauraciones no pueden compartir.
///
/// Es el espejo de [`Objetivo`] para la guarda de exclusión: un destino de directorio es su
/// `host_path`, y el de una base es **el contenedor junto con la base** —el servidor entero cuando
/// `db_name` es nulo, el fichero cuando es un `sqlite`—. La clave lleva delante el tipo de target
/// porque el mismo contenedor puede alojar un MariaDB y un PostgreSQL: son dos servidores distintos
/// y el mismo nombre de contenedor no los hace el mismo destino.
///
/// El separador es `:` y no una barra a propósito: una ruta del sistema de ficheros sí lleva
/// barras, y un destino de base es un nombre, no una ruta.
///
/// # Errors
///
/// Las mismas que [`comando`] para los campos que el tipo necesita: un objetivo sin `host_path` —o
/// sin contenedor, o sin la base de un `sqlite`— no puede nombrar su destino, y **no se inventa uno
/// vacío**, que haría que dos objetivos incoherentes se bloquearan entre sí sin ser el mismo sitio.
pub fn destino_del_objetivo(
    objetivo: &ObjetivoDeRestauracion,
) -> Result<String, ErrorDeRestauracion> {
    match objetivo.target_type {
        TipoDeTarget::Directory => {
            Ok(exigido(objetivo, "host_path", objetivo.host_path.as_deref())?.to_owned())
        }

        // El usuario no entra en la clave: dos filas del mismo contenedor con distinto `db_user`
        // apuntan al **mismo** servidor, y bloquearlas por separado dejaría dos restauraciones
        // escribiendo a la vez sobre los mismos datos.
        TipoDeTarget::Mariadb | TipoDeTarget::Postgresql => {
            let (contenedor, _usuario, base) = datos_de_una_base(objetivo)?;

            Ok(clave_de_destino(objetivo.target_type, contenedor, base))
        }

        TipoDeTarget::Sqlite => {
            let contenedor = exigido(
                objetivo,
                "container_name",
                objetivo.container_name.as_deref(),
            )?;
            let db_path = exigido(objetivo, "db_path", objetivo.db_path.as_deref())?;

            Ok(clave_de_destino(
                objetivo.target_type,
                contenedor,
                Some(db_path),
            ))
        }
    }
}

/// Compone la clave de un destino de base: el tipo, el contenedor y la base.
///
/// `base` vacía es el servidor entero de un `mariadb` o el `postgres` de un `psql`, así que se
/// escribe tal cual: lo que distingue dos destinos es el par que sí la nombra.
fn clave_de_destino(tipo: TipoDeTarget, contenedor: &str, base: Option<&str>) -> String {
    format!(
        "{}:{contenedor}:{}",
        nombre_del_tipo(tipo),
        base.unwrap_or_default()
    )
}

/// Los tres campos que un target de base necesita, o el error del que falta.
///
/// Es el espejo de la comprobación de la extracción y por eso no se comparte con ella: aquélla
/// informa [`ErrorDeExtraccion`](crate::engine::extraccion::ErrorDeExtraccion) y ésta
/// [`ErrorDeRestauracion`], y un ayudante genérico sobre dos errores distintos tendría que recibir
/// un constructor de error por parámetro para acabar diciendo lo mismo. Se comprueban **antes** de
/// construir nada y en el mismo orden que allí —contenedor, usuario, base—, de modo que el mismo
/// objetivo incoherente da el mismo motivo en los dos sentidos.
fn datos_de_una_base(
    objetivo: &ObjetivoDeRestauracion,
) -> Result<(&str, &str, Option<&str>), ErrorDeRestauracion> {
    let contenedor = exigido(
        objetivo,
        "container_name",
        objetivo.container_name.as_deref(),
    )?;
    let usuario = exigido(objetivo, "db_user", objetivo.db_user.as_deref())?;
    let base = campo_opcional(objetivo, "db_name", objetivo.db_name.as_deref())?;

    // El usuario y la base acaban en un `argv`, así que pasan por la guarda de las banderas. El
    // `container_name` **no** —es un parámetro de la API, no un argumento—.
    let usuario = sin_ser_bandera(objetivo, "db_user", usuario)?;
    let base = base
        .map(|base| sin_ser_bandera(objetivo, "db_name", base))
        .transpose()?;

    Ok((contenedor, usuario, base))
}

/// Rechaza un valor que la línea de comandos leería como una bandera.
///
/// Es la guarda de los campos que acaban en la `cmd` —`db_user`, `db_name` y `db_path`—, por la
/// misma razón que en la extracción: no hay intérprete de por medio, así que un `db_name` como
/// `-d` se pega como argumento suelto y **el cliente lo lee como una opción**, con lo que el
/// alcance que decidió la fila del §3 se salta sin que nada lo note. No se cita, ni se recorta, ni
/// se escapa: una corrección silenciosa convertiría un error de la fila en una orden distinta de
/// la que el operador cree haber pedido.
fn sin_ser_bandera<'a>(
    objetivo: &ObjetivoDeRestauracion,
    campo: &'static str,
    valor: &'a str,
) -> Result<&'a str, ErrorDeRestauracion> {
    if valor.starts_with('-') {
        return Err(ErrorDeRestauracion::ValorQuePareceBandera {
            tipo: nombre_del_tipo(objetivo.target_type),
            campo,
        });
    }

    Ok(valor)
}

/// Un campo que el tipo exige: o su valor, o el error que dice cuál falta.
///
/// El valor se devuelve **tal cual se informó**: se rechaza el que está en blanco —no puede
/// nombrar un contenedor ni una ruta—, pero no se recorta por dentro. Recortar una ruta o un
/// nombre de base sería cambiar en silencio lo que el operador escribió.
fn exigido<'a>(
    objetivo: &ObjetivoDeRestauracion,
    campo: &'static str,
    valor: Option<&'a str>,
) -> Result<&'a str, ErrorDeRestauracion> {
    let tipo = nombre_del_tipo(objetivo.target_type);

    match valor {
        None => Err(ErrorDeRestauracion::CampoQueFalta { tipo, campo }),

        // El que solo tiene espacios no nombra nada: un contenedor `" "` no es un contenedor y una
        // ruta `"   "` no es una ruta. Se distingue del ausente para que el motivo mande a la fila
        // del §3 —el campo está informado— y no a un «no existe» del motor.
        Some(valor) if valor.trim().is_empty() => {
            Err(ErrorDeRestauracion::CampoEnBlanco { tipo, campo })
        }

        Some(valor) => Ok(valor),
    }
}

/// Un campo que el §3 declara anulable: `None` es legítimo y significa lo que dice el §3.
///
/// `db_name` es el caso: `NULL` significa el servidor entero —o el `postgres` de `psql`— y no un
/// hueco que rellenar. Lo que sí se rechaza es un `Some` en blanco, que no es `NULL` ni una base.
fn campo_opcional<'a>(
    objetivo: &ObjetivoDeRestauracion,
    campo: &'static str,
    valor: Option<&'a str>,
) -> Result<Option<&'a str>, ErrorDeRestauracion> {
    match valor {
        None => Ok(None),

        Some(valor) if valor.trim().is_empty() => Err(ErrorDeRestauracion::CampoEnBlanco {
            tipo: nombre_del_tipo(objetivo.target_type),
            campo,
        }),

        Some(valor) => Ok(Some(valor)),
    }
}

/// El error de la restauración: lo que puede fallar antes de tener una orden que ejecutar.
///
/// Es el espejo de [`ErrorDeExtraccion`](crate::engine::extraccion::ErrorDeExtraccion), y se llama
/// así —y no `Error`— por la misma razón que aquél: el motor nombra a la vez su error, el del
/// transporte y el de la biblioteca, y el nombre más corto no puede quedarse con el que menos lo
/// merece.
///
/// El mensaje va en español, nombra el **campo** que falla y **no publica el valor**: lo que hace
/// falta para encontrar la fila que se editó mal es el campo, y el valor de un objetivo puede ser
/// una contraseña. Ninguna variante lleva la credencial ni la imprime.
#[derive(Debug, Error)]
pub enum ErrorDeRestauracion {
    /// El objetivo no trae un campo que su tipo necesita.
    #[error(
        "a un objetivo de tipo `{tipo}` le falta `{campo}`: sin él no hay restauración que valga"
    )]
    CampoQueFalta {
        /// Tipo del objetivo, con el texto que guarda el `CHECK` del §3.
        tipo: &'static str,
        /// El campo sin informar.
        campo: &'static str,
    },

    /// El objetivo trae el campo, pero no sirve para nada.
    ///
    /// Informado y en blanco **no** es lo mismo que ausente: un `container_name` con espacios
    /// llegaría al motor y el fallo aparecería como «ese contenedor no existe», que manda a
    /// buscar el problema en el motor en vez de en la fila que se editó mal.
    #[error(
        "el `{campo}` de un objetivo de tipo `{tipo}` está informado y en blanco: no nombra nada"
    )]
    CampoEnBlanco {
        /// Tipo del objetivo, con el texto que guarda el `CHECK` del §3.
        tipo: &'static str,
        /// El campo informado y vacío.
        campo: &'static str,
    },

    /// El objetivo trae un valor que la línea de comandos leería como una bandera.
    ///
    /// Los campos que acaban en la línea de comandos —`db_user`, `db_name` y `db_path`— los
    /// escribe un operador autenticado, pero eso no los hace inofensivos: no hay intérprete de
    /// por medio, así que un valor como `-d` se pega como argumento suelto y el cliente lo lee
    /// como una opción, de modo que el alcance de la restauración cambia sin que nada lo note.
    /// Se rechaza en vez de citarse: una corrección silenciosa convertiría un error de la fila en
    /// una orden distinta de la que el operador cree haber pedido. Y el valor **no** se publica.
    #[error(
        "el `{campo}` de un objetivo de tipo `{tipo}` empieza por `-`: la línea de comandos lo \
         leería como una bandera y la restauración cambiaría sin que nada lo note"
    )]
    ValorQuePareceBandera {
        /// Tipo del objetivo, con el texto que guarda el `CHECK` del §3.
        tipo: &'static str,
        /// El campo cuyo valor parece una bandera.
        campo: &'static str,
    },

    /// La ruta de un campo no se puede usar: queda fuera de su frontera.
    ///
    /// Es el desenlace de [`frontera::confinar_host_path`] cuando el `host_path` de un directorio
    /// no queda bajo la raíz de copias —resueltos sus enlaces simbólicos—. El motivo de
    /// [`FronteraError`] **nombra el campo** —`host_path`— y **no** publica ninguna ruta: es la
    /// parte que el operador necesita para encontrar la fila que se editó mal, y lo que no escribió
    /// no sale de aquí. Se construye con el `?` desde el error de la frontera, que es lo que
    /// conserva el campo sin reescribirlo.
    #[error("{causa}")]
    FueraDeLaFrontera {
        /// El motivo de la frontera, con el campo nombrado y sin publicar la ruta.
        #[from]
        causa: FronteraError,
    },
}

// ── El puente: del `Write` bloqueante de la biblioteca al `stdin` asíncrono ──
//
// La restauración tiene que unir dos mundos que no se hablan. Por un lado, la biblioteca de copia
// pide un `impl std::io::Write` **bloqueante** para volcar el nodo de la instantánea
// ([`Repository::dump`](rustic_core::Repository::dump)). Por otro, el `stdin` del `exec` del motor
// de contenedores es un `impl AsyncRead` **asíncrono** que se escribe con `await`.
//
// Es el espejo de [`crate::rustic::TunelDelVolcado`], y de ahí sale todo lo que aquí se repite:
// un canal **acotado** entre las dos partes —para que la memoria no dependa de cuál vaya más
// rápida— y el final declarado con una **señal**, nunca como último mensaje del canal (un mensaje
// de final se pierde justo cuando el canal está lleno al terminar). Lo que **cambia** es el
// sentido: en la extracción el lado bloqueante es el que **lee** (la biblioteca archiva) y el lado
// asíncrono el que **escribe** (el `exec` produce); aquí el lado bloqueante **escribe** (la
// biblioteca vuelca) y el lado asíncrono **lee** (el `exec` consume).
//
// Y de ese cambio de sentido sale la única asimetría de verdad: el error del lado bloqueante. En
// la extracción el error del lector lo silencia la biblioteca, y por eso la entrega completa viaja
// como una consulta aparte ([`EntregaDelVolcado`](crate::rustic::EntregaDelVolcado)). Aquí el lado
// que consume es el motor de contenedores, que **sí** propaga el error de
// [`LectorDelVolcado`] —`ejecutar_con_entrada` lo devuelve—, así que un volcado que el repositorio
// no pudo producir entero tiene que llegarle **como error y no como fin de fichero limpio**. Para
// eso, quien vuelca declara su fallo en una ranura compartida ([`EscritorDelVolcado::fallar`]) que
// el lector lee al ver el canal cerrado sin señal de final.
//
// [`Repository::dump`]: rustic_core::Repository::dump
// [`EntregaDelVolcado`]: crate::rustic::EntregaDelVolcado

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::io::ReadBuf;
use tokio::sync::mpsc;

/// Las dos mitades del puente, más nada: el escritor bloqueante y el lector asíncrono.
///
/// Se construye con [`tunel_de_restauracion`] y se desestructura para repartir cada mitad a su
/// lado: el escritor al hilo bloqueante que vuelca el repositorio, el lector al `stdin` del
/// `exec`. No se deriva `Debug` porque lo único que se podría publicar de cada mitad es su
/// nombre, que ya está en el de las dos —y que se imprime en los motivos de error—.
pub struct TunelDeRestauracion {
    /// La mitad que se le entrega a [`Repository::dump`](rustic_core::Repository::dump).
    pub escritor: EscritorDelVolcado,
    /// La mitad que consume el `stdin` del `exec`, ya como [`AsyncRead`].
    pub lector: LectorDelVolcado,
}

/// Construye las dos mitades del puente de restauración.
///
/// `nombre` es la identidad del volcado para los motivos —no existe en ningún sistema de ficheros,
/// así que su único nombre es el que le da quien lo pide— y `capacidad` es el número de **trozos**
/// que puede retener el canal antes de que el escritor tenga que esperar. El número entra por
/// parámetro **a propósito** —igual que en la extracción—: quien lo elija está decidiendo cuánta
/// memoria puede gastar el motor, y eso no lo puede decidir este módulo.
///
/// # `capacidad` está en trozos, no en bytes
///
/// El canal retiene hasta `capacidad` **mensajes**, así que la memoria retenida es `capacidad × el
/// tamaño de un trozo`, no una cantidad de bytes: quien elija el número tiene que multiplicar.
/// `capacidad == 0` es un **canal de encuentro**: el `blocking_send` del escritor no vuelve hasta
/// que haya una lectura en curso, así que no retiene memoria ninguna y frena al escritor a la
/// velocidad exacta del `stdin`.
///
/// # El escritor tiene que correr en otro hilo, y fuera del runtime
///
/// Dos restricciones, y las dos las sufre [`EscritorDelVolcado::write`]: es **bloqueante** —espera
/// hueco en el canal— y usa `blocking_send`, que **entra en pánico** si se le llama desde dentro de
/// un runtime de Tokio. El único sitio correcto es un hilo de bloqueo (`spawn_blocking` o un
/// `std::thread` pelado), que es donde corre el motor.
pub fn tunel_de_restauracion(nombre: impl Into<String>, capacidad: usize) -> TunelDeRestauracion {
    let nombre = nombre.into();

    // `channel` y no `sync_channel`: la mitad consumidora es asíncrona y espera con `await`, que es
    // lo que el runtime sabe aparcar; un canal de `std` obligaría al lector a bloquear un hilo.
    let (emisor, receptor) = mpsc::channel(capacidad);

    let terminado_bien = Arc::new(AtomicBool::new(false));
    let fallo = Arc::new(Mutex::new(None));

    TunelDeRestauracion {
        escritor: EscritorDelVolcado {
            nombre: nombre.clone(),
            emisor,
            terminado_bien: Arc::clone(&terminado_bien),
            fallo: Arc::clone(&fallo),
        },
        lector: LectorDelVolcado {
            nombre,
            receptor,
            sobrante: Vec::new(),
            consumido: 0,
            terminado_bien,
            fallo,
        },
    }
}

/// El extremo por el que la biblioteca **escribe** el volcado: el único que existe.
///
/// Implementa [`std::io::Write`] —es lo que [`Repository::dump`](rustic_core::Repository::dump)
/// pide— y entrega cada trozo al canal acotado con un `blocking_send`, que es la contrapresión
/// que acota la memoria. No se puede clonar: el canal se cierra cuando muere el **último** emisor,
/// y que este tipo sea el único que lo tiene, y que [`terminar_bien`](Self::terminar_bien) lo
/// consuma, es lo que hace imposible el cuelgue por un emisor retenido.
pub struct EscritorDelVolcado {
    /// El nombre del volcado, para poder nombrarlo en el motivo de un fallo.
    nombre: String,
    /// El extremo de envío, que cierra el canal al morir.
    emisor: mpsc::Sender<Vec<u8>>,
    /// La señal que [`terminar_bien`](Self::terminar_bien) pone y que el lector consulta al ver el
    /// cierre. Es la que distingue un final bueno de un volcado que se quedó a medias.
    terminado_bien: Arc<AtomicBool>,
    /// La ranura del motivo de un fallo, que el lector lee al ver el cierre sin señal.
    ///
    /// Existe porque el error del volcado **no** lo ve este tipo: lo devuelve
    /// [`Repository::dump`](rustic_core::Repository::dump) a quien lo llamó, y quien lo llamó es el
    /// hilo del motor. Sin esta ranura, un fallo del repositorio llegaría al `stdin` como un cierre
    /// limpio —un fin de fichero— y la restauración parecería buena con el volcado a medias.
    fallo: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for EscritorDelVolcado {
    /// Publica el nombre del volcado y nada de lo que viaja por el canal.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EscritorDelVolcado")
            .field("nombre", &self.nombre)
            .finish_non_exhaustive()
    }
}

impl std::io::Write for EscritorDelVolcado {
    /// Entrega un trozo al canal. **Bloquea** si el canal está lleno: esa espera es la
    /// contrapresión que acota la memoria, no un problema.
    ///
    /// Un trozo **vacío** no se envía: el canal solo transporta datos, y un mensaje sin datos no
    /// puede significar nada —el final no viaja por aquí—. Descartarlo aquí es lo que permite que
    /// el lector trate cualquier mensaje recibido como progreso.
    ///
    /// Devuelve error solo si el lector ha soltado el canal —el `exec` se rindió, el contenedor
    /// desapareció—: no es un fallo del volcado, significa «ya nadie lee», y lo que toca es que la
    /// biblioteca deje de volcar. [`Repository::dump`](rustic_core::Repository::dump) propaga ese
    /// error a quien lo llamó, que es el que decide qué contar.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let bytes = buf.len();
        let trozo = buf.to_vec();

        self.emisor.blocking_send(trozo).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                format!(
                    "el lector del volcado `{}` soltó el canal: ya nadie consume el volcado, así \
                     que el repositorio deja de volcarlo",
                    self.nombre
                ),
            )
        })?;

        Ok(bytes)
    }

    /// No hay nada que vaciar: lo que se entrega al canal se entrega entero en [`write`](Self::write).
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl EscritorDelVolcado {
    /// Declara el volcado **terminado bien** y cierra el canal. Consume: después de esto no queda
    /// ningún emisor, que es lo que hace que el lector vea el final.
    ///
    /// El orden de las dos cosas es el que importa: primero la señal —con `Release`, para que el
    /// lector, que la lee con `Acquire` cuando ve el canal cerrado, la vea puesta— y después el
    /// `drop` del extremo, que es lo que cierra el canal. Al revés, el lector podría ver el canal
    /// cerrado antes que la señal y dar por truncado un volcado que llegó entero.
    ///
    /// El defecto va en la dirección segura: quien termine su bucle **sin** llamar a esto —porque
    /// se cayó, o porque se olvidó— deja la señal en falso, y el lector trata el cierre del canal
    /// como un volcado **truncado**. Falla cerrado. Un volcado de cero bytes entregado con esto es
    /// una entrega completa: quien decide si un volcado vacío es un problema es el motor, que mira
    /// el código de salida del cliente de la base.
    pub fn terminar_bien(self) {
        self.terminado_bien.store(true, Ordering::Release);

        // El `drop` de `self` —al final de esta función, con el extremo de envío dentro— es lo que
        // cierra el canal. Y el orden no es negociable: primero la señal, después el cierre.
    }

    /// Declara que el volcado **no** se pudo producir entero, con el motivo que dijo el repositorio.
    ///
    /// Consume y **no** pone la señal del final, que es lo que hace que el lector vea el cierre del
    /// canal como un fallo —y con este motivo— en vez de como un fin de fichero limpio. Es lo que
    /// llama el hilo del motor cuando [`Repository::dump`](rustic_core::Repository::dump) devuelve
    /// un error: sin esta llamada, un fallo del repositorio se convertiría, al otro lado, en una
    /// restauración que parece buena con el volcado a medias.
    pub fn fallar(self, motivo: impl Into<String>) {
        // Si la ranura está envenenada no se puede hacer nada mejor que dejar el motivo fuera: la
        // señal sigue en falso, así que el lector falla igual y nombra el volcado. Se recupera el
        // candado en vez de entrar en pánico, por la misma razón que en el lector.
        match self.fallo.lock() {
            Ok(mut ranura) => *ranura = Some(motivo.into()),
            Err(veneno) => *veneno.into_inner() = Some(motivo.into()),
        }

        // El `drop` de `self` cierra el canal sin haber puesto la señal del final.
    }
}

/// El extremo por el que el `stdin` del `exec` **lee** el volcado, ya como [`AsyncRead`].
///
/// Cabe en un `Send + Unpin` —que es lo que el transporte exige— porque lo único que comparte con
/// el hilo del volcado es el receptor del canal y las dos señales atómicas.
pub struct LectorDelVolcado {
    /// El nombre del volcado, para poder nombrarlo en el motivo de un fallo.
    nombre: String,
    /// De dónde salen los trozos.
    receptor: mpsc::Receiver<Vec<u8>>,
    /// El trozo parcial del mensaje anterior: lo que no cupo en el `ReadBuf` del consumidor.
    ///
    /// Es el estado que hace que este lector sea correcto: cuando el consumidor pide 5 bytes y el
    /// mensaje trae 100, los otros 95 **se guardan aquí** para la lectura siguiente. Tirarlos
    /// entregaría un volcado incompleto y la restauración parecería buena.
    sobrante: Vec<u8>,
    /// Cuántos bytes de [`LectorDelVolcado::sobrante`] ya se entregaron.
    ///
    /// Se avanza el índice en vez de recortar el vector: mover el trozo entero en cada lectura
    /// convertiría una lectura de 5 bytes en una copia de todo lo que queda.
    consumido: usize,
    /// La señal del escritor: `true` cuando declaró el final **antes** de cerrar el canal.
    terminado_bien: Arc<AtomicBool>,
    /// La ranura del motivo de un fallo, escrita por el escritor antes de cerrar el canal.
    fallo: Arc<Mutex<Option<String>>>,
}

impl LectorDelVolcado {
    /// El error de un volcado que se cerró sin declarar el final bueno.
    ///
    /// Si el escritor dejó un motivo en la ranura, ese motivo es el del repositorio y es el que
    /// hay que publicar. Si no lo dejó, el volcado se quedó a medias sin decir por qué: el motivo
    /// entonces dice eso. Los dos desenlaces son [`std::io::ErrorKind::UnexpectedEof`], porque los
    /// dos son «el volcado no llegó entero», y el consumidor no puede leer ninguno como un fin de
    /// fichero limpio.
    fn volcado_a_medias(&self) -> std::io::Error {
        let motivo = match self.fallo.lock() {
            Ok(ranura) => ranura.clone(),
            // Un candado envenenado quiere decir que otro hilo se cayó teniéndolo tomado. Lo que
            // hay dentro sigue siendo válido, así que se recupera y se sigue: aquí no se entra en
            // pánico.
            Err(veneno) => veneno.into_inner().clone(),
        };

        let mensaje = match motivo {
            Some(motivo) => format!(
                "el volcado `{}` no se pudo leer del repositorio: {motivo}",
                self.nombre
            ),
            None => format!(
                "el escritor del volcado `{}` soltó el canal sin declarar el final: el volcado \
                 puede estar truncado, y restaurarlo daría por buena una base a medias",
                self.nombre
            ),
        };

        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, mensaje)
    }
}

impl std::fmt::Debug for LectorDelVolcado {
    /// Publica el nombre del volcado y nada de lo que viaja por el canal.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LectorDelVolcado")
            .field("nombre", &self.nombre)
            .finish_non_exhaustive()
    }
}

impl AsyncRead for LectorDelVolcado {
    /// Llena `buf` con bytes del volcado, esperando al escritor si hace falta.
    ///
    /// **Solo un desenlace significa «fin del volcado»**: el canal cerrado **con** la señal del
    /// escritor puesta. Todo lo demás es error, y cada uno nombra el volcado:
    ///
    /// - un canal cerrado **sin** la señal es un volcado **a medias**
    ///   ([`std::io::ErrorKind::UnexpectedEof`]): el repositorio falló volcando —y su motivo viaja
    ///   en el error— o el hilo que volcaba murió sin declarar el final. Devolver un fin de fichero
    ///   aquí **daría por buena una restauración incompleta sin que el código de salida del
    ///   cliente de la base lo delate**, que es el fallo que este puente existe para evitar.
    ///
    /// No hay plazo de inactividad aquí: quien acota el silencio del contenedor es el transporte
    /// (`ejecutar_con_entrada`), que drena la salida con su propio plazo mientras esto se escribe.
    /// El volcado del repositorio es local y bloqueante, así que «silencio» no es un desenlace que
    /// este lado pueda observar.
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // El contrato de `AsyncRead` no obliga a nada con un búfer sin hueco, pero entregar un
        // mensaje que no cabe en ningún sitio sería retenerlo sin destino: se contesta sin
        // consumir. Es el mismo desenlace que el búfer vacío del lector de la extracción.
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        // 1. Primero lo que sobró del mensaje anterior. Antes de pedir otro, siempre: al revés, el
        //    trozo parcial se perdería.
        if this.consumido < this.sobrante.len() {
            let pendientes = &this.sobrante[this.consumido..];
            let a_entregar = pendientes.len().min(buf.remaining());

            buf.put_slice(&pendientes[..a_entregar]);
            this.consumido += a_entregar;

            if this.consumido == this.sobrante.len() {
                this.sobrante.clear();
                this.consumido = 0;
            }

            return Poll::Ready(Ok(()));
        }

        // 2. Con `poll_recv` y no con un `await`: este es el `poll` de un `AsyncRead`, así que el
        //    aparcado lo hace el runtime por nosotros. El trozo no cabe en ningún otro sitio: el
        //    canal solo lleva datos.
        match this.receptor.poll_recv(cx) {
            Poll::Ready(Some(datos)) => {
                let a_entregar = datos.len().min(buf.remaining());
                buf.put_slice(&datos[..a_entregar]);

                if a_entregar < datos.len() {
                    this.sobrante = datos;
                    this.consumido = a_entregar;
                }

                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
            // El canal se cerró. Lo que decide es la señal del escritor, nunca el estado del
            // canal: los dos escritores —el que declara el final y el que se cae— dejan el canal
            // cerrado igual, y el receptor entrega lo que quede en cola antes de decir que está
            // desconectado.
            Poll::Ready(None) => {
                if this.terminado_bien.load(Ordering::Acquire) {
                    // El final bueno: se contesta sin rellenar `buf`, que es lo que `AsyncRead`
                    // lee como fin de fichero.
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(this.volcado_a_medias()))
                }
            }
        }
    }
}

// ── El flujo: las órdenes contra el `stdin` del `exec` ──
//
// Es la tercera pieza del espejo, y la que conduce lo que [`comando`] compuso: cada orden del
// camino corre como un `exec` del contenedor, y la que reimporta recibe el volcado por su `stdin`
// —el [`LectorDelVolcado`] del puente es su origen de lectura—. Las que no reimportan corren con
// la entrada cerrada, que es lo que un `exec` necesita para no quedarse esperando un volcado que
// no le corresponde.
//
// El desenlace se traduce **aquí** y no en el motor, por la misma razón por la que
// [`volcar`](crate::engine::extraccion::volcar) traduce el suyo: es este módulo el que tiene
// delante la orden —y con ella los valores que hay que quitar de un motivo— y el que sabe qué
// significa cada desenlace del transporte.

/// Si un nombre de base tiene forma de **cadena de conexión** para `psql`.
///
/// `psql -d <valor>` no trata cualquier valor como un nombre de base: libpq lee el valor como
/// *conninfo* —y de ahí saca `host`, `port`, `user`, `dbname`, `sslmode`…— si **contiene un `=`** o
/// si **empieza** por la URI `postgres://` o `postgresql://`, los dos esquemas que reconoce. Un
/// `\connect` del volcado es contenido no confiable, así que un nombre con esa forma permitiría
/// **desviar la conexión** a otro servidor con la credencial del entorno puesta; por eso
/// [`ejecutar_por_secciones`] lo rechaza en vez de entregarlo.
///
/// La comparación del prefijo es **sin distinguir mayúsculas** —`POSTGRES://x` también se rechaza—,
/// mientras libpq es **sensible a mayúsculas** y **no salta blancos**: reconoce el prefijo literal
/// en el byte 0 —su `uri_prefix_length` y su `recognized_connection_string`, en `fe-connect.c`— o un
/// `=` en cualquier posición. Así que esta función es un **superconjunto** de lo que libpq expande, y
/// eso es deliberado: rechazar de más es **fail-closed** —un nombre que libpq trataría como plano
/// (`POSTGRES://x`) se rechaza igual—, y el precio es un volcado con un nombre de base exótico, que
/// falla en voz alta.
pub(crate) fn parece_cadena_de_conexion(nombre: &str) -> bool {
    /// Los dos esquemas que libpq reconoce como URI de conexión.
    const ESQUEMAS: [&str; 2] = ["postgres://", "postgresql://"];

    nombre.contains('=')
        || ESQUEMAS.iter().any(|esquema| {
            nombre
                .get(..esquema.len())
                .is_some_and(|prefijo| prefijo.eq_ignore_ascii_case(esquema))
        })
}

/// La orden de **una** sección del volcado: la plantilla con la base en curso añadida.
///
/// La base va como **valor** de la opción `-d` —es el último argumento de la plantilla— y nunca
/// como un argumento suelto: un nombre que empiece por `-` lo leería `psql` como una opción, y
/// `psql -d -c …` buscaría una base llamada `-c` (verificado). La posición tras `-d` es **todo** lo
/// que esta función garantiza, y no cubre el resto: `psql -d` lee el valor como *conninfo* si tiene
/// un `=` o es una URI —ver [`parece_cadena_de_conexion`]—, que es un vector aparte y se rechaza
/// **antes** de llamar aquí. No es la guarda de la extracción —allí `sin_ser_bandera` **rechaza** el
/// valor que empieza por `-`, porque va como argumento suelto—: aquí la base sale de un
/// `\connect` del volcado, que es contenido no confiable, y lo que hay que rechazar es su forma.
///
/// El `env` se copia de la plantilla: es donde viaja la credencial, y cada sección es un `exec`
/// distinto que la necesita.
fn orden_de_una_seccion(plantilla: &Orden, base: &str) -> Orden {
    let mut cmd = plantilla.cmd.clone();
    cmd.push(base.to_owned());

    Orden {
        cmd,
        env: plantilla.env.clone(),
    }
}

/// Ejecuta las órdenes de un camino de contenedor, entregando el volcado por el `stdin`.
///
/// Conduce los `exec` **en el orden en que vienen**, que es contrato de [`comando`]: la retirada de
/// un `sqlite` antes de reimportar. La **última** orden es la que recibe el volcado; las anteriores
/// corren con la entrada cerrada —porque su trabajo no es leer nada, y darles un `stdin` abierto
/// las dejaría esperando a un volcado que no van a consumir—.
///
/// Para en la primera orden que no sale con cero: reimportar sobre un fichero que no se pudo retirar
/// es peor que no reimportar, así que lo que queda del camino no se ejecuta.
///
/// El `reparto` decide **cómo** llega el volcado a las órdenes: entero a la última —MariaDB y
/// SQLite, [`Reparto::ALaUltimaOrden`]— o troceado por base, una orden por sección —PostgreSQL,
/// [`Reparto::PorSeccionesDePostgres`]—. En el segundo caso no hay órdenes fijas: el motor compone
/// una por cada `\connect` del volcado, con el contenido de la sección por el `stdin`. Ver
/// [`ejecutar_por_secciones`].
///
/// `db_path` es la ruta declarada de un `sqlite` —`None` en los demás tipos— y su comprobación de
/// alcance va **antes** de la primera orden: la retirada es un `rm -f`, y un `db_path` que no sea su
/// propia forma canónica dentro del contenedor —un `..`, un enlace en el último componente o, el
/// peligroso, un enlace en un directorio padre— llevaría ese `rm -f` a borrar un fichero ajeno, así
/// que un `db_path` no canónico **no borra nada**.
///
/// # Errors
///
/// Devuelve [`ErrorDelMotor::Restauracion`] con el motivo del contenedor cuando una orden sale con
/// un código distinto de cero —el texto que el proceso escribió por su canal de error va dentro, ya
/// pasado por [`motivo_para_guardar`], así que sin la credencial— y cuando falla el transporte: el
/// contenedor que no está, el socket que no responde, el volcado que el repositorio no entregó
/// entero (que llega como [`crate::docker::Error::EntradaNoEntregada`], con el motivo del
/// repositorio dentro) y la orden que se queda sin producir salida. La comprobación del `db_path`
/// que no coincide —o que no se pudo comprobar, como el `readlink` que falta en el contenedor—
/// también llega como [`ErrorDelMotor::Restauracion`], y en los dos casos sin ejecutar la retirada.
pub async fn ejecutar_las_ordenes(
    cliente: &Cliente,
    contenedor: &str,
    ordenes: &[Orden],
    reparto: Reparto,
    db_path: Option<&str>,
    volcado: LectorDelVolcado,
    inactividad_maxima: Duration,
) -> Result<i32, ErrorDelMotor> {
    // La comprobación de alcance del `db_path`, **antes** de la primera orden: un `sqlite` cuyo
    // `db_path` no sea su propia forma canónica dentro del contenedor no retira ni reimporta nada.
    // El `None` es el de los tipos que no tienen `db_path`: una base de servidor no lleva ninguna
    // ruta de fichero en su comando.
    if let Some(db_path) = db_path {
        comprobar_la_forma_canonica_del_db_path(cliente, contenedor, db_path, inactividad_maxima)
            .await
            .map_err(|causa| ErrorDelMotor::Restauracion {
                motivo: recortar(&causa.to_string()),
            })?;
    }

    match reparto {
        // El volcado entero va al `stdin` de la **última** orden; las de antes corren con la
        // entrada cerrada.
        Reparto::ALaUltimaOrden => {
            // La última orden y las de antes, de una vez: así el volcado se entrega **a la última**
            // sin contar índices ni comprobar en cada vuelta si ésta es la que importa.
            let Some((reimportacion, retiradas)) = ordenes.split_last() else {
                // Un camino sin órdenes no existe —[`comando`] compone al menos una por tipo—, y
                // aquí no se inventa un desenlace: cero órdenes es cero trabajo hecho, y eso es un
                // cero.
                return Ok(0);
            };

            for orden in retiradas {
                ejecutar_una(
                    cliente,
                    contenedor,
                    orden,
                    tokio::io::empty(),
                    inactividad_maxima,
                )
                .await?;
            }

            ejecutar_una(
                cliente,
                contenedor,
                reimportacion,
                volcado,
                inactividad_maxima,
            )
            .await
        }

        // El volcado se trocea por base: una orden `psql -d <base>` por sección, con el contenido
        // de esa sección —y solo el de esa— por el `stdin`.
        Reparto::PorSeccionesDePostgres {
            plantilla,
            base_inicial,
        } => {
            ejecutar_por_secciones(
                cliente,
                contenedor,
                plantilla,
                base_inicial,
                volcado,
                inactividad_maxima,
            )
            .await
        }
    }
}

/// Ejecuta un volcado de PostgreSQL **troceado por base**, una orden `psql` por sección.
///
/// Es el reverso de `pg_dumpall`: el volcado separa las bases con `\connect`, y como el único modo
/// de `psql` que rechaza metacomandos —`\restrict`— **bloquea `\connect`**, no vale un `psql` único
/// con el volcado global. Aquí se lee el volcado **línea a línea**, el [`Troceador`] decide a qué
/// base va cada línea y qué líneas se descartan, y cada sección va al `stdin` de su propia orden
/// `psql -d <base>`, con la guarda `\restrict <nonce>` delante.
///
/// El volcado **no** se materializa: las secciones se leen y se entregan conforme se consumen, con
/// la memoria acotada al búfer del puente y a la línea en curso. Y una sección que falla falla la
/// restauración entera: el `?` corta el bucle y el fallo sube.
///
/// La garantía es **esa y no más**: se falla cuando una orden sale con un código distinto de cero.
/// Los errores de SQL **dentro** de una sección **no** se detectan —`psql` sin `ON_ERROR_STOP` sale
/// con `0` ante un error de SQL—, así que una sección cuyas sentencias fallaron se da por buena. Es
/// una decisión **aparte** de este endurecimiento y va por su cuenta: **issue #175**.
///
/// La base de una sección que sale de un `\connect` es contenido no confiable y **no** puede tener
/// forma de cadena de conexión —ver [`parece_cadena_de_conexion`]—: si la tiene, la restauración
/// falla sin ejecutar esa sección ni las siguientes.
///
/// # La precondición de `\restrict` es **obligatoria**
///
/// Antes del bucle se comprueba, con el **mismo binario** que usará el reparto, que el cliente del
/// contenedor honra `\restrict` —ver [`comprobar_que_el_cliente_soporta_restrict`]—. Con un `psql`
/// anterior a la rama que lo trae (17.6, o el mínimo de la suya) la capa 1 no basta: el cliente
/// ejecuta metacomandos precedidos de `\f`, `\v` o `\r` que el troceador, si fallara, dejaría
/// escapar, y un volcado hostil ejecutaría como `root`. Por eso la restauración **falla** con un
/// mensaje que nombra el contenedor y el mínimo, en vez de degradarse en silencio.
async fn ejecutar_por_secciones(
    cliente: &Cliente,
    contenedor: &str,
    plantilla: Orden,
    base_inicial: String,
    volcado: LectorDelVolcado,
    inactividad_maxima: Duration,
) -> Result<i32, ErrorDelMotor> {
    // La precondición, **antes** de leer una sola línea del volcado: sin un cliente que honre
    // `\restrict`, un volcado hostil ejecutaría como `root` en el contenedor destino.
    comprobar_que_el_cliente_soporta_restrict(cliente, contenedor, &plantilla, inactividad_maxima)
        .await?;

    // El nonce de la guarda `\restrict`: **aleatorio por restauración**, no derivado del contenido,
    // para que quien escribe el volcado no pueda adivinarlo y deshacer la guarda con un
    // `\unrestrict`. Ver [`nonce_de_restriccion`].
    let nonce = nonce_de_restriccion();

    // El volcado se lee **línea a línea** por un `BufReader`: su búfer interno acumula lo que llega
    // del puente, y así se puede pedir una línea sin bloquear el runtime. El troceador **atraviesa
    // las secciones** —su estado léxico y la base en curso son del volcado entero, no de una
    // sección—, y por eso vive fuera del bucle.
    let mut lineas = BufReader::new(volcado);
    let mut troceador = Troceador::nuevo(&base_inicial);

    // `true` cuando el volcado entero se terminó: lo pone el lector de la sección al ver el fin de
    // fichero del puente, y es lo que para el bucle. Una frontera **no** lo pone —el volcado sigue
    // con la base siguiente—.
    let mut fin_del_volcado = false;

    // `true` cuando la base de la sección en curso la fijó un `\connect` del volcado. La **primera**
    // sección va a la `base_inicial`, que la eligió el operador en la configuración del target y
    // **no** viene del volcado: no se valida, y por eso el bucle arranca en `false`.
    let mut base_del_volcado = false;

    while !fin_del_volcado {
        // La base de esta sección, antes de componer nada con ella.
        let base = troceador.base_en_curso();

        // `psql -d` lee el valor como *conninfo* —y de ahí saca `host`, `port`, `user`…— si trae un
        // `=` o es una URI, así que un `\connect "host=atacante.example dbname=postgres"` del volcado
        // llevaría la conexión al servidor del atacante **con `PGPASSWORD` puesto**: fuga de la
        // credencial y restauración en el sitio equivocado. Se **rechaza** —fail-closed— en vez de
        // intentar neutralizarlo, y sin ejecutar esta sección ni las siguientes.
        //
        // El motivo **no** lleva el valor: es contenido no confiable —saltos de línea, escapes ANSI—
        // y no tiene nada que hacer en `error_message`, que el panel muestra.
        if base_del_volcado && parece_cadena_de_conexion(base) {
            return Err(ErrorDelMotor::Restauracion {
                motivo: recortar(&format!(
                    "el volcado de `{contenedor}` trae un `\\connect` cuyo nombre de base tiene forma \
                     de cadena de conexión: la restauración no se ejecuta"
                )),
            });
        }

        // La orden de esta sección: la plantilla con la base en curso añadida como valor de `-d`.
        let orden = orden_de_una_seccion(&plantilla, base);

        // La sección, como [`AsyncRead`]: entrega la guarda `\restrict` delante, después el
        // contenido de la sección, y termina en la frontera siguiente —que **no** entrega— o en el
        // fin del volcado. Las líneas de metacomando de primer nivel las descarta el troceador.
        let seccion = SeccionDelVolcado {
            lineas: &mut lineas,
            troceador: &mut troceador,
            pendiente: format!("\\restrict {nonce}\n").into_bytes(),
            consumido: 0,
            en_curso: Vec::new(),
            acabada: false,
            fin_del_volcado: &mut fin_del_volcado,
        };

        // Una sección que falla falla la restauración entera: `?` corta el bucle y el fallo sube.
        ejecutar_una(cliente, contenedor, &orden, seccion, inactividad_maxima).await?;

        // A partir de aquí la base en curso ya la fijó un `\connect` del volcado —una sección solo
        // termina en una frontera o en el fin del volcado, y el fin para el bucle—: lo que venga en
        // la vuelta siguiente es contenido no confiable y se valida.
        base_del_volcado = true;
    }

    Ok(0)
}

/// El tope de bytes de la salida de `psql --version` que se conserva.
///
/// La versión es corta —«psql (PostgreSQL) 18.6»—, así que acotar la salida impide que un cliente
/// que escriba sin parar infle la memoria del motor. Lo que sobre no aportaría nada a la versión.
const TOPE_DE_LA_VERSION: usize = 128;

/// Comprueba que el cliente del contenedor honra `\restrict`, o falla la restauración.
///
/// Es la **precondición obligatoria** del reparto por secciones. Ejecuta `psql --version` con el
/// **mismo binario** que usará el reparto —el primer elemento del `cmd` de la plantilla— y sin
/// credencial: `--version` no conecta, así que la sonda no lleva el entorno de la orden.
///
/// El motivo **nunca** lleva la credencial: el [`Debug`] de [`Orden`] publica los **nombres** de las
/// variables de entorno y nunca sus valores, y aquí el motivo se compone a mano con el contenedor,
/// el binario y lo que el contenedor dijo —que ya pasó por [`motivo_para_guardar`], la red que quita
/// los valores del entorno—.
///
/// # Errors
///
/// Devuelve [`ErrorDelMotor::Restauracion`] cuando la sonda no llega a término o sale con un código
/// distinto de cero —no se pudo comprobar la versión— y cuando la versión **no** honra `\restrict`
/// —con un mensaje que nombra el contenedor, la versión leída y el mínimo, y el porqué—.
async fn comprobar_que_el_cliente_soporta_restrict(
    cliente: &Cliente,
    contenedor: &str,
    plantilla: &Orden,
    inactividad_maxima: Duration,
) -> Result<(), ErrorDelMotor> {
    let Some(binario) = plantilla.cmd.first() else {
        return Err(ErrorDelMotor::Restauracion {
            motivo: recortar(&format!(
                "no se pudo comprobar la versión del cliente `psql` en el contenedor \
                 `{contenedor}`: la plantilla del reparto no trae comando"
            )),
        });
    };

    // La sonda es el binario con `--version` y **sin** entorno: `--version` no conecta, así que no
    // necesita la credencial, y no meterla aquí es una boca menos por la que podría filtrarse.
    let sonda = Orden {
        cmd: vec![binario.clone(), "--version".to_owned()],
        env: Vec::new(),
    };

    let mut version: Vec<u8> = Vec::new();
    let mut crudo: Vec<u8> = Vec::new();

    let codigo = {
        let mut al_llegar = |trozo: Trozo| match trozo.canal {
            Canal::Salida => {
                if version.len() < TOPE_DE_LA_VERSION {
                    let libres = TOPE_DE_LA_VERSION - version.len();
                    version.extend_from_slice(&trozo.bytes[..libres.min(trozo.bytes.len())]);
                }
            }
            Canal::Error => acumular_el_motivo(&mut crudo, &trozo.bytes),
        };

        cliente
            .ejecutar(contenedor, &sonda, inactividad_maxima, &mut al_llegar)
            .await
    };

    let dijo = motivo_para_guardar(crudo, &sonda);
    let leida = String::from_utf8_lossy(&version);
    let leida = leida.trim();

    match codigo {
        Ok(0) => {}
        Ok(codigo) => {
            return Err(ErrorDelMotor::Restauracion {
                motivo: recortar(&format!(
                    "no se pudo comprobar la versión del cliente `psql` en el contenedor \
                     `{contenedor}`: `{binario} --version` salió con {codigo}{}",
                    lo_que_dijo_el_contenedor(&dijo)
                )),
            });
        }
        Err(causa) => {
            return Err(ErrorDelMotor::Restauracion {
                motivo: recortar(&format!(
                    "no se pudo comprobar la versión del cliente `psql` en el contenedor \
                     `{contenedor}`: {causa}{}",
                    lo_que_dijo_el_contenedor(&dijo)
                )),
            });
        }
    }

    if soporta_restrict(leida) {
        return Ok(());
    }

    Err(ErrorDelMotor::Restauracion {
        motivo: recortar(&format!(
            "el contenedor `{contenedor}` trae un `psql` que no honra `\\restrict` ({leida}): hace \
             falta `psql` 17.6, o el mínimo de su rama: 16.10, 15.14, 14.19, 13.22. Sin \
             `\\restrict` no se pueden ignorar los metacomandos del volcado —un `psql` anterior \
             ejecuta los que van precedidos de `\\f`, `\\v` o `\\r`— y un volcado hostil ejecutaría \
             como `root` en el contenedor destino, así que la restauración no se ejecuta"
        )),
    })
}

/// El mínimo `(mayor, menor)` de `psql` que trae `\restrict`, por rama mayor.
///
/// `\restrict` y `\unrestrict` entraron en el ciclo de parches de seguridad de **2025-08-14**:
/// `psql` 17.6, 16.10, 15.14, 14.19 y 13.22 son los primeros de cada rama que lo traen, y
/// PostgreSQL **18** lo trae de serie (de ahí el `0`). Para una rama anterior a 13 —o un número que
/// no corresponde a ninguna— no hay mínimo y el cliente **no** se acepta.
fn minimo_con_restrict(mayor: u32) -> Option<u32> {
    match mayor {
        18.. => Some(0),
        17 => Some(6),
        16 => Some(10),
        15 => Some(14),
        14 => Some(19),
        13 => Some(22),
        _ => None,
    }
}

/// El `(mayor, menor)` que declara `psql --version` —«psql (PostgreSQL) 18.6»—.
///
/// No vale con tomar el primer token: la salida lleva la versión dentro —`psql (PostgreSQL) 18.6`,
/// con un sufijo de distribución detrás en algunos paquetes—, así que se busca el primer token que
/// empiece por dígito y traiga un punto. Devuelve [`None`] si no hay ninguno con esa forma.
fn version_de_psql(salida: &str) -> Option<(u32, u32)> {
    salida.split_whitespace().find_map(|token| {
        let token = token.trim_start_matches('(');
        let (mayor, resto) = token.split_once('.')?;

        Some((mayor.parse().ok()?, resto.split('.').next()?.parse().ok()?))
    })
}

/// Si la versión que declara `psql --version` trae `\restrict`.
///
/// Es **fail-closed**: una salida de la que no se pueda leer la versión se trata como «no la trae».
/// Ver [`minimo_con_restrict`] para el porqué de los mínimos por rama.
fn soporta_restrict(salida: &str) -> bool {
    let Some((mayor, menor)) = version_de_psql(salida) else {
        return false;
    };

    match minimo_con_restrict(mayor) {
        Some(minimo) => menor >= minimo,
        None => false,
    }
}

/// Un nonce aleatorio para la guarda `\restrict` de una restauración.
///
/// Es **aleatorio por restauración** y no derivado del contenido: quien escribe el volcado no puede
/// adivinarlo, así que un `\unrestrict` que trajera no podría deshacer la guarda. El `\unrestrict`
/// cae igual con los demás metacomandos —el troceador los descarta—, pero esto es la **segunda
/// capa**: si uno se colara, seguiría sin valer. Dos `u64` de `rand` dan 128 bits, de sobra para
/// que no se repita entre restauraciones.
fn nonce_de_restriccion() -> String {
    format!(
        "{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    )
}

/// Una sección del volcado de PostgreSQL, como [`AsyncRead`], para el `stdin` de su `psql`.
///
/// Es el adaptador que convierte el volcado global —con sus `\connect`— en el flujo de **una**
/// base: entrega la guarda `\restrict <nonce>` delante, después el contenido de la sección en
/// curso, y termina en la frontera siguiente —que **no** entrega, porque pertenece a la sección
/// siguiente— o en el fin del volcado. Las líneas de metacomando de primer nivel las descarta el
/// [`Troceador`]: sin ellas que entregar, no hay nada que el `psql` pueda ejecutar.
///
/// Implementa [`poll_read`](AsyncRead::poll_read) **a mano** y no con un `read_until` en un futuro:
/// el futuro de una lectura que cruza el búfer tendría que vivir entre llamadas, y aquí el estado
/// es explícito —lo pendiente, el parcial de la línea en curso y las dos banderas—.
struct SeccionDelVolcado<'a> {
    /// De dónde salen las líneas, ya con búfer: la sección no vuelve a leer el volcado.
    lineas: &'a mut BufReader<LectorDelVolcado>,
    /// El troceador, que **atraviesa** las secciones.
    troceador: &'a mut Troceador,
    /// Lo que hay que entregar y todavía no se entregó: la guarda al principio, una línea después.
    pendiente: Vec<u8>,
    /// Cuántos bytes de [`SeccionDelVolcado::pendiente`] ya se entregaron.
    consumido: usize,
    /// La línea que se está leyendo, cuando cruza el búfer: lo que llegó y aún no tiene su `\n`.
    ///
    /// Sin esto, una línea partida entre dos lecturas se perdería a la mitad.
    en_curso: Vec<u8>,
    /// `true` cuando esta sección ya no tiene más que entregar: se alcanzó una frontera o el fin
    /// del volcado.
    acabada: bool,
    /// `true` cuando lo que se alcanzó fue el **fin del volcado**: lo pone el lector y es lo que
    /// para el bucle de secciones. Una frontera **no** lo pone.
    fin_del_volcado: &'a mut bool,
}

impl AsyncRead for SeccionDelVolcado<'_> {
    /// Entrega la sección, esperando al volcado si hace falta.
    ///
    /// El orden de los desenlaces es el que hace correcto el adaptador: primero lo pendiente —la
    /// guarda o la línea anterior—, después el fin de la sección, y solo entonces se pide una línea
    /// nueva. Una línea de metacomando descartada **no** termina la lectura: se pide la siguiente.
    /// Y una frontera termina la sección **sin entregar su línea**, que es de la sección siguiente.
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // Un búfer sin hueco no consume nada: es el mismo desenlace que el lector del puente.
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        loop {
            // 1. Lo pendiente primero: la guarda o la línea anterior.
            if this.consumido < this.pendiente.len() {
                let pendientes = &this.pendiente[this.consumido..];
                let a_entregar = pendientes.len().min(buf.remaining());

                buf.put_slice(&pendientes[..a_entregar]);
                this.consumido += a_entregar;

                if this.consumido == this.pendiente.len() {
                    this.pendiente.clear();
                    this.consumido = 0;
                }

                return Poll::Ready(Ok(()));
            }

            // 2. Una sección acabada entrega un fin de fichero: el `stdin` del `psql` se cierra.
            if this.acabada {
                return Poll::Ready(Ok(()));
            }

            // 3. La línea siguiente, o el fin del volcado.
            let linea = match leer_una_linea(this.lineas, &mut this.en_curso, cx) {
                Poll::Ready(Ok(Some(linea))) => linea,
                Poll::Ready(Ok(None)) => {
                    // El volcado se acabó: esta sección es la última, y el bucle tiene que parar.
                    *this.fin_del_volcado = true;
                    this.acabada = true;
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Err(causa)) => return Poll::Ready(Err(causa)),
                Poll::Pending => return Poll::Pending,
            };

            // 4. Qué hacer con la línea lo decide el troceador: contenido, descarte o frontera.
            match this.troceador.linea(&linea) {
                Instruccion::Contenido(contenido) => {
                    // El préstamo sale de `linea`, que vive hasta aquí: se copia para entregarlo.
                    this.pendiente = contenido.to_vec();
                }
                // Un metacomando de primer nivel **no** se entrega: se descarta y se sigue.
                Instruccion::Descartada => {}
                Instruccion::Frontera(_) => {
                    // La línea de la frontera pertenece a la sección siguiente, que la recompone
                    // desde su propia base: aquí no se entrega y la sección termina.
                    this.acabada = true;
                }
            }
        }
    }
}

/// Lee una línea completa del `BufReader`, con su terminador, sin bloquear el runtime.
///
/// Acumula en `en_curso` lo que llega hasta el `\n`: una línea que cruza el búfer se termina de
/// leer en la vuelta siguiente, y por eso el parcial vive **fuera** —en el llamante— y no aquí. Al
/// final del fichero devuelve lo que quede —una última línea sin `\n`— o `None` si no queda nada.
fn leer_una_linea(
    lineas: &mut BufReader<LectorDelVolcado>,
    en_curso: &mut Vec<u8>,
    cx: &mut Context<'_>,
) -> Poll<std::io::Result<Option<Vec<u8>>>> {
    loop {
        let disponible = match Pin::new(&mut *lineas).poll_fill_buf(cx) {
            Poll::Ready(Ok(disponible)) => disponible,
            Poll::Ready(Err(causa)) => return Poll::Ready(Err(causa)),
            Poll::Pending => return Poll::Pending,
        };

        if disponible.is_empty() {
            // Fin del volcado. Lo que quede en `en_curso` es una última línea sin terminador.
            if en_curso.is_empty() {
                return Poll::Ready(Ok(None));
            }
            return Poll::Ready(Ok(Some(std::mem::take(en_curso))));
        }

        match disponible.iter().position(|&byte| byte == b'\n') {
            Some(posicion) => {
                en_curso.extend_from_slice(&disponible[..=posicion]);
                lineas.consume(posicion + 1);
                return Poll::Ready(Ok(Some(std::mem::take(en_curso))));
            }
            None => {
                // Sin `\n` todavía: se acumula lo que hay y se pide más en la vuelta siguiente.
                let longitud = disponible.len();
                en_curso.extend_from_slice(disponible);
                lineas.consume(longitud);
            }
        }
    }
}

/// Ejecuta **una** orden con la entrada que se le dé y traduce su desenlace al del motor.
///
/// Es donde vive la lectura del canal de error: lo que el proceso escribe por ahí es el motivo de un
/// fallo, y se acumula **acotado** con [`acumular_el_motivo`] —sin tope, un cliente que escriba sin
/// parar inflaría memoria en el proceso del motor— y se pasa después por [`motivo_para_guardar`],
/// que es lo que quita la credencial: la contraseña viaja en el **entorno de esta misma orden**, y
/// un cliente de la base que la repita al fallar la dejaría en `error_message`, que el panel
/// muestra.
///
/// Lo que llegue por el canal de **salida** se descarta, y no por descuido: la salida de un cliente
/// de restauración no es un fallo —un `sqlite3` que importa no imprime el fichero— y acumularla
/// sería retener el volcado entero otra vez en memoria.
async fn ejecutar_una(
    cliente: &Cliente,
    contenedor: &str,
    orden: &Orden,
    entrada: impl AsyncRead + Unpin,
    inactividad_maxima: Duration,
) -> Result<i32, ErrorDelMotor> {
    let mut crudo: Vec<u8> = Vec::new();

    // El cierre va **prestado** al transporte —un `&mut` de un `FnMut` es `FnMut`— y el préstamo
    // acaba con la llamada, que es lo que permite consumir el crudo después.
    let codigo = {
        let mut al_llegar = |trozo: Trozo| match trozo.canal {
            Canal::Error => acumular_el_motivo(&mut crudo, &trozo.bytes),
            Canal::Salida => {}
        };

        cliente
            .ejecutar_con_entrada(
                contenedor,
                orden,
                inactividad_maxima,
                entrada,
                &mut al_llegar,
            )
            .await
    };

    let dijo = motivo_para_guardar(crudo, orden);

    match codigo {
        // El cero es la orden hecha. Cualquier otro código es un fallo, y el motivo del proceso es
        // la parte que el operador necesita: dice si fue la contraseña, el binario o la base.
        Ok(0) => Ok(0),
        Ok(codigo) => Err(ErrorDelMotor::Restauracion {
            motivo: recortar(&format!(
                "la orden de restauración en `{contenedor}` salió con el código {codigo}{}",
                lo_que_dijo_el_contenedor(&dijo)
            )),
        }),
        // El transporte no llegó a dar un código de salida: el contenedor que no está, el socket
        // que no responde, el volcado que no llegó entero. Y lo que el contenedor alcanzó a decir
        // —depurado— va detrás, porque en un fallo a mitad su última frase suele ser la única pista.
        Err(causa) => Err(ErrorDelMotor::Restauracion {
            motivo: recortar(&format!("{causa}{}", lo_que_dijo_el_contenedor(&dijo))),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, BufReader};

    use super::{
        CaminoDeRestauracion, ErrorDeRestauracion, EscritorDelVolcado, Reparto, SeccionDelVolcado,
        TunelDeRestauracion, comando, ejecutar_las_ordenes, orden_de_una_seccion,
        parece_cadena_de_conexion, soporta_restrict, tunel_de_restauracion, version_de_psql,
    };
    use crate::db::entities::{ObjetivoDeRestauracion, TipoDeTarget};
    use crate::docker::{Canal, Cliente, Orden, Trozo};
    use crate::engine::ErrorDelMotor;
    use crate::engine::extraccion::nombre_del_tipo;
    use crate::engine::frontera::{Campo, FronteraError};
    use crate::engine::troceador::Troceador;
    use crate::secret::SecretString;
    use crate::test_support::{ContenedorDePrueba, cliente_del_motor, directorio_de_prueba};

    /// Contraseña distinguible: si aparece en una salida, la salida es el fallo.
    const CONTRASENA: &str = "Zq7-clave-de-base-distinguible";

    /// La raíz de copias de las pruebas que no miden el alcance de una ruta.
    ///
    /// No hace falta que exista: la comparación por componentes de [`super::comando`] es de texto
    /// para las filas que no traen `host_path`, y los casos de directorio que sí miden el alcance
    /// declaran su propia raíz.
    const RAIZ: &str = "/backup_sources";

    // ── Ayudantes ────────────────────────────────────────────────────────────

    /// La raíz de copias de las pruebas de forma, como `&Path`.
    fn raiz() -> &'static Path {
        Path::new(RAIZ)
    }

    /// El texto de una ruta de prueba, que el arnés construye siempre en UTF-8.
    fn texto(ruta: &Path) -> &str {
        ruta.to_str().expect("las rutas de prueba son UTF-8")
    }

    /// Un `SecretString` con el valor dado.
    fn secreto(valor: &str) -> SecretString {
        SecretString::new(valor.to_owned())
    }

    /// La línea de comandos como la escribiría un `argv`, para poder compararla entera.
    fn argv(argumentos: &[&str]) -> Vec<String> {
        argumentos
            .iter()
            .map(|argumento| (*argumento).to_owned())
            .collect()
    }

    /// Afirma que una orden lleva la credencial en el **entorno** y nunca en su línea de comandos.
    ///
    /// `usuario_en_el_cmd` es el argumento que tiene que estar en el `cmd` —el usuario, que no es
    /// secreto— y `variable`, la variable de entorno del tipo de base que tiene que llevar el valor.
    fn afirmar_credencial_en_el_entorno(orden: &Orden, usuario_en_el_cmd: &str, variable: &str) {
        assert!(
            !orden
                .cmd
                .iter()
                .any(|argumento| argumento.contains(CONTRASENA)),
            "la contraseña está en la línea de comandos: {:?}",
            orden.cmd
        );
        assert!(
            orden
                .cmd
                .iter()
                .any(|argumento| argumento == usuario_en_el_cmd),
            "el comando tiene que llevar el usuario: {:?}",
            orden.cmd
        );
        assert_eq!(
            orden.env,
            vec![format!("{variable}={CONTRASENA}")],
            "la contraseña no viaja en `{variable}`: {:?}",
            orden.env
        );
    }

    /// Un objetivo de MariaDB con todo informado, y el alcance que diga `db_name`.
    fn objetivo_mariadb(db_name: Option<&str>) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            job_id: uuid::Uuid::new_v4(),
            target_id: uuid::Uuid::new_v4(),
            target_type: TipoDeTarget::Mariadb,
            container_name: Some("contenedor-de-la-base".to_owned()),
            db_user: Some("operador".to_owned()),
            db_password: Some(secreto(CONTRASENA)),
            db_name: db_name.map(ToOwned::to_owned),
            host_path: None,
            db_path: None,
            target_name: "target-de-prueba".to_owned(),
        }
    }

    /// Un objetivo de PostgreSQL con todo informado, y el alcance que diga `db_name`.
    fn objetivo_postgresql(db_name: Option<&str>) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            target_type: TipoDeTarget::Postgresql,
            ..objetivo_mariadb(db_name)
        }
    }

    /// Un objetivo de directorio: sin contenedor, sin credenciales y con su ruta.
    fn objetivo_directorio(host_path: Option<&str>) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            target_type: TipoDeTarget::Directory,
            container_name: None,
            db_user: None,
            db_password: None,
            db_name: None,
            host_path: host_path.map(ToOwned::to_owned),
            db_path: None,
            ..objetivo_mariadb(None)
        }
    }

    /// Un objetivo `sqlite`: contenedor y ruta del `.db` dentro de él, y **nada** más.
    fn objetivo_sqlite(db_path: Option<&str>) -> ObjetivoDeRestauracion {
        ObjetivoDeRestauracion {
            target_type: TipoDeTarget::Sqlite,
            db_user: None,
            db_password: None,
            db_name: None,
            db_path: db_path.map(ToOwned::to_owned),
            ..objetivo_mariadb(None)
        }
    }

    /// El camino de un objetivo, exigiendo que sea **por contenedor**.
    ///
    /// El `panic` del otro desenlace es la mitad de la prueba en todos los casos de base.
    fn por_contenedor(objetivo: &ObjetivoDeRestauracion) -> (String, Vec<Orden>, Reparto) {
        match comando(raiz(), objetivo).expect("este camino tiene que construirse") {
            CaminoDeRestauracion::PorContenedor {
                contenedor,
                ordenes,
                reparto,
                ..
            } => (contenedor, ordenes, reparto),
            CaminoDeRestauracion::PorDirectorio { ruta } => panic!(
                "un objetivo de tipo `{:?}` no restaura escribiendo un directorio: {ruta:?}",
                objetivo.target_type
            ),
        }
    }

    /// El reparto por secciones de PostgreSQL, exigiendo que el camino lo sea.
    ///
    /// El `panic` de la otra variante es la mitad de la prueba en los casos de PostgreSQL: un
    /// MariaDB o un SQLite no reparten por secciones.
    fn reparto_de_postgres(reparto: Reparto) -> (Orden, String) {
        match reparto {
            Reparto::PorSeccionesDePostgres {
                plantilla,
                base_inicial,
            } => (plantilla, base_inicial),
            Reparto::ALaUltimaOrden => {
                panic!("un PostgreSQL tiene que repartirse por secciones, y salió `ALaUltimaOrden`")
            }
        }
    }

    // ── El comando de cada tipo ──────────────────────────────────────────────

    /// MariaDB se invoca en **modo binario** —`--binary-mode`—, con y sin base nombrada.
    ///
    /// Es la mitad del endurecimiento: el volcado lo escribe quien controla el repositorio, y el
    /// modo binario del cliente es lo que hace que sus metacomandos —`\!`, `\.`— no se interpreten.
    /// Sin `db_name` el volcado es global y el cliente va **sin base nombrada**; con `db_name`, esa
    /// base es el alcance, igual que lo fue en la extracción.
    #[test]
    fn mariadb_restaura_en_modo_binario() {
        let (contenedor, ordenes, reparto) = por_contenedor(&objetivo_mariadb(None));

        assert_eq!(contenedor, "contenedor-de-la-base");
        assert_eq!(
            ordenes.len(),
            1,
            "una base se reimporta con una sola orden: {ordenes:?}"
        );
        assert_eq!(
            ordenes[0].cmd,
            argv(&["mariadb", "--binary-mode", "-uoperador"]),
            "el comando de un volcado global no es `mariadb --binary-mode` con el usuario: {:?}",
            ordenes[0].cmd
        );
        assert!(
            matches!(reparto, Reparto::ALaUltimaOrden),
            "un MariaDB entrega el volcado entero a su única orden: {reparto:?}"
        );

        let (_contenedor, ordenes, _reparto) = por_contenedor(&objetivo_mariadb(Some("tienda")));
        assert_eq!(
            ordenes[0].cmd,
            argv(&["mariadb", "--binary-mode", "-uoperador", "tienda"]),
            "el comando no nombra la base pedida: {:?}",
            ordenes[0].cmd
        );
    }

    /// PostgreSQL **no tiene órdenes fijas**: reparte el volcado en secciones, una por base.
    ///
    /// Es el reverso de `pg_dumpall`, que separa las bases con `\connect`: el único modo de `psql`
    /// que rechaza metacomandos —`\restrict`— **bloquea `\connect`**, así que un `psql` único con el
    /// volcado global desviaría los datos a la base equivocada. Por eso el camino no trae órdenes y
    /// sí una plantilla —`psql -X -U <usuario> -d`, sin base— y la base de la primera sección.
    #[test]
    fn postgresql_no_tiene_ordenes_fijas_y_reparte_por_secciones() {
        let (_contenedor, ordenes, reparto) = por_contenedor(&objetivo_postgresql(None));

        assert!(
            ordenes.is_empty(),
            "un volcado de PostgreSQL no tiene órdenes fijas: se compone una por sección: {ordenes:?}"
        );

        let (plantilla, base_inicial) = reparto_de_postgres(reparto);

        assert_eq!(
            plantilla.cmd,
            argv(&["psql", "-X", "-U", "operador", "-d"]),
            "la plantilla de una sección no es `psql -X -U <usuario> -d`: {:?}",
            plantilla.cmd
        );
        assert_eq!(
            base_inicial, "postgres",
            "sin `db_name` la primera sección tiene que ir a `postgres`: {base_inicial}"
        );
        assert_eq!(
            plantilla.env,
            vec![format!("PGPASSWORD={CONTRASENA}")],
            "la plantilla tiene que llevar la credencial en el entorno: {:?}",
            plantilla.env
        );
    }

    /// Con `db_name`, la base de la **primera** sección de PostgreSQL es esa base.
    ///
    /// `pg_dump -d almacen` no emite ningún `\connect` antes de su primera sección, así que la base
    /// a la que hay que entregarla es la que el operador declaró.
    #[test]
    fn postgresql_usa_el_db_name_como_base_inicial() {
        let (_contenedor, _ordenes, reparto) =
            por_contenedor(&objetivo_postgresql(Some("almacen")));

        let (_plantilla, base_inicial) = reparto_de_postgres(reparto);

        assert_eq!(
            base_inicial, "almacen",
            "con `db_name` la primera sección tiene que ir a esa base: {base_inicial}"
        );
    }

    /// **El reparto no publica la contraseña en su `Debug`.**
    ///
    /// El reparto de PostgreSQL guarda la plantilla —una [`Orden`] con la credencial en su `env`—,
    /// y su `Debug` es el de la plantilla: publica el comando y el **nombre** de la variable, nunca
    /// el valor. Es la misma guarda que la del camino, aplicada al dato nuevo.
    #[test]
    fn el_reparto_no_publica_la_contrasena_en_su_debug() {
        let (_contenedor, _ordenes, reparto) = por_contenedor(&objetivo_postgresql(None));

        let volcado = format!("{reparto:?}");

        assert!(
            !volcado.contains(CONTRASENA),
            "el Debug del reparto publica la contraseña: {volcado}"
        );
        assert!(
            volcado.contains("PGPASSWORD"),
            "el Debug del reparto tiene que nombrar la variable, aunque no su valor: {volcado}"
        );
    }

    /// **Un nombre de base que empieza por `-` va como valor de `-d`, no como una opción.**
    ///
    /// El nombre de base sale de un `\connect` del volcado, que es contenido no confiable: si se
    /// entregara como un argumento suelto, un `\connect -c` haría que `psql` buscara una base
    /// llamada `-c` —verificado: `psql -d -c …` y `psql -d --help`—. Al componer la orden de la
    /// sección, la base es el **último** argumento de la plantilla, que ya termina en `-d`, así que
    /// siempre queda como su valor.
    #[test]
    fn un_nombre_de_base_que_empieza_por_guion_va_como_valor_de_d() {
        let (_contenedor, _ordenes, reparto) = por_contenedor(&objetivo_postgresql(None));
        let (plantilla, _base_inicial) = reparto_de_postgres(reparto);

        let orden = orden_de_una_seccion(&plantilla, "-c");

        assert_eq!(
            orden.cmd,
            argv(&["psql", "-X", "-U", "operador", "-d", "-c"]),
            "la base que empieza por `-` no queda como valor de `-d`: {:?}",
            orden.cmd
        );
    }

    /// Un `sqlite` **retira el fichero antes de reimportar**, y reimporta con `sqlite3 -safe`.
    ///
    /// Es el escenario «SQLite reconstruye el fichero antes de reimportar», y las dos órdenes se
    /// afirman **enteras** y **en orden**: el `.dump` emite `CREATE TABLE` sin `DROP TABLE`, así
    /// que reimportar sobre el fichero vivo chocaría con las tablas existentes, y el `-wal`/`-shm`
    /// viejos aplicarían un diario sobre un fichero nuevo. La reimportación va en modo **seguro**
    /// —`-safe`—: el `.dump` lo escribe quien controla el repositorio, y ahí es donde no puede
    /// colarse un `.shell`.
    #[test]
    fn sqlite_reimporta_en_modo_seguro() {
        let (contenedor, ordenes, reparto) =
            por_contenedor(&objetivo_sqlite(Some("/var/lib/kesto/kesto.db")));

        assert_eq!(contenedor, "contenedor-de-la-base");
        assert_eq!(
            ordenes.len(),
            2,
            "un sqlite se reconstruye en dos órdenes, y no en una: {ordenes:?}"
        );
        assert_eq!(
            ordenes[0].cmd,
            argv(&[
                "rm",
                "-f",
                "/var/lib/kesto/kesto.db",
                "/var/lib/kesto/kesto.db-wal",
                "/var/lib/kesto/kesto.db-shm",
            ]),
            "la retirada no borra el fichero vivo y sus dos hermanos: {:?}",
            ordenes[0].cmd
        );
        assert_eq!(
            ordenes[1].cmd,
            argv(&["sqlite3", "-safe", "/var/lib/kesto/kesto.db"]),
            "la reimportación no es `sqlite3 -safe <db_path>`: {:?}",
            ordenes[1].cmd
        );
        assert!(
            matches!(reparto, Reparto::ALaUltimaOrden),
            "un SQLite entrega el volcado entero a la última orden: {reparto:?}"
        );
    }

    /// Un `sqlite` no usa la credencial ni el nombre de base, aunque vengan informados.
    ///
    /// El tipo los ignora: un fichero SQLite no tiene usuario ni contraseña. Se informan a
    /// propósito para fijar que el comando no los arrastra ni inventa un `MYSQL_PWD` que ningún
    /// cliente leería.
    #[test]
    fn un_sqlite_no_usa_la_credencial_ni_el_nombre_de_base() {
        let objetivo = ObjetivoDeRestauracion {
            db_user: Some("operador".to_owned()),
            db_password: Some(secreto(CONTRASENA)),
            db_name: Some("tienda".to_owned()),
            ..objetivo_sqlite(Some("/var/lib/kesto/kesto.db"))
        };

        let (_contenedor, ordenes, _reparto) = por_contenedor(&objetivo);

        for orden in &ordenes {
            assert!(
                orden.env.is_empty(),
                "un sqlite no tiene credencial de base, así que no puede llevar entorno: {:?}",
                orden.env
            );
            for prohibido in ["operador", CONTRASENA, "tienda"] {
                assert!(
                    !orden
                        .cmd
                        .iter()
                        .any(|argumento| argumento.contains(prohibido)),
                    "el comando de un sqlite lleva `{prohibido}`, que su tipo no usa: {:?}",
                    orden.cmd
                );
            }
        }
    }

    // ── La credencial ────────────────────────────────────────────────────────

    /// **La contraseña no está en el `cmd` y sí en la variable de entorno de su tipo.**
    ///
    /// Es el mismo contrato que la extracción fija: en un contenedor, la línea de comandos es
    /// pública para cualquiera que mire el listado de procesos. Se cubren los tres tipos: MariaDB y
    /// PostgreSQL llevan la credencial en su entorno —el primero en su única orden, el segundo en la
    /// plantilla del reparto—, y SQLite no usa ninguna.
    #[test]
    fn la_contrasena_viaja_en_el_entorno_y_nunca_en_la_linea_de_comandos() {
        // MariaDB: la credencial viaja en el `env` de su única orden.
        let (_contenedor, ordenes, _reparto) = por_contenedor(&objetivo_mariadb(None));
        afirmar_credencial_en_el_entorno(&ordenes[0], "-uoperador", "MYSQL_PWD");

        // PostgreSQL: no hay órdenes fijas, y la credencial vive en la plantilla del reparto.
        let (_contenedor, _ordenes, reparto) = por_contenedor(&objetivo_postgresql(None));
        let (plantilla, _base_inicial) = reparto_de_postgres(reparto);
        afirmar_credencial_en_el_entorno(&plantilla, "operador", "PGPASSWORD");

        // SQLite: su tipo no usa credencial ninguna, así que no puede llevar entorno.
        let (_contenedor, ordenes, _reparto) =
            por_contenedor(&objetivo_sqlite(Some("/var/lib/kesto/kesto.db")));
        for orden in &ordenes {
            assert!(
                orden.env.is_empty(),
                "un sqlite no tiene credencial de base, así que no puede llevar entorno: {:?}",
                orden.env
            );
            assert!(
                !orden
                    .cmd
                    .iter()
                    .any(|argumento| argumento.contains(CONTRASENA)),
                "el comando de un sqlite lleva la contraseña, que su tipo no usa: {:?}",
                orden.cmd
            );
        }

        // Y el `Debug` de los caminos —que llevan la credencial dentro— tampoco la publica.
        for objetivo in [objetivo_mariadb(None), objetivo_postgresql(None)] {
            let camino = comando(raiz(), &objetivo).expect("el camino ya se construyó una vez");
            assert!(
                !format!("{camino:?}").contains(CONTRASENA),
                "el Debug del camino publica la contraseña: {camino:?}"
            );
        }
    }

    /// Un target **sin** contraseña no inventa una variable de entorno.
    ///
    /// Cubre las dos formas de «sin contraseña»: la columna `NULL` —el `None`— y el secreto
    /// vacío, que tampoco puede convertirse en `MYSQL_PWD=` —eso sería «contraseña vacía»—.
    #[test]
    fn un_target_sin_contrasena_no_inventa_la_variable() {
        for contrasena in [None, Some(secreto(""))] {
            let objetivo = ObjetivoDeRestauracion {
                db_password: contrasena,
                ..objetivo_mariadb(None)
            };

            let (_contenedor, ordenes, _reparto) = por_contenedor(&objetivo);

            assert!(
                ordenes[0].env.is_empty(),
                "un target sin contraseña no puede llevar entorno: {:?}",
                ordenes[0].env
            );
            assert_eq!(
                ordenes[0].cmd,
                argv(&["mariadb", "--binary-mode", "-uoperador"]),
                "sin contraseña el comando tiene que ser el mismo: {:?}",
                ordenes[0].cmd
            );
        }
    }

    // ── Los objetivos incoherentes ───────────────────────────────────────────

    /// Un `sqlite` sin `db_path` **no construye ninguna orden**, y el motivo nombra el campo.
    ///
    /// El ausente y el que solo tiene espacios se distinguen a propósito: el motivo manda al
    /// operador a un sitio distinto en cada caso.
    #[test]
    fn un_sqlite_sin_db_path_utilizable_no_construye_ninguna_orden() {
        for (db_path, en_blanco) in [(None, false), (Some("   "), true)] {
            let error = comando(raiz(), &objetivo_sqlite(db_path))
                .expect_err("un sqlite sin una ruta utilizable no tiene órdenes que ejecutar");

            match (&error, en_blanco) {
                (ErrorDeRestauracion::CampoQueFalta { tipo, campo }, false) => {
                    assert_eq!(
                        *tipo, "sqlite",
                        "el motivo tiene que nombrar el tipo `sqlite`"
                    );
                    assert_eq!(
                        *campo, "db_path",
                        "el motivo tiene que nombrar el campo `db_path`"
                    );
                }
                (ErrorDeRestauracion::CampoEnBlanco { tipo, campo }, true) => {
                    assert_eq!(*tipo, "sqlite");
                    assert_eq!(*campo, "db_path");
                }
                (otro, _) => panic!(
                    "el rechazo de un `db_path` que no sirve tiene que ser del campo: {otro:?}"
                ),
            }

            assert!(
                error.to_string().contains("db_path"),
                "el motivo no nombra el campo que falta: {error}"
            );
        }
    }

    /// Un target de base **sin usuario** —o sin contenedor— no se puede restaurar.
    ///
    /// El motivo nombra el campo que falta y el tipo con el texto del `CHECK` del §3, que es lo
    /// que hace falta para encontrar la fila.
    #[test]
    fn un_target_de_base_sin_usuario_o_sin_contenedor_no_se_puede_restaurar() {
        for tipo in [TipoDeTarget::Mariadb, TipoDeTarget::Postgresql] {
            let sin_usuario = ObjetivoDeRestauracion {
                db_user: None,
                ..objetivo_mariadb(None)
            };
            let sin_usuario = ObjetivoDeRestauracion {
                target_type: tipo,
                ..sin_usuario
            };

            let error = comando(raiz(), &sin_usuario)
                .expect_err("sin usuario no hay comando que construir");

            assert!(
                matches!(
                    &error,
                    ErrorDeRestauracion::CampoQueFalta {
                        campo: "db_user",
                        tipo: cual,
                    } if *cual == nombre_del_tipo(tipo)
                ),
                "el fallo no nombra el campo que falta: {error:?}"
            );

            let sin_contenedor = ObjetivoDeRestauracion {
                container_name: None,
                ..sin_usuario
            };
            let sin_contenedor = ObjetivoDeRestauracion {
                db_user: Some("operador".to_owned()),
                ..sin_contenedor
            };

            let error = comando(raiz(), &sin_contenedor)
                .expect_err("sin contenedor no hay dónde ejecutar la orden");

            assert!(
                matches!(
                    &error,
                    ErrorDeRestauracion::CampoQueFalta {
                        campo: "container_name",
                        ..
                    }
                ),
                "el fallo no nombra el contenedor que falta: {error:?}"
            );
        }
    }

    /// Un valor que **parece una bandera** se rechaza, y el motivo no lo publica.
    #[test]
    fn un_valor_que_parece_bandera_se_rechaza() {
        let caso = |objetivo: ObjetivoDeRestauracion, campo: &'static str, valor: &str| {
            let error = comando(raiz(), &objetivo)
                .expect_err("un valor que parece una bandera no da un camino");
            let mensaje = error.to_string();

            assert!(
                matches!(
                    &error,
                    ErrorDeRestauracion::ValorQuePareceBandera { campo: cual, .. } if *cual == campo
                ),
                "el fallo tiene que nombrar el campo `{campo}`: {error:?}"
            );
            assert!(
                mensaje.contains(campo),
                "el motivo no nombra el campo `{campo}`: {mensaje}"
            );
            assert!(
                !mensaje.contains(valor),
                "el motivo publica el valor que se rechazó: {mensaje}"
            );
            assert!(
                !mensaje.contains(CONTRASENA),
                "el motivo publica la contraseña: {mensaje}"
            );
        };

        caso(
            ObjetivoDeRestauracion {
                db_name: Some("-d".to_owned()),
                ..objetivo_postgresql(None)
            },
            "db_name",
            "-d",
        );
        caso(
            ObjetivoDeRestauracion {
                db_user: Some("--user".to_owned()),
                ..objetivo_mariadb(None)
            },
            "db_user",
            "--user",
        );
        caso(objetivo_sqlite(Some("--version")), "db_path", "--version");
    }

    // ── El directorio ────────────────────────────────────────────────────────

    /// Un target de `directory` se declara como camino de directorio y **no ejecuta comando**.
    #[test]
    fn un_target_de_directorio_no_ejecuta_ningun_comando() {
        let ruta_esperada = PathBuf::from("/backup_sources/var_www");

        match comando(
            raiz(),
            &objetivo_directorio(Some("/backup_sources/var_www")),
        )
        .expect("un directorio con su ruta da un camino")
        {
            CaminoDeRestauracion::PorDirectorio { ruta } => assert_eq!(ruta, ruta_esperada),
            CaminoDeRestauracion::PorContenedor {
                contenedor,
                ordenes,
                ..
            } => panic!(
                "un directorio no pasa por Docker, y salió por el contenedor `{contenedor}` con \
                 las órdenes {ordenes:?}"
            ),
        }
    }

    /// Un directorio **sin** ruta no se puede restaurar, y el motivo nombra el campo.
    #[test]
    fn un_target_de_directorio_sin_ruta_no_se_puede_restaurar() {
        let error = comando(raiz(), &objetivo_directorio(None))
            .expect_err("sin ruta no hay destino donde restaurar");

        assert!(
            matches!(
                &error,
                ErrorDeRestauracion::CampoQueFalta {
                    campo: "host_path",
                    ..
                }
            ),
            "el fallo no nombra la ruta que falta: {error:?}"
        );
    }

    // ── El confinamiento del destino (SEC-04) ────────────────────────────────
    //
    // La regla entera vive en [`crate::engine::frontera`] y se prueba allí sin motor; lo que estas
    // pruebas fijan es que [`comando`] la **aplica** a la rama de directorio, que es donde se
    // decide el destino de escritura y de poda. «Sin escribir ni podar» se mide aquí como que no se
    // construye el camino: `CaminoDeRestauracion::PorDirectorio` es lo que la restauración usaría
    // para escribir, y un rechazo no lo devuelve.

    /// **Una restauración fuera de la raíz no da destino**, y no se toca el destino.
    ///
    /// Es el escenario «la restauración tampoco escribe fuera de la raíz»: el motor canonicaliza el
    /// destino y, al no quedar bajo la raíz, no construye el camino de escritura. El testigo —un
    /// fichero en el destino— sigue intacto: no se escribió ni se podó nada.
    #[test]
    fn una_restauracion_fuera_de_la_raiz_no_da_destino_ni_toca_el_destino() {
        let raiz_de_copias = directorio_de_prueba();
        let fuera = directorio_de_prueba();
        let testigo = fuera.ruta().join("no-se-toca.txt");
        std::fs::write(&testigo, "intacto\n").expect("el testigo de la prueba se escribe");

        let error = comando(
            raiz_de_copias.ruta(),
            &objetivo_directorio(Some(texto(fuera.ruta()))),
        )
        .expect_err("una restauración fuera de la raíz no puede dar destino");

        assert!(
            matches!(
                &error,
                ErrorDeRestauracion::FueraDeLaFrontera {
                    causa: FronteraError::FueraDeLaRaiz {
                        campo: Campo::HostPath
                    }
                }
            ),
            "el fallo no es el de la frontera: {error:?}"
        );
        assert!(
            error.to_string().contains("host_path"),
            "el motivo no nombra el campo: {error}"
        );
        assert!(
            testigo.exists(),
            "el destino se tocó pese a rechazarse la ruta: {}",
            testigo.display()
        );
    }

    /// **Un enlace que escapa de la raíz tampoco es destino de escritura.**
    ///
    /// Es la misma regla en la otra dirección: el texto de la ruta empieza por la raíz —el borde
    /// HTTP la deja pasar— y el motor la canonicaliza, ve que el enlace la lleva fuera y no
    /// construye el destino. Ni escribe ni poda en el directorio enlazado.
    #[test]
    fn un_enlace_que_escapa_de_la_raiz_no_es_destino_de_restauracion() {
        let raiz_de_copias = directorio_de_prueba();
        let fuera = directorio_de_prueba();
        symlink(fuera.ruta(), raiz_de_copias.ruta().join("enlace"))
            .expect("el enlace de la prueba se crea");

        let declarado = raiz_de_copias.ruta().join("enlace/restaurado");

        let error = comando(
            raiz_de_copias.ruta(),
            &objetivo_directorio(Some(texto(&declarado))),
        )
        .expect_err("un enlace que escapa de la raíz no puede ser destino");

        assert!(
            matches!(
                &error,
                ErrorDeRestauracion::FueraDeLaFrontera {
                    causa: FronteraError::FueraDeLaRaiz {
                        campo: Campo::HostPath
                    }
                }
            ),
            "el fallo no es el de la frontera: {error:?}"
        );
    }

    // ── El puente hacia el `stdin` del contenedor ────────────────────────────

    /// Espera —sin bloquear el runtime— a que el escritor haya entregado `objetivo` trozos.
    ///
    /// Cede el turno en cada vuelta: el escritor corre en su propio hilo, y esto solo mira su
    /// contador. El plazo es un tope de seguridad para que una prueba rota falle en vez de colgar.
    async fn esperar_a_que_entregue(contador: &AtomicUsize, objetivo: usize) -> usize {
        let tope = tokio::time::Instant::now() + Duration::from_secs(10);

        loop {
            let visto = contador.load(Ordering::SeqCst);
            if visto >= objetivo {
                return visto;
            }

            assert!(
                tokio::time::Instant::now() < tope,
                "el escritor no llegó a {objetivo} trozos en diez segundos: el puente está atascado"
            );

            tokio::task::yield_now().await;
        }
    }

    /// **El puente entrega todos los bytes, en orden, por un canal acotado.**
    ///
    /// Es el caso normal: el repositorio vuelca trozos y el `stdin` del `exec` los consume. El
    /// trozo del escritor (7 bytes) y el búfer del consumidor (`read_to_end`, que pide de 8 KiB en
    /// 8 KiB) **no coinciden**, así que la prueba muerde el camino del `sobrante`: si el lector
    /// tirara lo que no cabe, el volcado saldría incompleto.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_puente_entrega_todos_los_bytes_en_orden() {
        /// El tamaño de cada trozo del escritor: a propósito, distinto del búfer del consumidor.
        const TAMANO_DEL_TROZO: usize = 7;

        let esperado: Vec<u8> = (0..40_000u32).map(|n| (n % 251) as u8).collect();

        let TunelDeRestauracion {
            mut escritor,
            mut lector,
        } = tunel_de_restauracion("volcado-de-prueba.sql", 4);

        let trozos: Vec<Vec<u8>> = esperado
            .chunks(TAMANO_DEL_TROZO)
            .map(<[u8]>::to_vec)
            .collect();
        let hilo = std::thread::spawn(move || {
            for trozo in trozos {
                escritor
                    .write_all(&trozo)
                    .expect("el lector consume, así que escribir no puede fallar");
            }
            escritor.terminar_bien();
        });

        let mut recibido = Vec::new();
        lector
            .read_to_end(&mut recibido)
            .await
            .expect("el volcado tiene que llegar entero");

        hilo.join().expect("el hilo del volcado no se puede caer");

        assert_eq!(
            recibido, esperado,
            "el puente no entregó los bytes del volcado en orden"
        );
    }

    /// **La capacidad del canal acota la memoria: el escritor se frena cuando no hay hueco.**
    ///
    /// Es la escena del requisito con números: el volcado es mucho mayor que el canal y, mientras
    /// el consumidor no lee, el escritor **no puede** pasar de la capacidad. Se comprueba en las
    /// dos direcciones —no pasa de la capacidad sin lector, y pasa de ella en cuanto se drena—,
    /// porque lo primero solo prueba una dirección del fallo.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn el_puente_acota_la_memoria_con_la_capacidad_del_canal() {
        /// Trozos que caben en el canal sin lector.
        const CAPACIDAD: usize = 2;
        /// Trozos que escribe el escritor: muchos más que la capacidad.
        const TOTAL: usize = 5_000;
        /// Bytes de cada trozo.
        const TAMANO: usize = 1_024;

        let TunelDeRestauracion {
            mut escritor,
            mut lector,
        } = tunel_de_restauracion("volcado-grande.sql", CAPACIDAD);

        let entregados = Arc::new(AtomicUsize::new(0));
        let contador = Arc::clone(&entregados);

        let hilo = std::thread::spawn(move || {
            for _ in 0..TOTAL {
                escritor
                    .write_all(&[b'x'; TAMANO])
                    .expect("el lector consume, así que escribir no puede fallar");
                contador.fetch_add(1, Ordering::SeqCst);
            }
            escritor.terminar_bien();
        });

        // Sin leer, el escritor se queda sin hueco en cuanto llena el canal.
        let visto = esperar_a_que_entregue(&entregados, CAPACIDAD).await;
        assert_eq!(
            visto, CAPACIDAD,
            "el escritor entregó {visto} trozos sin lector: el canal no acota nada"
        );

        // Y sigue parado mientras nadie lee: la memoria no crece con el volcado.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            entregados.load(Ordering::SeqCst),
            CAPACIDAD,
            "el escritor siguió adelante sin lector: la memoria crece con el volcado"
        );

        // Al drenar, el escritor vuelve a avanzar hasta el final.
        let mut recibido = Vec::new();
        lector
            .read_to_end(&mut recibido)
            .await
            .expect("el volcado tiene que llegar entero");

        hilo.join().expect("el hilo del volcado no se puede caer");

        assert_eq!(
            recibido.len(),
            TOTAL * TAMANO,
            "no llegaron todos los bytes del volcado"
        );
        assert_eq!(
            entregados.load(Ordering::SeqCst),
            TOTAL,
            "el escritor no llegó a entregar el volcado entero tras drenar"
        );
    }

    /// **Un volcado que falla llega al lector como error, no como fin de fichero.**
    ///
    /// Es el fallo del repositorio: [`Repository::dump`](rustic_core::Repository::dump) devolvió
    /// error, y el hilo del motor lo declara con [`EscritorDelVolcado::fallar`]. Si el lector
    /// contestara un fin de fichero limpio, el cliente de la base recibiría un fin de entrada con
    /// la orden a medias y la restauración se daría por buena: es el peor desenlace posible.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn un_volcado_que_falla_llega_al_lector_como_error() {
        const PRIMER_TROZO: &[u8] = b"-- la mitad del volcado\n";
        const MOTIVO: &str = "la instantánea no está en el repositorio";

        let TunelDeRestauracion {
            mut escritor,
            mut lector,
        } = tunel_de_restauracion("volcado-a-medias.sql", 4);

        let hilo = std::thread::spawn(move || {
            escritor
                .write_all(PRIMER_TROZO)
                .expect("el lector consume, así que escribir no puede fallar");
            escritor.fallar(MOTIVO);
        });

        let mut primero = [0u8; 64];
        let leidos = lector.read(&mut primero).await.expect("el trozo llega");
        assert_eq!(&primero[..leidos], PRIMER_TROZO);

        let error = lector
            .read(&mut primero)
            .await
            .expect_err("un volcado que falló no puede terminar en fin de fichero");

        assert_eq!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "un volcado a medias no es un fin de fichero: {error}"
        );
        assert!(
            error.to_string().contains(MOTIVO),
            "el error no lleva el motivo del repositorio: {error}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
    }

    /// **Cerrar el canal sin declarar el final es un volcado truncado.**
    ///
    /// Es el hilo del motor que se cae —o que se olvida— sin llamar a
    /// [`EscritorDelVolcado::terminar_bien`]. El silencio no es un final: el lector falla cerrado.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cerrar_sin_declarar_el_final_es_un_volcado_truncado() {
        let TunelDeRestauracion {
            mut escritor,
            mut lector,
        } = tunel_de_restauracion("volcado-truncado.sql", 4);

        let hilo = std::thread::spawn(move || {
            escritor
                .write_all(b"-- y hasta aqui\n")
                .expect("el lector consume, así que escribir no puede fallar");
            // Sin `terminar_bien` y sin `fallar`: el hilo se cae a mitad del volcado.
        });

        /// El trozo que se alcanzó a escribir antes del cierre: es lo que hay que leer entero.
        const TROZO: &[u8] = b"-- y hasta aqui\n";

        // `read_exact` y no `read`: el trozo es de un solo mensaje, así que llega entero, y así la
        // prueba no depende de por dónde parta una lectura.
        let mut primero = [0u8; TROZO.len()];
        lector
            .read_exact(&mut primero)
            .await
            .expect("el trozo llega antes del cierre");

        let error = lector
            .read(&mut primero)
            .await
            .expect_err("cerrar sin declarar el final no es un fin de fichero");

        assert_eq!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "un cierre sin final no es un fin de fichero: {error}"
        );
        assert!(
            error.to_string().contains("truncado"),
            "el error no dice que el volcado pueda estar truncado: {error}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
    }

    /// **Un `Write` sobre un canal sin lector no miente: devuelve error.**
    ///
    /// Es el otro desenlace del escritor: el `exec` se rindió —el contenedor desapareció— y ya
    /// nadie lee. Escribir entonces tiene que fallar, para que la biblioteca deje de volcar en vez
    /// de acumular bytes que nadie va a consumir.
    #[test]
    fn escribir_sin_lector_es_un_error_de_escritura() {
        let tunel = tunel_de_restauracion("volcado-sin-lector.sql", 4);
        drop(tunel.lector);

        let mut escritor = tunel.escritor;
        let error = escritor
            .write_all(b"un trozo que nadie va a leer")
            .expect_err("sin lector, escribir tiene que fallar");

        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "el fallo del lector no llega como la tubería rota que es: {error}"
        );
    }

    /// **La memoria no crece con el volcado: la sección se lee sin materializarlo.**
    ///
    /// Es la forma de medir de las pruebas del puente —la contrapresión del canal—, aplicada al
    /// lector de secciones: mientras la sección no se drena, el que vuelca no pasa de la capacidad
    /// del canal; y al drenarla, la sección entera se consume sin que el volcado se copie a ningún
    /// sitio. El volcado de la prueba es mucho mayor que el canal, así que si el adaptador
    /// acumulara la sección entera, el escritor habría avanzado sin freno.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn la_memoria_no_crece_con_el_volcado_por_secciones() {
        /// Trozos que caben en el canal sin lector.
        const CAPACIDAD: usize = 2;
        /// Bytes de cada línea del volcado.
        const TAMANO: usize = 8 * 1_024;
        /// Líneas del volcado: muchas más que la capacidad, para que el total supere con mucho
        /// cualquier búfer.
        const LINEAS: usize = 2_000;

        let TunelDeRestauracion {
            mut escritor,
            lector,
        } = tunel_de_restauracion("volcado-por-secciones.sql", CAPACIDAD);

        let entregados = Arc::new(AtomicUsize::new(0));
        let contador = Arc::clone(&entregados);

        let hilo = std::thread::spawn(move || {
            for _ in 0..LINEAS {
                // Cada trozo es una línea completa —el troceador no tiene que unir nada—, y el `\n`
                // final la cierra.
                let mut linea = vec![b'x'; TAMANO - 1];
                linea.push(b'\n');
                escritor
                    .write_all(&linea)
                    .expect("el lector consume, así que escribir no puede fallar");
                contador.fetch_add(1, Ordering::SeqCst);
            }
            escritor.terminar_bien();
        });

        // La guarda de la prueba, con un nonce de la misma forma que el real: 32 dígitos hex.
        let cabecera = format!("\\restrict {}\n", "0".repeat(32));

        let mut lineas = BufReader::new(lector);
        let mut troceador = Troceador::nuevo("postgres");
        let mut fin_del_volcado = false;

        let mut seccion = SeccionDelVolcado {
            lineas: &mut lineas,
            troceador: &mut troceador,
            pendiente: cabecera.as_bytes().to_vec(),
            consumido: 0,
            en_curso: Vec::new(),
            acabada: false,
            fin_del_volcado: &mut fin_del_volcado,
        };

        // Sin drenar la sección, el que vuelca se queda sin hueco en cuanto llena el canal.
        let visto = esperar_a_que_entregue(&entregados, CAPACIDAD).await;
        assert_eq!(
            visto, CAPACIDAD,
            "el volcado entregó {visto} trozos sin que nadie lea la sección: la memoria crece con \
             el volcado"
        );

        // Y sigue parado mientras nadie lee: la sección no acumula el volcado.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            entregados.load(Ordering::SeqCst),
            CAPACIDAD,
            "el volcado siguió adelante sin que nadie lea la sección: la memoria crece con el volcado"
        );

        // Al drenar, la sección se consume entera: la guarda delante y las líneas después.
        let mut total = 0usize;
        let mut buf = vec![0u8; 4 * 1_024];
        loop {
            let leidos = seccion
                .read(&mut buf)
                .await
                .expect("la sección tiene que leerse entera");
            if leidos == 0 {
                break;
            }
            total += leidos;
        }

        drop(seccion);

        assert!(
            fin_del_volcado,
            "el lector de la sección no marcó el fin del volcado: el bucle de secciones no pararía"
        );
        assert_eq!(
            total,
            cabecera.len() + LINEAS * TAMANO,
            "la sección no entregó la guarda y todas sus líneas"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
    }

    // ── El flujo de las órdenes contra el contenedor ─────────────────────────
    //
    // Estas pruebas necesitan un motor de contenedores, así que van marcadas `#[ignore]` y se
    // ejecutan a mano: `cargo test -- --ignored`. El instrumento es el contenedor de prueba del
    // arnés —un `alpine` con un `sh` de busybox dentro— y la sonda es una segunda orden que lee lo
    // que la primera dejó escrito: lo que se mide es **qué** recibió cada `exec` por su `stdin`.

    /// Plazo de inactividad de las pruebas con motor.
    ///
    /// Holgado a propósito: lo que estas pruebas fijan es que la acotación no estorba a una orden que
    /// avanza. Los dos casos que **necesitan** que el plazo venza usan uno corto, y ahí el plazo
    /// corto es lo que se prueba.
    const INACTIVIDAD_DE_PRUEBA: Duration = Duration::from_secs(60);

    /// Plazo de inactividad corto: el que tiene que vencer en las pruebas del cuelgue.
    const INACTIVIDAD_CORTA: Duration = Duration::from_millis(500);

    /// Una orden de shell: el contenedor de prueba trae `sh` de busybox.
    fn orden_de_shell(guion: &str) -> Orden {
        Orden {
            cmd: vec!["sh".to_owned(), "-c".to_owned(), guion.to_owned()],
            env: Vec::new(),
        }
    }

    /// Ejecuta un guion en el contenedor y devuelve su salida estándar como texto.
    ///
    /// Es la sonda de estas pruebas: [`ejecutar_las_ordenes`] descarta la salida —la de un cliente
    /// de restauración no es un fallo—, así que lo que una orden escriba hay que ir a leerlo
    /// después, con el camino sin entrada.
    async fn texto_de_una_orden(cliente: &Cliente, contenedor: &str, guion: &str) -> String {
        let mut salida = Vec::new();

        cliente
            .ejecutar(
                contenedor,
                &orden_de_shell(guion),
                INACTIVIDAD_DE_PRUEBA,
                |trozo: Trozo| {
                    if trozo.canal == Canal::Salida {
                        salida.extend_from_slice(&trozo.bytes);
                    }
                },
            )
            .await
            .expect("la sonda del contenedor no puede fallar por el transporte");

        String::from_utf8_lossy(&salida).trim().to_owned()
    }

    /// Alimenta el túnel con el contenido dado desde un hilo **fuera del runtime**, como el motor.
    ///
    /// Devuelve el `JoinHandle` para que la prueba pueda unir el hilo: unir es la mitad de lo que se
    /// mide en las pruebas del cuelgue —el hilo que volcaba no puede quedarse atascado—.
    fn alimentar_el_tunel(
        escritor: EscritorDelVolcado,
        contenido: Vec<u8>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut escritor = escritor;

            // De mejor esfuerzo: si el volcado no se llega a consumir —que es justo lo que algunas
            // de estas pruebas provocan— el fallo de escritura es el desenlace esperado, y el hilo
            // tiene que terminar igualmente.
            let _ = escritor.write_all(&contenido);
            escritor.terminar_bien();
        })
    }

    /// **El volcado se entrega solo a la última orden; las de antes corren con la entrada cerrada.**
    ///
    /// Es la regla del flujo: la última orden es la que reimporta, y las anteriores —la retirada de
    /// un `sqlite`— no leen nada. Darles un `stdin` abierto las dejaría esperando a un volcado que no
    /// les corresponde; darle el volcado a todas entregaría el mismo contenido a quien no debe
    /// verlo. Se mide con dos `wc -c` que escriben su recuento a un fichero del contenedor: cero
    /// bytes en la primera orden, el volcado entero en la última.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn el_volcado_se_entrega_solo_a_la_ultima_orden() {
        /// Bytes del volcado de la prueba: unos pocos, para que quepan en el canal de encuentro.
        const BYTES: usize = 4_096;

        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("el_volcado_se_entrega_solo_a_la_ultima_orden").await;

        let ordenes = vec![
            orden_de_shell("wc -c > /tmp/recuento-de-la-retirada"),
            orden_de_shell("wc -c > /tmp/recuento-de-la-reimportacion"),
        ];

        let TunelDeRestauracion { escritor, lector } =
            tunel_de_restauracion("volcado-de-la-base.sql", 4);
        let hilo = alimentar_el_tunel(escritor, vec![b'k'; BYTES]);

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &ordenes,
            Reparto::ALaUltimaOrden,
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("las dos órdenes salen con cero, así que no hay fallo que devolver");

        assert_eq!(codigo, 0, "el volcado se entregó y la orden salió con 0");

        let retirada = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "cat /tmp/recuento-de-la-retirada",
        )
        .await;
        assert_eq!(
            retirada, "0",
            "la orden de la retirada leyó {retirada} bytes por su `stdin`: tiene que correr con la \
             entrada cerrada, sin esperar un volcado que no le corresponde"
        );

        let reimportacion = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "cat /tmp/recuento-de-la-reimportacion",
        )
        .await;
        assert_eq!(
            reimportacion,
            BYTES.to_string(),
            "la última orden leyó {reimportacion} bytes del volcado: el volcado entero tiene que \
             llegarle por su `stdin`"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **El flujo para en la primera orden que no sale con cero, y lo que queda no se ejecuta.**
    ///
    /// Es la regla de [`ejecutar_las_ordenes`]: reimportar sobre una base que no se pudo preparar es
    /// peor que no reimportar. Se mide con un testigo: la segunda orden escribiría un fichero, y si
    /// corriera, el fichero estaría ahí. Y el motivo del fallo lleva lo que el proceso dijo por su
    /// canal de error, que es lo que el operador necesita para saber qué pasó.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_orden_que_falla_para_el_flujo_y_no_reimporta() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("una_orden_que_falla_para_el_flujo_y_no_reimporta").await;

        let ordenes = vec![
            orden_de_shell("echo el-antes-no-se-pudo >&2; exit 3"),
            orden_de_shell("wc -c > /tmp/el-testigo-de-la-reimportacion"),
        ];

        let TunelDeRestauracion { escritor, lector } =
            tunel_de_restauracion("volcado-de-la-base.sql", 4);
        // El volcado que **no** se debe entregar: el flujo se para antes de la última orden.
        let hilo = alimentar_el_tunel(escritor, vec![b'k'; 4_096]);

        let desenlace = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &ordenes,
            Reparto::ALaUltimaOrden,
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await;

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => {
                assert!(
                    motivo.contains('3') && motivo.contains("el-antes-no-se-pudo"),
                    "el motivo no lleva el código ni lo que dijo el contenedor: {motivo}"
                );
            }
            otro => panic!("una orden que sale con 3 tiene que parar el flujo: {otro:?}"),
        }

        let sonda = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "test -e /tmp/el-testigo-de-la-reimportacion && echo esta || echo no-esta",
        )
        .await;
        assert_eq!(
            sonda, "no-esta",
            "la orden de la reimportación corrió después de que la anterior fallara"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Una orden que se calla y no drena su `stdin` acaba en el plazo, no colgada.**
    ///
    /// Es el cuelgue, por el camino de producción del motor: el cliente de la base colgado —ni lee su
    /// entrada ni escribe nada— deja la entrega bloqueada en cuanto se llena la tubería, y lo único
    /// que acota el trabajo es el plazo de inactividad del drenaje. Con la espera de las dos mitades
    /// que había antes, esta llamada no volvía **nunca**: el trabajo no terminaba, la fila se quedaba
    /// en `running` y la reserva del destino no se soltaba. El plazo de la prueba es lo que convierte
    /// ese cuelgue en un fallo, y unir el hilo es lo que mide que el que volcaba quedó suelto.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_orden_que_no_drena_su_stdin_acaba_en_el_plazo_y_no_cuelga_el_flujo() {
        /// Bytes del volcado: más de lo que traga una tubería, para que la entrega se atasque.
        const BYTES: usize = 4 * 1024 * 1024;

        /// El plazo de la prueba entera, contenedor incluido.
        const ESPERA_DE_LA_PRUEBA: Duration = Duration::from_secs(30);

        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "una_orden_que_no_drena_su_stdin_acaba_en_el_plazo_y_no_cuelga_el_flujo",
        )
        .await;

        let ordenes = vec![orden_de_shell("sleep 60")];

        let TunelDeRestauracion { escritor, lector } =
            tunel_de_restauracion("volcado-de-la-base.sql", 4);
        let hilo = alimentar_el_tunel(escritor, vec![b'k'; BYTES]);

        let desenlace = tokio::time::timeout(
            ESPERA_DE_LA_PRUEBA,
            ejecutar_las_ordenes(
                &cliente,
                contenedor.nombre(),
                &ordenes,
                Reparto::ALaUltimaOrden,
                None,
                lector,
                INACTIVIDAD_CORTA,
            ),
        )
        .await
        .expect(
            "el flujo no concluyó dentro del plazo: la entrega sobrevivió al drenaje, y con ella \
             se quedan la fila en `running` y el destino reservado",
        );

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => assert!(
                motivo.contains("no produjo salida") && motivo.contains(contenedor.nombre()),
                "el motivo no dice que la orden se quedó callada y en qué contenedor: {motivo}"
            ),
            otro => panic!("una orden que se calla más que el plazo tiene que fallar: {otro:?}"),
        }

        hilo.join()
            .expect("el hilo que volcaba se quedó atascado: el lector no se soltó");
        contenedor.retirar().await;
    }

    // ── La forma canónica del `db_path` en la restauración (SEC-05) ──────────
    //
    // La comprobación es la misma `readlink -f` dentro del contenedor que la extracción usa, y va
    // **antes** de la primera orden —la retirada—: su desenlace decide si el `rm -f` llega a
    // correr. El instrumento es el contenedor de prueba con `sqlite3` instalado; la sonda es su
    // propio sistema de ficheros, qué sigue ahí después.

    /// Un contenedor de prueba con `sqlite3` instalado.
    ///
    /// `rm -f` lo trae busybox, pero la reimportación es `sqlite3 <db_path>`, que no viene en la
    /// imagen base de `alpine`: se instala aquí, como en el escenario de punta a punta. `readlink`
    /// ya viene con busybox, así que el contenedor cumple los dos requisitos del target `sqlite`.
    async fn contenedor_con_sqlite(prueba: &str) -> ContenedorDePrueba {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(prueba).await;

        let codigo = cliente
            .ejecutar(
                contenedor.nombre(),
                &orden_de_shell("apk add --no-cache sqlite >/dev/null 2>&1"),
                INACTIVIDAD_DE_PRUEBA,
                |_| {},
            )
            .await
            .expect("la instalación de `sqlite3` no puede fallar por el transporte");
        assert_eq!(
            codigo, 0,
            "no se pudo instalar `sqlite3` en el contenedor de prueba"
        );

        contenedor
    }

    /// **Un `db_path` que es un enlace no retira nada**: su forma canónica difiere de la declarada.
    ///
    /// Es el escenario «un db_path que es un enlace simbólico se rechaza», en la restauración: el
    /// `rm -f` de la retirada habría borrado el fichero al que apunta el enlace, y la comprobación
    /// —antes de la primera orden— lo impide. La sonda es el fichero real, que sigue en su sitio.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_db_path_que_no_es_su_forma_canonica_no_retira_nada() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            contenedor_con_sqlite("un_db_path_que_no_es_su_forma_canonica_no_retira_nada").await;

        // Un `.db` real y un enlace que apunta a él: `readlink -f` del enlace da el real, que no es
        // el declarado, así que la retirada no puede correr.
        cliente
            .ejecutar(
                contenedor.nombre(),
                &orden_de_shell(
                    "mkdir -p /datos && : > /datos/base.db && ln -sf /datos/base.db /datos/enlace.db",
                ),
                INACTIVIDAD_DE_PRUEBA,
                |_| {},
            )
            .await
            .expect("la preparación del contenedor no puede fallar por el transporte");

        let (_, ordenes, _reparto) = por_contenedor(&objetivo_sqlite(Some("/datos/enlace.db")));
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, b"-- nada\n".to_vec());

        let desenlace = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &ordenes,
            Reparto::ALaUltimaOrden,
            Some("/datos/enlace.db"),
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await;

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => assert!(
                motivo.contains("db_path"),
                "el motivo no nombra el campo: {motivo}"
            ),
            otro => panic!("un `db_path` que no es su forma canónica tiene que fallar: {otro:?}"),
        }

        let sigue = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "test -e /datos/base.db && echo esta || echo no-esta",
        )
        .await;
        assert_eq!(
            sigue, "esta",
            "la retirada corrió pese a que el `db_path` no es su forma canónica: el fichero ya no está"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Un `db_path` canónico sí restaura, y su `rm -f` no toca a los vecinos.**
    ///
    /// Es el contrapeso y, a la vez, la prueba de que la retirada borra **solo** el fichero declarado
    /// y sus hermanos `-wal` y `-shm`: un vecino del mismo directorio —otro `.db` y el `-wal` de ese
    /// otro— sobrevive, que es lo que un `rm -f` con una ruta mal formada se llevaría por delante.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_db_path_canonico_restaura_y_no_toca_a_sus_vecinos() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            contenedor_con_sqlite("un_db_path_canonico_restaura_y_no_toca_a_sus_vecinos").await;

        cliente
            .ejecutar(
                contenedor.nombre(),
                &orden_de_shell(
                    "mkdir -p /datos \
                     && : > /datos/base.db \
                     && : > /datos/base.db-wal \
                     && : > /datos/base.db-shm \
                     && : > /datos/vecino.db \
                     && : > /datos/vecino.db-wal",
                ),
                INACTIVIDAD_DE_PRUEBA,
                |_| {},
            )
            .await
            .expect("la preparación del contenedor no puede fallar por el transporte");

        let (_, ordenes, _reparto) = por_contenedor(&objetivo_sqlite(Some("/datos/base.db")));
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, b"-- nada\n".to_vec());

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &ordenes,
            Reparto::ALaUltimaOrden,
            Some("/datos/base.db"),
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("un `db_path` canónico no es un fallo de la restauración");

        assert_eq!(
            codigo, 0,
            "la reimportación de un `db_path` canónico sale con cero"
        );

        let vecinos = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "test -e /datos/vecino.db && test -e /datos/vecino.db-wal && echo intactos || echo tocados",
        )
        .await;
        assert_eq!(
            vecinos, "intactos",
            "el `rm -f` de la retirada tocó ficheros que no eran suyos"
        );

        let hermanos = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "test -e /datos/base.db-wal && echo quedan || echo retirados",
        )
        .await;
        assert_eq!(
            hermanos, "retirados",
            "los hermanos `-wal`/`-shm` del `db_path` no se retiraron antes de reimportar"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Un contenedor sin `readlink` falla la comprobación en voz alta**, sin retirar nada.
    ///
    /// Es el escenario «falta readlink en el contenedor», en la restauración: sin el binario no se
    /// puede leer la forma canónica, y suponer que coincide sería confiar en el campo que se está
    /// acotando. El fallo llega como el motivo de la restauración —con la orden de comprobación y su
    /// código dentro— y la retirada no corre: el fichero sigue donde estaba.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_contenedor_sin_readlink_falla_la_restauracion_sin_retirar_nada() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = contenedor_con_sqlite(
            "un_contenedor_sin_readlink_falla_la_restauracion_sin_retirar_nada",
        )
        .await;

        // `alpine` trae `readlink` como enlace a busybox en `/usr/bin`; se retira y la comprobación
        // se queda sin binario.
        cliente
            .ejecutar(
                contenedor.nombre(),
                &orden_de_shell(
                    "mkdir -p /datos && : > /datos/base.db && rm -f /usr/bin/readlink /bin/readlink",
                ),
                INACTIVIDAD_DE_PRUEBA,
                |_| {},
            )
            .await
            .expect("la preparación del contenedor no puede fallar por el transporte");

        let (_, ordenes, _reparto) = por_contenedor(&objetivo_sqlite(Some("/datos/base.db")));
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, b"-- nada\n".to_vec());

        let desenlace = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &ordenes,
            Reparto::ALaUltimaOrden,
            Some("/datos/base.db"),
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await;

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => {
                assert!(
                    motivo.contains("forma canónica") && motivo.contains(contenedor.nombre()),
                    "sin `readlink` la comprobación tiene que fallar nombrando el contenedor: {motivo}"
                );
            }
            otro => panic!("sin `readlink` la comprobación tiene que fallar: {otro:?}"),
        }

        let sigue = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "test -e /datos/base.db && echo esta || echo no-esta",
        )
        .await;
        assert_eq!(
            sigue, "esta",
            "la retirada corrió sin haberse podido comprobar la forma canónica del `db_path`"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    // ── El troceado por secciones de PostgreSQL (SEC-08) ─────────────────────
    //
    // Estas pruebas también necesitan motor, pero **no** un `psql` de verdad: lo que se mide es el
    // **reparto** —a qué base va cada sección, qué líneas se descartan y que la guarda `\restrict`
    // va delante de cada una—, y eso se ve con un **doble de cliente** que lee su `stdin` y anota la
    // base con la que se le invocó. El doble se instala una vez en el contenedor —ver
    // [`instalar_el_doble_de_cliente`]— y la plantilla del reparto es él: [`orden_de_una_seccion`]
    // le añade la base como último argumento, que el doble lee como `$1`.

    /// El guion del doble de cliente que usan las pruebas del reparto.
    ///
    /// Hace dos cosas. Con `--version` —la sonda de la precondición de `\restrict`— imprime la
    /// versión de un `psql` que **sí** trae `\restrict` y sale con cero, que es lo que hace que la
    /// precondición pase. Para el resto se comporta como la sonda de antes: vuelca su `stdin` a
    /// `/tmp/seccion-$1` y anota la base en `/tmp/bases`; y falla a propósito cuando la base es
    /// `mala`, que es lo que mide el escenario de la sección que falla.
    const GUION_DEL_DOBLE: &str = "cat > /tmp/cliente <<'KESTO'\n\
#!/bin/sh\n\
if [ \"$1\" = \"--version\" ]; then\n\
  echo 'psql (PostgreSQL) 18.6'\n\
  exit 0\n\
fi\n\
cat > \"/tmp/seccion-$1\"\n\
printf '%s\\n' \"$1\" >> /tmp/bases\n\
if [ \"$1\" = \"mala\" ]; then\n\
  echo la-seccion-mala-fallo >&2\n\
  exit 9\n\
fi\n\
KESTO\n\
chmod +x /tmp/cliente\n";

    /// Instala el doble de cliente en el contenedor.
    ///
    /// La precondición de `\restrict` ejecuta `<cmd[0]> --version`, así que una sonda que fuera `sh`
    /// mediría la versión de `sh` —que en busybox ni contesta—: el doble contesta la de un `psql`
    /// que sí trae `\restrict`.
    async fn instalar_el_doble_de_cliente(cliente: &Cliente, contenedor: &str) {
        texto_de_una_orden(cliente, contenedor, GUION_DEL_DOBLE).await;
    }

    /// Un reparto por secciones con el **doble de cliente** en lugar de `psql`.
    ///
    /// El doble recibe la base como `$1` —[`orden_de_una_seccion`] la añade al final del `cmd`— y
    /// escribe lo que le llega por el `stdin` a `/tmp/seccion-<base>`, además de anotar la base en
    /// `/tmp/bases`. Así se lee después qué recibió cada sección y con qué base se invocó.
    fn reparto_de_sonda(base_inicial: &str) -> Reparto {
        Reparto::PorSeccionesDePostgres {
            plantilla: Orden {
                cmd: vec!["/tmp/cliente".to_owned()],
                env: Vec::new(),
            },
            base_inicial: base_inicial.to_owned(),
        }
    }

    /// **Cada `\connect` abre una sección, y cada sección va a su base.**
    ///
    /// Es el escenario «un `\connect` a primer nivel abre una sección por base»: dos fronteras dan
    /// tres secciones, la primera sin `\connect` previo va a la base inicial, y cada `psql -d
    /// <base>` recibe **solo** el contenido de su sección. La frontera **no** se entrega: pertenece
    /// a la sección siguiente.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn la_restauracion_por_secciones_entrega_cada_base_a_su_base() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("la_restauracion_por_secciones_entrega_cada_base_a_su_base")
                .await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        let volcado = b"CREATE TABLE a (n int);\n\
                        \\connect segunda\n\
                        CREATE TABLE b (n int);\n\
                        \\connect tercera\n\
                        CREATE TABLE c (n int);\n"
            .to_vec();

        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("primera"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("las tres secciones salen con cero, así que no hay fallo que devolver");

        assert_eq!(codigo, 0, "el reparto entero sale con cero");

        let bases = texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/bases").await;
        assert_eq!(
            bases, "primera\nsegunda\ntercera",
            "las secciones no se entregaron a su base y en orden: {bases}"
        );

        for (base, contenido) in [
            ("primera", "CREATE TABLE a (n int);"),
            ("segunda", "CREATE TABLE b (n int);"),
            ("tercera", "CREATE TABLE c (n int);"),
        ] {
            let recibido = texto_de_una_orden(
                &cliente,
                contenedor.nombre(),
                &format!("cat /tmp/seccion-{base}"),
            )
            .await;

            assert!(
                recibido.contains(contenido),
                "la sección de `{base}` no recibió su contenido: {recibido}"
            );
            assert!(
                !recibido.contains("\\connect"),
                "la sección de `{base}` recibió una frontera, que se tiene que descartar: {recibido}"
            );
        }

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Una sección que falla falla la restauración entera.**
    ///
    /// Es el escenario «una sección que falla falla la restauración entera»: la primera sección
    /// acaba bien y la segunda falla, y el fallo sube con el motivo del contenedor —no se da por
    /// buena la restauración aunque la primera haya ido bien—.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn una_seccion_que_falla_falla_la_restauracion_entera() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("una_seccion_que_falla_falla_la_restauracion_entera").await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        // El doble falla —con su motivo— cuando la base es `mala`; la primera sección es `buena`.
        let reparto = reparto_de_sonda("buena");

        let volcado =
            b"CREATE TABLE a (n int);\n\\connect mala\nCREATE TABLE b (n int);\n".to_vec();
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let desenlace = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto,
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await;

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => assert!(
                motivo.contains('9') && motivo.contains("la-seccion-mala-fallo"),
                "el motivo no lleva el código ni lo que dijo el contenedor: {motivo}"
            ),
            otro => {
                panic!("una sección que falla tiene que fallar la restauración entera: {otro:?}")
            }
        }

        // La primera sección sí se entregó: el fallo es de la segunda.
        let primera =
            texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/seccion-buena").await;
        assert!(
            primera.contains("CREATE TABLE a"),
            "la primera sección no llegó a su base: {primera}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Los metacomandos hostiles del volcado no llegan al cliente.**
    ///
    /// Es el escenario «los metacomandos del volcado no se ejecutan»: un `\!` de primer nivel
    /// —que ejecutaría una orden de shell— y un `\unrestrict` que intentaría deshacer la guarda,
    /// éste precedido de un tabulador, porque el cliente acepta el metacomando así. Los dos se
    /// descartan y **no** se entregan al `psql`, así que no hay nada que ejecutar.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn los_metacomandos_hostiles_no_llegan_al_cliente() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("los_metacomandos_hostiles_no_llegan_al_cliente").await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        let volcado = b"CREATE TABLE t (n int);\n\
                        \\! touch /tmp/pwned-por-el-metacomando\n\
                        \t\\unrestrict lo-que-sea\n\
                        CREATE TABLE u (n int);\n"
            .to_vec();

        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("postgres"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("los metacomandos se descartan, así que la restauración completa");

        assert_eq!(codigo, 0, "descartar metacomandos no es un fallo");

        let recibido =
            texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/seccion-postgres").await;
        for prohibido in ["\\!", "unrestrict", "touch", "pwned"] {
            assert!(
                !recibido.contains(prohibido),
                "la sección recibió `{prohibido}`, que es un metacomando que tenía que descartarse: \
                 {recibido}"
            );
        }
        assert!(
            recibido.contains("CREATE TABLE t") && recibido.contains("CREATE TABLE u"),
            "el contenido legítimo no llegó entero: {recibido}"
        );

        let pwned = texto_de_una_orden(
            &cliente,
            contenedor.nombre(),
            "test -e /tmp/pwned-por-el-metacomando && echo si || echo no",
        )
        .await;
        assert_eq!(pwned, "no", "la orden de shell del `\\!` se ejecutó");

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **La guarda `\restrict` va delante de cada sección, con el mismo nonce.**
    ///
    /// Es la segunda capa del endurecimiento: cada sección lleva `\restrict <nonce>` como primera
    /// línea, y el nonce es el **mismo** en todas —uno por restauración— y aleatorio. Si un
    /// metacomando se colara, `\restrict` lo rechazaría.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn el_restrict_va_delante_de_cada_seccion() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo("el_restrict_va_delante_de_cada_seccion").await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        let volcado =
            b"CREATE TABLE a (n int);\n\\connect segunda\nCREATE TABLE b (n int);\n".to_vec();
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("primera"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("las dos secciones salen con cero");

        assert_eq!(codigo, 0);

        let mut nonce: Option<String> = None;
        for base in ["primera", "segunda"] {
            let primera_linea = texto_de_una_orden(
                &cliente,
                contenedor.nombre(),
                &format!("head -1 /tmp/seccion-{base}"),
            )
            .await;

            let (etiqueta, valor) = primera_linea.split_once(' ').unwrap_or_else(|| {
                panic!("la guarda de `{base}` no tiene etiqueta y nonce: {primera_linea}")
            });

            assert_eq!(
                etiqueta, "\\restrict",
                "la sección de `{base}` no empieza por la guarda: {primera_linea}"
            );
            assert_eq!(
                valor.len(),
                32,
                "el nonce de `{base}` no son 32 dígitos hexadecimales: {valor}"
            );
            assert!(
                valor.chars().all(|caracter| caracter.is_ascii_hexdigit()),
                "el nonce de `{base}` no es hexadecimal: {valor}"
            );

            match &nonce {
                None => nonce = Some(valor.to_owned()),
                Some(esperado) => assert_eq!(
                    valor, esperado,
                    "el nonce cambió entre secciones: tiene que ser uno por restauración"
                ),
            }
        }

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Un `\connect` cuyo nombre tiene forma de cadena de conexión falla la restauración.**
    ///
    /// Es el escenario «El nombre de base del `\connect` no se convierte en una cadena de conexión»:
    /// `psql -d` lee el valor como *conninfo* —y de ahí saca `host`, `port`, `user`…— si trae un `=`
    /// o es una URI, así que un `\connect` que nombre así su base desviaría la conexión a otro
    /// servidor **con `PGPASSWORD` puesto**. La restauración **falla** y la base hostil **no** llega
    /// a ejecutarse: el doble no la anota en `/tmp/bases`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_connect_con_nombre_de_conninfo_falla_la_restauracion() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("un_connect_con_nombre_de_conninfo_falla_la_restauracion")
                .await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        // El volcado trae la base hostil entre comillas, como la acepta el troceador.
        let volcado = b"CREATE TABLE a (n int);\n\
                        \\connect \"host=atacante.example dbname=postgres\"\n\
                        CREATE TABLE b (n int);\n"
            .to_vec();

        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let desenlace = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("primera"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await;

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => {
                assert!(
                    motivo.contains(contenedor.nombre()) && motivo.contains("cadena de conexión"),
                    "el motivo no nombra el contenedor y la forma del nombre: {motivo}"
                );
                // El valor crudo es contenido no confiable y **no** puede acabar en el motivo, que
                // el panel muestra.
                assert!(
                    !motivo.contains("atacante") && !motivo.contains("host="),
                    "el motivo publica el nombre de base del volcado: {motivo}"
                );
            }
            otro => panic!(
                "un nombre de base con forma de conninfo tiene que fallar la restauración: {otro:?}"
            ),
        }

        // La sección hostil **no** se ejecutó: el doble anota la base de cada sección que corre, y
        // ahí solo puede estar la primera —la que elige el operador—.
        let bases = texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/bases").await;
        assert_eq!(
            bases, "primera",
            "la base con forma de cadena de conexión llegó a ejecutarse: {bases}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Un nombre de base que empieza por `-` sobrevive el viaje hasta el cliente.**
    ///
    /// Mide el **viaje**, no la posición: el `\connect "-x"` del volcado se trocea, la base se
    /// compone con la plantilla y llega al cliente —el doble— entera y como un argumento suyo, sin
    /// que nada la reinterprete por el camino. Que quede **inmediatamente después de `-d`** —que es
    /// lo que la hace un valor y no una opción— lo fija el test puro
    /// [`un_nombre_de_base_que_empieza_por_guion_va_como_valor_de_d`]: la plantilla de estas pruebas
    /// es el doble **sin `-d`**, así que por sí solo este test no distinguiría un valor de `-d` de un
    /// argumento posicional.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_nombre_de_base_que_empieza_por_guion_sobrevive_el_viaje_hasta_el_cliente() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "un_nombre_de_base_que_empieza_por_guion_sobrevive_el_viaje_hasta_el_cliente",
        )
        .await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        let volcado =
            b"CREATE TABLE a (n int);\n\\connect \"-x\"\nCREATE TABLE b (n int);\n".to_vec();
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("primera"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("un nombre con guion no es un fallo: sobrevive el viaje hasta el cliente");

        assert_eq!(codigo, 0, "las dos secciones salen con cero");

        let bases = texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/bases").await;
        assert_eq!(
            bases, "primera\n-x",
            "el nombre con guion no llegó entero al cliente: {bases}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **La base de la primera sección NO se valida: la elige el operador, no el volcado.**
    ///
    /// Aunque la `base_inicial` **parezca** una cadena de conexión —aquí `host=x`—, sale de la
    /// **configuración del target** (`db_name`, o `postgres` por defecto), no del volcado, así que
    /// no es contenido no confiable y se entrega tal cual: la primera sección corre con esa base. La
    /// asimetría es deliberada —si el predicado se aplicara también aquí, un target legítimo con un
    /// nombre de base exótico dejaría de restaurar— y este test la fija para que nadie la «arregle»
    /// aplicando [`parece_cadena_de_conexion`] a la primera sección.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn la_base_inicial_no_se_valida_porque_la_elige_el_operador() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor =
            ContenedorDePrueba::nuevo("la_base_inicial_no_se_valida_porque_la_elige_el_operador")
                .await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        let volcado =
            b"CREATE TABLE a (n int);\n\\connect segunda\nCREATE TABLE b (n int);\n".to_vec();
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let codigo = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("host=x"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await
        .expect("la base inicial la elige el operador: no es un fallo");

        assert_eq!(codigo, 0, "las dos secciones salen con cero");

        let bases = texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/bases").await;
        assert_eq!(
            bases, "host=x\nsegunda",
            "la primera sección no corrió con la base del operador: {bases}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    /// **Un `\connect` hostil en la primera línea del volcado también falla la restauración.**
    ///
    /// Es un camino distinto del de una frontera a media altura: aquí el nombre con forma de cadena
    /// de conexión es la **primera** base que sale de un `\connect`, sin contenido de sección por
    /// delante. La restauración **falla** y la base hostil **no** llega a ejecutarse —el doble no la
    /// anota en `/tmp/bases`—, así que no se restaura nada en el servidor al que apuntaba la
    /// *conninfo*. La sección que va a la `base_inicial` —vacía, porque el volcado abre con el
    /// `\connect`— sí corre, con la base del operador; lo que no corre es la hostil.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requiere un motor de contenedores, y el CI no levanta ninguno: se ejecuta con cargo test -- --ignored"]
    async fn un_connect_hostil_en_la_primera_linea_falla_la_restauracion() {
        let cliente = cliente_del_motor().expect("hace falta un socket de Docker o Podman");
        let contenedor = ContenedorDePrueba::nuevo(
            "un_connect_hostil_en_la_primera_linea_falla_la_restauracion",
        )
        .await;
        instalar_el_doble_de_cliente(&cliente, contenedor.nombre()).await;

        // El `\connect` hostil abre el volcado: no hay contenido de sección antes de él.
        let volcado = b"\\connect \"host=atacante.example dbname=postgres\"\n\
                        CREATE TABLE b (n int);\n"
            .to_vec();
        let TunelDeRestauracion { escritor, lector } = tunel_de_restauracion("volcado.sql", 4);
        let hilo = alimentar_el_tunel(escritor, volcado);

        let desenlace = ejecutar_las_ordenes(
            &cliente,
            contenedor.nombre(),
            &[],
            reparto_de_sonda("primera"),
            None,
            lector,
            INACTIVIDAD_DE_PRUEBA,
        )
        .await;

        match desenlace {
            Err(ErrorDelMotor::Restauracion { motivo }) => assert!(
                motivo.contains(contenedor.nombre()) && motivo.contains("cadena de conexión"),
                "el motivo no nombra el contenedor y la forma del nombre: {motivo}"
            ),
            otro => panic!(
                "un `\\connect` hostil en la primera línea tiene que fallar la restauración: {otro:?}"
            ),
        }

        // La base hostil **no** se ejecutó: el doble anota la base de cada sección que corre, y ahí
        // solo puede estar la del operador —la sección vacía que abre el volcado, que no lleva
        // contenido—. Si la hostil hubiera llegado, aparecería aquí.
        let bases = texto_de_una_orden(&cliente, contenedor.nombre(), "cat /tmp/bases").await;
        assert_eq!(
            bases, "primera",
            "la base con forma de cadena de conexión llegó a ejecutarse: {bases}"
        );

        hilo.join().expect("el hilo del volcado no se puede caer");
        contenedor.retirar().await;
    }

    // ── La precondición de `\restrict`: las funciones puras ──────────────────
    //
    // No necesitan motor: lo que se fija es la lectura de `psql --version` y el mínimo por rama.

    /// La versión se lee de la salida de `psql --version`, con el sufijo de la distribución detrás
    /// si lo trae.
    #[test]
    fn la_version_del_psql_se_lee_de_su_salida() {
        assert_eq!(
            version_de_psql("psql (PostgreSQL) 18.6"),
            Some((18, 6)),
            "la forma limpia no se leyó"
        );
        assert_eq!(
            version_de_psql("psql (PostgreSQL) 17.6 (Debian 17.6-1.pgdg120+1)"),
            Some((17, 6)),
            "el sufijo de la distribución no se ignoró"
        );
        assert_eq!(
            version_de_psql("psql (PostgreSQL) 9.6.24"),
            Some((9, 6)),
            "una versión de dos dígitos de mayor no se leyó"
        );
        assert_eq!(
            version_de_psql("esto no es una versión"),
            None,
            "la basura sin dígitos no puede leerse como una versión"
        );
    }

    /// `soporta_restrict` es el mínimo por rama del ciclo de parches de 2025-08-14: 17.6, 16.10,
    /// 15.14, 14.19 y 13.22, y PostgreSQL 18 de serie.
    #[test]
    fn soporta_restrict_aplica_el_minimo_de_cada_rama() {
        for (salida, esperado) in [
            ("psql (PostgreSQL) 18.0", true),
            ("psql (PostgreSQL) 17.5", false),
            ("psql (PostgreSQL) 17.6", true),
            ("psql (PostgreSQL) 16.9", false),
            ("psql (PostgreSQL) 16.10", true),
            ("psql (PostgreSQL) 15.13", false),
            ("psql (PostgreSQL) 15.14", true),
            ("psql (PostgreSQL) 14.18", false),
            ("psql (PostgreSQL) 14.19", true),
            ("psql (PostgreSQL) 13.21", false),
            ("psql (PostgreSQL) 13.22", true),
            ("psql (PostgreSQL) 12.20", false),
        ] {
            assert_eq!(
                soporta_restrict(salida),
                esperado,
                "el veredicto de `{salida}` no es el esperado"
            );
        }
    }

    /// Una salida de la que no se puede leer la versión es **fail-closed**: se trata como «no la
    /// trae».
    #[test]
    fn soporta_restrict_es_fail_closed_sin_version() {
        assert!(!soporta_restrict(""));
        assert!(!soporta_restrict("psql: command not found"));
    }

    // ── El nombre de base del `\connect`: las funciones puras ────────────────
    //
    // Tampoco necesitan motor: lo que se fija es qué nombres tienen forma de cadena de conexión para
    // `psql`, que es lo que se rechaza antes de componer la orden de una sección.

    /// `psql -d` lee el valor como *conninfo* si **contiene un `=`** o si empieza por `postgres://` o
    /// `postgresql://`; cualquier otro nombre es un nombre.
    #[test]
    fn parece_cadena_de_conexion_reconoce_la_forma_de_conninfo() {
        for nombre in [
            "a=b",
            "host=atacante.example dbname=postgres",
            "postgres://x/y",
            "POSTGRESQL://x/y",
        ] {
            assert!(
                parece_cadena_de_conexion(nombre),
                "`{nombre}` tiene forma de cadena de conexión y no se reconoció"
            );
        }

        for nombre in ["postgres", "mi base", "a-b", "a$b", "postgres_db"] {
            assert!(
                !parece_cadena_de_conexion(nombre),
                "`{nombre}` es un nombre de base y se tomó por una cadena de conexión"
            );
        }
    }
}
