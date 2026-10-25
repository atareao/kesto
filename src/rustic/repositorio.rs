//! El repositorio de copias: ponerlo en marcha, archivar dentro de él y listar lo que tiene.
//!
//! Esta es la mitad del módulo que le habla a la biblioteca de copia en su idioma —un
//! `Repository` con sus opciones de archivado y sus instantáneas—, y el puente
//! ([`crate::rustic::origen`]) es la otra. Lo que cruza esta frontera son los tipos de
//! aquí: un [`Acceso`] que trae los dos secretos **ya descifrados**, una [`Copia`] que
//! describe lo que se copia, una [`CopiaHecha`] que cuenta lo que quedó y una
//! [`Retencion`] que dice qué hay que conservar.
//!
//! # La clave maestra no entra aquí
//!
//! El módulo recibe el `secret_access_key` y la `passphrase` en claro y **nada más**. No
//! recibe la clave maestra del proceso, y eso no es una comodidad de la firma: el cifrado
//! en reposo del §3 vive en la capa de acceso, y un módulo que además tuviera la clave
//! maestra sería el **segundo** sitio del programa capaz de descifrar credenciales —el
//! primero es el que ya las descifra—. Con dos sitios, la cifra de secretos deja de acotar
//! quién puede leerlos y pasa a acotar quién se acuerda de no hacerlo. La frontera es
//! limpia: quien va a usar el secreto es quien lo recibe en claro.
//!
//! # Los tres desenlaces se deciden preguntándole **al destino**
//!
//! Poner un repositorio en marcha tiene tres desenlaces y no dos: no existía y se ha
//! creado, existía y se ha conectado, y existía pero **no abre**. Para el operador no se
//! arreglan igual —«lo he creado», «he conectado» y «revisa la passphrase» son tres
//! acciones distintas—, así que las tres tienen que llegar distinguibles a quien llama.
//!
//! Y la biblioteca no las distingue: está medido que `open()` señala «no hay fichero de
//! configuración» con el **mismo** `ErrorKind::Configuration` con el que `init()` señala
//! «ya hay uno». Por el tipo es imposible; por el texto sería atarse a una cadena de una
//! dependencia que puede cambiar en cualquier versión menor. Lo que sí es público es el
//! destino: se le puede preguntar qué ficheros de configuración tiene, y esa respuesta
//! —ninguno, uno, o un error— **es** la distinción. Ver [`poner_en_marcha_con`].
//!
//! # El `Ok` del archivado no es la condición de éxito
//!
//! Está medido, y condiciona cómo se usa este módulo: el archivado de la biblioteca
//! **silencia** el error del lector con un aviso, se lo traga y devuelve un resultado
//! correcto **guardando la instantánea igualmente**, sin el volcado dentro —la entrada que
//! falló no llega al árbol, así que el fichero desaparece—. Y en el peor caso medido —el
//! productor murió y la instantánea anterior sí tenía el nodo del volcado— no hay **ni una
//! línea de aviso**.
//!
//! Por eso [`CopiaHecha`] **no tiene ningún campo que signifique «éxito»**: cuenta lo que
//! el archivado procesó y da el identificador de la instantánea, y la condición de éxito
//! la compone el motor, que es el único que ve las tres cosas que la forman —el volcado se
//! entregó entero ([`OrigenDelVolcado`] y su
//! [`EntregaDelVolcado::completa`](crate::rustic::EntregaDelVolcado::completa)), el proceso
//! de extracción terminó bien, y la instantánea tiene el volcado dentro—. Heredar el éxito
//! del `Ok` de aquí es exactamente el fallo que este módulo existe para no cometer.
//!
//! # El repositorio se abre **por ejecución**
//!
//! Medido: reutilizar un repositorio ya abierto entre dos copias **desactiva la
//! deduplicación por completo**, porque el índice en memoria no contiene el árbol que la
//! copia anterior acaba de escribir y la biblioteca avisa de que no encuentra el árbol del
//! padre. Un repositorio de larga vida no ahorra trabajo: lo esconde. Así que el
//! repositorio se pone en marcha una vez por ejecución, y por eso este tipo es de un solo
//! uso práctico en vez de un servicio que se guarde en el estado del motor.
//!
//! De ahí sale también la decisión de guardar el repositorio en su estado **indexado**
//! ([`IndexedIdsStatus`]): las tres operaciones que este tipo expone —archivar, retener y
//! listar— se piden sobre `&self`, y el índice se lee una sola vez, al poner en marcha, en
//! vez de una vez por operación. Es el índice reducido —`to_indexed_ids`, solo los
//! identificadores—, que es el que basta para añadir datos y para purgar; el completo
//! guarda además el tamaño y la situación de cada trozo, y con él la memoria retenida
//! crecería con el repositorio entero.
//!
//! # El esquema del destino, y con el nombre que funciona
//!
//! Medido: `OpenDALBackend::new("opendal:s3", …)` **falla**. La ruta se parte por el primer
//! `:` y lo que queda delante es el esquema que `opendal` busca en su registro —«s3»,
//! «memory», «fs»…—, así que «opendal:s3» le pide un servicio llamado «opendal», que no
//! existe:
//!
//! ```text
//! Err(RusticError { kind: Backend, guidance: "Creating Operator from path `{path}` failed…",
//!     context: [("path", "opendal:s3"), ("schema", "opendal")],
//!     source: Some(Unsupported (permanent) at  => scheme is not registered …
//!         available: azblob, …, s3, sftp, swift, webdav, …) })
//! ```
//!
//! Lo que hay que pasar es el **esquema de `opendal`** —`"s3"`—, que es justo lo que
//! `rustic_backend` calcula por su cuenta cuando se le da la URL de un backend
//! (`location_to_type_and_path`: el tipo del backend delante, su ruta detrás). Aquí no hay
//! URL que partir: el esquema lo pone este módulo y el destino lo describen los campos de
//! la fila.
//!
//! # Lo que el nodo de un volcado **no** lleva, y por qué hay que desactivar el padre
//!
//! El nodo que el puente fabrica para un volcado lleva tamaño cero y no lleva fechas ni
//! inodo, porque no puede llevarlos: la longitud de un volcado por `stdout` no se conoce
//! hasta que termina. Y el emparejamiento de la biblioteca compara exactamente esos
//! campos, así que dos copias del mismo destino comparan igual, la segunda **empareja**, la
//! biblioteca **descarta el lector sin leerlo** y la instantánea nueva apunta al contenido
//! de la anterior. Está medido restaurando: la copia de hoy devolvió los bytes de ayer.
//!
//! Por eso [`Repositorio::archivar_volcado`] archiva con `parent_opts.force = true`, que es
//! lo que hace la propia biblioteca en su ruta de entrada estándar y lo que quien llama al
//! archivado directamente —como este motor— se salta si no lo pone él. Y por eso
//! [`Repositorio::archivar_directorio`] **no** lo pone: un directorio sí tiene metadatos de
//! verdad con los que decidir si un fichero cambió, y desactivar ahí el padre tiraría la
//! deduplicación sin ganar nada.
//!
//! # La procedencia que la instantánea graba
//!
//! La biblioteca tiene aquí un defecto peligroso y medido: si el campo del comando no se
//! informa, `SnapshotFile::from_options` lo rellena con `std::env::args_os()`, es decir con
//! **la línea de comandos de Kesto** —y en un arranque con variables por argumento, con
//! secretos dentro—. Queda guardado de forma permanente en el repositorio y es visible en
//! el listado. Por eso el comando, el host y la etiqueta se informan **siempre**, con lo
//! que dice la [`Copia`] y nunca con lo que diga el proceso.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rustic_backend::opendal::OpenDALBackend;
use rustic_core::repofile::{SnapshotFile, SnapshotId};
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, Excludes, FileType, IndexedIdsStatus, KeyOptions,
    LocalSource, LocalSourceFilterOptions, LocalSourceSaveOptions, ParentOptions, ReadBackend,
    Repository, RepositoryBackends, RepositoryOptions, SnapshotOptions, WriteBackend,
};
use time::OffsetDateTime;

use super::{BarrasDelMotor, Error, InformeDeRetencion, OrigenDelVolcado, Retencion};
use crate::secret::SecretString;

/// Lo que hace falta para hablar con un repositorio: la fila del §3, con los dos secretos
/// **ya descifrados** por la capa de acceso.
///
/// Los dos secretos son [`SecretString`] y no `String` a propósito: aunque el módulo tenga
/// que entregárselos en claro a la biblioteca —no hay otra forma de abrir un repositorio—,
/// el tipo impide que se cuelen en un `Debug`, en un `Display` o en el motivo de un error.
/// Se sacan con [`SecretString::expose`], que es un método con nombre: cada sitio que los
/// usa queda visible al leer el código.
///
/// **No es `Clone`**, y es deliberado: con la copia clonable, cada duplicación del acceso
/// duplica en memoria los dos secretos —la clave y la passphrase—, que es justo lo que este
/// tipo evita al no guardarse dentro del repositorio una vez abierto (el repositorio guarda
/// el **nombre** del destino, no el acceso del que salió). Quien necesite hablar dos veces
/// con el mismo destino construye el acceso otra vez desde la fila, que es de donde salió.
///
/// La `region` viaja tal cual y **no** se valida: el §3 la guarda, hay destinos que la
/// declaran vacía y `opendal` decide qué hacer con eso. Lo que sí se valida es que el
/// destino y las credenciales estén informados, porque un cubo vacío o una credencial
/// vacía no son una configuración floja: son una fila que no puede funcionar.
#[derive(Debug)]
pub struct Acceso {
    /// Endpoint del servicio S3.
    pub endpoint: String,
    /// Cubo de destino.
    pub bucket: String,
    /// Región del cubo.
    pub region: String,
    /// Identificador público de la credencial.
    pub access_key_id: String,
    /// Secreto de la credencial, **descifrado**.
    pub secret_access_key: SecretString,
    /// Passphrase del repositorio Rustic, **descifrada**.
    pub passphrase: SecretString,
}

impl Acceso {
    /// Cómo se nombra este destino en el motivo de un error.
    ///
    /// `endpoint` y `bucket` juntos, que es lo que identifica al cubo sin depender de cómo
    /// esté escrito el endpoint: con barra final o sin ella, lo que se lee es el mismo
    /// sitio. Ni la clave de acceso ni, por supuesto, los secretos entran aquí.
    fn nombre_del_destino(&self) -> String {
        format!(
            "{}/{}",
            self.endpoint.trim_end_matches('/'),
            self.bucket.trim_matches('/')
        )
    }

    /// Comprueba que la fila describe un destino que puede funcionar.
    ///
    /// No se valida la `region`, que puede estar vacía por diseño del destino; sí el
    /// resto, porque un endpoint, un cubo o una credencial vacíos no son una configuración
    /// floja del operador: son una fila que no puede abrir nada, y es mejor decirlo
    /// nombrando el campo que dejar que lo diga `opendal` con un mensaje sobre un
    /// parámetro que no se ve.
    ///
    /// Y no se valida la **longitud** de nada: la contraseña de un repositorio Rustic es
    /// tan larga como quiera quien la eligiera, y negarse por corta convertiría una
    /// decisión del operador en un fallo de la copia.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::AccesoInvalido`] nombrando el primer campo vacío.
    fn validar(&self) -> Result<(), Error> {
        let obligatorios = [
            ("endpoint", self.endpoint.as_str()),
            ("bucket", self.bucket.as_str()),
            ("access_key_id", self.access_key_id.as_str()),
            ("secret_access_key", self.secret_access_key.expose()),
            ("passphrase", self.passphrase.expose()),
        ];

        for (campo, valor) in obligatorios {
            if valor.trim().is_empty() {
                return Err(Error::AccesoInvalido {
                    campo: campo.to_owned(),
                });
            }
        }

        Ok(())
    }
}

/// Cuál de los tres desenlaces ocurrió al poner el repositorio en marcha.
///
/// No es cosmético: «no existía, lo he creado» y «existía y he conectado» no se arreglan
/// igual. Un destino recién creado está vacío y la primera copia que llegue será la
/// primera del repositorio; uno que ya existía tiene un historial que la nueva copia
/// viene a continuar. El tercer desenlace —existía y no abre— no está aquí porque no es un
/// desenlace sino un fallo: ver [`Error::PassphraseNoAbre`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PuestaEnMarcha {
    /// El destino no tenía repositorio y se ha creado ahora.
    Inicializado,
    /// El destino ya tenía repositorio y se ha conectado con él.
    Conectado,
}

/// Lo que se le pide a una copia.
///
/// Es la traducción de una tarea y su target a lo que la biblioteca necesita saber de una
/// instantánea, y nada más: ni la contraseña de la base —que viaja por el entorno del
/// `exec` y no llega hasta aquí— ni el repositorio, que va por otro camino.
#[derive(Debug, Clone)]
pub struct Copia {
    /// Las rutas que se copian, tal como las ve el proceso.
    ///
    /// Son las que la instantánea **declara** que contiene. Para un volcado de flujo el
    /// árbol solo puede contener un fichero —el que nombra [`Copia::nombre`]—, así que
    /// declarar aquí otra cosa deja una instantánea que dice contener lo que no tiene.
    pub rutas: Vec<PathBuf>,
    /// El nombre del volcado dentro de la instantánea: el que valida
    /// [`tunel_del_volcado`](crate::rustic::tunel_del_volcado).
    pub nombre: String,
    /// La etiqueta que identifica la tarea en la instantánea.
    pub etiqueta: String,
    /// El nombre del equipo, para la instantánea.
    pub host: String,
    /// La procedencia: la orden de la que salió el volcado.
    pub comando: String,
}

