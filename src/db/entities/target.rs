//! La tabla `backup_targets` del §3: la entidad, el alta y la capa de acceso.
//!
//! Es la tabla con más columnas anulables del §3: un target de tipo `directory` no
//! tiene contenedor ni credenciales, y uno de base de datos no tiene ruta de host.
//! `NULL` y cadena vacía **no** son lo mismo, así que lo anulable viaja como
//! `Option` de punta a punta y nunca se rellena con `""`.
//!
//! `db_password` es el tercer secreto del §3 y se guarda cifrado, como los dos de
//! `repositories`: la variante `Option` decide **si hay** secreto, no si está
//! cifrado. Un `None` escribe `NULL` en la columna; un `Some`, un sobre.

use std::sync::Arc;

use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::db::DbError;
use crate::db::crypto::Clave;
use crate::secret::SecretString;

/// Tipo de fuente de respaldo, tal y como lo declara el `CHECK` del §3.
///
/// El texto almacenado es el nombre de la variante en minúsculas —`mariadb`,
/// `postgresql`, `directory`—, que es lo que el `CHECK` acepta y lo que hace que la
/// columna sea legible con `sqlite3`; guardarlo como número rompería el §3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
pub enum TipoDeTarget {
    /// Volcado de un contenedor MariaDB.
    Mariadb,
    /// Volcado de un contenedor PostgreSQL.
    Postgresql,
    /// Copia de un directorio del host.
    Directory,
}

/// Fila de `backup_targets`, con su `db_password` ya descifrado.
#[derive(Debug, Clone)]
pub struct Target {
    /// UUIDv4 en forma canónica con guiones.
    pub id: Uuid,
    /// Nombre del target.
    pub name: String,
    /// Tipo de fuente.
    pub target_type: TipoDeTarget,
    /// Contenedor del que extraer el volcado; `None` si es un directorio.
    pub container_name: Option<String>,
    /// Usuario de la base; `None` si es un directorio.
    pub db_user: Option<String>,
    /// Contraseña de la base, **descifrada**; `None` si es un directorio.
    pub db_password: Option<SecretString>,
    /// Base concreta del volcado; `None` implica volcado de todas.
    pub db_name: Option<String>,
    /// Ruta del host; `None` si es una base de datos.
    pub host_path: Option<String>,
    /// Fecha de alta, en UTC.
    pub created_at: OffsetDateTime,
}

/// Los datos de un alta. **Sin `id`**: lo genera la capa de acceso.
#[derive(Debug, Clone)]
pub struct AltaDeTarget {
    /// Nombre del target.
    pub name: String,
    /// Tipo de fuente.
    pub target_type: TipoDeTarget,
    /// Contenedor del que extraer el volcado.
    pub container_name: Option<String>,
    /// Usuario de la base.
    pub db_user: Option<String>,
    /// Contraseña de la base; se cifra antes de escribirla.
    pub db_password: Option<SecretString>,
    /// Base concreta del volcado.
    pub db_name: Option<String>,
    /// Ruta del host.
    pub host_path: Option<String>,
}

/// La capa de acceso de `backup_targets`.
pub struct Targets {
    pool: SqlitePool,
    clave: Arc<Clave>,
}

// Las sentencias, como constantes: `DbError::Consulta` lleva el texto de la consulta,
// así que la que se ejecuta y la que viaja en el error tienen que ser **la misma
// cadena**. Con el literal repetido en los dos sitios, lo que se ejecuta y lo que se
// registra pueden separarse sin que nada lo delate.
//
// Los valores van **enlazados** (`?1`, `?2`…): ninguna se compone concatenando ni se
// interpola un valor, ni siquiera para armarla en piezas.

/// Columnas del `SELECT`, en el orden del §3 —el mismo que recorre [`desde_fila`]—.
const SQL_LEER: &str = "SELECT id, name, target_type, container_name, db_user, db_password, db_name, \
     host_path, created_at FROM backup_targets WHERE id = ?1";

