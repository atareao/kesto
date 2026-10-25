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
/// La `region` viaja tal cual y **no** se valida: el §3 la guarda, hay destinos que la
/// declaran vacía y `opendal` decide qué hacer con eso. Lo que sí se valida es que el
/// destino y las credenciales estén informados, porque un cubo vacío o una credencial
/// vacía no son una configuración floja: son una fila que no puede funcionar.
#[derive(Debug, Clone)]
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
    pub ficheros: u64,
    /// Los bytes que el archivado contó como procesados.
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
    /// [`Error::PassphraseNoAbre`] si el repositorio existe y la passphrase no lo abre.
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
    /// # Errors
    ///
    /// Devuelve [`Error::CopiaSinRutas`] si la copia no declara ninguna ruta —la
    /// biblioteca indexaría su primera ruta sin preguntar— y [`Error::CopiaFallida`] si el
    /// archivado falla. **Que no devuelva error no significa que la copia esté completa**:
    /// ver el encabezado del módulo.
    pub fn archivar_volcado(
        &self,
        copia: &Copia,
        origen: &OrigenDelVolcado,
    ) -> Result<CopiaHecha, Error> {
        let rutas = copia.rutas()?;
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
    /// Devuelve [`Error::RetencionFallida`] si falla cualquiera de las tres. **Una
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
    acceso.validar()?;

    let opciones: BTreeMap<String, String> = [
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
    .collect();

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
///    destino, porque se acaba de listar y por tanto se alcanza.
///
/// # Visibilidad
///
/// `pub(crate)` y no privada: las pruebas de [`crate::rustic::retencion`] también la
/// necesitan, y compartir el banco de pruebas de un módulo entre dos ficheros de pruebas
/// es más barato que duplicarlo. No es `pub`: no forma parte de la superficie del crate, y
/// el destino local que recibe es un gancho de prueba, no un modo de funcionamiento.
///
/// # Errors
///
/// Las tres del desenlace y las de la construcción del propio repositorio. Ver
/// [`Repositorio::poner_en_marcha`].
pub(crate) fn poner_en_marcha_con(
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
                // que falla es la passphrase. Se comprueba con el código que la biblioteca
                // publica para esa pregunta —el mismo que usa ella— y no por el texto del
                // mensaje, que puede cambiar en cualquier versión menor.
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

    use rustic_core::repofile::SnapshotFile;
    use rustic_core::{FileType, ReadBackend, SnapshotOptions};

    use crate::rustic::tunel_del_volcado;
    use crate::test_support::{
        CLAVE_SECRETA_DE_PRUEBA, DirectorioDePrueba, ETIQUETA_SEMBRADA, OTRA_PASSPHRASE_DE_PRUEBA,
        PASSPHRASE_DE_PRUEBA, SEMILLA_CONTENIDO, acceso_local, backends_locales, barras_mudas,
        directorio_de_prueba, sembrar_snapshots, snapshot_guardado, volcado_guardado,
    };

    use super::{
        Acceso, Copia, CopiaHecha, Error, PuestaEnMarcha, Repositorio, poner_en_marcha_con,
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
}