impl Copia {
    /// La instantánea que describe esta copia, con su procedencia grabada.
    ///
    /// El comando, el host y la etiqueta se informan **siempre**: es lo que impide que la
    /// biblioteca rellene el campo del comando con el `argv` de Kesto.
    fn instantanea(&self) -> Result<SnapshotFile, Error> {
        SnapshotOptions::default()
            .label(self.etiqueta.clone())
            .host(self.host.clone())
            .command(self.comando.clone())
            .to_snapshot()
            .map_err(|error| Error::CopiaFallida {
                etiqueta: self.etiqueta.clone(),
                motivo: format!(
                    "no se ha podido construir la instantánea del volcado `{}`: {error}",
                    self.nombre
                ),
            })
    }

    /// Las rutas de la copia, comprobadas.
    ///
    /// La biblioteca indexa `backup_paths[0]` sin preguntar, así que una copia sin rutas la
    /// haría entrar en **pánico** dentro de una dependencia. Se rechaza aquí, que es el
    /// único sitio donde todavía es un error con motivo en vez de un hilo de archivado
    /// muerto con la instantánea a medias.
    ///
    /// Prestadas y no copiadas: las rutas solo se leen para archivarlas, y una copia del
    /// vector por cada copia de seguridad es trabajo que no compra nada.
    fn rutas(&self) -> Result<&[PathBuf], Error> {
        if self.rutas.is_empty() {
            return Err(Error::CopiaSinRutas {
                etiqueta: self.etiqueta.clone(),
            });
        }

        Ok(&self.rutas)
    }
}

/// Lo que quedó de una copia.
///
/// **No hay aquí ningún campo que signifique «éxito»**, y es deliberado: la condición de
/// éxito la compone el motor, porque el `Ok` del archivado no la da. Lo que hay son hechos
/// medidos —qué instantánea se guardó y qué contó el archivado como procesado—, y el
/// motor los combina con la señal de la entrega del volcado y con el código de salida del
/// proceso de extracción.
///
/// # Cuál de los dos números sirve para decidir, y cuál **no**
///
/// El motor necesita una tercera comprobación que no sea el `Ok` del archivado ni la señal
/// de la entrega, y de los dos números que hay aquí **sólo uno sirve**:
///
/// - [`bytes`](Self::bytes) **no sirve**: vale `0` tanto en una copia truncada —el
///   archivado descarta la entrada que falló— como en una copia legítimamente vacía, así
///   que no distingue un fallo de un éxito. `0` no es un veredicto.
/// - [`ficheros`](Self::ficheros) sí sirve, y está **medido** en las dos direcciones:
///   archivar un volcado truncado deja `ficheros == 0` —ver
///   `un_volcado_que_no_se_entrega_entero_no_deja_el_volcado_dentro`—, y archivar un
///   volcado de **longitud cero** —un productor que declara el final sin enviar un solo
///   byte— deja `ficheros == 1` con `bytes == 0` y el nodo presente en el árbol, con
///   tamaño declarado cero. Un volcado vacío sigue siendo un nodo: por eso el apoderado
///   sirve, y por eso `bytes` no.
///
/// Lo medido, con un volcado de **longitud cero** archivado por el puente:
///
/// ```text
/// ficheros = 1
/// bytes    = 0
/// nodo     = File, con `size: 0` y contenido vacío (`Some([])`)
/// ```
///
/// Lo miden dos pruebas y en las dos direcciones:
/// `un_volcado_que_no_se_entrega_entero_no_deja_el_volcado_dentro` —el truncado, con
/// `ficheros == 0` y el nodo ausente— y
/// `un_volcado_de_longitud_cero_deja_un_nodo_vacio_y_cuenta_un_fichero` —el vacío, con
/// `ficheros == 1` y el nodo presente—.
///
/// Quien componga la condición de éxito con `ficheros` está midiendo que **el volcado llegó
/// al árbol**; quien la componga con `bytes` está midiendo que llegó algo, que no es lo
/// mismo —una base de datos vacía produce un volcado vacío perfectamente bueno—.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiaHecha {
    /// El identificador de la instantánea, en hex completo.
    ///
    /// Completo y no los ocho caracteres del `Display` de `SnapshotId`: esa forma corta es
    /// un prefijo ambiguo, y con ella dos instantáneas que empiecen igual se confunden.
    /// Este es el valor que se guarda en `job_executions.snapshot_id` y el que se le pasa
    /// al listado.
    pub id: String,
    /// Los ficheros que el archivado contó como procesados.
    ///
    /// Es el apoderado que sirve para decidir si el volcado llegó: `0` significa que el
    /// archivado **no procesó la entrada**, que es lo medido con un volcado truncado; `1`
    /// significa que el nodo del volcado está en la instantánea, aunque esté vacío. Ver el
    /// rustdoc del tipo.
    pub ficheros: u64,
    /// Los bytes que el archivado contó como procesados.
    ///
    /// **No vale para decidir el éxito**, y por eso no hay que leerlo como veredicto: `0` es
    /// también el número de un volcado legítimamente vacío. Sirve para lo que sí dice —el
    /// tamaño de la copia que el panel publica—, que es el único sitio donde ese número
    /// existe: el nodo del volcado declara tamaño cero, porque su longitud no se conoce
    /// hasta que termina.
    pub bytes: u64,
}

impl CopiaHecha {
    /// Lee de una instantánea lo que el motor necesita saber.
    fn desde(snapshot: &SnapshotFile) -> Self {
        let resumen = snapshot.summary.as_ref();

        Self {
            id: snapshot.id.to_hex().as_str().to_owned(),
            // El resumen de una instantánea recién archivada siempre está —lo rellena el
            // archivador—, así que el `0` de aquí es para una instantánea que viniera de
            // otra parte: no se inventa una cifra, se dice que no hay ninguna.
            ficheros: resumen.map_or(0, |resumen| resumen.total_files_processed),
            bytes: resumen.map_or(0, |resumen| resumen.total_bytes_processed),
        }
    }
}

/// Lo que el panel necesita de un snapshot.
///
/// Es la traducción de una instantánea de la biblioteca a algo que se pueda publicar sin
/// arrastrar su formato: ni el árbol, ni los padres, ni el resumen entero de veinte
/// campos. La fecha es un [`OffsetDateTime`] —la misma familia de fechas que usa el resto
/// de Kesto— y no el `Zoned` de la biblioteca, que lleva dentro su zona horaria y su
/// implementación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumenDeSnapshot {
    /// El identificador de la instantánea, en hex completo.
    pub id: String,
    /// La fecha de la copia.
    pub fecha: OffsetDateTime,
    /// Las rutas que la instantánea declara contener.
    pub rutas: Vec<String>,
    /// La etiqueta de la tarea, si la instantánea la lleva.
    pub etiqueta: Option<String>,
    /// Los ficheros que la instantánea procesó.
    pub ficheros: u64,
    /// Los bytes que la instantánea procesó.
    pub bytes: u64,
}

impl ResumenDeSnapshot {
    /// Traduce una instantánea de la biblioteca al resumen del panel.
    ///
    /// La fecha se convierte por el **instante** —nanosegundos desde el origen— y no por
    /// su representación: el `Zoned` de la biblioteca lleva zona horaria, y quedarse con
    /// el texto de su fecha local sería publicar una hora que depende de dónde se ejecutó
    /// la copia.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::ListadoFallido`] si el instante no entra en el rango de fechas que
    /// `time` puede representar. Es un caso que no debería darse —fuera de él no hay
    /// fechas de verdad— pero que se puede dar con un repositorio manipulado, y publicar
    /// una fecha inventada para ese caso sería mentir en el único campo que dice cuándo se
    /// copió.
    fn desde(snapshot: &SnapshotFile, destino: &str) -> Result<Self, Error> {
        let instante = snapshot.time.timestamp().as_nanosecond();
        let fecha = OffsetDateTime::from_unix_timestamp_nanos(instante).map_err(|error| {
            Error::ListadoFallido {
                destino: destino.to_owned(),
                motivo: format!(
                    "la fecha de la instantánea `{}` no es representable: {error}",
                    snapshot.id.to_hex().as_str()
                ),
            }
        })?;

        Ok(Self {
            id: snapshot.id.to_hex().as_str().to_owned(),
            fecha,
            rutas: snapshot.paths.iter().cloned().collect(),
            etiqueta: (!snapshot.label.is_empty()).then(|| snapshot.label.clone()),
            ficheros: snapshot
                .summary
                .as_ref()
                .map_or(0, |resumen| resumen.total_files_processed),
            bytes: snapshot
                .summary
                .as_ref()
                .map_or(0, |resumen| resumen.total_bytes_processed),
        })
    }
}

/// Un repositorio de copias, abierto y listo para archivar.
///
/// Se construye con [`poner_en_marcha`](Repositorio::poner_en_marcha) y **se usa una vez
/// por ejecución**: ver el encabezado del módulo, donde está medido por qué reutilizarlo
/// desactiva la deduplicación.
pub struct Repositorio {
    /// El repositorio de la biblioteca, ya abierto y con su índice en memoria.
    repo: Repository<IndexedIdsStatus>,
    /// El nombre del destino, para poder nombrarlo en el motivo de un fallo.
    ///
    /// Se guarda la cadena y **no** el [`Acceso`] del que salió: guardar el acceso dejaría
    /// los dos secretos vivos en memoria todo lo que dure la copia, y lo único que hace
    /// falta después de poner el repositorio en marcha es saber a quién culpar en un motivo.
    destino: String,
}

impl std::fmt::Debug for Repositorio {
    /// Publica el destino y nada del interior de la biblioteca.
    ///
    /// El `Debug` se escribe a mano porque el derivado imprimiría el de `Repository`, que
    /// lleva dentro las opciones de acceso al repositorio y su caché: nada de eso dice algo
    /// de un fallo de la copia, y en un diagnóstico lo que identifica a este objeto es
    /// **dónde** está el repositorio.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repositorio")
            .field("destino", &self.destino)
            .finish_non_exhaustive()
    }
}

impl Repositorio {
    /// Pone el repositorio en marcha: **conecta si existe, inicializa si no**, y dice cuál
    /// de los dos desenlaces ocurrió.
    ///
    /// Construye el destino de S3 con los datos del [`Acceso`] y delega en
    /// [`poner_en_marcha_con`], que es donde viven los tres desenlaces.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::AccesoInvalido`] si la fila no describe un destino utilizable,
    /// [`Error::DestinoNoAlcanzable`] si el destino no se alcanza o no se puede preparar, y
    /// [`Error::PassphraseNoAbre`] si el repositorio existe y la passphrase no lo abre —lo
    /// que incluye, medido, el caso de un repositorio sin claves utilizables: ver el
    /// rustdoc de esa variante—.
    pub fn poner_en_marcha(
        acceso: &Acceso,
        barras: BarrasDelMotor,
    ) -> Result<(Self, PuestaEnMarcha), Error> {
        let backends = backends_de_s3(acceso)?;

        poner_en_marcha_con(acceso, backends, barras)
    }

    /// Archiva un volcado que llega por el puente.
    ///
    /// Archiva con el emparejamiento con la instantánea anterior **desactivado**
    /// (`parent_opts.force = true`): el nodo de un volcado de flujo no lleva tamaño ni
    /// fechas, así que emparejaría consigo mismo de la copia anterior y la biblioteca
    /// descartaría el lector sin leerlo. Ver el encabezado del módulo para la medición.
    ///
    /// El nombre del volcado ya está validado por
    /// [`tunel_del_volcado`](crate::rustic::tunel_del_volcado): es el `path` del nodo
    /// dentro del árbol y no puede ser un camino.
    ///
    /// # El nombre de la copia y el del origen tienen que coincidir
    ///
    /// Antes de archivar nada se comprueba que [`Copia::nombre`] y
    /// [`OrigenDelVolcado::nombre`] sean la misma cadena, y si no lo son se rechaza la copia
    /// con [`Error::NombreDeVolcadoIncoherente`] **sin escribir nada**. Es la comprobación
    /// que el tipo no impone y que hasta ahora sostenía un comentario en las pruebas
    /// —«coinciden a propósito»—: la instantánea declara las rutas que le da la [`Copia`],
    /// y el nodo que acaba dentro del árbol lo nombra el puente, así que dos nombres
    /// distintos dejan una copia que dice contener lo que no tiene. Se comprueba antes de
    /// `archive` y no después, porque después ya está guardada.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::CopiaSinRutas`] si la copia no declara ninguna ruta —la
    /// biblioteca indexaría su primera ruta sin preguntar—,
    /// [`Error::NombreDeVolcadoIncoherente`] si el nombre de la copia y el del origen no
    /// coinciden, y [`Error::CopiaFallida`] si el archivado falla. **Que no devuelva error
    /// no significa que la copia esté completa**: ver el encabezado del módulo.
    pub fn archivar_volcado(
        &self,
        copia: &Copia,
        origen: &OrigenDelVolcado,
    ) -> Result<CopiaHecha, Error> {
        let rutas = copia.rutas()?;

        // El nombre que la instantánea publica y el del nodo que se archiva tienen que ser
        // el mismo, y eso no lo impone ningún tipo: la copia lleva el suyo y el puente
        // nombra el suyo. Se comprueba aquí, que es el último punto en que discrepancia es
        // un error con motivo en vez de una instantánea mintiendo sobre su contenido.
        if copia.nombre != origen.nombre() {
            return Err(Error::NombreDeVolcadoIncoherente {
                en_la_copia: copia.nombre.clone(),
                en_el_origen: origen.nombre().to_owned(),
            });
        }

        let instantanea = copia.instantanea()?;
        let opciones = BackupOptions::default().parent_opts(ParentOptions::default().force(true));

        let guardada = self
            .repo
            .archive(&opciones, origen, instantanea, rutas)
            .map_err(|error| Error::CopiaFallida {
                etiqueta: copia.etiqueta.clone(),
                motivo: error.display_log(),
            })?;

        Ok(CopiaHecha::desde(&guardada))
    }