/// Listado de todos los targets.
///
/// `ORDER BY created_at, id` y **no** `created_at` a secas: las filas dadas de alta en
/// el mismo segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y el orden
/// entre dos llamadas seguidas dejaría de ser estable. El `id` es el desempate.
const SQL_LISTAR: &str = "SELECT id, name, target_type, container_name, db_user, db_password, db_name, \
     host_path, created_at FROM backup_targets ORDER BY created_at, id";

/// Alta de un target, devolviendo la fila tal y como quedó.
///
/// El `RETURNING` evita el segundo viaje: el `created_at` es el que puso la base y no
/// hay carrera entre el `INSERT` y un `SELECT` posterior.
const SQL_ALTA: &str = "INSERT INTO backup_targets \
     (id, name, target_type, container_name, db_user, db_password, db_name, host_path) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
     RETURNING id, name, target_type, container_name, db_user, db_password, db_name, host_path, \
     created_at";

/// Actualización de la fila entera.
///
/// `created_at` **no** entra: es la fecha de nacimiento de la fila, no un campo que se
/// edite —reescribirla cambiaría la historia y el orden del listado—.
const SQL_ACTUALIZAR: &str = "UPDATE backup_targets SET name = ?1, target_type = ?2, container_name = ?3, \
     db_user = ?4, db_password = ?5, db_name = ?6, host_path = ?7 WHERE id = ?8";

/// Borrado por identificador.
const SQL_BORRAR: &str = "DELETE FROM backup_targets WHERE id = ?1";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las
/// `SQL_*`: dice dónde falló, que es leer o descifrar una columna. Es lo que el
/// registro necesita saber cuando el fallo no viene del motor.
const CONSULTA_MAPEO: &str = "mapeo de la fila de backup_targets";

impl Targets {
    /// Construye la capa de acceso sobre un pool y la clave de cifrado.
    ///
    /// La clave entra en un [`Arc`] porque se comparte por todo el proceso: cada capa
    /// que la necesite recibe la misma, y no se copia el material.
    pub fn nuevo(pool: SqlitePool, clave: Arc<Clave>) -> Self {
        Targets { pool, clave }
    }

    /// Da de alta un target y devuelve la fila tal y como quedó.
    pub async fn alta(&self, alta: AltaDeTarget) -> Result<Target, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`
        // y un valor por defecto en el DDL obligaría a interpolarlo en la sentencia.
        let id = Uuid::new_v4();

        // El secreto se cifra **solo si lo hay**: un target de directorio no tiene
        // contraseña y su columna queda `NULL`, no un sobre de la cadena vacía. El
        // `None` viaja tal cual hasta el `bind`, que escribe `NULL`.
        // `DbError::Cifrado` tiene `#[from]`, así que el `?` encadena solo.
        let db_password = match &alta.db_password {
            Some(claro) => Some(self.clave.cifrar(claro)?),
            None => None,
        };

