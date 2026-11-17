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
use std::sync::mpsc;
use std::time::Duration;

use rustic_backend::opendal::OpenDALBackend;
use rustic_core::repofile::{Node, NodeType, SnapshotFile, SnapshotId};
use rustic_core::{
    BackupOptions, ConfigOptions, Credentials, Excludes, FileType, IndexedIdsStatus, KeyOptions,
    LocalDestination, LocalSource, LocalSourceFilterOptions, LocalSourceSaveOptions, LsOptions,
    ParentOptions, ReadBackend, Repository, RepositoryBackends, RepositoryOptions, RestoreOptions,
    SnapshotOptions, WriteBackend,
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
    ///
    /// **`pub(crate)`** para que el motor pueda nombrar el destino en el tope que él mismo impone
    /// al **listado** —ver [`con_tope`] y el rustdoc de
    /// [`Repositorio::listar_snapshots`]—: el nombre tiene que salir del mismo sitio en los dos
    /// topes, o el operador leería dos nombres distintos para el mismo destino.
    pub(crate) fn nombre_del_destino(&self) -> String {
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
    /// Construye el destino de S3 con los datos del [`Acceso`] y abre con [`abrir_con`], que es
    /// donde viven los tres desenlaces —con el tope de [`TOPE_DEL_DESTINO`] por delante, que es la
    /// parte que este camino tiene y el de prueba no—.
    ///
    /// # El tope del destino
    ///
    /// La apertura corre bajo [`con_tope`], así que un destino que no responde devuelve
    /// [`Error::DestinoNoAlcanzable`] en diez segundos en vez de agotar los cinco reintentos de la
    /// biblioteca —el defecto medido de la incidencia #100: 36,9–38,4 s—. La construcción del
    /// backend **no** entra en el tope, porque no habla con nadie: es local y no puede quedarse
    /// colgada.
    ///
    /// # ⚠️ La biblioteca entra en pánico si se la llama desde dentro de un runtime
    ///
    /// Con el backend de S3 la biblioteca construye su **propio** runtime y bloquea el hilo
    /// para hablar con el destino, así que llamarla desde dentro de otro runtime entra
    /// en **pánico** —medido, literal: «Cannot start a runtime from within a runtime.»—, y un
    /// pánico no es un error devuelto: se lleva por delante al hilo que lo sufre. Esa llamada es
    /// ahora la del hilo de [`con_tope`] y no la de quien llama, así que por **este** método el
    /// pánico no ocurre: quien lo llame desde dentro de un runtime obtiene el error del destino,
    /// no un hilo caído. Lo que **no** cambia es la regla para el resto del módulo —archivar,
    /// restaurar, retener y olvidar siguen llamando a la biblioteca en el hilo de quien llama, y
    /// sus avisos siguen en pie—, así que la advertencia del módulo no se retira.
    ///
    /// Los sitios desde los que sí se llama son el hilo del trabajador del motor —un `std::thread`
    /// pelado, ver `src/engine/cola.rs`— y
    /// [`Motor::abrir_repositorio`](crate::engine::Motor::abrir_repositorio), que saca el
    /// trabajo del runtime antes de llegar aquí.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::AccesoInvalido`] si la fila no describe un destino utilizable,
    /// [`Error::DestinoNoAlcanzable`] si el destino no se alcanza, no se puede preparar o agota el
    /// tope, y [`Error::PassphraseNoAbre`] si el repositorio existe y la passphrase no lo abre —lo
    /// que incluye, medido, el caso de un repositorio sin claves utilizables: ver el
    /// rustdoc de esa variante—.
    pub fn poner_en_marcha(
        acceso: &Acceso,
        barras: BarrasDelMotor,
    ) -> Result<(Self, PuestaEnMarcha), Error> {
        // Construir el backend no habla con el destino —es lo que hace `OpenDALBackend::new`, sin
        // red—, así que se queda fuera del tope y el acceso no tiene que cruzar al hilo.
        let backends = backends_de_s3(acceso)?;
        let destino = acceso.nombre_del_destino();
        let destino_del_hilo = destino.clone();
        let credenciales = Credentials::Password(acceso.passphrase.expose().to_owned());

        con_tope(&destino, TOPE_DEL_DESTINO, move || {
            abrir_con(credenciales, destino_del_hilo, backends, barras)
        })
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
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// El archivado bloquea el hilo y, con el backend de S3, la biblioteca construye su
    /// **propio** runtime para hablar con el destino: llamar a esto desde dentro de otro
    /// runtime entra en **pánico** —medido, literal: «Cannot start a runtime from within a
    /// runtime.»—. Quien lo llama es el hilo del trabajador del motor, que es un `std::thread`
    /// pelado y no está dentro de ninguno.
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
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// El origen es local, pero el repositorio no: con el backend de S3 la biblioteca construye
    /// su **propio** runtime y bloquea el hilo para hablar con el destino, así que llamar a esto
    /// desde dentro de otro runtime entra en **pánico** —medido, literal: «Cannot start a runtime
    /// from within a runtime.»—. Quien lo llama es el hilo del trabajador del motor, que es un
    /// `std::thread` pelado y no está dentro de ninguno.
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
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// Las tres operaciones van contra el repositorio, y con el backend de S3 la biblioteca
    /// construye su **propio** runtime y bloquea el hilo para hablar con el destino: llamar a
    /// esto desde dentro de otro runtime entra en **pánico** —medido, literal: «Cannot start a
    /// runtime from within a runtime.»—. Quien lo llama es el hilo del trabajador del motor, que
    /// es un `std::thread` pelado y no está dentro de ninguno.
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
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// Listar es leer del repositorio, y con el backend de S3 la biblioteca construye su
    /// **propio** runtime y bloquea el hilo para hablar con el destino: llamar a esto desde
    /// dentro de otro runtime entra en **pánico** —medido, literal: «Cannot start a runtime from
    /// within a runtime.»—, y ese pánico no es un error devuelto. Y este es el caso más fácil de
    /// cometer de los cinco: el listado lo pide una ruta de la API, que es `async` por
    /// definición. Quien lo llama de verdad es
    /// [`Motor::listar_snapshots`](crate::engine::Motor::listar_snapshots), que saca el trabajo
    /// del runtime antes de llegar aquí.
    ///
    /// # El tope de esta operación lo pone el motor, y aquí está el porqué
    ///
    /// El listado también tiene tope —un repositorio que se abre y luego deja de contestar no
    /// puede dejar al panel cargando sin fin—, pero **no** puede vivir en este método: un tope
    /// necesita mover el trabajo a otro hilo y este tipo no tiene nada que mover. El repositorio
    /// de la biblioteca no es `Clone` —su estado de índices no lo es— y el backend que lleva
    /// dentro no se puede sacar, así que lo único portable sería el repositorio entero, y eso es
    /// consumirlo. Consumirlo rompería a quien lista dos veces —el motor no, dos pruebas sí— y
    /// además ataría el tope a que el repositorio se suelte aquí. Quien **sí** lo posee es
    /// [`Motor::listar_snapshots`](crate::engine::Motor::listar_snapshots), que es donde está el
    /// tope, y por eso este método se queda con el cuerpo puro: [`listar_del_repositorio`].
    ///
    /// Lo que **no** cambia por eso es el desenlace: un tope agotado sigue llegando como
    /// [`Error::DestinoNoAlcanzable`], que es el error que el borde traduce a `502`.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::ListadoFallido`] si el repositorio no se puede listar, si alguna
    /// instantánea no se puede leer o si su fecha no es representable.
    pub fn listar_snapshots(&self, ventana: usize) -> Result<Vec<ResumenDeSnapshot>, Error> {
        listar_del_repositorio(&self.repo, &self.destino, ventana)
    }

    /// Dice si el repositorio tiene una instantánea con ese identificador, **sin restaurarla**.
    ///
    /// Es la lectura que la capa HTTP necesita para contestar `404` de verdad: el manejador puede
    /// resolver la fila del repositorio y el vínculo de la instantánea con su target sin abrir el
    /// almacenamiento, así que sin esta pregunta una instantánea que ya no está —una copia que la
    /// retención purgó, un repositorio recreado en el mismo destino— daría un `200` que promete un
    /// trabajo que no puede ocurrir, y el fallo solo saldría en el registro.
    ///
    /// Resuelve el identificador **igual que la restauración**: lo parsea a [`SnapshotId`] y lo
    /// busca en el índice de identificadores del repositorio, **en hex completo** —el de
    /// [`CopiaHecha::id`] y el del listado—, nunca en la forma corta del `Display`, que son ocho
    /// caracteres y es un prefijo ambiguo.
    ///
    /// # Por qué se pregunta al índice y no por la instantánea suelta
    ///
    /// Está medido: `get_snapshots` con un identificador que no está **no** devuelve una lista
    /// vacía, falla —la biblioteca intenta leer el fichero de esa instantánea y el backend contesta
    /// «no such file or directory»—, así que por ahí un «no está» y un repositorio roto se ven
    /// igual. El índice de identificadores, en cambio, es la lectura que el listado ya hace
    /// ([`Repositorio::listar_snapshots`]) y contesta sin tocar ninguno de los ficheros: enumera, y
    /// estar o no estar es una comparación.
    ///
    /// Un identificador que no llega a ser un hex completo es un `false` y **no** un error: lo que
    /// se pregunta es si esa instantánea está, y una cadena que no puede ser el identificador de
    /// ninguna no está. Así el rechazo que ve el operador es el mismo en los dos casos —«esa copia no
    /// está»— en vez de un error de servidor por una errata en la ruta.
    ///
    /// No se lee el índice **completo** —el que trae los tamaños, que es lo que exige
    /// `prepare_restore`— ni se toca el destino ni el árbol: es una lectura de metadatos.
    ///
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio, y el tope no vive aquí
    ///
    /// La biblioteca construye su propio runtime para hablar con el destino, así que llamarla desde
    /// dentro de otro entra en **pánico**. Quien la llama es
    /// [`Motor::tiene_la_instantanea`](crate::engine::Motor::tiene_la_instantanea), que saca el
    /// trabajo del runtime **y** le pone el tope de [`TOPE_DEL_DESTINO`] por delante: este método
    /// es el cuerpo puro, sin plazo, y el motivo es el mismo que en
    /// [`Repositorio::listar_snapshots`] —mover el `list` a otro hilo exige poseer el repositorio, y
    /// este tipo no es `Clone`—. Con el tope por delante, la llamada corre en el hilo de
    /// [`con_tope`] y no en el de quien pregunta, así que el pánico del runtime no ocurre por ese
    /// camino.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::ListadoFallido`] si el repositorio no se deja leer —el mismo desenlace que
    /// el listado, porque es la misma lectura de índice—. El tope agotado **no** sale de aquí: sale
    /// de [`Motor::tiene_la_instantanea`], como [`Error::DestinoNoAlcanzable`].
    pub fn tiene_la_instantanea(&self, id: &str) -> Result<bool, Error> {
        let fallo = |motivo: String| Error::ListadoFallido {
            destino: self.destino.clone(),
            motivo,
        };

        let Ok(buscada) = id.parse::<SnapshotId>() else {
            return Ok(false);
        };

        self.repo
            .list::<SnapshotId>()
            .map_err(|error| fallo(error.display_log()))
            .map(|mut presentes| presentes.any(|candidata| candidata == buscada))
    }

    /// Olvida una instantánea: la retira del repositorio por su identificador.
    ///
    /// Es para una copia que **no se ha dado por buena** después de que el archivado ya escribiera
    /// su instantánea —la biblioteca silencia el fallo del lector y guarda la instantánea sin el
    /// volcado dentro; ver el encabezado del módulo—. El motor la llama para no dejar en el
    /// repositorio el residuo de una copia que no llegó a serlo: una instantánea que sale en el
    /// explorador, cuenta como espacio y no la retira la retención, que solo corre tras copias
    /// exitosas.
    ///
    /// La retirada es **de mejor esfuerzo** y quien la llama decide qué hacer con su fallo: el
    /// motor la trata como limpieza y **no** cambia por ella el veredicto de la ejecución.
    ///
    /// El identificador se pasa en **hex completo** —el de [`CopiaHecha::id`] y el que el listado
    /// publica—, y no en la forma corta del `Display` de `SnapshotId`, que son ocho caracteres y
    /// es un prefijo ambiguo.
    ///
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// Retirar es escribir en el repositorio, y con el backend de S3 la biblioteca construye su
    /// **propio** runtime y bloquea el hilo para hablar con el destino: llamar a esto desde dentro
    /// de otro runtime entra en **pánico** —medido, literal: «Cannot start a runtime from within a
    /// runtime.»—. Quien la llama es el hilo del trabajador del motor, que es un `std::thread`
    /// pelado y no está dentro de ninguno.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::OlvidoFallido`] si el identificador no es un hex completo o si el
    /// repositorio no deja retirar la instantánea.
    pub fn olvidar_instantanea(&self, id: &str) -> Result<(), Error> {
        let fallo = |motivo: String| Error::OlvidoFallido {
            destino: self.destino.clone(),
            motivo,
        };

        let instantanea: SnapshotId = id
            .parse()
            .map_err(|error: Box<rustic_core::RusticError>| fallo(error.display_log()))?;

        self.repo
            .delete_snapshots(&[instantanea])
            .map_err(|error| fallo(error.display_log()))
    }

    /// Restaura una instantánea completa sobre un directorio del sistema de ficheros.
    ///
    /// Es la operación que hace útil una copia: leer del repositorio y dejar los datos otra vez
    /// donde estaban. Se restaura **la instantánea entera** y no una ruta suya —el panel restaura
    /// una copia, no un fichero suelto—, así que el recorrido baja por todo el árbol.
    ///
    /// # La ruta montada se descuenta
    ///
    /// El contenido queda en la **raíz** del destino, no anidado bajo la ruta con la que se copió.
    /// La instantánea guarda el contenido con la ruta con la que se montó
    /// (`openspec/specs/extraction/spec.md:97-102`), así que el árbol de una copia de
    /// `/backup_sources/x` cuelga de `backup_sources/x`; restaurarlo desde la raíz sintética
    /// recrearía esa ruta entera **dentro** del destino, y lo que pide el motor para un target
    /// `directory` es escribir al `host_path` con `LocalDestination`
    /// (`openspec/specs/engine/spec.md:291`). El prefijo sale de la ruta que la instantánea declara
    /// —[`ruta_declarada`], que dice también cuándo no hay ninguno que descontar—.
    ///
    /// # El índice tiene que ser el completo, y eso es lo que decide la firma
    ///
    /// Medido en la biblioteca: preparar un plan de restauración exige saber **cuánto ocupa cada
    /// trozo** —el plan agrupa los ficheros por el trozo que los contiene para leerlos una vez—, y
    /// ese tamaño no está en el índice reducido con el que este tipo se abre
    /// ([`IndexedIdsStatus`]: solo los identificadores, que es lo que basta para archivar y para
    /// purgar). De ahí que la restauración **consuma** el repositorio: pasar al índice completo es
    /// una conversión que se lleva el valor, y el repositorio es de un solo uso práctico de todas
    /// formas —ver el encabezado del módulo—.
    ///
    /// El precio está medido y se paga entero: leer el índice completo cuesta una pasada más sobre
    /// el índice —el reducido ya se leyó al poner en marcha— y retiene en memoria el tamaño y la
    /// situación de cada trozo. Es lo que la biblioteca exige para restaurar, y hacerlo de otra
    /// forma sería deducir los tamaños de otra fuente.
    ///
    /// # El destino se crea, y lo que sobra solo se retira si se ha confirmado
    ///
    /// El destino es la ruta del `host_path` del target: se **crea** si no está y la restauración
    /// escribe dentro. Lo que ya estaba y **no** está en la instantánea se queda donde está, salvo
    /// que `podar` venga activado: entonces el destino queda **igual a la instantánea** y lo que
    /// sobra se retira —la poda de la biblioteca, `RestoreOptions::delete`—.
    ///
    /// Por defecto no se poda, y es la decisión conservadora: `delete: true` borra del host todo lo
    /// que la instantánea no contenga, y una restauración lanzada por error sobre el directorio
    /// equivocado se lleva por delante datos que no tienen copia. Por eso poda quien **confirma** la
    /// sobrescritura, con la misma bandera booleana del contrato de la petición que una base exige
    /// siempre (`openspec/specs/restauracion/spec.md`, «La restauración de un target de base exige
    /// confirmación explícita de sobrescritura»): un destino de directorio se sustituye cuando el
    /// operador lo ha autorizado, y sin ese «sí» deliberado solo recibe
    /// (`openspec/changes/bateria-e2e-de-copia-y-restauracion/specs/verificacion-e2e/spec.md`,
    /// «Sobrescritura de un destino con contenido»).
    ///
    /// La poda es de **mejor esfuerzo**, y eso es de la biblioteca, no de aquí: retirar un fichero
    /// que no se deja se registra y la restauración sigue, así que un `Ok` con poda no promete que
    /// el destino quedara exactamente igual a la instantánea. Lo que promete es que la
    /// restauración escribió todo lo que tenía que escribir.
    ///
    /// Y es **irreversible**, que es el otro lado de lo mismo: la poda ocurre en
    /// `prepare_restore`, **antes** de escribir el contenido, de modo que un fallo a mitad de
    /// escritura deja el destino **ya podado** y a medias —lo que sobraba no vuelve—, y el motivo
    /// de la ejecución hablará del fallo de restauración, no de la pérdida de datos.
    ///
    /// En cambio `verify_existing` va **activado**: un fichero que ya está y coincide se compara por
    /// contenido y no por fecha y tamaño. Lo que cuesta es relectura, y lo que compra es no dar por
    /// bueno un fichero que cambió sin que la fecha lo delate.
    ///
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// Restaurar bloquea el hilo y, con el backend de S3, la biblioteca construye su **propio**
    /// runtime para hablar con el destino: llamar a esto desde dentro de otro runtime entra en
    /// **pánico** —medido, literal: «Cannot start a runtime from within a runtime.»—. Quien la llama
    /// es [`Motor::restaurar`](crate::engine::Motor::restaurar), que saca el trabajo del runtime
    /// antes de llegar aquí.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::RestauracionFallida`] si el identificador no es un hex completo, si la
    /// instantánea no está en el repositorio, si la instantánea declara más de una ruta, si declara
    /// una ruta que su árbol no tiene, si el destino no se deja preparar o si la restauración no
    /// termina.
    pub fn restaurar_snapshot(
        self,
        instantanea: &str,
        destino_local: &str,
        podar: bool,
    ) -> Result<(), Error> {
        let Repositorio { repo, destino } = self;

        let fallo = |motivo: String| Error::RestauracionFallida {
            destino: destino.clone(),
            instantanea: instantanea.to_owned(),
            motivo,
        };

        // El identificador se pasa en **hex completo** —el del listado y el de la respuesta de la
        // API—, no en la forma corta del `Display` de `SnapshotId`, que son ocho caracteres y es un
        // prefijo ambiguo.
        let identificador: SnapshotId = instantanea
            .parse()
            .map_err(|error: Box<rustic_core::RusticError>| fallo(error.display_log()))?;

        // El índice completo **lo antes posible**: es el requisito de `prepare_restore` y el único
        // sitio del recorrido donde se puede pagar, antes de tocar el destino.
        let repo = repo
            .to_indexed()
            .map_err(|error| fallo(error.display_log()))?;

        // La instantánea, por su identificador: es lo que trae el árbol que se va a recorrer.
        let hex = identificador.to_hex().as_str().to_owned();
        let mut copias = repo
            .get_snapshots(&[hex.as_str()])
            .map_err(|error| fallo(error.display_log()))?;

        let Some(copia) = copias.pop() else {
            return Err(fallo(format!(
                "el repositorio no tiene ninguna instantánea con el identificador `{hex}`"
            )));
        };

        // El nodo del que cuelga todo el árbol que se restaura: el de la **ruta declarada**, que es
        // el prefijo que se descuenta. El árbol de una copia de directorio cuelga de la ruta con la
        // que se montó —`backup_sources/x`, y no la raíz—, así que empezar el recorrido en la raíz
        // sintética recrearía esa ruta entera **dentro** del destino; empezarlo en el nodo de la
        // ruta declarada deja su contenido en la raíz del destino, que es lo que pide el motor para
        // un target `directory` —`openspec/specs/engine/spec.md:291`, el `host_path` con
        // `LocalDestination`—. La función que decide el prefijo, [`ruta_declarada`], dice por qué.
        //
        // La cadena vacía sigue siendo el caso del `None`, y no es un atajo: la biblioteca
        // interpreta una ruta sin componentes como el nodo raíz **sintético** —sin nombre y con el
        // árbol entero por debajo—, que es justo lo que se pide cuando no hay un prefijo que
        // descontar. Y una instantánea que declare **varias** rutas no llega hasta aquí: es un
        // fallo de restauración, con su motivo, porque no hay un prefijo único que descontar.
        let prefijo = ruta_declarada(&copia).map_err(&fallo)?;
        let raiz = repo
            .node_from_snapshot_and_path(&copia, prefijo.unwrap_or(""))
            .map_err(|error| fallo(error.display_log()))?;

        // El recorrido, clonable: se consume una vez para preparar el plan y otra para restaurar,
        // y las dos tienen que ver **los mismos** nodos. Y las rutas que devuelve son **relativas
        // al nodo de partida** —lo promete `Repository::ls`—, que es la otra mitad del descuento:
        // con el nodo de la ruta declarada por raíz, `a.txt` es `a.txt` y no `backup_sources/x/a.txt`.
        let nodos = repo
            .ls(&raiz, &LsOptions::default())
            .map_err(|error| fallo(error.display_log()))?;

        // El destino se crea si no está y **no** se espera un fichero suelto: lo que se restaura es
        // un árbol dentro de un directorio. El tercer argumento es `expect_file`, no la poda: la
        // poda viaja en las opciones, más abajo.
        let destino_local = LocalDestination::new(destino_local, true, false)
            .map_err(|error| fallo(error.display_log()))?;

        // Las opciones, por sus mutadores y no por literal: el tipo es `#[non_exhaustive]`, así que
        // desde fuera del crate no se puede construir con una expresión de struct —ni con la
        // actualización funcional—, y los dos valores que se tocan son los dos que están medidos
        // arriba: `verify_existing` siempre activado, y `delete` —la poda— según la confirmación.
        let opciones = RestoreOptions::default()
            .verify_existing(true)
            .delete(podar);

        let plan = repo
            .prepare_restore(&opciones, nodos.clone(), &destino_local, false)
            .map_err(|error| fallo(error.display_log()))?;

        repo.restore(plan, &opciones, nodos, &destino_local)
            .map_err(|error| fallo(error.display_log()))
    }

    /// Vuelca al `destino` **el volcado** que la instantánea guarda.
    ///
    /// Es el primer tramo de la restauración de una base de datos: el volcado que la copia archivó
    /// —un **único** nodo de fichero dentro de la instantánea— sale del repositorio por el
    /// [`std::io::Write`] que se le pasa, que es lo que la biblioteca exige
    /// ([`Repository::dump`]). El otro tramo —llevar esos bytes al `stdin` del contenedor— es de
    /// [`crate::engine::restauracion`], que es quien le entrega un escritor que habla con el `exec`;
    /// este módulo no sabe nada de contenedores.
    ///
    /// # Por qué vuelca **el único** nodo de fichero y no una ruta
    ///
    /// Quien restaura no conoce el nombre del volcado: la copia lo decidió al extraer y lo dejó
    /// dentro del árbol. Lo único que se sabe de él es que una copia de volcado produce un solo
    /// fichero —el puente
    /// [`archivar_volcado`](Self::archivar_volcado) no deja otro—, así que la instantánea se
    /// recorre y se exige que haya exactamente uno. Cero o más de uno es un fallo con motivo
    /// propio ([`Error::VolcadoQueNoEsUno`]): una instantánea que no es de un volcado no se puede
    /// restaurar como si lo fuera adivinando cuál de sus ficheros era.
    ///
    /// # El índice tiene que ser el completo
    ///
    /// Igual que en [`restaurar_snapshot`](Self::restaurar_snapshot), y por la misma razón de la
    /// biblioteca: leer el contenido de un nodo exige el índice completo
    /// ([`IndexedFull`](rustic_core::IndexedFull)), no el reducido con el que este tipo se abre.
    /// De ahí que el método **consuma** el repositorio.
    ///
    /// # ⚠️ No se puede llamar desde dentro de un runtime de Tokio
    ///
    /// Volcar bloquea el hilo y, con el backend de S3, la biblioteca construye su **propio**
    /// runtime para hablar con el destino: llamar a esto desde dentro de otro runtime entra en
    /// **pánico** —medido, literal: «Cannot start a runtime from within a runtime.»—. Quien lo
    /// llama es un hilo de bloqueo, que es donde corre el motor.
    ///
    /// # El `destino` es **bloqueante**, y ahí está la trampa
    ///
    /// La biblioteca escribe con [`std::io::Write`] y no tiene forma de escribir con un
    /// [`tokio::io::AsyncWrite`], así que el escritor que se le pasa bloquea el hilo que llama. Lo
    /// que se le entregue tiene que ser un puente hacia el lado asíncrono —ver
    /// [`crate::engine::restauracion::EscritorDelVolcado`]—, no un escritor asíncrono.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::RestauracionFallida`] si el identificador no es un hex completo, si la
    /// instantánea no está en el repositorio o si el volcado falla; y
    /// [`Error::VolcadoQueNoEsUno`] si la instantánea no trae exactamente un nodo de fichero.
    pub fn volcar_a(
        self,
        instantanea: &str,
        destino: &mut impl std::io::Write,
    ) -> Result<(), Error> {
        let Repositorio {
            repo,
            destino: nombre_del_destino,
        } = self;

        let fallo = |motivo: String| Error::RestauracionFallida {
            destino: nombre_del_destino.clone(),
            instantanea: instantanea.to_owned(),
            motivo,
        };

        // El identificador en **hex completo**, igual que en `restaurar_snapshot`: la forma corta
        // del `Display` de `SnapshotId` son ocho caracteres y es un prefijo ambiguo.
        let identificador: SnapshotId = instantanea
            .parse()
            .map_err(|error: Box<rustic_core::RusticError>| fallo(error.display_log()))?;

        // El índice completo, lo antes posible: es el requisito de `dump` y el único sitio del
        // recorrido donde se puede pagar, antes de volcar un byte.
        let repo = repo
            .to_indexed()
            .map_err(|error| fallo(error.display_log()))?;

        let hex = identificador.to_hex().as_str().to_owned();
        let mut copias = repo
            .get_snapshots(&[hex.as_str()])
            .map_err(|error| fallo(error.display_log()))?;

        let Some(copia) = copias.pop() else {
            return Err(fallo(format!(
                "el repositorio no tiene ninguna instantánea con el identificador `{hex}`"
            )));
        };

        // El nodo raíz **sintético** de la instantánea, como en la restauración a un directorio.
        let raiz = repo
            .node_from_snapshot_and_path(&copia, "")
            .map_err(|error| fallo(error.display_log()))?;

        // El volcado: el **único** nodo de fichero de la instantánea. Se cuenta todo lo que haya
        // —no se corta al primero— porque dos ficheros también es un fallo, y con el primero solo
        // se volcaría el que la biblioteca listara antes sin que nadie lo sepa.
        let mut volcado: Option<Node> = None;
        let mut encontrados = 0_usize;

        let nodos = repo
            .ls(&raiz, &LsOptions::default())
            .map_err(|error| fallo(error.display_log()))?;

        for entrada in nodos {
            let (_, nodo) = entrada.map_err(|error| fallo(error.display_log()))?;

            if nodo.node_type == NodeType::File {
                encontrados += 1;
                volcado.get_or_insert(nodo);
            }
        }

        // El filtro por `encontrados == 1` es lo que hace que ni el vacío ni el de dos pasen: sin
        // él, el `Option` de arriba aceptaría el primero de dos.
        let Some(nodo) = volcado.filter(|_| encontrados == 1) else {
            return Err(Error::VolcadoQueNoEsUno {
                destino: nombre_del_destino,
                instantanea: instantanea.to_owned(),
                encontrados,
            });
        };

        repo.dump(&nodo, destino)
            .map_err(|error| fallo(error.display_log()))
    }
}

/// Lista las instantáneas de un repositorio **ya abierto**, como mucho `ventana`.
///
/// Es el cuerpo de [`Repositorio::listar_snapshots`] separado por el tope: [`con_tope`] ejecuta su
/// cierre en otro hilo, así que lo que cruza es el repositorio —un puñado de `Arc`, de ahí el
/// `clone` de la llamada— y el **nombre** del destino, que es lo único que hace falta para el
/// motivo de un fallo. Nada de aquí se guarda: el repositorio del hilo se suelta con él.
///
/// # Errors
///
/// Las mismas que [`Repositorio::listar_snapshots`]: [`Error::ListadoFallido`] si el repositorio no
/// se deja listar, si una instantánea no se puede leer o si su fecha no es representable.
fn listar_del_repositorio(
    repo: &Repository<IndexedIdsStatus>,
    destino: &str,
    ventana: usize,
) -> Result<Vec<ResumenDeSnapshot>, Error> {
    let fallo = |motivo: String| Error::ListadoFallido {
        destino: destino.to_owned(),
        motivo,
    };

    // El identificador se pasa en **hex completo**: el `Display` de `SnapshotId` da
    // solo sus ocho primeros caracteres, que es un prefijo ambiguo.
    let identificadores: Vec<String> = repo
        .list::<SnapshotId>()
        .map_err(|error| fallo(error.display_log()))?
        .take(ventana)
        .map(|id| id.to_hex().as_str().to_owned())
        .collect();

    if identificadores.is_empty() {
        return Ok(Vec::new());
    }

    let mut instantaneas = repo
        .get_snapshots(&identificadores)
        .map_err(|error| fallo(error.display_log()))?;
    instantaneas.sort_unstable_by(|una, otra| otra.time.cmp(&una.time));

    instantaneas
        .iter()
        .map(|instantanea| ResumenDeSnapshot::desde(instantanea, destino))
        .collect()
}

/// La ruta de la que cuelga el árbol de una instantánea, cuando declara **una sola**.
///
/// Es el prefijo que una restauración tiene que descontar. La instantánea guarda el contenido con
/// la ruta con la que se montó y no con la del host (`openspec/specs/extraction/spec.md:97-102`),
/// así que el árbol de una copia de `/backup_sources/x` cuelga de `backup_sources/x`: la biblioteca
/// archiva las rutas tal cual y descarta solo la barra inicial, de modo que el nodo de esa ruta
/// declarada **es** el contenido del directorio. Restaurar desde la raíz sintética lo dejaría
/// anidado —`<destino>/backup_sources/x/a.txt`— y lo que se pide es que quede en la raíz del
/// destino.
///
/// La ruta se devuelve **tal como la declaró la instantánea**, prestada de ella y sin quitarle
/// nada: la biblioteca descarta por su cuenta los componentes que no son nombres —la raíz de una
/// ruta absoluta— al resolver el nodo, así que lo único que hay que hacer aquí es no inventarse un
/// prefijo.
///
/// # Cuándo no hay prefijo, y por qué varias rutas son un fallo
///
/// `Ok(None)` es el caso de **cero rutas declaradas**: no hay ningún prefijo que descontar y
/// adivinarlo de la forma del árbol sería inventarse de dónde salió la copia; un árbol que ya es
/// plano —la instantánea cuelga entera de la raíz sintética— es legítimo, y entonces el recorrido
/// empieza ahí, que es lo que se hacía antes de existir esto.
///
/// `Err` con el motivo es el caso de **dos o más**: no hay un prefijo **único** que descontar, y
/// elegir uno dejaría fuera del destino el contenido del resto. Antes se devolvía `None` también
/// aquí, y eso **reintroducía el anidado en silencio** —el recorrido arrancaba en la raíz sintética
/// y el árbol salía anidado bajo las rutas declaradas— con la ejecución acabando en `success`. En
/// vez de eso se rechaza, con el mismo trato que los demás fallos de restauración: acaba en
/// `failed` y el motivo lo dice. Hoy es inalcanzable por API —una instantánea sin vínculo con la
/// ejecución contesta `409`—, así que no rompe ningún camino legítimo.
///
/// Kesto no produce ninguna de las dos formas: una copia declara la ruta de su `host_path` o el
/// nombre de su volcado, siempre una.
fn ruta_declarada(copia: &SnapshotFile) -> Result<Option<&str>, String> {
    let mut declaradas = copia.paths.iter();

    let Some(unica) = declaradas.next() else {
        return Ok(None);
    };

    if let Some(otra) = declaradas.next() {
        return Err(format!(
            "la instantánea declara más de una ruta —`{unica}` y `{otra}`—, así que no hay un \
             prefijo único que descontar al restaurar"
        ));
    }

    Ok(Some(unica.as_str()))
}

/// El tope con el que Kesto acota las operaciones que **abren o listan** el destino.
///
/// Se impone en Kesto y no en las opciones del backend porque la versión pinada no tiene dónde
/// ponerlo: `opendal` 0.58.2 no declara ningún campo de tiempo en la configuración de S3, y
/// `rustic_backend` 0.7.0 lee del mapa solo `retry`, `connections` y `throttle` —construye el
/// operador por dentro, con `Operator::via_iter`, sin punto de inyección—. Las versiones están
/// pinadas a la caché y el build es **offline**, así que subir `opendal` o llevarlo a un `git`
/// no es una alternativa: lo que hay es esta capa.
///
/// El valor es el que decidió el operador para la incidencia #100: contra un destino que no
/// responde, el panel esperaba **36,9–38,4 s** antes de dar el `502`, y lo que el operador ve
/// tiene que ser un error en un plazo que se note como «no responde» y no como «va lento».
/// # Dónde vive
///
/// **`pub(crate)`**: el tope de la apertura se impone en [`Repositorio::poner_en_marcha`]; el del
/// **listado** y el de la **pregunta por una instantánea** —el `list` del índice que hace el `404`
/// de la restauración— en [`Motor::listar_snapshots`](crate::engine::Motor::listar_snapshots) y
/// [`Motor::tiene_la_instantanea`](crate::engine::Motor::tiene_la_instantanea), que son los únicos
/// sitios donde el repositorio está en la mano para moverlo a un hilo. La constante y el mecanismo
/// son los mismos, y por eso están expuestos al crate en vez de escritos dos veces: una política
/// con dos valores no es una política.
///
/// # El peor caso de una lectura del panel
///
/// Cada tramo lleva **su** tope, así que una petición que encadena dos —el
/// `GET /api/v1/snapshots`, que abre y lista— puede tardar hasta **2 × esta constante** en el caso
/// extremo: una apertura lenta que aún responde dentro de su plazo y un listado que se agota. No se
/// funde en un tope único a propósito; el porqué, en el rustdoc de [`con_tope`].
pub(crate) const TOPE_DEL_DESTINO: Duration = Duration::from_secs(10);

/// Los reintentos que Kesto le declara a la biblioteca en el mapa de opciones.
///
/// No es una opción de `opendal`: es la clave que `rustic_backend` lee para acotar el reintento de
/// su capa, y su valor por defecto —`DEFAULT_RETRY = 5`— es el que producía los 36,9–38,4 s de
/// retroceso exponencial del informe. **Uno**, y no cero: un reintento es lo que sobrevive a un
/// fallo pasajero del destino —cuando ya no se puede reintentar, el fallo es del destino y no del
/// momento—, y el total no depende de este número porque [`TOPE_DEL_DESTINO`] lo acota en la capa
/// de Kesto. Con uno, un destino que rechaza en el acto contesta en torno a un segundo.
const REINTENTOS_DEL_DESTINO: &str = "1";

/// Ejecuta `operacion` en un hilo propio y espera su desenlace **como mucho** `limite`.
///
/// Es el tope de Kesto: cuando el límite se agota, la operación se **abandona** y lo que se
/// devuelve es [`Error::DestinoNoAlcanzable`] —el mismo error tipado del destino que no se
/// alcanza, que el borde ya traduce a `502`—, con un motivo escrito aquí y no el texto crudo de la
/// dependencia. El `destino` es el nombre con el que el operador reconoce el sitio al que no se
/// llegó.
///
/// # Por qué un hilo y un canal
///
/// Porque no hay otra forma de abandonar una llamada que bloquea el hilo: la biblioteca no ofrece
/// cancelación, así que la única manera de **volver** dentro del plazo es dejar la operación en un
/// hilo y no esperarla más que el límite. El precio, que hay que saber, es que la operación
/// abandonada **sigue corriendo** hasta que la dependencia se rinda sola —una contra un destino
/// que se traga los paquetes sin contestar nunca no termina, y ahí queda un hilo por petición—.
/// No es un pánico ni una fuga silenciosa: el hilo muere con su desenlace, y que el receptor ya no
/// esté no es un error.
///
/// # Qué **no** lleva este tope
///
/// Solo las operaciones que abren o listan: abandonar una lectura cuesta una lectura, y abandonar
/// una **escritura** —el archivado, la restauración, la retención— dejaría un escritor trabajando
/// por detrás mientras quien mandó la copia ya leyó un fallo, que es exactamente la copia a medias
/// que este módulo existe para no dar por buena.
///
/// # Lo que este tope cambia de sitio
///
/// La operación deja de correr en el hilo de quien llama y corre en uno de Kesto, así que el
/// pánico con el que la biblioteca protesta cuando se la llama desde dentro de un runtime —«Cannot
/// start a runtime from within a runtime»— no ocurre por los caminos que pasan por aquí: el hilo
/// que abre y el que lista no están dentro de ninguno. La advertencia de cada método afectado dice
/// de quién es el hilo en cada caso.
///
/// # Lo que cambia en el borde, y es a propósito
///
/// Un **pánico** de dentro de la biblioteca, en el hilo de esta operación, ya no llega al borde
/// como [`crate::engine::ErrorDelMotor::TrabajoInterrumpido`] —que se traduce a `500`—: el emisor
/// se suelta sin mandar nada y el receptor lo lee como
/// [`mpsc::RecvTimeoutError::Disconnected`], que aquí es [`Error::DestinoNoAlcanzable`] y el borde
/// traduce a `502`. Antes de este tope ese pánico subía por `spawn_blocking` y el motor lo
/// devolvía como trabajo interrumpido; ahora la llamada que puede caerse ya no está en el hilo del
/// motor y lo que el operador ve es el destino inalcanzable. Se ha elegido así porque un hilo de la
/// biblioteca que se cae al hablar con el destino **es**, para quien pregunta, un destino del que no
/// se ha sabido nada, y porque el `500` de un pánico no es una respuesta que el panel pueda
/// arreglar; lo que este cambio **no** hace es dejar de distinguir un fallo del destino —sigue
/// siendo `Error::DestinoNoAlcanzable`, con su variante tipada, y no otro error—, ni retirar
/// [`crate::engine::ErrorDelMotor::TrabajoInterrumpido`], que sigue cubriendo el pánico del propio
/// hilo de bloqueo del motor.
///
/// # El peor caso, y por qué son dos topes y no uno
///
/// Cada operación lleva **su** tope, y una lectura del panel encadena dos: abrir el repositorio y
/// listarlo. Si el destino responde muy despacio al abrir —sin llegar a agotar el tope— y luego se
/// atasca al listar, el `GET /api/v1/snapshots` puede tardar hasta **2 × [`TOPE_DEL_DESTINO`]**. Es
/// el precio de que cada tramo tenga su propio plazo y de no meter una apertura ya lenta dentro del
/// de un listado que aún no ha empezado: un tope único sobre el conjunto haría que un listado sano
/// heredara la lentitud de la apertura. Se declara aquí porque es una cifra que el operador puede
/// medir.
pub(crate) fn con_tope<T>(
    destino: &str,
    limite: Duration,
    operacion: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error>
where
    T: Send + 'static,
{
    let (emisor, receptor) = mpsc::channel();
    let destino = destino.to_owned();

    // `Builder` y no `thread::spawn`, que **panica** si el SO no puede crear el hilo —un límite de
    // hilos por proceso, memoria agotada—: un tope que se cae con un pánico al ponerse es peor que
    // el problema que viene a resolver, así que el `Err` se devuelve como el error tipado del
    // destino. El nombre del hilo es lo que sale en un volcado de pilas del proceso.
    if let Err(error) = std::thread::Builder::new()
        .name("kesto-tope-del-destino".to_owned())
        // Detachado a propósito: si el tope se agota, este hilo es lo que se abandona.
        .spawn(move || {
            let _ = emisor.send(operacion());
        })
    {
        return Err(Error::DestinoNoAlcanzable {
            destino,
            motivo: format!(
                "Kesto no ha podido abrir un hilo para hablar con el destino ({error}), así que \
                 la operación no llegó a empezar"
            ),
        });
    }

    match receptor.recv_timeout(limite) {
        Ok(desenlace) => desenlace,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::DestinoNoAlcanzable {
            destino,
            motivo: format!(
                "el destino no ha contestado en {limite:?}: Kesto ha agotado su tope de espera y \
                 ha abandonado la operación, así que no se sabe si el repositorio está ni si se \
                 llegó a preparar"
            ),
        }),
        // El emisor se suelta sin mandar nada solo si el hilo se cayó antes —un pánico de dentro
        // de la biblioteca—: eso es un destino del que no se ha sabido nada, no una espera larga,
        // y por eso llega como el mismo error del destino y no como un trabajo interrumpido.
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::DestinoNoAlcanzable {
            destino,
            motivo:
                "el hilo que hablaba con el destino se ha caído antes de contestar, así que no \
                     se sabe si el repositorio está ni si se llegó a preparar"
                    .to_owned(),
        }),
    }
}

/// Construye el destino de S3 a partir de la fila.
///
/// Las claves de las opciones son las de `opendal` —`endpoint`, `region`, `bucket`,
/// `access_key_id`, `secret_access_key`—, y la del esquema es la del **esquema**, no la de
/// la URL del backend. Ver el encabezado del módulo para la medición de `"opendal:s3"`.
///
/// El `retry` va dentro por lo mismo que el `endpoint`: sin declararlo, `rustic_backend` aplica sus
/// cinco reintentos y el destino que no responde multiplica su espera. No es una clave de `opendal`
/// —la lee `rustic_backend`—, y por eso el valor está en [`REINTENTOS_DEL_DESTINO`] y no aquí.
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
        ("retry", REINTENTOS_DEL_DESTINO.to_owned()),
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
/// # El tope no va aquí
///
/// Y es a propósito: este es el camino por el que las pruebas abren un repositorio **local**, y un
/// destino local contesta en el acto —lo que sí tarda, medido, son las 6,4 s de derivación de clave
/// de `init` y `open` en la compilación de pruebas—, así que un tope de diez segundos aquí sería un
/// margen de uno y medio sobre algo que no es el problema. El tope de producción lo pone
/// [`Repositorio::poner_en_marcha`], que es la boca que habla con S3.
///
/// # Visibilidad
///
/// **`pub(crate)`** y **solo en las pruebas** (`#[cfg(test)]`): la prueba del motor necesita abrir
/// un repositorio **local** —un directorio del sistema de ficheros en vez de un cubo de S3— para
/// poder recorrer el ciclo de vida completo sin red, sin credenciales y sin motor de contenedores,
/// y este es el único punto por el que se le puede dar un backend que no sea el de S3. Lo que no es,
/// es `pub`: el destino local que recibe es un gancho de prueba, no un modo de funcionamiento, y la
/// API pública construye siempre el de S3. En un binario de producción nadie la usa —de ahí el
/// `cfg`—, porque el camino de verdad es [`Repositorio::poner_en_marcha`] con [`abrir_con`] dentro.
///
/// # Errors
///
/// Las tres del desenlace y las de la construcción del propio repositorio. Ver
/// [`Repositorio::poner_en_marcha`].
#[cfg(test)]
pub(crate) fn poner_en_marcha_con(
    acceso: &Acceso,
    backends: RepositoryBackends,
    barras: BarrasDelMotor,
) -> Result<(Repositorio, PuestaEnMarcha), Error> {
    acceso.validar()?;

    abrir_con(
        Credentials::Password(acceso.passphrase.expose().to_owned()),
        acceso.nombre_del_destino(),
        backends,
        barras,
    )
}

/// Pone el repositorio en marcha sobre unos backends ya construidos y un destino **nombrado**.
///
/// Es el cuerpo de [`poner_en_marcha_con`] con lo que hace falta para trabajar suelto en un hilo:
/// las credenciales ya construidas y el nombre del destino, en vez del [`Acceso`] prestado. Existe
/// por el tope —[`con_tope`] necesita un cierre `'static` y el acceso **no se clona** a propósito,
/// para no duplicar sus dos secretos en memoria—, y ahí está la razón de que los dos secretos se
/// conviertan en [`Credentials`] antes de cruzar: lo que viaja al hilo es el material que la
/// biblioteca necesita y no el acceso entero.
fn abrir_con(
    credenciales: Credentials,
    destino: String,
    backends: RepositoryBackends,
    barras: BarrasDelMotor,
) -> Result<(Repositorio, PuestaEnMarcha), Error> {
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

/// El hecho que el destino devuelve cuando se le pregunta por la salud de un repositorio.
///
/// Es lo que sale de **mirar** el destino sin inicializarlo, y son dos desenlaces y no los tres de
/// [`PuestaEnMarcha`] a propósito: aquí no hay `.init()`, así que «no había repositorio» no se
/// arregla creando uno —se dice y se deja el destino como estaba—. El tercer desenlace de la
/// puesta en marcha, «existía y no abre», tampoco es un valor de este tipo: es un fallo, y viaja
/// como [`Error::PassphraseNoAbre`].
///
/// **No es la salud persistida.** El vocabulario que la columna guarda y que la API sirve
/// —`ok`, `sin_repositorio`, `passphrase`, `inaccesible`, `sin_permiso`— vive en
/// `crate::db::entities::SaludDeRepositorio`, y este módulo **no lo conoce**: traducir estos hechos
/// y los errores tipados de aquí a aquel vocabulario es trabajo del motor, que es quien tiene la
/// fila del repositorio. Aquí solo se mide; allí se decide qué estado es.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoQueContestaElDestino {
    /// El destino responde y **no tiene** repositorio todavía. No se ha creado ninguno.
    SinRepositorio,
    /// El destino responde, el repositorio abre con su passphrase y el listado de instantáneas
    /// ocurre. Es lo único que autoriza a decir «sano».
    AbreYLista,
}

/// Comprueba la salud de un repositorio **sin inicializarlo** y dice qué contestó el destino.
///
/// Es la operación que el panel pide a demanda, y su contrato entero cabe en una frase: **mira el
/// destino, no lo cambia**. Por eso **no** pasa por [`Repositorio::poner_en_marcha`], que cuando el
/// destino no tiene configuración lo **inicializa** —crearía un repositorio vacío donde no lo
/// hay—; aquí se pregunta por la configuración y, si no está, se devuelve
/// [`LoQueContestaElDestino::SinRepositorio`] dejando el destino intacto.
///
/// La comprobación es **de solo lectura y con la profundidad de la puesta en marcha más el
/// listado**: pregunta si hay configuración, abre con la passphrase de la fila y **lista las
/// instantáneas**. Ese último paso es el que distingue «el destino responde» de «el destino es
/// utilizable»: un cubo alcanzable y con la configuración presente pero sin permiso de **lectura**
/// abre y no lista, y dárselo por sano sería prometer una copia que no se podría leer después.
///
/// # El tope del destino
///
/// Igual que la apertura, la comprobación corre bajo [`con_tope`] con [`TOPE_DEL_DESTINO`]: un
/// destino que acepta la conexión y no contesta se abandona en diez segundos en vez de dejar al
/// panel esperando sin fin. Un tope agotado llega como [`Error::DestinoNoAlcanzable`], el mismo
/// error tipado que un destino que rechaza, y por eso se traducen al mismo estado.
///
/// # Los tres desenlaces que son fallo, no valor
///
/// `Ok` solo cubre los dos hechos de [`LoQueContestaElDestino`]; todo lo demás es un [`Error`]:
///
/// - [`Error::AccesoInvalido`] — la fila no describe un destino utilizable (un campo obligatorio
///   vacío). Se decide antes de hablar con nadie.
/// - [`Error::DestinoNoAlcanzable`] — el destino no contesta, lo rechaza, no se puede preparar, o
///   agota el tope.
/// - [`Error::PassphraseNoAbre`] — hay configuración y ninguna clave del repositorio se deja abrir
///   con la passphrase de la fila. Se decide por el código de la biblioteca —`is_incorrect_password`
///   — y no por el texto del mensaje, igual que en [`abrir_con`].
/// - [`Error::ListadoFallido`] — el repositorio abre pero su contenido no se deja listar; cubre
///   tanto la lectura del índice como el listado de instantáneas.
///
/// Quien traduce errores a estados es el motor, no esto: aquí se devuelven los errores tipados tal
/// cual, para que la decisión de qué es «inaccesible» y qué es «sin permiso» viva en un solo sitio.
///
/// # ⚠️ La biblioteca entra en pánico si se la llama desde dentro de un runtime
///
/// La misma advertencia que [`Repositorio::poner_en_marcha`]: con el backend de S3 la biblioteca
/// construye su **propio** runtime. Aquí la llamada que habla con el destino es la del hilo de
/// [`con_tope`], así que por **este** camino el pánico no ocurre; lo que **sí** puede entrar en
/// pánico es [`backends_de_s3`]... que no llama a la biblioteca, solo construye el operador. Quien
/// llama a esto es el motor, que de todos modos saca el trabajo del runtime antes de llegar aquí.
///
/// # Errors
///
/// Las cuatro de arriba.
pub fn salud_del_repositorio(
    acceso: &Acceso,
    barras: BarrasDelMotor,
) -> Result<LoQueContestaElDestino, Error> {
    // Construir el backend no habla con el destino —es lo que hace `OpenDALBackend::new`, sin
    // red—, así que se queda fuera del tope y el acceso no tiene que cruzar al hilo.
    let backends = backends_de_s3(acceso)?;
    let destino = acceso.nombre_del_destino();
    let destino_del_hilo = destino.clone();
    let credenciales = Credentials::Password(acceso.passphrase.expose().to_owned());

    con_tope(&destino, TOPE_DEL_DESTINO, move || {
        medir_la_salud(credenciales, destino_del_hilo, backends, barras)
    })
}

/// El mismo camino de [`salud_del_repositorio`] con unos backends **ya construidos** y sin tope.
///
/// Es el gancho de las pruebas —gemelo de [`poner_en_marcha_con`]—: el motor inyecta una función
/// que lo llama para comprobar la salud de un repositorio **local** sin red ni credenciales. El
/// tope de producción lo pone [`salud_del_repositorio`], que es la boca que habla con S3; un
/// destino local contesta en el acto y el tope aquí sería un margen sobre algo que no es el
/// problema, por la misma razón que en [`poner_en_marcha_con`].
///
/// **`pub(crate)` y solo en las pruebas** (`#[cfg(test)]`): no es un modo de funcionamiento, es un
/// punto de inyección. En un binario de producción nadie lo usa.
///
/// # Errors
///
/// Las mismas que [`salud_del_repositorio`] salvo el tope.
#[cfg(test)]
pub(crate) fn salud_del_repositorio_con(
    acceso: &Acceso,
    backends: RepositoryBackends,
    barras: BarrasDelMotor,
) -> Result<LoQueContestaElDestino, Error> {
    acceso.validar()?;

    medir_la_salud(
        Credentials::Password(acceso.passphrase.expose().to_owned()),
        acceso.nombre_del_destino(),
        backends,
        barras,
    )
}

/// Pregunta al destino por la configuración, abre el repositorio si la hay y lista sus instantáneas.
///
/// Es el cuerpo de [`salud_del_repositorio`] con lo que hace falta para trabajar suelto en un hilo:
/// las credenciales ya construidas y el nombre del destino, en vez del [`Acceso`] prestado. Existe
/// por el tope, como [`abrir_con`].
///
/// Lo que **no** hace, y es la diferencia con [`abrir_con`]: **no inicializa** cuando no hay
/// configuración. Ahí está la mitad de «de solo lectura» que este camino tiene y el de la puesta en
/// marcha no.
///
/// # Errors
///
/// Las cuatro de [`salud_del_repositorio`].
fn medir_la_salud(
    credenciales: Credentials,
    destino: String,
    backends: RepositoryBackends,
    barras: BarrasDelMotor,
) -> Result<LoQueContestaElDestino, Error> {
    let fallo_del_destino = |error: Box<rustic_core::RusticError>| Error::DestinoNoAlcanzable {
        destino: destino.clone(),
        motivo: error.display_log(),
    };

    // La copia del backend **antes** de entregárselo al repositorio: es lo único que queda para
    // preguntarle al destino, porque `RepositoryBackends` se consume.
    let configuracion = backends.repository();
    let repositorio =
        Repository::new_with_progress(&RepositoryOptions::default(), &backends, barras)
            .map_err(fallo_del_destino)?;

    // ¿Hay configuración? Se pregunta **sin inicializar**: `.init()` crearía un repositorio donde
    // no lo hay, que es justo lo que una comprobación no puede hacer.
    if configuracion
        .list(FileType::Config)
        .map_err(fallo_del_destino)?
        .is_empty()
    {
        return Ok(LoQueContestaElDestino::SinRepositorio);
    }

    // Hay configuración: se abre con la passphrase de la fila. Un fallo que la biblioteca señala
    // como «password incorrecta» —el código C002, el mismo que distingue [`abrir_con`]— es una
    // passphrase que no abre; cualquier otro fallo de apertura es un fallo del destino.
    let abierto = repositorio.open(&credenciales).map_err(|error| {
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
    })?;

    // A partir de aquí el repositorio abre: lo que falle es su **lectura** —el índice primero y las
    // instantáneas después—, y eso es un destino alcanzable pero inutilizable, no uno inalcanzable.
    // Se marca como fallo del listado para que el motor lo distinga de «no se alcanza».
    let fallo_del_listado = |error: Box<rustic_core::RusticError>| Error::ListadoFallido {
        destino: destino.clone(),
        motivo: error.display_log(),
    };

    // El listado, de verdad y hasta el final: `.list()` devuelve un iterador perezoso, así que hay
    // que recorrerlo para que la lectura ocurra. Es lo que separa «el destino responde» de «el
    // destino es utilizable», y por eso no vale con pedirlo y no mirarlo.
    let instantaneas = abierto.to_indexed_ids().map_err(fallo_del_listado)?;
    instantaneas
        .list::<SnapshotId>()
        .map_err(fallo_del_listado)?
        .count();

    Ok(LoQueContestaElDestino::AbreYLista)
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
        acceso_local, backends_locales, barras_mudas, destino_de_s3_que_rechaza,
        destino_de_s3_que_se_traga_la_peticion, directorio_de_prueba, nodo_del_snapshot,
        sembrar_snapshot_ajeno, sembrar_snapshots, snapshot_guardado, volcado_guardado,
    };

    use super::{
        Acceso, Copia, CopiaHecha, ESQUEMA_DE_S3, Error, LoQueContestaElDestino, PuestaEnMarcha,
        Repositorio, TOPE_DEL_DESTINO, backends_de_s3, con_tope, opciones_de_s3,
        poner_en_marcha_con, salud_del_repositorio, salud_del_repositorio_con,
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

    /// **El destino de S3 que no responde da un error limpio y sin secretos, y no un pánico.**
    ///
    /// Es la prueba que cierra el hueco que tenía la suite entera: todas las del motor abren un
    /// repositorio **local** (`rustic_backend::local`, sin `opendal`), así que el camino de **S3**
    /// —el único que construye su propio runtime y por tanto el único que entra en pánico si se le
    /// llama desde dentro de un runtime— no lo ejercitaba ninguna prueba automática, y el CI no lo
    /// habría cazado. Aquí sí: el backend es el de S3 de verdad, contra un destino donde nadie
    /// escucha, y el desenlace tiene que ser un `Err` con su motivo.
    ///
    /// El desenlace está **medido** en las dos direcciones, y es el motivo de que el motor tenga
    /// las dos operaciones que sacan el trabajo del runtime:
    ///
    /// - desde **dentro** de un runtime, la misma llamada entra en pánico —«`Cannot start a runtime
    ///   from within a runtime`», en `opendal-core-0.58.2/src/blocking/operator.rs`—;
    /// - desde un hilo **sin** runtime, que es donde corre una prueba `#[test]` y donde corre el
    ///   hilo del trabajador del motor, devuelve el error de red.
    ///
    /// Por eso esta prueba es `#[test]` y **no** `#[tokio::test]`: lo que fija es el desenlace del
    /// camino de S3 tal y como lo ve quien **no** está dentro de un runtime, que es el único sitio
    /// desde el que se puede llamar. Para el otro lado está `Motor::abrir_repositorio`, que saca el
    /// trabajo del runtime por su cuenta.
    ///
    /// # Los cinco reintentos de la biblioteca, apagados a propósito
    ///
    /// El único cambio sobre el mapa de opciones de producción es `retry = 0`, y está medido por
    /// qué: `rustic_backend` reintenta **cinco** veces con retroceso exponencial y temblor
    /// (`DEFAULT_RETRY = 5`), así que contra un puerto donde nadie escucha tarda **entre 41 y 54
    /// segundos** en decirlo —medido—. Lo que aquí se mide es el **desenlace** y el motivo, no el
    /// reintento, y con `retry = 0` la misma llamada tarda **13 ms**. Ver
    /// `src/test_support.rs::destino_de_s3_que_rechaza` para la otra forma de ejercitarlo sin
    /// pagarlos: un oyente local que contesta `403`.
    #[test]
    fn un_destino_de_s3_inalcanzable_da_un_error_limpio_y_sin_secretos() {
        let acceso = Acceso {
            endpoint: "http://127.0.0.1:9".to_owned(),
            bucket: "cubo-al-que-nadie-responde".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(OTRA_PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let mut opciones = opciones_de_s3(&acceso).expect("la fila describe un destino utilizable");
        assert_eq!(
            opciones.get("secret_access_key").map(String::as_str),
            Some(CLAVE_SECRETA_DE_PRUEBA),
            "el mapa tiene que llevar la clave secreta en claro —la dependencia la necesita así—, \
             o la aserción de abajo no mediría lo que dice medir"
        );
        opciones.insert("retry".to_owned(), "0".to_owned());

        let backend: std::sync::Arc<dyn rustic_core::WriteBackend> = std::sync::Arc::new(
            OpenDALBackend::new(ESQUEMA_DE_S3, opciones)
                .expect("el esquema de S3 construye el backend sin red"),
        );
        let backends = rustic_core::RepositoryBackends::new(backend, None);

        let error = poner_en_marcha_con(&acceso, backends, barras_mudas())
            .expect_err("un destino que no responde no puede poner nada en marcha");

        // El desenlace tiene que ser el del **destino inalcanzable**, no el de una passphrase que
        // no abre: son dos cosas que el operador arregla de forma distinta, y una prueba que
        // aceptara cualquiera de las dos no mediría ninguna.
        let (destino, motivo) = match error {
            Error::DestinoNoAlcanzable { destino, motivo } => (destino, motivo),
            otro => panic!(
                "un destino que no se alcanza tiene que llegar como `DestinoNoAlcanzable` y llegó \
                 como {otro:?}: confundirlo con una passphrase que no abre manda al operador a \
                 revisar lo que no es"
            ),
        };

        assert!(
            destino.contains(&acceso.endpoint) && destino.contains(&acceso.bucket),
            "el motivo no nombra el sitio al que no se llegó: {destino}"
        );
        assert!(
            motivo.contains(&acceso.bucket),
            "el motivo no nombra el cubo: {motivo}"
        );
        assert!(
            !motivo.contains(CLAVE_SECRETA_DE_PRUEBA),
            "el motivo de un fallo de S3 publica la clave secreta: {motivo}"
        );
        assert!(
            !motivo.contains(OTRA_PASSPHRASE_DE_PRUEBA),
            "el motivo de un fallo de S3 publica la passphrase: {motivo}"
        );
    }

    /// **#100 — Un destino que no responde agota el tope, no la paciencia del operador.**
    ///
    /// Es el defecto medido y esta es su medición, por la **boca de producción**: contra un puerto
    /// donde nadie escucha —`http://127.0.0.1:9`, el mismo destino del caso reportado— el listado
    /// del panel tardaba **36,9–38,4 s** y acababa en `502`, porque `rustic_backend` reintenta
    /// cinco veces con retroceso exponencial (`DEFAULT_RETRY = 5`) y cada reintento suma su
    /// espera. La llamada de aquí es la de verdad —[`Repositorio::poner_en_marcha`], con el mapa
    /// de opciones de producción tal cual, sin `retry = 0` de prueba— y lo que se mide es que el
    /// desenlace llegue **acotado**: el tope de Kesto son diez segundos y el reintento que Kesto
    /// declara está acotado, así que quince segundos de margen solo se pasan si el tope no está
    /// —con el defecto, esta misma llamada tarda los 38,4 s del informe—.
    ///
    /// El desenlace tiene que ser [`Error::DestinoNoAlcanzable`] y no otra cosa: es el error
    /// tipado que el borde ya traduce a `502`, así que el contrato HTTP no cambia por esto.
    ///
    /// **Lo que esta prueba no demuestra**, y por eso está la siguiente: con el `retry` acotado, un
    /// puerto cerrado contesta en torno al segundo, así que pasaría aunque se quitara [`con_tope`]
    /// de [`Repositorio::poner_en_marcha`]. Lo que fija es el desenlace y el rumbo del reloj, no que
    /// el tope sea lo que corta: eso lo mide
    /// [`un_destino_que_se_traga_la_peticion_lo_corta_el_tope_y_no_la_biblioteca`].
    #[test]
    fn un_destino_que_no_responde_agota_el_tope_y_no_la_paciencia_del_operador() {
        let acceso = Acceso {
            endpoint: "http://127.0.0.1:9".to_owned(),
            bucket: "cubo-que-no-responde".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(OTRA_PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let empezado = std::time::Instant::now();
        let error = Repositorio::poner_en_marcha(&acceso, barras_mudas())
            .expect_err("un destino que no responde no pone nada en marcha");
        let tardado = empezado.elapsed();

        assert!(
            tardado < Duration::from_secs(15),
            "el tope del destino no acotó la espera: tardó {tardado:?} contra un destino que no \
             responde, y el caso del informe tardaba 38,4 s sin tope"
        );

        assert!(
            matches!(error, Error::DestinoNoAlcanzable { .. }),
            "un tope agotado tiene que llegar como el destino inalcanzable —que es el error que el \
             borde ya traduce a `502`— y llegó como {error:?}"
        );
    }

    /// **Un tope agotado abandona la operación y no espera a que conteste.**
    ///
    /// Es la mitad que la prueba anterior **no** puede medir sola: contra el puerto muerto el tope
    /// casi nunca llega a agotarse —el reintento acotado contesta antes—, así que lo que aquí se
    /// mide es el tope **en sí**, con una operación que tarda cinco segundos y un tope de
    /// cincuenta milisegundos. Lo que se afirma es que la espera termina cuando se agota el tope
    /// —no cuando termina la operación— y que el desenlace es el error tipado del destino, con el
    /// destino nombrado en su motivo.
    #[test]
    fn un_tope_agotado_abandona_la_operacion_y_no_espera_a_que_conteste() {
        let empezado = std::time::Instant::now();
        let desenlace = con_tope(
            "s3://cubo-que-no-contesta",
            Duration::from_millis(50),
            || {
                std::thread::sleep(Duration::from_secs(5));
                Ok(())
            },
        );
        let tardado = empezado.elapsed();

        let (destino, motivo) = match desenlace {
            Err(Error::DestinoNoAlcanzable { destino, motivo }) => (destino, motivo),
            otro => panic!(
                "un tope agotado tiene que llegar como `DestinoNoAlcanzable` y llegó como {otro:?}: \
                 el operador tiene que poder distinguirlo de una passphrase que no abre"
            ),
        };

        assert!(
            tardado < Duration::from_secs(2),
            "la espera terminó cuando terminó la operación y no cuando se agotó el tope: tardó \
             {tardado:?} con un tope de 50ms"
        );
        assert!(
            destino.contains("cubo-que-no-contesta"),
            "el motivo no nombra el destino al que no se llegó: {destino}"
        );
        assert!(
            motivo.contains("50ms"),
            "el motivo no dice cuál fue el tope que se agotó: {motivo}"
        );
    }

    /// **El tope es lo que termina la espera contra un destino que se traga la petición.**
    ///
    /// Es la prueba **discriminante** del tope, y la que el caso del puerto cerrado no puede ser:
    /// contra un puerto donde nadie escucha, un `retry` de uno contesta en torno al segundo y el
    /// desenlace es el mismo con tope y sin él, así que
    /// [`un_destino_que_no_responde_agota_el_tope_y_no_la_paciencia_del_operador`] pasaría aunque se
    /// quitara [`con_tope`] de [`Repositorio::poner_en_marcha`]. Aquí el oyente **acepta** la
    /// conexión y no contesta: `reqwest`, por debajo de `opendal`, no trae plazo propio, así que sin
    /// tope la llamada no vuelve —y el oyente cierra sin respuesta a los quince segundos, para que
    /// una regresión sea un fallo medido y no un cuelgue—, y lo que se afirma es que con el tope la
    /// espera termina dentro del plazo y en [`Error::DestinoNoAlcanzable`], con el motivo del tope y
    /// no el de la biblioteca.
    #[test]
    fn un_destino_que_se_traga_la_peticion_lo_corta_el_tope_y_no_la_biblioteca() {
        let acceso = Acceso {
            endpoint: destino_de_s3_que_se_traga_la_peticion(),
            bucket: "cubo-que-se-traga-la-peticion".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(OTRA_PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let empezado = std::time::Instant::now();
        let error = Repositorio::poner_en_marcha(&acceso, barras_mudas())
            .expect_err("un destino que no contesta no pone nada en marcha");
        let tardado = empezado.elapsed();

        // Doce segundos: el tope son diez, y el margen es para el arranque del hilo y la carrera del
        // planificador —sin tope, la primera respuesta del oyente no llega hasta los quince—.
        assert!(
            tardado < Duration::from_secs(12),
            "el tope no cortó la espera: tardó {tardado:?} contra un destino que acepta y no \
             contesta, con un tope declarado de {TOPE_DEL_DESTINO:?}"
        );

        let (destino, motivo) = match error {
            Error::DestinoNoAlcanzable { destino, motivo } => (destino, motivo),
            otro => panic!(
                "el tope agotado tiene que llegar como `DestinoNoAlcanzable` y llegó como {otro:?}"
            ),
        };

        // El motivo es el del **tope** y no el de la biblioteca: sin tope, la respuesta llegaría a
        // los quince segundos con el texto de `opendal`, y lo que se mide aquí es que la espera la
        // cortó Kesto.
        assert!(
            motivo.contains("tope"),
            "el desenlace tiene que ser el del tope de Kesto y no el de la biblioteca: {motivo}"
        );
        assert!(
            destino.contains("cubo-que-se-traga-la-peticion"),
            "el motivo no nombra el destino al que no se llegó: {destino}"
        );
    }

    /// **Las opciones del destino acotan el reintento de la biblioteca.**
    ///
    /// El mapa que devuelve [`opciones_de_s3`] es el contrato que viaja hasta la dependencia, y el
    /// `retry` va dentro por lo mismo que `endpoint` o `bucket`: sin declararlo, `rustic_backend`
    /// aplica su `DEFAULT_RETRY = 5` —medido, 36,9–38,4 s de retroceso exponencial contra un
    /// destino que no responde—, y el reintento existe para sobrevivir a un fallo pasajero, no para
    /// multiplicar por cinco la espera de un operador. Se afirma el valor **literal** a propósito:
    /// cambiarlo es cambiar cuánto puede tardar el panel, y eso se decide aquí, no en un `use`.
    #[test]
    fn las_opciones_del_destino_acotan_el_reintento_de_la_biblioteca() {
        let acceso = Acceso {
            endpoint: "http://127.0.0.1:9".to_owned(),
            bucket: "cubo-que-no-responde".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(OTRA_PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let opciones = opciones_de_s3(&acceso).expect("la fila describe un destino utilizable");

        assert_eq!(
            opciones.get("retry").map(String::as_str),
            Some("1"),
            "el mapa tiene que declarar el reintento acotado —uno—, o la biblioteca aplica sus \
             cinco reintentos de retroceso exponencial"
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

    /// **24.** `olvidar_instantanea` retira del repositorio la instantánea que se le nombra.
    ///
    /// Es lo que el motor usa para no dejar residuo cuando una copia no se da por buena: la
    /// instantánea la escribió el archivado antes de que el motor pudiera decidir, así que hay
    /// que poder retirarla por su identificador en hex completo —el de
    /// [`CopiaHecha`](super::CopiaHecha)—. La prueba archiva de verdad, comprueba que está, la
    /// olvida y comprueba que ya no está **listando el repositorio**, que es la única forma de
    /// saber que se ha ido y no solo que el olvido no dio error.
    #[test]
    fn olvidar_instantanea_retira_la_instantanea_del_repositorio() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);

        let hecho = archivar(
            &repositorio,
            &copia,
            b"-- tienda\nCREATE TABLE tienda (id INT);\n",
        )
        .expect("archivar no puede fallar");

        let antes = repositorio
            .listar_snapshots(100)
            .expect("el repositorio se puede listar");
        assert_eq!(
            antes.len(),
            1,
            "el destino recién inicializado tenía que tener una sola instantánea y dice {}",
            antes.len()
        );

        repositorio
            .olvidar_instantanea(&hecho.id)
            .expect("retirar la instantánea recién escrita no puede fallar");

        let despues = repositorio
            .listar_snapshots(100)
            .expect("el repositorio se puede listar");
        assert!(
            despues.iter().all(|resumen| resumen.id != hecho.id),
            "la instantánea `{}` sigue en el repositorio después de olvidarla",
            hecho.id
        );
        assert_eq!(
            despues.len(),
            0,
            "el repositorio tenía una sola instantánea y tras olvidarla dice {}",
            despues.len()
        );

        drop(repositorio);
    }

    /// **25.** `olvidar_instantanea` con un identificador que no es un hex válido devuelve un
    /// error del módulo y **no** entra en pánico.
    ///
    /// El identificador viaja como cadena desde la fila de la ejecución, así que un valor vacío o
    /// mal formado tiene que salir como [`Error::OlvidoFallido`] —nunca como un pánico dentro de
    /// la biblioteca, que se llevaría por delante al hilo del trabajador—. Se miden el vacío y el
    /// que no es hex.
    #[test]
    fn olvidar_instantanea_con_un_identificador_invalido_devuelve_error_sin_panico() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");

        for id in ["", "no-es-un-hexadecimal"] {
            let fallo = repositorio
                .olvidar_instantanea(id)
                .expect_err("un identificador que no es un hex completo no puede olvidarse");

            assert!(
                matches!(fallo, Error::OlvidoFallido { .. }),
                "el fallo de olvidar un identificador inválido no es `OlvidoFallido`: {fallo:?}"
            );
        }

        drop(repositorio);
    }

    /// **26.** `tiene_la_instantanea` distingue la que está de la que no, **sin restaurar nada**.
    ///
    /// Es la lectura que sostiene el `404` de la restauración, y lo que se mide son sus tres
    /// desenlaces: la instantánea sembrada está, un identificador bien formado que no corresponde a
    /// ninguna no está, y una cadena que no llega a ser un hex completo tampoco —y **sin** error—,
    /// porque lo que se pregunta es si esa copia está y una cadena que no puede ser el identificador
    /// de ninguna no está.
    ///
    /// El identificador sembrado se toma en **hex completo** —64 caracteres— y se usa tal cual: es
    /// el mismo valor que viaja en la URL y en la fila de la ejecución, así que si la comprobación
    /// exigiera la forma corta del `Display` de `SnapshotId` —ocho caracteres, prefijo ambiguo—
    /// esta prueba lo diría.
    ///
    /// Se pregunta **una sola vez** por cada desenlace sobre el mismo repositorio ya abierto: abrir
    /// un repositorio cuesta segundos en la compilación de pruebas —derivar su clave— y este tipo es
    /// de un solo uso práctico, así que no se reabre para volver a mirar.
    #[test]
    fn tiene_la_instantanea_distingue_la_que_esta_de_la_que_no() {
        let destino = Destino::nuevo();
        let siembra = sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );
        let sembrada = siembra.identificadores[0].clone();

        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino sembrado se abre");

        // Bien formado y ausente: 64 caracteres hexadecimales que no son el resumen de nada. Se
        // afirma antes su forma para que el `false` de abajo no lo pueda dar un identificador mal
        // formado, que es el otro caso y tiene su propia comprobación.
        let ausente = "f".repeat(64);

        assert_eq!(
            sembrada.len(),
            ausente.len(),
            "los dos identificadores tienen que tener la misma forma, y el sembrado mide {}",
            sembrada.len()
        );

        assert!(
            repositorio
                .tiene_la_instantanea(&sembrada)
                .expect("preguntar por una instantánea con el repositorio abierto no puede fallar"),
            "la instantánea sembrada está en el repositorio y la comprobación dijo que no: `{sembrada}`"
        );

        assert!(
            !repositorio
                .tiene_la_instantanea(&ausente)
                .expect("preguntar por una instantánea que no está no es un fallo"),
            "un identificador bien formado que no es el resumen de ninguna instantánea dice estar: \
             `{ausente}`"
        );

        assert!(
            !repositorio
                .tiene_la_instantanea("no-es-un-hexadecimal")
                .expect("una cadena que no puede ser un identificador no es un fallo: no está"),
            "una cadena que no llega a ser un hex completo no puede ser la de ninguna instantánea"
        );

        drop(repositorio);
    }

    // ── Restaurar el árbol de un directorio ──────────────────────────────────

    /// **Cuál es el prefijo que se descuenta, cuándo no hay ninguno y cuándo es un fallo.**
    ///
    /// Es la decisión de [`super::ruta_declarada`] medida sola, sin repositorio: el prefijo es la
    /// ruta que la instantánea declara, y solo cuando declara **una**. Las instantáneas se montan a
    /// mano porque lo que se mide es la función, no una copia: construir un repositorio para esto
    /// costaría una derivación de clave por caso y no añadiría nada a lo que se afirma.
    ///
    /// Los tres casos se afirman por separado porque **cero** y **dos** no son el mismo: sin rutas
    /// no hay ningún prefijo declarado que descontar —`Ok(None)`, el árbol ya es plano y el
    /// recorrido empieza en la raíz sintética—, mientras que con dos no hay uno **único** —elegir
    /// uno dejaría fuera del destino el contenido de la otra—, así que es un `Err` con el motivo y
    /// **no** un `None`: devolver `None` aquí volvería a anidar el árbol en silencio, con la
    /// ejecución acabando en `success`.
    #[test]
    fn ruta_declarada_es_la_unica_ruta_que_la_instantanea_declara() {
        let instantanea = || {
            SnapshotOptions::default()
                .to_snapshot()
                .expect("una instantánea vacía se construye")
        };

        let sin_rutas = instantanea();
        assert_eq!(
            super::ruta_declarada(&sin_rutas),
            Ok(None),
            "una instantánea que no declara ninguna ruta no tiene prefijo que descontar"
        );

        let con_una = {
            let mut copia = instantanea();
            copia.paths.add("/backup_sources/tienda".to_owned());
            copia
        };
        assert_eq!(
            super::ruta_declarada(&con_una),
            Ok(Some("/backup_sources/tienda")),
            "con una sola ruta declarada, el prefijo es esa ruta tal cual"
        );

        let con_dos = {
            let mut copia = con_una.clone();
            copia.paths.add("/backup_sources/otra".to_owned());
            copia
        };
        let motivo = super::ruta_declarada(&con_dos)
            .expect_err("con dos rutas declaradas no hay un prefijo único que descontar");
        assert!(
            motivo.contains("/backup_sources/tienda") && motivo.contains("/backup_sources/otra"),
            "el motivo tiene que nombrar las rutas declaradas y por qué no se puede elegir una, y \
             dijo `{motivo}`"
        );
    }

    /// **27.** La restauración de un directorio deja su contenido en la **raíz** del destino.
    ///
    /// Es el contrato de la copia leído al revés. La instantánea guarda el contenido del
    /// directorio **con la ruta con la que se montó, no con la del host**
    /// (`openspec/specs/extraction/spec.md:97-102`), así que el árbol cuelga de
    /// `backup_sources/x` —de `/tmp/…/origen`, en una prueba— y no de la raíz. Y la restauración
    /// de un target `directory` escribe al `host_path` con `LocalDestination`
    /// (`openspec/specs/engine/spec.md:291`): lo que había en la raíz del origen tiene que quedar
    /// en la raíz del destino.
    ///
    /// Sin descontar ese prefijo, restaurar `/tmp/x/origen` recrea la ruta absoluta **dentro** del
    /// destino —`<destino>/tmp/x/origen/a.txt`—, que es lo que midió la batería de punta a punta y
    /// lo que esta prueba mide sin contenedores ni S3: la ruta del origen se sortea con un UUID,
    /// así que el anidado no se puede confundir con el contenido restaurado.
    ///
    /// Se afirman las dos mitades del contrato y en el orden en que ocurren: que la instantánea
    /// **declara** la ruta montada —de donde sale el prefijo— y que la restauración la descuenta.
    #[test]
    fn la_restauracion_deja_el_contenido_del_directorio_en_la_raiz_del_destino() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");

        let fuente = directorio_de_prueba();
        std::fs::create_dir_all(fuente.ruta().join("sub/anidado"))
            .expect("el árbol de la fuente se crea");
        std::fs::write(fuente.ruta().join("a.txt"), b"contenido de a\n")
            .expect("el fichero de la raíz de la fuente se escribe");
        std::fs::write(fuente.ruta().join("sub/b.bin"), [0u8, 1, 2, 255])
            .expect("el fichero anidado de la fuente se escribe");

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
        drop(repositorio);

        // La primera mitad: la instantánea declara la ruta con la que se montó, que es de donde
        // sale el prefijo que la restauración tiene que descontar.
        let guardado = snapshot_guardado(&destino.ruta, PASSPHRASE_DE_PRUEBA, &hecho.id);
        assert_eq!(
            guardado.paths.iter().cloned().collect::<Vec<String>>(),
            vec![fuente.ruta().to_string_lossy().into_owned()],
            "la instantánea tiene que declarar la ruta con la que se montó el directorio"
        );

        // Y la segunda: la restauración, a un destino **vacío**. El repositorio se vuelve a abrir
        // porque restaurar lo consume —pasa al índice completo—, que es lo que el motor hace en
        // cada trabajo.
        let restaurado = directorio_de_prueba();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el repositorio se vuelve a abrir");
        repositorio
            .restaurar_snapshot(&hecho.id, &restaurado.ruta().to_string_lossy(), false)
            .expect("restaurar una instantánea que está en el repositorio no puede fallar");

        assert_eq!(
            std::fs::read(restaurado.ruta().join("a.txt"))
                .expect("el fichero de la raíz del origen tiene que estar en la raíz del destino"),
            b"contenido de a\n".to_vec(),
            "el fichero de la raíz del origen no quedó en la raíz del destino"
        );
        assert_eq!(
            std::fs::read(restaurado.ruta().join("sub/b.bin"))
                .expect("el fichero anidado del origen tiene que estar en el mismo sitio"),
            vec![0u8, 1, 2, 255],
            "el fichero anidado del origen no quedó en el mismo sitio del destino"
        );

        let anidada = restaurado.ruta().join(
            fuente
                .ruta()
                .strip_prefix("/")
                .expect("la ruta de la fuente de la prueba es absoluta"),
        );
        assert!(
            !anidada.exists(),
            "la restauración recreó la ruta absoluta del origen dentro del destino (`{}`): el \
             prefijo con el que se montó el directorio no se descontó",
            anidada.display()
        );
    }

    /// **28.** La poda del destino la decide la confirmación de sobrescritura, y sin ella no se
    /// retira nada.
    ///
    /// Es la otra mitad de la restauración de un directorio: lo que había en el destino y **no**
    /// está en la instantánea. Con la poda activada el destino queda **igual a la instantánea** —el
    /// fichero que sobra desaparece, y el **directorio** que sobra también, porque `remove_dir_all`
    /// es recursivo—, y sin ella se queda donde está, que es la conducta conservadora por defecto:
    /// podar borra del host datos que no tienen copia, así que lo hace quien **confirma** la
    /// sobrescritura
    /// (`openspec/changes/bateria-e2e-de-copia-y-restauracion/specs/verificacion-e2e/spec.md`,
    /// «Sobrescritura de un destino con contenido»).
    ///
    /// Se siembran los **dos** tipos de resto a la vez —un fichero suelto y un directorio no vacío—
    /// porque no se retiran por el mismo camino, y se restaura la **misma** instantánea dos veces,
    /// primero sin poda y luego con ella, y por eso lo que sobra se vuelve a sembrar entre las dos:
    /// si la primera pasada lo hubiera retirado, la segunda no probaría nada —no habría nada que
    /// podar— y la prueba pasaría por no medir. El repositorio se reabre entre las dos porque
    /// restaurar lo consume.
    #[test]
    fn la_poda_del_destino_la_decide_la_confirmacion_de_sobrescritura() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");

        let fuente = directorio_de_prueba();
        std::fs::write(fuente.ruta().join("a.txt"), b"contenido de a\n")
            .expect("el fichero de la fuente se escribe");

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
        drop(repositorio);

        let restaurado = directorio_de_prueba();
        let obsoleto = restaurado.ruta().join("obsoleto.txt");
        // Un **directorio** que sobra, además del fichero: la poda lo retira porque `remove_dir_all`
        // es recursivo, y con un fichero suelto ese camino no se medía. No está vacío —lleva un
        // fichero dentro, en un subdirectorio—, que es lo que hace que la poda tenga que ser
        // recursiva y no un `remove` a secas.
        let sobrante = restaurado.ruta().join("sobrante");
        let dentro_del_sobrante = sobrante.join("anidado").join("sobrante.bin");
        let sembrar_lo_que_sobra = || {
            std::fs::write(&obsoleto, b"no esta en la instantanea\n")
                .expect("el fichero que sobra se escribe en el destino");
            std::fs::create_dir_all(sobrante.join("anidado"))
                .expect("el directorio que sobra se crea en el destino");
            std::fs::write(&dentro_del_sobrante, b"tampoco esta en la instantanea\n")
                .expect("el fichero del directorio que sobra se escribe en el destino");
        };

        // Sin confirmación: el destino solo recibe, y lo que sobra se queda.
        sembrar_lo_que_sobra();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el repositorio se vuelve a abrir");
        repositorio
            .restaurar_snapshot(&hecho.id, &restaurado.ruta().to_string_lossy(), false)
            .expect("restaurar sin poda no puede fallar");

        assert!(
            obsoleto.exists(),
            "sin la confirmación de sobrescritura el destino no se poda, y `obsoleto.txt` \
             desapareció de `{}`",
            restaurado.ruta().display()
        );
        assert!(
            dentro_del_sobrante.exists(),
            "sin la confirmación de sobrescritura el directorio que sobra tampoco se toca, y \
             `{}` desapareció de `{}`",
            dentro_del_sobrante.display(),
            restaurado.ruta().display()
        );

        // Y con ella: el destino queda igual a la instantánea, sin nada de lo previo.
        sembrar_lo_que_sobra();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el repositorio se vuelve a abrir");
        repositorio
            .restaurar_snapshot(&hecho.id, &restaurado.ruta().to_string_lossy(), true)
            .expect("restaurar con poda no puede fallar");

        assert!(
            !obsoleto.exists(),
            "con la confirmación de sobrescritura el contenido previo se sustituye por el del \
             snapshot, y `obsoleto.txt` sigue en `{}`",
            restaurado.ruta().display()
        );
        assert!(
            !sobrante.exists(),
            "con la confirmación de sobrescritura el contenido previo se sustituye por el del \
             snapshot, y el directorio que sobra sigue en `{}`",
            restaurado.ruta().display()
        );
        assert_eq!(
            std::fs::read(restaurado.ruta().join("a.txt"))
                .expect("la poda no puede llevarse por delante lo que sí está en la instantánea"),
            b"contenido de a\n".to_vec(),
            "el contenido del snapshot no quedó en el destino después de podar"
        );
    }

    // ── Volcar el volcado al escritor ────────────────────────────────────────

    /// **El volcado sale del repositorio por donde entró.**
    ///
    /// Es el primer tramo de la restauración de base: archivar un volcado con el puente y luego
    /// volverlo a sacar por `volcar_a`, al búfer, tiene que devolver los mismos bytes. Se
    /// reaprovecha el camino de la copia —que ya está probado— para no inventar una instantánea a
    /// mano, y el escritor es un `Vec<u8>`, que es lo que permite comprobarlo byte a byte.
    #[test]
    fn volcar_a_devuelve_los_bytes_del_volcado_archivado() {
        let destino = Destino::nuevo();
        let (repositorio, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el destino se inicializa");
        let copia = copia_de("volcado-de-tienda.sql", ETIQUETA);
        let contenido =
            b"-- tienda\nCREATE TABLE tienda (id INT);\nINSERT INTO tienda VALUES (1);\n";

        let hecho = archivar(&repositorio, &copia, contenido).expect("archivar no puede fallar");
        drop(repositorio);

        let (otro, _) = destino
            .en_marcha(PASSPHRASE_DE_PRUEBA)
            .expect("el repositorio se vuelve a abrir");
        let mut salida = Vec::new();
        otro.volcar_a(&hecho.id, &mut salida)
            .expect("el volcado de la instantánea se vuelca al búfer");

        assert_eq!(
            salida, contenido,
            "el volcado que sale del repositorio no es el que entró"
        );
    }

    /// **Una instantánea sin un único volcado no se vuelca, y el motivo lo dice.**
    ///
    /// Los dos extremos del requisito —cero ficheros y dos— se miden con instantáneas de verdad,
    /// archivando directorios por el camino de directorios: una vacía —cero nodos de fichero— y
    /// otra con dos ficheros. En los dos casos tiene que salir [`Error::VolcadoQueNoEsUno`] y
    /// **nada** puede haberse escrito en el destino: una restauración no puede empezar con el
    /// volcado equivocado.
    #[test]
    fn volcar_a_rechaza_una_instantanea_que_no_trae_un_unico_volcado() {
        for (nombre, ficheros) in [
            ("sin-ficheros", &[] as &[&str]),
            ("dos-ficheros", &["uno", "dos"]),
        ] {
            let destino = Destino::nuevo();
            let (repositorio, _) = destino
                .en_marcha(PASSPHRASE_DE_PRUEBA)
                .expect("el destino se inicializa");

            let fuente = directorio_de_prueba();
            for fichero in ficheros {
                std::fs::write(fuente.ruta().join(fichero), b"contenido")
                    .expect("el fichero de la prueba se escribe");
            }

            let copia = Copia {
                rutas: vec![fuente.ruta().to_path_buf()],
                nombre: "no-es-un-volcado".to_owned(),
                etiqueta: ETIQUETA.to_owned(),
                host: "host-de-prueba".to_owned(),
                comando: COMANDO.to_owned(),
            };

            let hecho = repositorio
                .archivar_directorio(&copia)
                .expect("el directorio de la prueba se archiva");
            drop(repositorio);

            let (otro, _) = destino
                .en_marcha(PASSPHRASE_DE_PRUEBA)
                .expect("el repositorio se vuelve a abrir");
            let mut salida = Vec::new();
            let error = otro
                .volcar_a(&hecho.id, &mut salida)
                .expect_err("una instantánea que no es de un volcado no se puede volcar");

            assert!(
                matches!(
                    &error,
                    Error::VolcadoQueNoEsUno {
                        instantanea,
                        encontrados,
                        ..
                    } if instantanea == &hecho.id && *encontrados == ficheros.len()
                ),
                "para `{nombre}` el fallo no nombra la instantánea y cuenta lo que hay: {error:?}"
            );
            assert!(
                salida.is_empty(),
                "para `{nombre}` se escribió algo en el destino antes de rechazar el volcado"
            );
        }
    }

    // ── La comprobación de salud: de solo lectura ────────────────────────────
    //
    // Cubre `repository`, requisito «La comprobación de salud es una lectura y no crea ni
    // inicializa el repositorio», con sus cinco escenarios. Los instrumentos son el gancho local
    // del arnés —un destino del sistema de ficheros— para los desenlaces que dependen del
    // contenido, y los dos destinos de S3 simulados del arnés —un oyente que rechaza y otro que
    // se traga la petición— para los dos que dependen de que nadie conteste, que es lo que
    // permite recorrer el camino de S3 sin red y sin contenedores.

    /// Un destino con un repositorio y una instantánea está **sano**, y se ha listado.
    #[test]
    fn un_destino_que_responde_abre_y_lista_esta_sano() {
        let destino = Destino::nuevo();
        sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let desenlace = salud_del_repositorio_con(
            &destino.acceso(PASSPHRASE_DE_PRUEBA),
            destino.backends(),
            barras_mudas(),
        )
        .expect("un destino sano no puede devolver error");

        assert_eq!(
            desenlace,
            LoQueContestaElDestino::AbreYLista,
            "un destino con repositorio y una instantánea tiene que abrir y listar; salió \
             {desenlace:?}"
        );
    }

    /// Un destino sin configuración **no se inicializa** al comprobarlo.
    ///
    /// Es el escenario que distingue esta comprobación de la puesta en marcha: si se reutilizara
    /// `poner_en_marcha`, el destino quedaría con un repositorio vacío recién creado.
    #[test]
    fn la_comprobacion_no_inicializa_un_destino_sin_configuracion() {
        let destino = Destino::nuevo();
        std::fs::create_dir_all(&destino.ruta).expect("el destino vacío de la prueba se crea");

        let desenlace = salud_del_repositorio_con(
            &destino.acceso(PASSPHRASE_DE_PRUEBA),
            destino.backends(),
            barras_mudas(),
        )
        .expect("un destino alcanzable y vacío no es un error");

        assert_eq!(
            desenlace,
            LoQueContestaElDestino::SinRepositorio,
            "un destino sin configuración tiene que decir que no hay repositorio"
        );
        assert!(
            !destino.tiene_repositorio(),
            "la comprobación inicializó el destino: hay configuración donde no la había"
        );
        assert!(
            destino.instantanea().is_empty(),
            "la comprobación escribió algo en un destino que estaba vacío: {:?}",
            destino.instantanea()
        );
    }

    /// Una passphrase que no abre se distingue de un destino inalcanzable.
    #[test]
    fn una_passphrase_que_no_abre_es_un_fallo_de_la_passphrase() {
        let destino = Destino::nuevo();
        sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        let error = salud_del_repositorio_con(
            &destino.acceso(OTRA_PASSPHRASE_DE_PRUEBA),
            destino.backends(),
            barras_mudas(),
        )
        .expect_err("con la passphrase equivocada el repositorio no abre");

        assert!(
            matches!(error, Error::PassphraseNoAbre { .. }),
            "la comprobación tiene que distinguir «la passphrase no abre» de «no se alcanza»: \
             salió {error:?}"
        );
    }

    /// Un nombre de fichero de índice que la caché de la biblioteca **no** conoce, a partir de uno
    /// que sí.
    ///
    /// Los nombres de los ficheros de índice son identificadores: 64 dígitos hexadecimales. Cambiar
    /// el primero por otro distinto da otro identificador **válido** —`Id` se construye decodificando
    /// el nombre en 32 bytes, y cualquier hex de 64 dígitos vale— y distinto del de siempre, que es
    /// justo lo que hace falta para que una lectura no pueda servirse de la copia cacheada.
    fn nombre_de_indice_fresco(nombre: &str) -> String {
        let mut fresco: Vec<char> = nombre.chars().collect();
        if let Some(primero) = fresco.first_mut() {
            *primero = if *primero == '0' { '1' } else { '0' };
        }
        fresco.into_iter().collect()
    }

    /// Un repositorio que **abre pero no lista** no se da por sano.
    ///
    /// Cómo, sin red: se deja el destino con su configuración y sus claves legibles —para que
    /// `open()` funcione— y se ponen en modo `000` los **ficheros** del contenido (`index/`,
    /// `snapshots/`, `data/`), que es lo que hay que **leer** para listar. Es el desenlace que un
    /// cubo sin permiso de lectura produce, y hacen falta dos cuidados para reproducirlo, porque sin
    /// ninguno de los dos sale «sano»:
    ///
    /// - **El modo `000` va en los ficheros, no en sus directorios.** El backend local resuelve el
    ///   listado con un `WalkDir` que se **traga** el error de un directorio ilegible y devuelve lo
    ///   que pudo reunir —nada—, así que un directorio en modo `000` da «abre y lista cero
    ///   instantáneas»: el falso «sano» que esta prueba descarta. Un fichero ilegible, en cambio,
    ///   falla al leerse de verdad.
    /// - **El índice, además, con un nombre nuevo** ([`nombre_de_indice_fresco`]). La biblioteca
    ///   cachea en disco los ficheros de índice y de instantánea —son los dos tipos «cacheables»—
    ///   en una caché **indexada por el identificador del fichero**, y sembrar el destino ya deja
    ///   ahí una copia del índice: con el nombre de siempre, la lectura del índice se resolvería en
    ///   la caché, el destino no se llegaría a tocar y el desenlace volvería a ser el falso «sano».
    ///   Con un nombre que la caché no conoce, la lectura no tiene más remedio que ir al destino,
    ///   que es donde el listado tiene que leer.
    ///
    /// El paso que lee es el índice, y es posterior a `open()`, que no lo toca —la configuración y
    /// las claves viven en la raíz y en `keys/`, que se quedan legibles—: así el destino responde,
    /// abre, y el listado falla con el error de permisos. Los permisos se restauran antes de
    /// afirmar nada, para que el `Drop` de la guarda pueda borrar el directorio.
    #[test]
    fn un_repositorio_que_abre_pero_no_lista_no_se_da_por_sano() {
        use std::os::unix::fs::PermissionsExt;

        let destino = Destino::nuevo();
        sembrar_snapshots(
            &destino.ruta,
            PASSPHRASE_DE_PRUEBA,
            &["2026-01-01T10:00:00Z"],
        );

        // Los ficheros de los directorios de contenido, todos menos `keys/`: las claves las lee
        // `open()`, y hacerlas ilegibles llevaría el desenlace a «no se alcanza» en vez de a «abre
        // y no lista».
        let mut permisos_originales = Vec::new();
        let mut hay_indice = false;
        for entrada in std::fs::read_dir(&destino.ruta).expect("el destino se puede recorrer") {
            let directorio = entrada.expect("cada entrada del destino se lee").path();
            if !directorio.is_dir() {
                continue;
            }

            let Some(nombre) = directorio.file_name() else {
                continue;
            };
            if nombre == "keys" {
                continue;
            }
            let es_el_indice = nombre == "index";

            for contenido in std::fs::read_dir(&directorio).expect("el contenido se puede recorrer")
            {
                let mut fichero = contenido.expect("cada entrada del contenido se lee").path();
                if !fichero.is_file() {
                    continue;
                }

                // Al índice, además de dejarlo ilegible, se le da el nombre nuevo que explica la
                // cabecera: es lo que impide que la lectura se resuelva en la caché de la
                // biblioteca sin llegar a tocar el destino.
                if es_el_indice {
                    let de_siempre = fichero
                        .file_name()
                        .expect("cada fichero del índice tiene nombre")
                        .to_string_lossy()
                        .into_owned();
                    let renombrado = fichero.with_file_name(nombre_de_indice_fresco(&de_siempre));
                    std::fs::rename(&fichero, &renombrado)
                        .expect("el fichero del índice se puede renombrar");
                    fichero = renombrado;
                    hay_indice = true;
                }

                let originales = std::fs::metadata(&fichero)
                    .expect("cada fichero del destino tiene metadatos")
                    .permissions();
                let mut sin_permisos = originales.clone();
                sin_permisos.set_mode(0o000);
                std::fs::set_permissions(&fichero, sin_permisos)
                    .expect("los permisos del destino se pueden cambiar");
                permisos_originales.push((fichero, originales));
            }
        }
        assert!(
            !permisos_originales.is_empty(),
            "el destino sembrado tiene que tener ficheros de contenido que hacer ilegibles"
        );
        assert!(
            hay_indice,
            "el destino sembrado tiene que tener ficheros de índice, que es lo que el listado lee"
        );

        // La simulación tiene que morder: si el proceso pudiera leer igualmente el fichero —correr
        // como `root`, por ejemplo—, el modo `000` no provocaría ningún fallo y la prueba daría un
        // falso «sano». Se comprueba aquí para que el fallo lo diga, no para afirmar el desenlace,
        // que se afirma más abajo.
        let (muestra, _) = &permisos_originales[0];
        assert!(
            matches!(
                std::fs::File::open(muestra),
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
            ),
            "hacer ilegible el contenido del destino no surtió efecto —¿se corre como `root`?—: \
             sin un fichero que no se deje leer, la simulación no provoca ningún fallo de listado"
        );

        let resultado = salud_del_repositorio_con(
            &destino.acceso(PASSPHRASE_DE_PRUEBA),
            destino.backends(),
            barras_mudas(),
        );

        for (camino, originales) in permisos_originales {
            std::fs::set_permissions(&camino, originales)
                .expect("los permisos del destino se pueden devolver");
        }

        let error = resultado.expect_err("un repositorio que no se deja listar no está sano");
        assert!(
            matches!(error, Error::ListadoFallido { .. }),
            "un destino que abre y no lista tiene que distinguirse del que no se alcanza: \
             salió {error:?}"
        );
    }

    /// Un destino que **rechaza** la conexión es un fallo del destino, no una passphrase.
    #[test]
    fn un_destino_que_rechaza_es_un_fallo_del_destino() {
        let acceso = Acceso {
            endpoint: destino_de_s3_que_rechaza(),
            bucket: "cubo-que-rechaza".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let error = salud_del_repositorio(&acceso, barras_mudas())
            .expect_err("un destino que rechaza no se puede comprobar");

        assert!(
            matches!(error, Error::DestinoNoAlcanzable { .. }),
            "un destino que rechaza tiene que llegar como destino inalcanzable: salió {error:?}"
        );
    }

    /// El tope acota la comprobación contra un destino que **acepta y no contesta**.
    ///
    /// Es la misma prueba discriminante que la de la puesta en marcha, y se repite aquí porque el
    /// camino es otro —esta operación no inicializa— y porque el requisito dice explícitamente que
    /// un tope agotado en la comprobación tiene que llegar como «el destino no se alcanza» y no
    /// como un pánico ni como una espera sin fin.
    #[test]
    fn un_destino_que_se_traga_la_peticion_lo_corta_el_tope_en_la_comprobacion() {
        let acceso = Acceso {
            endpoint: destino_de_s3_que_se_traga_la_peticion(),
            bucket: "cubo-que-se-traga-la-peticion".to_owned(),
            region: "eu-south-2".to_owned(),
            access_key_id: "clave-de-acceso-distinguible".to_owned(),
            secret_access_key: SecretString::new(CLAVE_SECRETA_DE_PRUEBA.to_owned()),
            passphrase: SecretString::new(PASSPHRASE_DE_PRUEBA.to_owned()),
        };

        let empezado = std::time::Instant::now();
        let error = salud_del_repositorio(&acceso, barras_mudas())
            .expect_err("un destino que no contesta no se puede comprobar");
        let tardado = empezado.elapsed();

        assert!(
            tardado < Duration::from_secs(12),
            "el tope no cortó la comprobación: tardó {tardado:?} contra un destino que acepta y no \
             contesta, con un tope declarado de {TOPE_DEL_DESTINO:?}"
        );

        let Error::DestinoNoAlcanzable { motivo, .. } = error else {
            panic!("el tope agotado tiene que llegar como `DestinoNoAlcanzable`: llegó {error:?}");
        };
        assert!(
            motivo.contains("tope"),
            "el desenlace tiene que ser el del tope de Kesto y no el de la biblioteca: {motivo}"
        );
        assert!(
            !motivo.contains(CLAVE_SECRETA_DE_PRUEBA) && !motivo.contains(PASSPHRASE_DE_PRUEBA),
            "el motivo del tope publica un secreto: {motivo}"
        );
    }
}