    /// Archiva un directorio leído del sistema de ficheros.
    ///
    /// Es para un target de tipo `directory`, que **no pasa por Docker**: el §4 del SRS
    /// solo fija los comandos de las dos bases, así que este camino es un añadido
    /// declarado, y vive aquí —y no en el motor— para que el motor no tenga que importar
    /// la biblioteca de copia ni construir su fuente local.
    ///
    /// No lleva `force`: un directorio sí tiene metadatos de verdad, así que la biblioteca
    /// puede decidir por ellos qué ficheros cambiaron, que es lo que hace útil la
    /// deduplicación entre copias del mismo directorio.
    ///
    /// # Errors
    ///
    /// Las mismas dos que [`archivar_volcado`](Self::archivar_volcado), más el fallo de
    /// construcción de la fuente local, que se reporta con el mismo [`Error::CopiaFallida`].
    pub fn archivar_directorio(&self, copia: &Copia) -> Result<CopiaHecha, Error> {
        let rutas = copia.rutas()?;
        let instantanea = copia.instantanea()?;
        let fallo = |motivo: String| Error::CopiaFallida {
            etiqueta: copia.etiqueta.clone(),
            motivo,
        };

        let origen = LocalSource::new(
            LocalSourceSaveOptions::default(),
            &Excludes::default(),
            &LocalSourceFilterOptions::default(),
            rutas,
        )
        .map_err(|error| fallo(format!("no se ha podido leer el directorio: {error}")))?;

        let guardada = self
            .repo
            .archive(&BackupOptions::default(), &origen, instantanea, rutas)
            .map_err(|error| fallo(error.display_log()))?;

        Ok(CopiaHecha::desde(&guardada))
    }

    /// Aplica la retención: decidir, retirar y purgar.
    ///
    /// Las tres operaciones viven en [`crate::rustic::retencion`], que es donde está
    /// escrito por qué son tres y no una.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::RetencionQueLoBorraTodo`] si las tres reglas valen cero —no es una
    /// retención que conserva de menos, es el historial entero del repositorio— y
    /// [`Error::RetencionFallida`] si falla cualquiera de las tres operaciones. **Una
    /// retención que falla no cambia el resultado de la copia**: la copia ya está hecha y
    /// es correcta, y lo que hay que reportar es que la copia está y la retención no.
    pub fn aplicar_retencion(&self, retencion: &Retencion) -> Result<InformeDeRetencion, Error> {
        super::retencion::aplicar(&self.repo, retencion)
    }

    /// Lista las instantáneas del repositorio, como mucho `ventana`.
    ///
    /// La lista sale **del repositorio** y no del histórico de ejecuciones: la base sabe
    /// qué copias se intentaron y el repositorio sabe qué copias están, y la diferencia es
    /// justamente lo que hace útil al explorador —una instantánea que no está en el
    /// histórico es una copia hecha con otra herramienta, y una ejecución sin instantánea
    /// es una copia que se dio por buena y no está—.
    ///
    /// La ventana se aplica a los **identificadores** antes de resolverlos, que es lo que
    /// acota de verdad el trabajo: resolver es una lectura por instantánea, y con la
    /// ventana detrás el listado de un repositorio grande sería el listado entero. El
    /// precio, que hay que saber, es que el orden en que el backend devuelve esos
    /// identificadores es suyo: mientras el repositorio tenga menos instantáneas que la
    /// ventana salen todas y el listado es el de verdad, pero con más, lo que sale es un
    /// subconjunto y **no** está garantizado que sean las más recientes. No se corrige aquí
    /// porque la única forma de quedarse con las más recientes es resolverlas todas, y eso
    /// es justo lo que la ventana y el requisito que la pide existen para no hacer.
    ///
    /// Lo que sí se promete es que lo devuelto sale ordenado de la más reciente a la más
    /// antigua.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::ListadoFallido`] si el repositorio no se puede listar, si alguna
    /// instantánea no se puede leer o si su fecha no es representable.
    pub fn listar_snapshots(&self, ventana: usize) -> Result<Vec<ResumenDeSnapshot>, Error> {
        let fallo = |motivo: String| Error::ListadoFallido {
            destino: self.destino.clone(),
            motivo,
        };

        // El identificador se pasa en **hex completo**: el `Display` de `SnapshotId` da
        // solo sus ocho primeros caracteres, que es un prefijo ambiguo.
        let identificadores: Vec<String> = self
            .repo
            .list::<SnapshotId>()
            .map_err(|error| fallo(error.display_log()))?
            .take(ventana)
            .map(|id| id.to_hex().as_str().to_owned())
            .collect();

        if identificadores.is_empty() {
            return Ok(Vec::new());
        }

        let mut instantaneas = self
            .repo
            .get_snapshots(&identificadores)
            .map_err(|error| fallo(error.display_log()))?;
        instantaneas.sort_unstable_by(|una, otra| otra.time.cmp(&una.time));

        instantaneas
            .iter()
            .map(|instantanea| ResumenDeSnapshot::desde(instantanea, &self.destino))
            .collect()
    }
}

/// Construye el destino de S3 a partir de la fila.
///
/// Las claves de las opciones son las de `opendal` —`endpoint`, `region`, `bucket`,
/// `access_key_id`, `secret_access_key`—, y la del esquema es la del **esquema**, no la de
/// la URL del backend. Ver el encabezado del módulo para la medición de `"opendal:s3"`.
///
/// # Errors
///
/// Devuelve [`Error::AccesoInvalido`] si la fila no describe un destino utilizable y
/// [`Error::DestinoNoAlcanzable`] si el destino no se puede construir con esas opciones.
fn backends_de_s3(acceso: &Acceso) -> Result<RepositoryBackends, Error> {
    let opciones = opciones_de_s3(acceso)?;

    let backend: Arc<dyn WriteBackend> = Arc::new(
        OpenDALBackend::new(ESQUEMA_DE_S3, opciones).map_err(|error| {
            Error::DestinoNoAlcanzable {
                destino: acceso.nombre_del_destino(),
                motivo: error.display_log(),
            }
        })?,
    );

    Ok(RepositoryBackends::new(backend, None))
}

/// Las opciones de `opendal` que describen el destino, y la validación de la fila.
///
/// Está separada de [`backends_de_s3`] porque es lo único de ese camino que se puede
/// inspeccionar sin red: construir un operador de S3 no la necesita, pero **comprobar que
/// los valores de la fila llegan al mapa sí es una prueba** y no hay otra forma de verlo
/// desde fuera —`opendal` no publica las opciones con las que construyó su operador—. Así
/// que esta función es el contrato que se prueba: la fila entra, y lo que sale es lo que
/// viajará hasta la dependencia, incluida la clave secreta.
///
/// La validación de la fila va aquí y no en [`backends_de_s3`] a propósito: lo que no puede
/// ser un destino no llega a convertirse en opciones.
///
/// # Errors
///
/// Devuelve [`Error::AccesoInvalido`] nombrando el primer campo vacío de la fila.
fn opciones_de_s3(acceso: &Acceso) -> Result<BTreeMap<String, String>, Error> {
    acceso.validar()?;

    Ok([
        ("endpoint", acceso.endpoint.clone()),
        ("region", acceso.region.clone()),
        ("bucket", acceso.bucket.clone()),
        ("access_key_id", acceso.access_key_id.clone()),
        (
            "secret_access_key",
            acceso.secret_access_key.expose().to_owned(),
        ),
    ]
    .into_iter()
    .map(|(clave, valor)| (clave.to_owned(), valor))
    .collect())
}

/// El esquema de `opendal` que sirve a S3.
///
/// Ver el encabezado del módulo: **no** es `"opendal:s3"` —eso le pide a `opendal` un
/// servicio llamado «opendal», que no existe— y no es configurable, porque es un dato del
/// transporte y no del destino: el destino lo describen el endpoint, el cubo y las
/// credenciales.
const ESQUEMA_DE_S3: &str = "s3";