        let fila = sqlx::query(SQL_ALTA)
            // El `id` va como texto con guiones: un `bind` de `Uuid` lo escribiría como
            // un BLOB de 16 bytes y el §3 dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(&alta.name)
            // El enumerado se enlaza tal cual: su `Type` escribe el texto que el `CHECK`
            // del §3 acepta, no un número.
            .bind(alta.target_type)
            .bind(&alta.container_name)
            .bind(&alta.db_user)
            .bind(&db_password)
            .bind(&alta.db_name)
            .bind(&alta.host_path)
            .fetch_one(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA))?;

        desde_fila(&fila, &self.clave)
    }

    /// Lee un target por su identificador: `Ok(None)` si no está.
    ///
    /// «No está» y «no se pudo leer» son cosas distintas: lo primero es `Ok(None)` y lo
    /// segundo, un error. Confundirlos convertiría una base caída en «no existe», que
    /// es lo que hace que un fallo se persiga en el sitio equivocado.
    pub async fn leer(&self, id: &Uuid) -> Result<Option<Target>, DbError> {
        let fila = sqlx::query(SQL_LEER)
            .bind(id.hyphenated().to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LEER))?;

        match fila {
            Some(fila) => desde_fila(&fila, &self.clave).map(Some),
            None => Ok(None),
        }
    }

    /// Lista los targets con un orden **estable**.
    pub async fn listar(&self) -> Result<Vec<Target>, DbError> {
        let filas = sqlx::query(SQL_LISTAR)
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LISTAR))?;

        filas
            .iter()
            .map(|fila| desde_fila(fila, &self.clave))
            .collect()
    }

    /// Actualiza la fila **entera** y devuelve si existía.
    ///
    /// El secreto se **vuelve a cifrar** —y solo si sigue habiendo secreto—, así que la
    /// fila estrena sobre y nonce aunque el valor en claro no cambie: el nonce es
    /// aleatorio por valor, y eso es lo correcto. Consecuencia práctica: el texto
    /// almacenado cambia en cada actualización.
    pub async fn actualizar(&self, target: &Target) -> Result<bool, DbError> {
        let db_password = match &target.db_password {
            Some(claro) => Some(self.clave.cifrar(claro)?),
            None => None,
        };

        let resultado = sqlx::query(SQL_ACTUALIZAR)
            .bind(&target.name)
            .bind(target.target_type)
            .bind(&target.container_name)
            .bind(&target.db_user)
            .bind(&db_password)
            .bind(&target.db_name)
            .bind(&target.host_path)
            .bind(target.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

        Ok(resultado.rows_affected() == 1)
    }

    /// Borra la fila y devuelve si existía.
    ///
    /// El `ON DELETE CASCADE` del §3 arrastra sus `backup_jobs` y, con ellas, sus
    /// `job_executions`: el borrado en cascada lo hace la base, no esta capa.
    pub async fn borrar(&self, id: &Uuid) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_BORRAR)
            .bind(id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_BORRAR))?;

        Ok(resultado.rows_affected() == 1)
    }
}

/// Envuelve un fallo de `sqlx` con **la misma** sentencia que se ejecutó.
///
/// La sentencia entra por parámetro en vez de repetirse en cada `map_err`: así la que
/// se ejecuta y la que viaja en el error son una sola cadena —la constante—, y no dos
/// literales que pueden separarse.
fn error_de_consulta(consulta: &str) -> impl FnOnce(sqlx::Error) -> DbError + '_ {
    move |causa| DbError::Consulta {
        consulta: consulta.to_owned(),
        causa,
    }
}

/// Lee una columna de la fila, conservando la causa si falta o no tiene el tipo.
///
/// El parámetro es **`Option`** cuando la columna lo es: así `NULL` llega como `None` y
/// nunca se confunde con la cadena vacía.
fn columna<'fila, T>(fila: &'fila SqliteRow, nombre: &str) -> Result<T, DbError>
where
    T: sqlx::Decode<'fila, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
{
    fila.try_get::<T, _>(nombre)
        .map_err(|causa| DbError::Consulta {
            consulta: CONSULTA_MAPEO.to_owned(),
            causa,
        })
}