/// Pone el repositorio en marcha sobre unos backends **ya construidos**.
///
/// Existe separada de [`Repositorio::poner_en_marcha`] para que las pruebas puedan
/// entregar un backend **local**: un directorio del sistema de ficheros en vez de un cubo
/// de S3. La API pública construye siempre el de S3 y no ofrece una variante local, porque
/// el §7 del SRS describe S3 y un destino que nadie pidió no se añade.
///
/// Los tres desenlaces se deciden **preguntándole al destino**:
///
/// 1. se pide la lista de ficheros de configuración. Si esa pregunta falla, el destino no
///    se alcanza —es lo que devuelve S3 cuando el cubo no está o la credencial no vale—;
/// 2. si la lista está **vacía**, no hay repositorio todavía: se **inicializa** y el
///    desenlace es [`PuestaEnMarcha::Inicializado`];
/// 3. si tiene ficheros, hay repositorio: se **abre** y el desenlace es
///    [`PuestaEnMarcha::Conectado`]. Si el abrir falla y el fallo es el de una passphrase
///    que no abre, se reporta como tal; cualquier otro fallo al abrir es un fallo del
///    destino, porque se acaba de listar y por tanto se alcanza. Ver
///    [`Error::PassphraseNoAbre`] para lo que ese desenlace significa de verdad, que no es
///    exactamente «la passphrase es la equivocada».
///
/// # Visibilidad
///
/// **Privada**, y no `pub(crate)` como decía antes su documentación: sus únicos
/// consumidores son las pruebas de este mismo fichero, que son un módulo hijo y ven lo
/// privado. Lo que sí necesitan las pruebas de [`crate::rustic::retencion`] son los
/// ayudantes del arnés —`abrir_local` y compañía, que solo existen con `cfg(test)`—, y ésos
/// no pasan por aquí: aquella prueba mide las reglas sobre un repositorio que abre por su
/// cuenta. Tampoco es `pub`: el destino local que recibe es un gancho de prueba, no un modo
/// de funcionamiento.
///
/// # Errors
///
/// Las tres del desenlace y las de la construcción del propio repositorio. Ver
/// [`Repositorio::poner_en_marcha`].
fn poner_en_marcha_con(
    acceso: &Acceso,
    backends: RepositoryBackends,
    barras: BarrasDelMotor,
) -> Result<(Repositorio, PuestaEnMarcha), Error> {
    acceso.validar()?;

    let destino = acceso.nombre_del_destino();
    let fallo_del_destino = |error: Box<rustic_core::RusticError>| Error::DestinoNoAlcanzable {
        destino: destino.clone(),
        motivo: error.display_log(),
    };

    // La copia del backend **antes** de entregárselo al repositorio: es lo único que queda
    // para preguntarle al destino, porque `RepositoryBackends` se consume.
    let configuracion = backends.repository();
    let repositorio =
        Repository::new_with_progress(&RepositoryOptions::default(), &backends, barras)
            .map_err(fallo_del_destino)?;

    let credenciales = Credentials::Password(acceso.passphrase.expose().to_owned());
    let sin_configuracion = configuracion
        .list(FileType::Config)
        .map_err(fallo_del_destino)?
        .is_empty();

    let (abierto, puesta) = if sin_configuracion {
        (
            repositorio
                .init(
                    &credenciales,
                    &KeyOptions::default(),
                    &ConfigOptions::default(),
                )
                .map_err(fallo_del_destino)?,
            PuestaEnMarcha::Inicializado,
        )
    } else {
        (
            repositorio.open(&credenciales).map_err(|error| {
                // Se acaba de listar el destino, así que se alcanza: si el abrir falla, lo
                // que falla es la apertura del repositorio. Se comprueba con el código que
                // la biblioteca publica para esa pregunta —el mismo que usa ella— y no por
                // el texto del mensaje, que puede cambiar en cualquier versión menor.
                //
                // Lo que ese código **no** distingue, y está medido: C002 sale también
                // cuando el repositorio tiene configuración y **ninguna clave utilizable**
                // —los ficheros de `keys/` no están, o están corruptos—. La biblioteca
                // recorre sus claves saltando las que no puede leer y, si no le queda
                // ninguna que acepte la contraseña, emite el mismo «no suitable key found»
                // que con una contraseña equivocada. Así que la variante que se devuelve
                // dice lo que se puede afirmar —no hay ninguna clave que abra con esta
                // passphrase—, y no «la passphrase es la equivocada». Ver
                // [`Error::PassphraseNoAbre`].
                if error.is_incorrect_password() {
                    Error::PassphraseNoAbre {
                        destino: destino.clone(),
                        motivo: error.display_log(),
                    }
                } else {
                    Error::DestinoNoAlcanzable {
                        destino: destino.clone(),
                        motivo: error.display_log(),
                    }
                }
            })?,
            PuestaEnMarcha::Conectado,
        )
    };

    let repo = abierto.to_indexed_ids().map_err(fallo_del_destino)?;

    Ok((Repositorio { repo, destino }, puesta))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use rustic_backend::opendal::OpenDALBackend;
    use rustic_core::repofile::SnapshotFile;
    use rustic_core::{FileType, ReadBackend, SnapshotOptions};

    use crate::rustic::tunel_del_volcado;
    use crate::secret::SecretString;
    use crate::test_support::{
        AJENA_CONTENIDO, CLAVE_SECRETA_DE_PRUEBA, DirectorioDePrueba, ETIQUETA_AJENA,
        ETIQUETA_SEMBRADA, OTRA_PASSPHRASE_DE_PRUEBA, PASSPHRASE_DE_PRUEBA, SEMILLA_CONTENIDO,
        acceso_local, backends_locales, barras_mudas, directorio_de_prueba, nodo_del_snapshot,
        sembrar_snapshot_ajeno, sembrar_snapshots, snapshot_guardado, volcado_guardado,
    };

    use super::{
        Acceso, Copia, CopiaHecha, ESQUEMA_DE_S3, Error, PuestaEnMarcha, Repositorio,
        backends_de_s3, opciones_de_s3, poner_en_marcha_con,
    };

    /// El plazo de inactividad de los túneles de prueba.
    ///
    /// Un minuto: tan por encima de lo que tardan que si alguna prueba lo tocara sería que
    /// el puente está atascado, no que va despacio.
    const PLAZO_DE_PRUEBA: Duration = Duration::from_secs(60);

    /// La etiqueta con la que se identifican las copias de estas pruebas.
    const ETIQUETA: &str = "tarea-de-prueba";

    /// El comando del volcado de las pruebas: distinguible y de verdad.
    const COMANDO: &str =
        "mariadb-dump --single-transaction --quick --lock-tables=false -u tienda --all-databases";

    /// El destino de una prueba: un directorio temporal y el repositorio que vive dentro.
    ///
    /// El nombre del directorio lleva un UUID, así que dos pruebas que corran a la vez no
    /// se pisan, y la guarda lo borra al soltarse: si una prueba dejara el destino como
    /// estaba, el que lo nota es el sistema de ficheros de quien corre la suite.
    struct Destino {
        /// La guarda del directorio temporal. Al soltarse se borra, con el repositorio dentro.
        _directorio: DirectorioDePrueba,
        /// La ruta del repositorio. Es también el nombre con el que los motivos de error
        /// tienen que nombrar el destino: [`acceso_local`] la pone en el `endpoint`.
        ruta: PathBuf,
    }

    impl Destino {
        /// Un destino recién creado y **vacío**: no tiene ningún repositorio todavía.
        fn nuevo() -> Self {
            let directorio = directorio_de_prueba();
            let ruta = directorio.ruta().join("repositorio");

            Self {
                _directorio: directorio,
                ruta,
            }
        }

        /// El acceso de prueba al destino.
        fn acceso(&self, passphrase: &str) -> Acceso {
            acceso_local(&self.ruta, passphrase)
        }

        /// Un juego **nuevo** de backends contra el mismo destino.
        ///
        /// Nuevo cada vez a propósito: el motor abre el repositorio por ejecución, así que
        /// reutilizar los backends de la puesta en marcha anterior probaría un flujo que no
        /// es el de producción.
        fn backends(&self) -> rustic_core::RepositoryBackends {
            backends_locales(&self.ruta)
        }

        /// Pone el repositorio en marcha por el camino de las pruebas.
        fn en_marcha(&self, passphrase: &str) -> Result<(Repositorio, PuestaEnMarcha), Error> {
            poner_en_marcha_con(&self.acceso(passphrase), self.backends(), barras_mudas())
        }

        /// ¿Tiene ya el destino un fichero de configuración?
        ///
        /// Es **la misma pregunta** que el módulo le hace al destino para decidir los tres
        /// desenlaces, y es a propósito: si la prueba la respondiera mirando el directorio
        /// a mano —buscando un `config` en su raíz— comprobaría la disposición interna de
        /// un backend concreto en vez de lo que el módulo mide.
        fn tiene_repositorio(&self) -> bool {
            !self
                .backends()
                .repository()
                .list(FileType::Config)
                .expect("listar un destino local no puede fallar")
                .is_empty()
        }

        /// La foto del destino: cada ruta relativa con su tamaño, ordenada.
        ///
        /// Es lo que permite afirmar que un intento fallido **no tocó** el destino, y no
        /// solo que siga existiendo: mira cada fichero y cada byte. Ordenada porque el
        /// orden en que el sistema de ficheros devuelve las entradas no es una propiedad
        /// que se esté afirmando.
        fn instantanea(&self) -> Vec<(String, u64)> {
            /// Recorre `actual` —que cuelga de `raiz`— acumulando lo que encuentra.
            fn recorrer(raiz: &Path, actual: &Path, vistos: &mut Vec<(String, u64)>) {
                for entrada in std::fs::read_dir(actual).expect("el destino se puede recorrer") {
                    let entrada = entrada.expect("cada entrada del destino se lee");
                    let ruta = entrada.path();

                    if ruta.is_dir() {
                        recorrer(raiz, &ruta, vistos);
                    } else {
                        let relativa = ruta
                            .strip_prefix(raiz)
                            .expect("todo lo recorrido cuelga de la raíz")
                            .to_string_lossy()
                            .into_owned();
                        let tamano = entrada
                            .metadata()
                            .expect("cada entrada del destino tiene metadatos")
                            .len();

                        vistos.push((relativa, tamano));
                    }
                }
            }

            let mut vistos = Vec::new();
            if self.ruta.exists() {
                recorrer(&self.ruta, &self.ruta, &mut vistos);
            }
            vistos.sort();

            vistos
        }
    }

    /// Una copia de prueba del destino que se le diga.
    ///
    /// El nombre y las rutas coinciden a propósito: el nombre es el `path` del nodo que el
    /// puente fabrica dentro del árbol y las rutas son lo que la instantánea declara, así
    /// que lo declarado y lo contenido quedan de acuerdo.
    fn copia_de(nombre: &str, etiqueta: &str) -> Copia {
        Copia {
            rutas: vec![PathBuf::from(nombre)],
            nombre: nombre.to_owned(),
            etiqueta: etiqueta.to_owned(),
            host: "host-de-prueba".to_owned(),
            comando: COMANDO.to_owned(),
        }
    }

    /// Archiva un volcado con el contenido dado, con el productor **en su propio hilo**.
    ///
    /// El orden es el del puente y no se puede cambiar: el productor en su hilo, el
    /// archivado en éste, y la unión —más la consulta de la entrega— al final. Un
    /// productor que corriera en el hilo del archivado se bloquearía en su primer `enviar`
    /// en cuanto el canal se llenara, esperando a un consumidor que estaría esperándole.
    fn archivar(
        repositorio: &Repositorio,
        copia: &Copia,
        contenido: &[u8],
    ) -> Result<CopiaHecha, Error> {
        let tunel = tunel_del_volcado(copia.nombre.clone(), 4, PLAZO_DE_PRUEBA)
            .expect("el nombre del volcado de la prueba es válido");
        let trozos = vec![contenido.to_vec()];
        let productor = tunel.productor;

        let emisor = std::thread::spawn(move || {
            for trozo in trozos {
                productor
                    .enviar(trozo)
                    .expect("el archivado sigue leyendo, así que el envío no puede fallar");
            }
            productor.terminar();
        });

        let hecho = repositorio.archivar_volcado(copia, &tunel.origen);
        emisor.join().expect("el hilo productor no puede caerse");

        assert!(
            tunel.entrega.completa(),
            "el volcado se entregó entero y la señal de la entrega sigue en falso: sin ella \
             la copia no se podría dar por buena"
        );

        hecho
    }

    /// Archiva un volcado **truncado**: el productor manda un trozo y muere sin declarar
    /// el final.
    ///
    /// Es el desenlace que el módulo tiene que saber reportar sin que nadie lo confunda
    /// con un éxito: el archivo existe, ocupa sitio y no se puede restaurar.
    fn archivar_truncado(
        repositorio: &Repositorio,
        copia: &Copia,
        trozo: &[u8],
    ) -> Result<CopiaHecha, Error> {
        let tunel = tunel_del_volcado(copia.nombre.clone(), 4, PLAZO_DE_PRUEBA)
            .expect("el nombre del volcado de la prueba es válido");
        let productor = tunel.productor;

        productor
            .enviar(trozo.to_vec())
            .expect("el canal tiene sitio de sobra");

        // Muere sin llamar a `terminar`: es el caso de un `exec` que se cayó a medias.
        drop(productor);

        let hecho = repositorio.archivar_volcado(copia, &tunel.origen);

        assert!(
            !tunel.entrega.completa(),
            "un productor que murió sin declarar el final dejó la señal de la entrega puesta"
        );

        hecho
    }

    /// **1.** Un destino vacío se **inicializa** y queda con su fichero de configuración.
    ///
    /// Es el primer desenlace de los tres, y el que deja el destino utilizable: si la
    /// puesta en marcha no escribiera la configuración, la copia siguiente no tendría dónde
    /// archivar y el fallo aparecería lejos de su causa.
    #[test]
    fn un_destino_vacio_se_inicializa_y_queda_con_su_configuracion() {
        let destino = Destino::nuevo();

        assert!(
            !destino.tiene_repositorio(),
            "el destino de la prueba no empieza vacío"
        );

        let (repositorio, puesta) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("un destino vacío y alcanzable se inicializa");

        assert_eq!(
            puesta,
            PuestaEnMarcha::Inicializado,
            "un destino sin repositorio se inicializa, y dijo otra cosa"
        );
        assert!(
            destino.tiene_repositorio(),
            "el destino se inicializó y no quedó ningún fichero de configuración en {}",
            destino.ruta.display()
        );

        drop(repositorio);
    }

    /// **2.** Un destino que ya tiene repositorio se **abre**, sin fallar por «ya existe».
    ///
    /// Es el caso que un `init()` a ciegas rompería: el motor abre el repositorio **por
    /// ejecución**, así que la segunda copia sobre el mismo destino es un repositorio nuevo
    /// contra un destino que ya está inicializado, y lo que tiene que ocurrir es una
    /// conexión —y no un error que diga que el fichero de configuración ya está ahí—.
    #[test]
    fn un_destino_con_repositorio_se_conecta_sin_fallar_por_ya_existe() {
        let destino = Destino::nuevo();

        let (primero, puesta) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el primer destino se inicializa");
        assert_eq!(
            puesta,
            PuestaEnMarcha::Inicializado,
            "la primera puesta en marcha tenía que inicializar"
        );
        drop(primero);

        let (segundo, puesta) = destino.en_marcha(PASSPHRASE_DE_PRUEBA).expect(
            "un destino que ya tiene repositorio tiene que abrirse, y no fallar por existir ya",
        );

        assert_eq!(
            puesta,
            PuestaEnMarcha::Conectado,
            "la segunda puesta en marcha tenía que conectar con el repositorio que ya estaba"
        );

        drop(segundo);
    }

    /// **3.** Una passphrase equivocada **no** se confunde con un repositorio inexistente,
    /// y deja el destino sin tocar.
    ///
    /// Son las dos mitades del escenario: el desenlace tiene que ser el de «existe y no
    /// abre» —un motivo accionable: revisa la passphrase— y no el de «no existe», que
    /// mandaría al operador a mirar un cubo que está perfectamente. Y el destino no puede
    /// quedar tocado: un intento de apertura fallido que escribiera algo dejaría el
    /// repositorio en un estado que nadie ha pedido.
    #[test]
    fn una_passphrase_equivocada_no_se_confunde_con_un_repositorio_inexistente() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        drop(repositorio);

        let antes = destino.instantanea();
        assert!(
            !antes.is_empty(),
            "el destino se inicializó y su foto está vacía: no habría nada que comparar"
        );

        let error = destino
            .en_marcha(OTRA_PASSPHRASE_DE_PRUEBA)
            .expect_err("con el repositorio ya creado, otra passphrase no puede abrirlo");

        let Error::PassphraseNoAbre {
            destino: nombrado,
            motivo,
        } = &error
        else {
            panic!(
                "una passphrase que no abre tiene que reportarse como tal, y se reportó como \
                 otra cosa: {error}"
            );
        };

        assert!(
            nombrado.contains(&destino.ruta.display().to_string()),
            "el motivo del fallo no nombra el destino ({}): {nombrado}",
            destino.ruta.display()
        );
        assert!(
            !motivo.is_empty(),
            "el motivo del fallo de la passphrase está vacío"
        );
        assert!(
            !error.to_string().contains(OTRA_PASSPHRASE_DE_PRUEBA)
                && !error.to_string().contains(PASSPHRASE_DE_PRUEBA),
            "el error publicó una passphrase: {error}"
        );

        assert_eq!(
            destino.instantanea(),
            antes,
            "un intento de apertura con la passphrase equivocada tocó el destino"
        );
    }

    /// **4.** Un destino **inalcanzable** da un error distinto de los dos anteriores, y el
    /// motivo **nombra el destino**.
    ///
    /// Medido, y hay que decirlo porque cambia lo que esta prueba mide: con un destino
    /// local, `list(FileType::Config)` **nunca** falla —devuelve la lista vacía lo mismo
    /// sobre una ruta que no existe que sobre una que es un fichero—, así que el desenlace
    /// «el destino no se alcanza» no se puede provocar por el listado. Lo que sí se puede
    /// provocar es el otro extremo del mismo caso: un destino que **no se puede preparar**,
    /// que es la misma familia —no se puede hablar con él— y el mismo error. El camino del
    /// listado es el que devuelve S3 cuando el cubo no está o la credencial no vale, y
    /// necesita red, así que queda declarado aquí en vez de simulado.
    #[test]
    fn un_destino_inalcanzable_da_un_error_distinto_y_nombra_el_destino() {
        let directorio = directorio_de_prueba();
        let obstaculo = directorio.ruta().join("esto-no-es-un-directorio");
        std::fs::write(&obstaculo, b"soy un fichero, no un directorio").expect("el obstáculo");
        let ruta = obstaculo.join("repositorio");

        let error = poner_en_marcha_con(
            &acceso_local(&ruta, PASSPHRASE_DE_PRUEBA),
            backends_locales(&ruta),
            barras_mudas(),
        )
        .expect_err("un destino que no se puede preparar no puede ponerse en marcha");

        let Error::DestinoNoAlcanzable {
            destino: nombrado,
            motivo,
        } = &error
        else {
            panic!(
                "un destino que no se alcanza tiene que reportarse como un fallo de destino, y \
                 se reportó como otra cosa: {error}"
            );
        };

        assert!(
            nombrado.contains(&ruta.display().to_string()),
            "el motivo del fallo no nombra el destino ({}): {nombrado}",
            ruta.display()
        );
        assert!(
            !motivo.is_empty(),
            "el motivo del destino inalcanzable está vacío"
        );

        // Y el error **no** es ninguno de los otros dos desenlaces: ni una passphrase que
        // no abre, ni un nombre de volcado mal formado.
        assert!(
            !matches!(
                error,
                Error::PassphraseNoAbre { .. } | Error::NombreDeVolcadoInvalido { .. }
            ),
            "el destino inalcanzable se confundió con otro fallo: {error}"
        );
    }

    /// **5.** El módulo se pone en marcha con los secretos **ya descifrados** y **no**
    /// recibe la clave maestra del proceso.
    ///
    /// Esta prueba es **verde por compilación**, y es lo único que puede ser: que una
    /// función no reciba un argumento no se mide en tiempo de ejecución, se mide en el
    /// tipo. Lo que fija es que la única forma de poner el repositorio en marcha es
    /// entregarle un [`Acceso`] con la clave secreta y la passphrase **en claro** y ningún
    /// otro secreto: no hay hueco para la clave maestra, así que el módulo no puede
    /// descifrar nada aunque quisiera.
    ///
    /// El otro camino por el que un secreto sale —el `Debug`— sí se mide aquí: el acceso
    /// lleva dos [`SecretString`](crate::secret::SecretString) precisamente para que ni su
    /// `Debug` ni su `Display` publiquen el valor, y se comprueba con el control de que la
    /// marca de redacción aparece, que es lo que impide que las dos aserciones de ausencia
    /// pasen por vacío.
    #[test]
    fn el_modulo_se_pone_en_marcha_sin_recibir_la_clave_maestra() {
        let destino = Destino::nuevo();
        let acceso = destino.acceso(PASSPHRASE_DE_PRUEBA);

        let (repositorio, puesta) =
            poner_en_marcha_con(&acceso, destino.backends(), barras_mudas())
                .expect("con los dos secretos ya descifrados, el repositorio se pone en marcha");

        assert_eq!(
            puesta,
            PuestaEnMarcha::Inicializado,
            "la puesta en marcha de un destino vacío se inicializa"
        );

        let publicado = format!("{acceso:?}");
        assert!(
            !publicado.contains(PASSPHRASE_DE_PRUEBA),
            "el Debug del acceso publicó la passphrase: {publicado}"
        );
        assert!(
            !publicado.contains(CLAVE_SECRETA_DE_PRUEBA),
            "el Debug del acceso publicó la clave secreta: {publicado}"
        );
        assert!(
            publicado.contains("<redactado>"),
            "el Debug del acceso no redacta nada, así que las dos comprobaciones de arriba \
             estarían pasando sin medir nada: {publicado}"
        );

        drop(repositorio);
    }

    /// **6.** `archivar_volcado` guarda el volcado y dice los ficheros y los bytes que
    /// procesó.
    ///
    /// Lo segundo no es un adorno: `bytes_processed` es lo que el panel publica como
    /// tamaño de la copia, y el único sitio donde ese número existe —el nodo del volcado
    /// declara tamaño cero, porque su longitud no se conoce hasta que termina— es el
    /// resumen de la instantánea.
    #[test]
    fn archivar_volcado_guarda_el_volcado_con_sus_ficheros_y_sus_bytes() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);
        let contenido =
            b"-- tienda\nCREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (1);\n";

        let hecho = archivar(&repositorio, &copia, contenido).expect("archivar no puede fallar");

        assert_eq!(
            hecho.id.len(),
            64,
            "el identificador del snapshot no es el hex completo: {:?}. La forma corta del \
             `Display` de `SnapshotId` son ocho caracteres y es un prefijo ambiguo",
            hecho.id
        );
        assert_eq!(
            hecho.ficheros, 1,
            "el archivado procesó un fichero y dijo {}",
            hecho.ficheros
        );
        assert_eq!(
            hecho.bytes,
            contenido.len() as u64,
            "el archivado procesó {} bytes y dijo {}",
            contenido.len(),
            hecho.bytes
        );
        assert_eq!(
            volcado_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &hecho.id,
                &copia.nombre
            )
            .expect("el snapshot tiene que tener el volcado dentro"),
            contenido,
            "el volcado que se restaura del repositorio no es el que se entregó"
        );

        drop(repositorio);
    }

    /// **7.** ⚠️ Dos copias consecutivas del **mismo destino** llevan **cada una su
    /// contenido**.
    ///
    /// Es la prueba del hallazgo más grave del change: el nodo que el puente fabrica para
    /// un volcado no lleva tamaño ni fechas —no puede llevarlos—, así que sin desactivar
    /// el emparejamiento con la instantánea anterior las dos copias comparan igual, la
    /// segunda empareja, la biblioteca **descarta el lector sin leerlo** y la instantánea
    /// nueva apunta al contenido de la anterior. Medido restaurando: la copia de hoy
    /// devolvía los bytes de ayer.
    ///
    /// Las dos puestas en marcha son **dos repositorios distintos** contra el mismo
    /// destino, que es como los usa el motor: reutilizar el abierto entre las dos copias
    /// desactivaría además la deduplicación por otra razón, y esta prueba quedaría midiendo
    /// dos cosas a la vez.
    #[test]
    fn dos_copias_consecutivas_del_mismo_destino_llevan_cada_una_su_contenido() {
        let destino = Destino::nuevo();
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);
        let de_ayer = b"-- de ayer\nCREATE TABLE tienda (id INT);\n";
        let de_hoy = b"-- de hoy\nCREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (7);\n";

        let (primero, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let ayer = archivar(&primero, &copia, de_ayer).expect("la copia de ayer se archiva");
        drop(primero);

        let (segundo, puesta) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("la segunda copia abre el repositorio que ya está");
        assert_eq!(
            puesta,
            PuestaEnMarcha::Conectado,
            "la segunda copia tenía que conectar con el repositorio"
        );
        let hoy = archivar(&segundo, &copia, de_hoy).expect("la copia de hoy se archiva");
        drop(segundo);

        assert_eq!(
            volcado_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &ayer.id, &copia.nombre)
                .expect("la copia de ayer tiene que tener su volcado"),
            de_ayer,
            "la copia de ayer no devuelve los bytes de ayer"
        );
        assert_eq!(
            volcado_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &hoy.id, &copia.nombre)
                .expect("la copia de hoy tiene que tener su volcado"),
            de_hoy,
            "la copia de hoy devolvió los bytes de ayer: el emparejamiento con la instantánea \
             anterior no está desactivado y la segunda copia apunta al contenido de la primera"
        );
        assert_eq!(
            hoy.bytes,
            de_hoy.len() as u64,
            "la copia de hoy dice haber procesado {} bytes y su contenido tiene {}: una copia \
             que reutiliza el contenido anterior no lee nada",
            hoy.bytes,
            de_hoy.len()
        );
    }

    /// **8.** ⚠️ Un volcado que **no se entrega entero** no deja dentro de la instantánea
    /// nada que el motor pueda confundir con una copia.
    ///
    /// Está medido que el archivado de la biblioteca silencia el error del lector y
    /// devuelve un resultado correcto **guardando la instantánea igualmente**, sin el
    /// volcado dentro. Lo que esta prueba fija es la parte que le toca a este módulo: el
    /// resultado **no lleva el volcado dentro** —el resumen cuenta cero ficheros y el árbol
    /// no tiene el nodo—, así que quien componga la condición de éxito con
    /// [`CopiaHecha::ficheros`] no puede dar por buena una copia que no existe.
    #[test]
    fn un_volcado_que_no_se_entrega_entero_no_deja_el_volcado_dentro() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-truncado.sql", ETIQUETA);

        let devuelto = archivar_truncado(&repositorio, &copia, b"-- tienda\nCREATE TABLE tie");
        let hecho = devuelto
            .expect(
                "medido: el archivado de la biblioteca **no** devuelve error con el volcado \
             truncado, lo descarta con un aviso y guarda la instantánea igualmente. Que esto \
             falle significa que la biblioteca cambió de comportamiento y hay que volver a \
             medir qué se puede afirmar del resultado",
            )
            .clone();

        assert_eq!(
            hecho.ficheros, 0,
            "el archivado dice haber procesado {} ficheros con el volcado truncado: el resumen \
             no puede contar lo que no llegó a leer",
            hecho.ficheros
        );
        assert_eq!(
            volcado_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &hecho.id,
                &copia.nombre
            ),
            None,
            "el volcado truncado quedó dentro de la instantánea: es una copia que existe, ocupa \
             sitio y no se puede restaurar"
        );

        drop(repositorio);
    }

    /// **9.** La instantánea graba **el comando del volcado** y **no** la línea de
    /// comandos de Kesto.
    ///
    /// La biblioteca tiene aquí un defecto medido: si el campo del comando no se informa,
    /// `SnapshotFile::from_options` lo rellena con `std::env::args_os()`, es decir con el
    /// `argv` del proceso —y en un arranque con variables por argumento, con secretos
    /// dentro—. La primera mitad de la prueba **reproduce ese defecto a propósito**, para
    /// que las dos comprobaciones siguientes no puedan pasar por vacío: si algún día la
    /// biblioteca deja de rellenar el campo, esto lo dice. Y el argumento proceso
    /// distinguible que se comprueba que no aparece es el `argv[0]` —el camino del binario
    /// de pruebas, que no se parece a nada del volcado—, más todos los demás.
    #[test]
    fn el_snapshot_graba_el_comando_del_volcado_y_no_el_argv_de_kesto() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);

        let hecho = archivar(&repositorio, &copia, b"-- tienda\nSELECT 1;\n")
            .expect("archivar no puede fallar");

        let argv: Vec<String> = std::env::args_os()
            .map(|argumento| argumento.to_string_lossy().into_owned())
            .collect();
        assert!(
            !argv.is_empty(),
            "un proceso tiene al menos un argumento: sin `argv` esta prueba no mide nada"
        );

        let por_defecto = SnapshotFile::from_options(&SnapshotOptions::default())
            .expect("la instantánea por defecto se construye");
        let comando_por_defecto = por_defecto
            .summary
            .expect("la instantánea por defecto trae resumen")
            .command;
        assert!(
            comando_por_defecto.contains(&argv[0]),
            "la biblioteca dejó de rellenar el comando con el `argv` del proceso \
             ({comando_por_defecto:?}): la comprobación de abajo ya no mediría lo que dice \
             medir, y habría que volver a medir por dónde sale la línea de comandos"
        );

        let guardado = snapshot_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &hecho.id);
        let comando = guardado
            .summary
            .expect("la instantánea guardada trae su resumen")
            .command;

        assert_eq!(
            comando, copia.comando,
            "el comando que la instantánea grabó no es el del volcado"
        );
        for argumento in &argv {
            assert!(
                !comando.contains(argumento.as_str()),
                "el comando de la instantánea lleva un argumento del proceso de Kesto \
                 ({argumento:?}): es un campo de auditoría que, dejado a su defecto, audita lo \
                 que no debe. Comando grabado: {comando:?}"
            );
        }

        drop(repositorio);
    }

    /// **10.** La instantánea lleva **sus rutas**, **su etiqueta** y su host.
    ///
    /// Es la otra mitad de la procedencia: además de no heredar la línea de comandos, la
    /// instantánea tiene que decir qué contiene y de qué tarea salió, porque es lo único
    /// que queda de la copia cuando el histórico de ejecuciones se borre.
    #[test]
    fn el_snapshot_lleva_sus_rutas_su_etiqueta_y_su_host() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);

        let hecho = archivar(&repositorio, &copia, b"-- tienda\nSELECT 1;\n")
            .expect("archivar no puede fallar");
        let guardado = snapshot_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &hecho.id);

        assert_eq!(
            guardado.paths.iter().cloned().collect::<Vec<String>>(),
            copia
                .rutas
                .iter()
                .map(|ruta| ruta.to_string_lossy().into_owned())
                .collect::<Vec<String>>(),
            "las rutas que la instantánea declara no son las de la copia"
        );
        assert_eq!(
            guardado.label, copia.etiqueta,
            "la etiqueta de la instantánea no es la que identifica a la tarea"
        );
        assert_eq!(
            guardado.hostname, copia.host,
            "el host de la instantánea no es el de la copia"
        );

        drop(repositorio);
    }

    /// **11.** `archivar_directorio` copia un directorio del sistema de ficheros, sin pasar
    /// por Docker.
    ///
    /// Es el camino de un target de tipo `directory`, que el §4 del SRS deja abierto: las
    /// dos bases tienen su comando, y un directorio no tiene ninguno. Lo que se fija aquí
    /// es que no hace falta ningún motor de contenedores para copiarlo —lo que hace que
    /// esta prueba corra en el CI— y que lo que se copia es el contenido de las rutas.
    #[test]
    fn archivar_directorio_copia_un_directorio_sin_pasar_por_docker() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");

        let fuente = directorio_de_prueba();
        std::fs::create_dir_all(fuente.ruta().join("subdirectorio")).expect("el subdirectorio");
        std::fs::write(
            fuente.ruta().join("subdirectorio").join("datos.txt"),
            b"datos del directorio\n",
        )
        .expect("el fichero del directorio");

        let copia = Copia {
            rutas: vec![fuente.ruta().to_path_buf()],
            nombre: "directorio".to_owned(),
            etiqueta: ETIQUETA.to_owned(),
            host: "host-de-prueba".to_owned(),
            comando: "copia de directorio".to_owned(),
        };

        let hecho = repositorio
            .archivar_directorio(&copia)
            .expect("copiar un directorio no puede fallar");

        assert!(
            hecho.bytes >= b"datos del directorio\n".len() as u64,
            "la copia del directorio dice haber procesado {} bytes, menos que el único fichero \
             que contiene",
            hecho.bytes
        );
        let ruta = format!("{}/subdirectorio/datos.txt", fuente.ruta().display());
        let relativa = ruta.trim_start_matches('/');
        assert_eq!(
            volcado_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &hecho.id, relativa),
            Some(b"datos del directorio\n".to_vec()),
            "el fichero del directorio no se puede leer del snapshot (nodo {relativa:?})"
        );

        drop(repositorio);
    }

    /// **12.** La retención del repositorio retira lo que sobra, purga y deja lo que
    /// conserva, a través de la API pública del repositorio.
    ///
    /// Es la mitad de arriba de la retención —la que ve el motor—: que
    /// [`Repositorio::aplicar_retencion`] lleve a las tres operaciones y que el que sobre
    /// siga estando **en el repositorio**, comprobado volviendo a listarlo. Las reglas, en
    /// detalle, se prueban en [`crate::rustic::retencion`].
    #[test]
    fn la_retencion_del_repositorio_retira_los_que_sobran_y_purga() {
        let destino = Destino::nuevo();
        let siembra = sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );
        let sembrados = &siembra.identificadores;

        let (repositorio, puesta) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino sembrado se abre");
        assert_eq!(
            puesta,
            PuestaEnMarcha::Conectado,
            "el destino ya tenía repositorio"
        );

        let informe = repositorio
            .aplicar_retencion(&crate::rustic::Retencion {
                keep_daily: 1,
                keep_weekly: 0,
                keep_monthly: 0,
            })
            .expect("aplicar la retención no puede fallar");

        let mut retirados = informe.retirados.clone();
        retirados.sort();
        let mut sobrantes = vec![sembrados[0].clone(), sembrados[1].clone()];
        sobrantes.sort();
        assert_eq!(
            retirados, sobrantes,
            "la retención tenía que retirar los dos más antiguos"
        );
        assert!(
            informe.purgado,
            "la retención retiró instantáneas y no purgó lo que quedó huérfano"
        );

        let quedan = repositorio
            .listar_snapshots(100)
            .expect("el repositorio se puede listar");
        assert_eq!(
            quedan.len(),
            1,
            "tenía que quedar una instantánea y quedan {}",
            quedan.len()
        );
        assert_eq!(
            quedan[0].id, sembrados[2],
            "la instantánea que quedó no es la más reciente"
        );
        assert_eq!(
            volcado_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &quedan[0].id,
                &siembra.nodo
            ),
            Some(SEMILLA_CONTENIDO.to_vec()),
            "la purga se llevó por delante el contenido de la instantánea que se conserva"
        );

        drop(repositorio);
    }

    /// **13.** El listado sale del repositorio y va **acotado**.
    ///
    /// Lo que el módulo promete son tres cosas y la prueba fija las tres: que salen **como
    /// mucho** `ventana` instantáneas —la ventana se aplica a los identificadores, antes de
    /// resolverlos, que es lo que acota el trabajo de verdad—, que ninguna de las que salen
    /// es inventada, y que lo que sale va de la más reciente a la más antigua. Se comprueba
    /// además cada campo del resumen, porque es lo único que el panel publica y un resumen a
    /// medias sería una fila con huecos.
    ///
    /// Lo que **no** promete —y por eso esta prueba no lo afirma— es que la ventana sean las
    /// instantáneas más recientes del repositorio: el orden en que un backend devuelve sus
    /// identificadores es suyo, y quedarse con las más recientes exigiría resolverlas todas,
    /// que es justo lo que la ventana existe para no hacer. Con la ventana ancha —la que usa
    /// el panel— salen todas y la primera sí es la más reciente, y eso también se comprueba.
    #[test]
    fn el_listado_sale_del_repositorio_y_va_acotado() {
        let destino = Destino::nuevo();
        let siembra = sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );
        let sembrados = &siembra.identificadores;

        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino sembrado se abre");

        let acotado = repositorio
            .listar_snapshots(2)
            .expect("el repositorio se puede listar");
        assert_eq!(
            acotado.len(),
            2,
            "la ventana es de dos y el listado devolvió {}: no está acotado",
            acotado.len()
        );

        for resumen in &acotado {
            assert_eq!(
                resumen.id.len(),
                64,
                "el identificador del resumen no es el hex completo: {:?}",
                resumen.id
            );
            assert!(
                sembrados.contains(&resumen.id),
                "el listado devolvió una instantánea que no está en el repositorio: {}",
                resumen.id
            );
            let guardada = snapshot_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &resumen.id);
            assert_eq!(
                resumen.rutas,
                guardada.paths.iter().cloned().collect::<Vec<String>>(),
                "el resumen no trae las rutas que la instantánea declara"
            );
            assert_eq!(
                resumen.etiqueta.as_deref(),
                Some(ETIQUETA_SEMBRADA),
                "el resumen no trae la etiqueta de la tarea"
            );
            assert!(
                resumen.ficheros >= 1,
                "el resumen dice que la instantánea procesó {} ficheros y el árbol tiene uno",
                resumen.ficheros
            );
            assert!(
                resumen.bytes > 0,
                "el resumen dice que la instantánea procesó cero bytes"
            );
            assert!(
                resumen.fecha > time::OffsetDateTime::UNIX_EPOCH,
                "el resumen trae una fecha que no es la de una copia: {}",
                resumen.fecha
            );
        }

        assert!(
            acotado[0].fecha > acotado[1].fecha,
            "el listado va de la más antigua a la más reciente: {} y {}",
            acotado[0].fecha,
            acotado[1].fecha
        );

        let todos = repositorio
            .listar_snapshots(100)
            .expect("el repositorio se puede listar");
        assert_eq!(
            todos.len(),
            3,
            "con una ventana mayor que el repositorio tienen que salir las tres instantáneas"
        );
        assert_eq!(
            todos[0].id, sembrados[2],
            "con la ventana ancha, la primera del listado tiene que ser la más reciente"
        );
        assert!(
            todos[0].fecha > todos[1].fecha && todos[1].fecha > todos[2].fecha,
            "el listado no va de la más reciente a la más antigua: {:?}",
            todos
                .iter()
                .map(|resumen| resumen.fecha)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            todos[0].fecha,
            time::macros::datetime!(2026-01-03 10:00:00 UTC),
            "la fecha del resumen no es la de la instantánea: se convirtió por su \
             representación local en vez de por su instante"
        );

        drop(repositorio);
    }

    /// **14.** El nombre del volcado de la copia y el del origen tienen que **coincidir**, y
    /// si no, no se archiva nada.
    ///
    /// Es la comprobación que ningún tipo impone y que antes sostenía un comentario de las
    /// pruebas —«el nombre y las rutas coinciden a propósito»—, que es reconocer que la
    /// pareja podía divergir. Lo que se fija aquí es fail-closed por construcción: con los
    /// dos nombres iguales se archiva, y con dos distintos se devuelve
    /// [`Error::NombreDeVolcadoIncoherente`] **sin tocar el destino**.
    ///
    /// La segunda mitad es la que importa y se comprueba de dos formas: por el error —que
    /// nombra los dos nombres, para que quien lo lea vea la discrepancia— y por la foto del
    /// destino, que tiene que quedar idéntica. Comprobar solo el error dejaría pasar una
    /// implementación que rechazara **después** de escribir la instantánea, que es
    /// exactamente el desenlace que hay que evitar: una copia que existe, dice contener algo
    /// y contiene otra cosa.
    ///
    /// El caso de los nombres distintos se construye con el **túnel** —que es quien nombra
    /// el nodo del árbol— pidiéndole un nombre y entregándole una copia con otro, que es
    /// justo el fallo que se teme: el motor llama al túnel con un nombre y construye la copia
    /// con otro.
    #[test]
    fn archivar_volcado_exige_que_el_nombre_de_la_copia_y_el_del_origen_coincidan() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");

        // El lado que se conserva: los dos nombres iguales archivan.
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);
        let hecho = archivar(&repositorio, &copia, b"-- tienda\nSELECT 1;\n")
            .expect("con los dos nombres iguales el volcado se archiva");
        assert_eq!(
            volcado_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &hecho.id,
                &copia.nombre
            ),
            Some(b"-- tienda\nSELECT 1;\n".to_vec()),
            "el volcado archivado no se puede leer con el nombre que la copia declara"
        );

        // El lado que se rechaza: el túnel nombra el nodo con un nombre y la copia declara
        // otro. La instantánea diría contener `en_la_copia` y contendría `en_el_origen`.
        let antes = destino.instantanea();
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);
        let tunel = tunel_del_volcado("volcado-de-otra-cosa.sql".to_owned(), 4, PLAZO_DE_PRUEBA)
            .expect("el nombre del otro volcado es válido");
        let productor = tunel.productor;
        let emisor = std::thread::spawn(move || productor.terminar());

        let error = repositorio
            .archivar_volcado(&copia, &tunel.origen)
            .expect_err("con los dos nombres distintos no se puede archivar");
        emisor.join().expect("el hilo productor no puede caerse");

        let Error::NombreDeVolcadoIncoherente {
            en_la_copia,
            en_el_origen,
        } = &error
        else {
            panic!(
                "una copia cuyo nombre no es el del nodo tiene que rechazarse nombrando la \
                 discrepancia, y se rechazó de otra forma: {error}"
            );
        };
        assert_eq!(
            en_la_copia, &copia.nombre,
            "el error no dice el nombre que la copia declara"
        );
        assert_eq!(
            en_el_origen,
            tunel.origen.nombre(),
            "el error no dice el nombre con el que el origen nombra su nodo"
        );
        assert_ne!(
            en_la_copia, en_el_origen,
            "los dos nombres del error son iguales: el error no describe una discrepancia"
        );

        assert_eq!(
            destino.instantanea(),
            antes,
            "la copia con el nombre incoherente se rechazó **después** de escribir en el \
             destino: la instantánea que declara una cosa y contiene otra quedó guardada"
        );

        drop(repositorio);
    }

    /// **15.** El nodo del volcado declara **tamaño cero**, y el tamaño real está en el
    /// resumen.
    ///
    /// Es la mitad del contrato que faltaba por comprobar: el escenario dice literalmente
    /// que el nodo del volcado declara tamaño cero y que el tamaño de verdad está en el
    /// resumen de la instantánea, y hasta ahora la prueba del volcado comprobaba el resumen
    /// y **no** el nodo. Y la afirmación tiene consecuencia: es la razón de que el
    /// emparejamiento con la instantánea anterior haya que desactivarlo para un volcado —dos
    /// copias del mismo destino comparan igual, la biblioteca descarta el lector sin leerlo y
    /// la instantánea nueva apunta al contenido de la anterior—, así que si el nodo pasara a
    /// llevar el tamaño real, media docena de comentarios del módulo quedarían mintiendo.
    ///
    /// Se comprueba con un volcado de contenido **no vacío**, que es cuando se nota: un nodo
    /// de tamaño cero con un contenido de verdad es un nodo que no puede tener el tamaño
    /// dentro.
    #[test]
    fn el_nodo_del_volcado_declara_tamano_cero() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);
        let contenido = b"-- tienda\nCREATE TABLE tienda (id INT);\n";

        let hecho = archivar(&repositorio, &copia, contenido).expect("archivar no puede fallar");

        let nodo = nodo_del_snapshot(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &hecho.id,
            &copia.nombre,
        );
        assert_eq!(
            nodo.meta.size, 0,
            "el nodo del volcado declara tamaño {} y el contrato dice que declara cero: el \
             tamaño de un volcado por `stdout` no se conoce hasta que termina, y el único \
             sitio donde existe es el resumen de la instantánea. Si el nodo lo llevara, el \
             emparejamiento con la instantánea anterior lo compararía y la segunda copia \
             del mismo destino se descartaría sin leerla",
            nodo.meta.size
        );
        assert!(
            hecho.bytes > 0,
            "el volcado archivado no tiene bytes: la comprobación de arriba pasa por vacío"
        );
        assert_eq!(
            volcado_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &hecho.id,
                &copia.nombre
            ),
            Some(contenido.to_vec()),
            "el nodo declara tamaño cero pero el contenido tiene que estar entero dentro"
        );
        assert_eq!(
            hecho.bytes,
            contenido.len() as u64,
            "el tamaño real del volcado tiene que estar en el resumen, y el resumen dice {} \
             para un contenido de {}",
            hecho.bytes,
            contenido.len()
        );

        drop(repositorio);
    }

    /// **16.** Un volcado de **longitud cero** —un productor que declara el final sin mandar
    /// un solo byte— deja **un** fichero dentro del árbol y **cero** bytes.
    ///
    /// Es la medición que decide cuál de los dos números de [`CopiaHecha`] sirve para
    /// componer la condición de éxito: `bytes` no puede servir porque vale `0` lo mismo en un
    /// volcado vacío que en uno truncado, y `ficheros` sólo sirve si un volcado vacío sigue
    /// siendo un nodo. Aquí se mide, y sale `ficheros == 1` y `bytes == 0` con el nodo
    /// presente y de tamaño declarado cero.
    ///
    /// El caso no es un invento: una base de datos recién creada, sin tablas y sin datos,
    /// produce un volcado perfectamente vacío. Y `terminar` sin enviar nada **es** una
    /// entrega completa —lo que distingue un volcado vacío de uno truncado es el final
    /// declarado, no el tamaño—, y la prueba lo comprueba: sin eso, lo que se estaría
    /// midiendo es el caso del truncado, que ya tiene la suya.
    #[test]
    fn un_volcado_de_longitud_cero_deja_un_nodo_vacio_y_cuenta_un_fichero() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-vacio.sql", ETIQUETA);

        let hecho = archivar(&repositorio, &copia, b"").expect("archivar no puede fallar");

        assert_eq!(
            hecho.ficheros, 1,
            "un volcado de longitud cero tiene que dejar su nodo en el árbol —es la medición \
             que decide que `ficheros` sirva como apoderado del éxito—, y el archivado contó \
             {} ficheros. Si esto cambia, el apoderado cambia con él y lo que hay que usar \
             para decidir el éxito es otra cosa",
            hecho.ficheros
        );
        assert_eq!(
            hecho.bytes, 0,
            "un volcado vacío no puede tener bytes, y el archivado contó {}",
            hecho.bytes
        );
        assert_eq!(
            volcado_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &hecho.id,
                &copia.nombre
            ),
            Some(Vec::new()),
            "el nodo del volcado vacío no está en el árbol: `ficheros == 1` y el nodo ausente \
             no pueden ser verdad a la vez"
        );
        let nodo = nodo_del_snapshot(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &hecho.id,
            &copia.nombre,
        );
        assert_eq!(
            nodo.meta.size, 0,
            "el nodo del volcado vacío declara tamaño {} y no puede declarar otro",
            nodo.meta.size
        );

        drop(repositorio);
    }

    /// **17.** El listado trae **también** las instantáneas que no creó Kesto.
    ///
    /// Es la mitad del escenario del listado que no estaba medida: el módulo promete que la
    /// lista sale **del repositorio** y no del histórico de ejecuciones —«una instantánea que
    /// no está en el histórico es una copia hecha con otra herramienta»—, pero las tres
    /// instantáneas de la prueba del listado las siembra el mismo ayudante, así que un
    /// listado que dedujera su contenido del histórico las habría devuelto igual y la prueba
    /// habría pasado midiendo otra cosa.
    ///
    /// Aquí se siembra una instantánea **ajena** —la archiva la biblioteca, con otro host,
    /// otra etiqueta, otro comando, otro camino de fichero y otro contenido— y se comprueba
    /// que aparece en el listado, con sus datos: no vale con que aparezca el identificador,
    /// porque lo que se está midiendo es que el resumen se construye desde el repositorio.
    #[test]
    fn el_listado_trae_tambien_las_instantaneas_que_no_creo_kesto() {
        let destino = Destino::nuevo();
        let siembra = sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );
        let (ajena, nodo_ajeno) = sembrar_snapshot_ajeno(&destino.ruta, PASSPHRASE_DE_PRUEBA);

        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino sembrado se abre");

        let todos = repositorio
            .listar_snapshots(100)
            .expect("el repositorio se puede listar");
        assert_eq!(
            todos.len(),
            4,
            "el repositorio tiene tres instantáneas sembradas y una ajena, y el listado \
             devolvió {}: el listado no está saliendo del repositorio",
            todos.len()
        );

        let resumen = todos
            .iter()
            .find(|resumen| resumen.id == ajena)
            .unwrap_or_else(|| {
                panic!(
                    "la instantánea ajena no está en el listado ({ajena}): el listado solo \
                     devuelve lo que el histórico de Kesto conoce, y su contrato dice que \
                     sale del repositorio"
                )
            });

        assert_eq!(
            resumen.etiqueta.as_deref(),
            Some(ETIQUETA_AJENA),
            "el resumen de la instantánea ajena no trae su etiqueta"
        );
        assert_eq!(
            resumen.rutas.len(),
            1,
            "el resumen de la instantánea ajena no trae sus rutas: {:?}",
            resumen.rutas
        );
        assert_ne!(
            resumen.rutas,
            snapshot_guardado(
                &destino.ruta,
                PASSPHRASE_DE_PRUEBA,
                &siembra.identificadores[0]
            )
            .paths
            .iter()
            .cloned()
            .collect::<Vec<String>>(),
            "la instantánea ajena declara las mismas rutas que una sembrada: no es ajena, y \
             esta prueba no mediría lo que dice medir"
        );
        assert!(
            resumen.bytes > 0,
            "el resumen de la instantánea ajena dice que procesó cero bytes"
        );
        assert_eq!(
            volcado_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &ajena, &nodo_ajeno),
            Some(AJENA_CONTENIDO.to_vec()),
            "el contenido de la instantánea ajena no se puede restaurar y el resumen dice que \
             está: el listado la leyó a medias"
        );

        // Y el orden del listado sigue siendo el suyo: la ajena se archivó **ahora**, así que
        // es la más reciente de las cuatro. Es lo que hace que la ventana del panel deje
        // fuera las sembradas de enero antes que la de hoy.
        assert_eq!(
            todos[0].id, ajena,
            "la instantánea ajena es la más reciente y no encabeza el listado"
        );

        drop(repositorio);
    }

    /// **18.** Un repositorio con su configuración y **sin claves utilizables** se reporta
    /// como [`Error::PassphraseNoAbre`], y **está medido** que es lo que ocurre.
    ///
    /// Es la corrección del hallazgo: `is_incorrect_password()` es `is_code("C002")`, y C002
    /// lo emite la biblioteca **sólo** al final del bucle que busca una clave que acepte la
    /// contraseña —un bucle que salta los errores de las claves que no puede leer y sigue—.
    /// Así que un destino que tiene configuración y **ninguna clave utilizable** —una clave
    /// corrupta, un repositorio a medio inicializar, un `keys/` que se perdió— cae también
    /// por ahí, y el módulo lo reporta con la variante de la passphrase.
    ///
    /// Lo que esta prueba fija es **la medición, no una aspiración**: se monta el caso
    /// —inicializar el destino y retirarle todo lo que no sea `config`— y se comprueba que
    /// sale esta variante y no la de destino inalcanzable. Si algún día la biblioteca
    /// distingue los dos casos, esta prueba cae y hay que volver a medir; entonces la
    /// variante podrá prometer algo más de lo que promete hoy. Lo que **no** puede pasar es
    /// que el módulo prometa distinguirlos mientras la biblioteca no los distinga.
    ///
    /// Y se comprueba además la parte del motivo que sí se puede afirmar: lleva el código
    /// que la biblioteca publica para esa pregunta —`C002`—, y nombra el destino. Ese código
    /// es lo que hace que el desenlace salga por el camino de la passphrase en vez del de
    /// destino inalcanzable, así que dejarlo escrito es lo que ata la prueba a la causa.
    #[test]
    fn un_repositorio_sin_claves_utilizables_se_reporta_como_una_passphrase_que_no_abre() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        drop(repositorio);

        // El destino queda con su `config` y sin nada más: es el repositorio al que se le
        // perdió el directorio de claves. `config` se conserva porque es lo que decide que
        // el desenlace sea «existe» y no «no existe» —que es lo que hace que la biblioteca
        // llegue al bucle de las claves en vez de decir que no hay repositorio—.
        let mut quedan = 0;
        for (relativa, _) in destino.instantanea() {
            if !relativa.starts_with("config") {
                std::fs::remove_file(destino.ruta.join(&relativa))
                    .expect("los ficheros del destino se pueden retirar");
            } else {
                quedan += 1;
            }
        }
        assert_eq!(
            quedan, 1,
            "el destino tiene que quedarse con un solo fichero de configuración y quedaron \
             {quedan}: sin él el desenlace sería «el repositorio no existe», que es otro caso"
        );

        let error = destino.en_marcha(PASSPHRASE_DE_PRUEBA).expect_err(
            "un repositorio sin claves utilizables no puede abrirse, ni siquiera con la \
             passphrase con la que se creó",
        );

        let Error::PassphraseNoAbre {
            destino: nombrado,
            motivo,
        } = &error
        else {
            panic!(
                "medido: sin claves utilizables la biblioteca devuelve el mismo código que con \
                 una contraseña equivocada —C002—, así que la variante tiene que ser la de la \
                 passphrase. Salió otra cosa: {error}"
            );
        };
        assert!(
            nombrado.contains(&destino.ruta.display().to_string()),
            "el motivo del fallo no nombra el destino ({}): {nombrado}",
            destino.ruta.display()
        );
        assert!(
            motivo.contains("C002"),
            "el motivo tiene que llevar el código con el que la biblioteca señala «no hay \
             ninguna clave que acepte esta contraseña» —C002—, porque es el código el que \
             decide el desenlace y no el texto: {motivo}"
        );
    }

    /// **19.** Un fallo de `open()` que **no** es el de la passphrase no se reporta como
    /// passphrase: sale como fallo del destino.
    ///
    /// Es el otro lado de la rama `if` de [`poner_en_marcha_con`], y hasta ahora no estaba
    /// cubierto: el caso «destino inalcanzable» se provocaba **antes** de `open()`, así que
    /// una implementación que asignara la variante de la passphrase a **cualquier** fallo de
    /// `open()` habría pasado todas las pruebas anteriores. Aquí se provoca un fallo de
    /// `open()` de verdad y distinto.
    ///
    /// Cómo, sin red y sin depender del texto de la biblioteca: se deja el destino con su
    /// configuración y se ponen sus ficheros en modo `000`. Listar el destino —la pregunta
    /// que decide si hay repositorio— es un recorrido de directorio y **no** necesita leer
    /// los ficheros, así que el desenlace es «existe»; leer la configuración, en cambio,
    /// falla con un error de permisos del backend, que no es C002. Medido, el motivo es:
    ///
    /// ```text
    /// Error: Failed to read the contents of the file. Please check the file and try again.
    ///     (kind: related to the backend): caused by: Permission denied (os error 13)
    /// ```
    ///
    /// Los permisos se restauran al terminar: el directorio de la prueba se borra al soltar
    /// su guarda, y `remove_dir_all` sobre un directorio en modo `000` tampoco puede. El
    /// `Drop` de la guarda ignora el fallo del borrado, así que sin restaurarlos lo que
    /// quedaría es basura en el directorio temporal —y en `/tmp`, que aquí es RAM—.
    #[test]
    fn un_fallo_de_apertura_que_no_es_el_de_la_passphrase_es_un_fallo_del_destino() {
        use std::os::unix::fs::PermissionsExt;

        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        drop(repositorio);

        let ficheros = destino.instantanea();
        assert!(
            !ficheros.is_empty(),
            "el destino se inicializó y su foto está vacía"
        );

        let mut permisos_originales = Vec::new();
        for (relativa, _) in &ficheros {
            let camino = destino.ruta.join(relativa);
            let originales = std::fs::metadata(&camino)
                .expect("cada fichero del destino tiene metadatos")
                .permissions();
            let mut sin_permisos = originales.clone();
            sin_permisos.set_mode(0o000);
            std::fs::set_permissions(&camino, sin_permisos)
                .expect("los permisos del destino se pueden cambiar");
            permisos_originales.push((camino, originales));
        }

        let resultado = destino.en_marcha(PASSPHRASE_DE_PRUEBA);

        // Se devuelven los permisos **antes** de comprobar nada: si una aserción entra en
        // pánico, el `Drop` de la guarda tiene que poder borrar el directorio.
        for (camino, originales) in permisos_originales {
            std::fs::set_permissions(&camino, originales)
                .expect("los permisos del destino se pueden devolver");
        }

        let error = resultado.expect_err("con los ficheros ilegibles, el destino no abre");

        let Error::DestinoNoAlcanzable {
            destino: nombrado,
            motivo,
        } = &error
        else {
            panic!(
                "un fallo de apertura que no es el de la passphrase —un error de permisos del \
                 backend— tiene que reportarse como fallo del destino y no como «la passphrase \
                 no abre»: una implementación que asignara esa variante a cualquier fallo de \
                 `open()` pasaría todas las demás pruebas. Salió: {error}"
            );
        };
        assert!(
            !matches!(error, Error::PassphraseNoAbre { .. }),
            "el fallo de permisos del backend se confundió con una passphrase que no abre: \
             {error}"
        );
        assert!(
            nombrado.contains(&destino.ruta.display().to_string()),
            "el fallo del destino no nombra el destino ({}): {nombrado}",
            destino.ruta.display()
        );
        assert!(
            motivo.contains("Permission denied") || motivo.contains("os error 13"),
            "el motivo no es el del fallo de permisos que provoca la prueba, así que esta \
             prueba puede estar midiendo otro fallo del destino: {motivo}"
        );
    }

    /// **20.** El destino de S3 se construye con el esquema que funciona (`"s3"`) y **no**
    /// con el que le pide a `opendal` un servicio que no existe (`"opendal:s3"`).
    ///
    /// Es la prueba que faltaba del camino de S3, y el único camino del módulo que quedaba
    /// sin candado. Lo que fija importa porque quien «arregle» la constante a `"opendal:s3"`
    /// —que es lo que parece natural, porque es la forma de una URL de backend— pasa las
    /// cuatro puertas del CI: ninguna prueba tocaba esta función. Y no necesita red:
    /// construir un operador de S3 no la necesita, porque `opendal` no contacta con el
    /// destino hasta que se le pide un dato.
    ///
    /// Los dos errores son distinguibles y la prueba comprueba **el texto medido**: el
    /// esquema malo le pide un servicio llamado «opendal», que no está registrado, y el
    /// error lo dice nombrando el esquema y la lista de los que sí existen. Medido:
    ///
    /// ```text
    /// Error: Creating Operator from path `opendal:s3` failed. Please check the given schema
    ///     and options. (kind: related to the backend): caused by: Unsupported (permanent) at
    ///     , context: { scheme: opendal, available: azblob, …, s3, … } => scheme is not
    ///     registered
    /// ```
    #[test]
    fn el_destino_de_s3_se_construye_con_el_esquema_bueno_y_no_con_el_malo() {
        let destino = Destino::nuevo();
        let acceso = destino.acceso(PASSPHRASE_DE_PRUEBA);
        let opciones = opciones_de_s3(&acceso).expect("la fila de prueba es un destino válido");

        // El esquema bueno construye el backend, y el destino que construye es el que se le
        // pidió: `opendal` publica su localización, y ahí está el cubo.
        let bueno = OpenDALBackend::new(ESQUEMA_DE_S3, opciones.clone()).unwrap_or_else(|error| {
            panic!(
                "el esquema `{ESQUEMA_DE_S3}` tiene que construir el backend sin red y falló: \
                 {}. Es el esquema que la biblioteca registra para S3; si esto falla, la \
                 constante que usa el módulo no es la que `opendal` espera",
                error.display_log()
            )
        });
        assert_eq!(
            ESQUEMA_DE_S3, "s3",
            "el módulo tiene que pasarle a `opendal` el **esquema** de S3, que es `s3`"
        );
        assert!(
            bueno.location().ends_with(&acceso.bucket),
            "la localización del backend ({}) no termina en el cubo de la fila ({})",
            bueno.location(),
            acceso.bucket
        );

        // Y el camino de producción entero —la función que el motor usa— construye los
        // backends con esa misma constante: es la comprobación de que lo que se prueba
        // arriba es lo que se usa, y no una constante paralela que solo existe en la prueba.
        let backends = backends_de_s3(&acceso)
            .expect("el camino de producción construye los backends de S3 sin red");
        assert!(
            backends.repository().location().ends_with(&acceso.bucket),
            "el backend que construye el camino de producción no apunta al cubo de la fila"
        );

        // El esquema malo falla, y el error nombra el esquema equivocado.
        let error = OpenDALBackend::new("opendal:s3", opciones).expect_err(
            "`opendal:s3` no es un esquema: la ruta se parte por el primer `:` y lo que \
                 queda delante es el servicio que `opendal` busca en su registro",
        );
        let motivo = error.display_log();
        assert!(
            motivo.contains("opendal"),
            "el error del esquema malo no nombra el esquema que no está registrado: {motivo}"
        );
        assert!(
            motivo.contains("scheme is not registered"),
            "el error del esquema malo no dice que el esquema no está registrado: {motivo}"
        );
    }

    /// **21.** Los valores de la fila —endpoint, región y cubo— **llegan** al destino de S3.
    ///
    /// Es la comprobación que el camino de S3 no tenía, y va por dentro: `opendal` no publica
    /// las opciones con las que construyó su operador, así que la única forma de ver desde
    /// fuera que los valores de la fila viajan es mirar el mapa que el módulo le entrega a la
    /// dependencia. Ese mapa es el contrato —lo que sale de aquí es lo que la dependencia
    /// recibe, y ahí dentro va el `secret_access_key`—, y por eso [`opciones_de_s3`] está
    /// separada de [`backends_de_s3`].
    ///
    /// La región se comprueba aunque el §3 admita que esté vacía: lo que se afirma no es que
    /// tenga un valor útil, sino que **el que la fila trae es el que llega**, sin que nadie lo
    /// descarte por no validarlo.
    #[test]
    fn los_valores_de_la_fila_llegan_al_destino_de_s3() {
        let acceso = Acceso {
            endpoint: "https://s3.example.test:9000".to_owned(),
            bucket: "cesto-de-copias".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let opciones = opciones_de_s3(&acceso).expect("la fila describe un destino utilizable");

        for (clave, valor) in [
            ("endpoint", acceso.endpoint.as_str()),
            ("region", acceso.region.as_str()),
            ("bucket", acceso.bucket.as_str()),
            ("access_key_id", acceso.access_key_id.as_str()),
        ] {
            assert_eq!(
                opciones.get(clave).map(String::as_str),
                Some(valor),
                "el valor de `{clave}` no llega al destino de S3: el mapa que se le entrega a \
                 la dependencia trae {:?}",
                opciones.get(clave)
            );
        }

        // Y el cubo se puede ver también desde el backend ya construido, que es lo que
        // confirma que el mapa no se queda por el camino.
        let backend = OpenDALBackend::new(ESQUEMA_DE_S3, opciones)
            .expect("el esquema de S3 construye el backend sin red");
        assert!(
            backend.location().ends_with(&acceso.bucket),
            "el backend construido no apunta al cubo de la fila: {}",
            backend.location()
        );
    }

    /// **22.** Un fallo al construir el destino de S3 **no publica la clave secreta**.
    ///
    /// El único sitio del módulo donde el `secret_access_key` entra en un `BTreeMap` que se
    /// le entrega a una dependencia, y el único que quedaba sin mirar. Lo que se comprueba es
    /// que el camino de salida de un error de esa construcción no arrastra el mapa: el
    /// `secret_access_key` es un [`SecretString`], así que no puede salir por formateo, pero
    /// el mapa que se construye **sí lleva el valor en claro** —la dependencia lo necesita
    /// así— y lo que hay que asegurar es que ese mapa no acaba dentro de un mensaje.
    ///
    /// El fallo se provoca con el **esquema malo**, que es lo único de la construcción que
    /// falla sin red: un endpoint al que no se puede llegar no falla aquí, porque `opendal`
    /// no contacta con el destino hasta que le piden un dato. La aserción lleva su control
    /// —que el error no esté vacío— para que no pase por vacío.
    #[test]
    fn un_fallo_al_construir_el_destino_de_s3_no_publica_la_clave_secreta() {
        let acceso = Acceso {
            endpoint: "https://s3.example.test:9000".to_owned(),
            bucket: "cesto-de-copias".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
        };
        let opciones = opciones_de_s3(&acceso).expect("la fila describe un destino utilizable");
        assert_eq!(
            opciones.get("secret_access_key").map(String::as_str),
            Some(CLAVE_SECRETA_DE_PRUEBA),
            "el mapa tiene que llevar la clave secreta en claro —la dependencia la necesita \
             así—, o esta prueba no mediría lo que dice medir"
        );

        let error = OpenDALBackend::new("opendal:s3", opciones)
            .expect_err("el esquema malo no construye nada");
        let publicado = format!("{} {error:?} {}", error.display_log(), error);

        assert!(
            !publicado.is_empty() && publicado.trim().len() > 1,
            "el error de la construcción está vacío, así que la comprobación de abajo pasaría \
             sin medir nada"
        );
        assert!(
            !publicado.contains(CLAVE_SECRETA_DE_PRUEBA),
            "un error al construir el destino de S3 publicó la clave secreta: {publicado}"
        );
        assert!(
            !publicado.contains(PASSPHRASE_DE_PRUEBA),
            "un error al construir el destino de S3 publicó la passphrase: {publicado}"
        );
    }

    /// **23.** El informe de la retención lleva lo que el plan de purga dice que va a
    /// reclamar, y el número sale del plan.
    ///
    /// Es lo que hace verdad el comentario que decía que el plan se calcula antes «para que
    /// quede constancia de lo que se va a borrar»: hasta ahora el plan se calculaba y se
    /// entregaba a la purga sin que nadie le leyera nada, así que la constancia no existía.
    ///
    /// El escenario se construye con **tres volcados de contenido distinto** y no con la
    /// siembra de fechas: las instantáneas de la siembra comparten el mismo árbol —son el
    /// mismo árbol con otro sello de tiempo—, así que retirar dos no deja nada sin
    /// referenciar y el plan diría `0`. Con contenidos distintos cada instantánea tiene su
    /// propio contenido, y al retirar dos el plan tiene algo que reclamar de verdad: es el
    /// caso que hace que el campo se pueda comprobar en positivo y no solo «que sea cero».
    ///
    /// **Medido, y es donde la primera versión de esta prueba se equivocó**: el número son
    /// bytes de **paquete**, no bytes de contenido. Los tres volcados suman `217` bytes de
    /// contenido y el plan dice que va a reclamar `514`, porque dentro de los paquetes van
    /// además los árboles y el armazón de los paquetes:
    ///
    /// ```text
    /// // FALLO MEDIDO:
    /// el plan dice que va a reclamar 514 bytes y lo archivado en total fueron 217: una purga
    /// no puede reclamar más de lo que se escribió en el repositorio
    /// ```
    ///
    /// Por eso lo que se comprueba es que sea mayor que cero —que el plan se leyó y tenía
    /// trabajo— y no una cota por el tamaño de los volcados, que no es la magnitud de este
    /// campo. La interpretación está en el rustdoc de
    /// [`InformeDeRetencion::bytes_a_reclamar`].
    ///
    /// Y se comprueba también el otro lado, que es el de la primera prueba de la retención
    /// del repositorio: cuando lo retirado compartía árbol con lo que se conserva, el plan no
    /// tiene nada que reclamar y el campo vale `0` sin que eso sea un fallo.
    #[test]
    fn el_informe_de_retencion_lleva_los_bytes_que_el_plan_va_a_reclamar() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);

        for contenido in [
            &b"-- de ayer\nCREATE TABLE tienda (id INT);\n"[..],
            &b"-- de hoy\nCREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (7);\n"[..],
            &b"-- de manana\nCREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (8);\n\
              INSERT INTO tienda VALUES (9);\n"[..],
        ] {
            archivar(&repositorio, &copia, contenido).expect("archivar no puede fallar");
        }

        let informe = repositorio
            .aplicar_retencion(&crate::rustic::Retencion {
                keep_daily: 1,
                keep_weekly: 0,
                keep_monthly: 0,
            })
            .expect("aplicar la retención no puede fallar");

        assert_eq!(
            informe.retirados.len(),
            2,
            "con `keep_daily = 1` sobre tres instantáneas del mismo día tienen que sobrar dos, \
             y sobraron {}",
            informe.retirados.len()
        );
        assert!(
            informe.purgado,
            "la retención retiró instantáneas y no purgó"
        );
        assert!(
            informe.bytes_a_reclamar > 0,
            "los dos volcados retirados tenían contenido propio y el plan dice que no hay nada \
             que reclamar: el informe no está leyendo el plan, o lo está leyendo después de \
             que la purga hiciera su trabajo"
        );

        // El otro lado: retirar sin dejar nada huérfano da `0`, y `0` no es un fallo.
        let destino = Destino::nuevo();
        sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &[
                "2026-01-01T10:00:00Z",
                "2026-01-02T10:00:00Z",
                "2026-01-03T10:00:00Z",
            ],
        );
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino sembrado se abre");

        let informe = repositorio
            .aplicar_retencion(&crate::rustic::Retencion {
                keep_daily: 1,
                keep_weekly: 0,
                keep_monthly: 0,
            })
            .expect("aplicar la retención no puede fallar");

        assert_eq!(
            informe.retirados.len(),
            2,
            "la siembra tiene tres fechas en tres días distintos y `keep_daily = 1` tiene que \
             retirar dos"
        );
        assert_eq!(
            informe.bytes_a_reclamar, 0,
            "las tres instantáneas sembradas comparten el mismo árbol, así que al retirar dos \
             no queda nada sin referenciar y el plan no tiene nada que reclamar: el informe \
             dice {}",
            informe.bytes_a_reclamar
        );

        drop(repositorio);
    }
}