/// Mapea una fila de `backup_targets` a la entidad, **descifrando** su secreto si lo
/// tiene.
///
/// Es el único sitio del crate que ve el sobre almacenado de esta tabla: el resto de la
/// aplicación solo conoce valores en claro dentro de un [`SecretString`].
fn desde_fila(fila: &SqliteRow, clave: &Clave) -> Result<Target, DbError> {
    // El `id` viaja como `TEXT` con guiones, así que la conversión a `Uuid` es del
    // mapeo y no de `sqlx`. Su error **no** es un `sqlx::Error`, y envolverlo en
    // `sqlx::Error::Decode` es lo que lo hace viajar por `DbError::Consulta` con la
    // causa intacta, sin estrenar una variante para un fallo que ya tiene sitio.
    let id = columna::<String>(fila, "id")?;
    let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO.to_owned(),
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    // El secreto se descifra **solo si la columna trae algo**: un `NULL` da `None` y un
    // `Some` pasa por el descifrado, que falla en alto si el sobre no autentica. Un
    // target de directorio no tiene contraseña, y eso no es un secreto vacío.
    let db_password = match columna::<Option<String>>(fila, "db_password")? {
        Some(cifrado) => Some(clave.descifrar(&cifrado)?),
        None => None,
    };

    Ok(Target {
        id,
        name: columna(fila, "name")?,
        // El enumerado se lee por su `Type`: la columna guarda el texto del §3, así que
        // no hay ninguna conversión a número que hacer.
        target_type: columna(fila, "target_type")?,
        container_name: columna(fila, "container_name")?,
        db_user: columna(fila, "db_user")?,
        db_password,
        db_name: columna(fila, "db_name")?,
        host_path: columna(fila, "host_path")?,
        created_at: columna(fila, "created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;
    use time::OffsetDateTime;

    use super::{AltaDeTarget, Targets, TipoDeTarget};
    use crate::db::entities::execution::{AltaDeEjecucion, Ejecuciones, Estado};
    use crate::db::entities::job::{AltaDeTarea, Tareas};
    use crate::db::entities::repository::{AltaDeRepositorio, Repositorios};
    use crate::secret::SecretString;
    use crate::test_support::{DirectorioDePrueba, base_con_clave};

    /// Contraseña de base distinguible y larga: si aparece en una salida, la salida es
    /// el fallo.
    const PASSWORD: &str = "db-password-distinguible-0000000000000000000000001";

    /// Un `SecretString` con el valor dado.
    fn secreto(valor: &str) -> SecretString {
        SecretString::new(valor.to_owned())
    }

    /// Un target de base de datos, con todos los campos informados.
    fn alta_mariadb(nombre: &str) -> AltaDeTarget {
        AltaDeTarget {
            name: nombre.to_owned(),
            target_type: TipoDeTarget::Mariadb,
            container_name: Some(format!("contenedor-{nombre}")),
            db_user: Some("usuario-de-base".to_owned()),
            db_password: Some(secreto(PASSWORD)),
            db_name: Some(format!("base-{nombre}")),
            host_path: None,
        }
    }

    /// Un target de directorio: sin contenedor, sin credenciales y con su ruta.
    fn alta_directorio(nombre: &str, ruta: &str) -> AltaDeTarget {
        AltaDeTarget {
            name: nombre.to_owned(),
            target_type: TipoDeTarget::Directory,
            container_name: None,
            db_user: None,
            db_password: None,
            db_name: None,
            host_path: Some(ruta.to_owned()),
        }
    }

    /// La capa de acceso sobre una base de prueba migrada.
    async fn capa() -> (DirectorioDePrueba, SqlitePool, Targets) {
        let (directorio, pool, clave) = base_con_clave().await;
        let targets = Targets::nuevo(pool.clone(), clave);

        (directorio, pool, targets)
    }

    /// 1.25 — El ciclo completo de un target, y su tipo almacenado **como texto**.
    ///
    /// La comprobación con SQL directo es la que fija que el enumerado no se guarde
    /// como número: `mariadb` tiene que estar en la columna tal cual.
    #[tokio::test]
    async fn el_ciclo_de_un_target_y_su_tipo_guardado_como_texto() {
        let (_directorio, pool, targets) = capa().await;

        let dado_de_alta = targets
            .alta(alta_mariadb("target-1-25"))
            .await
            .expect("el alta no puede fallar");

        let leido = targets
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila recién dada de alta debe estar");

        assert_eq!(leido.id, dado_de_alta.id, "el id leído no es el del alta");
        assert_eq!(leido.name, "target-1-25");
        assert_eq!(leido.target_type, TipoDeTarget::Mariadb);
        assert_eq!(
            leido.container_name.as_deref(),
            Some("contenedor-target-1-25")
        );
        assert_eq!(leido.db_user.as_deref(), Some("usuario-de-base"));
        assert_eq!(
            leido.db_password.as_ref().map(SecretString::expose),
            Some(PASSWORD)
        );
        assert_eq!(leido.db_name.as_deref(), Some("base-target-1-25"));
        assert_eq!(leido.host_path, None);
        assert_eq!(leido.created_at, dado_de_alta.created_at);

        let almacenado: String =
            sqlx::query_scalar("SELECT target_type FROM backup_targets WHERE id = ?1")
                .bind(dado_de_alta.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el tipo del target debe poder leerse");

        assert_eq!(
            almacenado, "mariadb",
            "el enumerado no se guardó como texto, sino como {almacenado}"
        );

        let listado = targets.listar().await.expect("el listado no puede fallar");
        assert_eq!(listado.len(), 1, "el listado debe traer el target");

        let mut cambiado = leido.clone();
        cambiado.name = "target-1-25-despues".to_owned();
        cambiado.db_password = Some(secreto("db-password-rotada-1-25"));

        assert!(
            targets
                .actualizar(&cambiado)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let releido = targets
            .leer(&cambiado.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(
            releido.name, "target-1-25-despues",
            "el campo tocado no cambió"
        );
        assert_eq!(
            releido.db_password.as_ref().map(SecretString::expose),
            Some("db-password-rotada-1-25"),
            "el secreto tocado no cambió"
        );
        assert_eq!(
            releido.created_at, dado_de_alta.created_at,
            "el created_at no debía cambiar"
        );

        assert!(
            targets
                .borrar(&cambiado.id)
                .await
                .expect("el borrado no puede fallar"),
            "la fila existía, así que el borrado debe decir que sí"
        );

        let tras_borrar = targets
            .leer(&cambiado.id)
            .await
            .expect("la lectura no puede fallar");

        assert!(tras_borrar.is_none(), "la fila debía desaparecer");
    }

    /// 1.26 — Un `db_name` informado se lee informado, y uno `NULL` se lee `None`:
    /// **`NULL` no se confunde con la cadena vacía**.
    #[tokio::test]
    async fn el_db_name_nulo_no_se_confunde_con_la_cadena_vacia() {
        let (_directorio, pool, targets) = capa().await;

        let con_nombre = targets
            .alta(alta_mariadb("target-1-26-con"))
            .await
            .expect("el alta no puede fallar");

        let sin_nombre = targets
            .alta(AltaDeTarget {
                db_name: None,
                ..alta_mariadb("target-1-26-sin")
            })
            .await
            .expect("el alta no puede fallar");

        let leido_con = targets
            .leer(&con_nombre.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            leido_con.db_name.as_deref(),
            Some("base-target-1-26-con"),
            "un db_name informado debe leerse informado"
        );

        let leido_sin = targets
            .leer(&sin_nombre.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            leido_sin.db_name, None,
            "un db_name NULL debe leerse como `None`"
        );

        // Y en la columna es `NULL` de verdad: `length(NULL)` es `NULL`, mientras que
        // `length('')` sería 0. Es lo que distingue `NULL` de la cadena vacía.
        let es_nulo: i64 =
            sqlx::query_scalar("SELECT db_name IS NULL FROM backup_targets WHERE id = ?1")
                .bind(sin_nombre.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el `IS NULL` debe poder leerse");

        assert_eq!(es_nulo, 1, "la columna no es NULL, sino otra cosa");

        let longitud: Option<i64> =
            sqlx::query_scalar("SELECT length(db_name) FROM backup_targets WHERE id = ?1")
                .bind(sin_nombre.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("la longitud debe poder leerse");

        assert_eq!(
            longitud, None,
            "un db_name NULL no debe medir 0: eso sería la cadena vacía"
        );
    }

    /// 1.27 — Un target de tipo `directory` no necesita contenedor, ni usuario, ni
    /// contraseña, y su `db_name` es `None`.
    #[tokio::test]
    async fn un_target_de_directorio_no_necesita_contenedor_ni_credenciales() {
        let (_directorio, _pool, targets) = capa().await;

        let dado_de_alta = targets
            .alta(alta_directorio("target-1-27", "/srv/datos"))
            .await
            .expect("un directorio no necesita contenedor ni credenciales");

        let leido = targets
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(leido.target_type, TipoDeTarget::Directory);
        assert_eq!(leido.host_path.as_deref(), Some("/srv/datos"));
        assert_eq!(leido.container_name, None);
        assert_eq!(leido.db_user, None);
        assert!(
            leido.db_password.is_none(),
            "un target de directorio no lleva contraseña de base"
        );
        assert_eq!(leido.db_name, None);
    }

    /// 1.32 — **La cascada de los tres niveles**: borrar un target deja sin tareas sus
    /// `backup_jobs` y sin ejecuciones esas tareas, con el repositorio intacto.
    ///
    /// La hace la base, no esta capa: los tres `ON DELETE CASCADE` del §3, con los
    /// índices de las columnas hijas que añade `0002_indexes.sql`.
    #[tokio::test]
    async fn borrar_un_target_arrastra_sus_tareas_y_sus_ejecuciones_y_deja_el_repositorio() {
        let (_directorio, pool, clave) = base_con_clave().await;

        let repositorios = Repositorios::nuevo(pool.clone(), clave.clone());
        let targets = Targets::nuevo(pool.clone(), clave);
        let tareas = Tareas::nuevo(pool.clone());
        let ejecuciones = Ejecuciones::nuevo(pool.clone());

        let repositorio = repositorios
            .alta(AltaDeRepositorio {
                name: "repo-1-32".to_owned(),
                endpoint: "https://s3.example.test/repo-1-32".to_owned(),
                bucket: "cubo-repo-1-32".to_owned(),
                region: "eu-west-1".to_owned(),
                access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
                secret_access_key: secreto("AKIAIOSFODNN7EXAMPLE-1-32"),
                passphrase: secreto("passphrase-distinguible-1-32"),
            })
            .await
            .expect("el alta del repositorio no puede fallar");

        let target = targets
            .alta(alta_mariadb("target-1-32"))
            .await
            .expect("el alta del target no puede fallar");

        // **Dos** tareas del mismo target, y una ejecución de cada una: así se ve que la
        // cascada alcanza a todas, no a una.
        let mut tareas_del_target = Vec::new();
        let mut ejecuciones_de_esas_tareas = Vec::new();

        for cron in ["0 3 * * *", "0 4 * * *"] {
            let tarea = tareas
                .alta(AltaDeTarea {
                    target_id: target.id,
                    repository_id: repositorio.id,
                    cron_expression: cron.to_owned(),
                    keep_daily: 7,
                    keep_weekly: 4,
                    keep_monthly: 12,
                    enabled: true,
                })
                .await
                .expect("el alta de la tarea no puede fallar");

            ejecuciones_de_esas_tareas.push(
                ejecuciones
                    .alta(AltaDeEjecucion {
                        job_id: tarea.id,
                        status: Estado::Pending,
                        snapshot_id: None,
                        bytes_processed: 0,
                        bytes_uploaded: 0,
                        error_message: None,
                        started_at: OffsetDateTime::now_utc(),
                        finished_at: None,
                    })
                    .await
                    .expect("el alta de la ejecución no puede fallar")
                    .id,
            );

            tareas_del_target.push(tarea.id);
        }

        assert!(
            targets
                .borrar(&target.id)
                .await
                .expect("el borrado no puede fallar"),
            "la fila existía, así que el borrado debe decir que sí"
        );

        let tareas_restantes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_jobs")
            .fetch_one(&pool)
            .await
            .expect("el recuento de tareas no puede fallar");

        assert_eq!(
            tareas_restantes, 0,
            "las tareas del target sobrevivieron; el `CASCADE` no ocurrió"
        );

        let ejecuciones_restantes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_executions")
            .fetch_one(&pool)
            .await
            .expect("el recuento de ejecuciones no puede fallar");

        assert_eq!(
            ejecuciones_restantes, 0,
            "las ejecuciones sobrevivieron a sus tareas"
        );

        for tarea in &tareas_del_target {
            assert!(
                tareas
                    .leer(tarea)
                    .await
                    .expect("la lectura no puede fallar")
                    .is_none(),
                "la tarea {tarea} sobrevivió al target"
            );
        }

        for ejecucion in &ejecuciones_de_esas_tareas {
            assert!(
                ejecuciones
                    .leer(ejecucion)
                    .await
                    .expect("la lectura no puede fallar")
                    .is_none(),
                "la ejecución {ejecucion} sobrevivió a su tarea"
            );
        }

        // El repositorio **no** era del target: sigue ahí.
        let repositorio_sigue = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar");

        assert!(
            repositorio_sigue.is_some(),
            "el repositorio no debía desaparecer: el que se borró fue el target"
        );
    }
}
