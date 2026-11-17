//! La tabla `repositories` del §3: la entidad, el alta y la capa de acceso.
//!
//! Sus dos columnas de secreto —`secret_access_key` y `passphrase`— no se guardan
//! en claro: el mapeo fila→struct las descifra con la [`Clave`], y el alta las
//! cifra. La capa de acceso es, por eso, el único sitio del crate que conoce la
//! forma del valor almacenado, y lo es solo a través de [`crate::db::crypto`].

use std::sync::Arc;

use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::db::DbError;
use crate::db::crypto::Clave;
use crate::secret::SecretString;

/// El desenlace de la última comprobación de salud de un repositorio.
///
/// Los cinco valores son los del `CHECK` de la columna `repositories.health`, y el
/// **sexto estado no es una variante**: «desconocido» —la fila que todavía no se ha
/// comprobado— se representa con la **ausencia** del valor, es decir `None` en el
/// `Option` que lleva la entidad. Inventar aquí una variante `Desconocido` obligaría
/// a escribirla en la columna y el `CHECK` la rechazaría: el nulo y el estado no se
/// confunden.
///
/// El texto almacenado es el nombre de la variante en `snake_case` —`ok`,
/// `sin_repositorio`, `passphrase`, `inaccesible`, `sin_permiso`—, que es lo que el
/// `CHECK` acepta y lo que hace que la columna sea legible con `sqlite3`. Lo produce
/// **una sola** derivación, la de `sqlx`; el texto que viaja por el cable lo fija la
/// capa HTTP, que además distingue el `None` —«desconocido»—, algo que un `Serialize`
/// de la variante no sabría hacer. Una derivación de `serde` aquí sería un segundo
/// vocabulario que nadie lee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum SaludDeRepositorio {
    /// El repositorio contesta y se puede abrir con sus credenciales.
    Ok,
    /// El destino no tiene repositorio todavía.
    SinRepositorio,
    /// La passphrase no abre el repositorio.
    Passphrase,
    /// No se pudo alcanzar el destino.
    Inaccesible,
    /// Las credenciales no tienen permiso sobre el cubo.
    SinPermiso,
}

/// Fila de `repositories`, con sus dos secretos ya descifrados.
#[derive(Debug, Clone)]
pub struct Repositorio {
    /// UUIDv4 en forma canónica con guiones.
    pub id: Uuid,
    /// Nombre del repositorio.
    pub name: String,
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
    /// Fecha de alta, en UTC.
    pub created_at: OffsetDateTime,
    /// Desenlace de la última comprobación de salud; `None` es «desconocido».
    ///
    /// No es un campo que el operador edite: lo escribe la comprobación, y ni el alta
    /// ni la actualización lo tocan.
    pub health: Option<SaludDeRepositorio>,
    /// Cuándo se hizo esa comprobación, en UTC; `None` mientras no se haya hecho.
    pub health_checked_at: Option<OffsetDateTime>,
    /// Motivo acotado de esa comprobación; `None` si no hay nada que contar.
    pub health_detail: Option<String>,
}

/// Los datos de un alta. **Sin `id`**: lo genera la capa de acceso.
#[derive(Debug, Clone)]
pub struct AltaDeRepositorio {
    /// Nombre del repositorio.
    pub name: String,
    /// Endpoint del servicio S3.
    pub endpoint: String,
    /// Cubo de destino.
    pub bucket: String,
    /// Región del cubo.
    pub region: String,
    /// Identificador público de la credencial.
    pub access_key_id: String,
    /// Secreto de la credencial; se cifra antes de escribirlo.
    pub secret_access_key: SecretString,
    /// Passphrase del repositorio Rustic; se cifra antes de escribirlo.
    pub passphrase: SecretString,
}

/// La capa de acceso de `repositories`.
pub struct Repositorios {
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
///
/// Las tres últimas —la salud— las añade la `0008` y van **detrás** de `created_at`,
/// que es donde las dejó el `ALTER TABLE`; van en el `SELECT` porque la entidad las
/// lleva, y no en el `RETURNING` del alta: la fila que acaba de nacer no tiene salud.
const SQL_LEER: &str = "SELECT id, name, endpoint, bucket, region, access_key_id, secret_access_key, \
     passphrase, created_at, health, health_checked_at, health_detail FROM repositories WHERE id = ?1";

/// Listado de todos los repositorios.
///
/// `ORDER BY created_at, id` y **no** `created_at` a secas: las filas dadas de alta en
/// el mismo segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y el orden
/// entre dos llamadas seguidas dejaría de ser estable. El `id` es el desempate.
const SQL_LISTAR: &str = "SELECT id, name, endpoint, bucket, region, access_key_id, secret_access_key, \
     passphrase, created_at, health, health_checked_at, health_detail FROM repositories \
     ORDER BY created_at, id";

/// Alta de un repositorio, devolviendo la fila tal y como quedó.
///
/// El `RETURNING` evita el segundo viaje: el `created_at` es el que puso la base y no
/// hay carrera entre el `INSERT` y un `SELECT` posterior.
const SQL_ALTA: &str = "INSERT INTO repositories \
     (id, name, endpoint, bucket, region, access_key_id, secret_access_key, passphrase) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
     RETURNING id, name, endpoint, bucket, region, access_key_id, secret_access_key, \
     passphrase, created_at";

/// Actualización de la fila entera.
///
/// `created_at` **no** entra: es la fecha de nacimiento de la fila, no un campo que se
/// edite —reescribirla cambiaría la historia y el orden del listado—.
const SQL_ACTUALIZAR: &str = "UPDATE repositories SET name = ?1, endpoint = ?2, bucket = ?3, \
     region = ?4, access_key_id = ?5, secret_access_key = ?6, passphrase = ?7 WHERE id = ?8";

/// Borrado por identificador.
const SQL_BORRAR: &str = "DELETE FROM repositories WHERE id = ?1";

/// Anotación del desenlace de una comprobación de salud.
///
/// Escribe **solo** las tres columnas de la salud, y es la única sentencia que lo
/// hace: ni el [`SQL_ALTA`] ni el [`SQL_ACTUALIZAR`] las nombran, así que editar el
/// repositorio no puede pisar lo que anotó una comprobación.
///
/// La fecha la pone la base con `CURRENT_TIMESTAMP` —UTC, como el `created_at` del
/// §3— y no quien llama: el reloj de la fila es el de la base, y así la comparación
/// con `created_at` es entre dos fechas del mismo reloj.
const SQL_ANOTAR_SALUD: &str = "UPDATE repositories SET health = ?1, health_detail = ?2, \
     health_checked_at = CURRENT_TIMESTAMP WHERE id = ?3";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las
/// `SQL_*`: dice dónde falló, que es leer o descifrar una columna. Es lo que el
/// registro necesita saber cuando el fallo no viene del motor.
const CONSULTA_MAPEO: &str = "mapeo de la fila de repositories";

impl Repositorios {
    /// Construye la capa de acceso sobre un pool y la clave de cifrado.
    ///
    /// La clave entra en un [`Arc`] porque se comparte por todo el proceso: cada capa
    /// que la necesite recibe la misma, y no se copia el material.
    pub fn nuevo(pool: SqlitePool, clave: Arc<Clave>) -> Self {
        Repositorios { pool, clave }
    }

    /// Da de alta un repositorio y devuelve la fila tal y como quedó.
    pub async fn alta(&self, alta: AltaDeRepositorio) -> Result<Repositorio, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`
        // y un valor por defecto en el DDL obligaría a interpolarlo en la sentencia.
        let id = Uuid::new_v4();

        // Un secreto **vacío** no se guarda: se rechaza aquí, antes de cifrar, para que un
        // campo en blanco no sustituya la credencial por el cifrado de nada sin señal.
        exigir_no_vacio("secret_access_key", &alta.secret_access_key)?;
        exigir_no_vacio("passphrase", &alta.passphrase)?;

        // Los dos secretos se guardan cifrados. `DbError::Cifrado` tiene `#[from]`, así
        // que el `?` encadena `ErrorDeCifrado → DbError` sin ceremonia.
        let secret_access_key = self.clave.cifrar(&alta.secret_access_key)?;
        let passphrase = self.clave.cifrar(&alta.passphrase)?;

        let fila = sqlx::query(SQL_ALTA)
            // El `id` va como texto con guiones: un `bind` de `Uuid` lo escribiría como
            // un BLOB de 16 bytes y el §3 dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(&alta.name)
            .bind(&alta.endpoint)
            .bind(&alta.bucket)
            .bind(&alta.region)
            .bind(&alta.access_key_id)
            .bind(&secret_access_key)
            .bind(&passphrase)
            .fetch_one(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA))?;

        // El `RETURNING` del alta es el del §3: la fila recién nacida no tiene salud, y el
        // mapeo la deja en «desconocido» por la puerta que le corresponde.
        desde_fila_sin_salud(&fila, &self.clave)
    }

    /// Lee un repositorio por su identificador: `Ok(None)` si no está.
    ///
    /// «No está» y «no se pudo leer» son cosas distintas: lo primero es `Ok(None)` y lo
    /// segundo, un error. Confundirlos convertiría una base caída en «no existe», que
    /// es lo que hace que un fallo se persiga en el sitio equivocado.
    pub async fn leer(&self, id: &Uuid) -> Result<Option<Repositorio>, DbError> {
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

    /// Lista los repositorios con un orden **estable**.
    pub async fn listar(&self) -> Result<Vec<Repositorio>, DbError> {
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
    /// **Quien actualice tiene que rellenar los dos secretos con el valor leído**: la
    /// actualización es de fila entera y los vuelve a cifrar, así que dejar el campo en
    /// blanco no «conserva» la credencial —la sustituye por el cifrado de nada—. Por eso
    /// el vacío se **rechaza** en vez de escribirse: para que ese olvido no pase en
    /// silencio.
    ///
    /// Los dos secretos se **vuelven a cifrar**, así que la fila estrena sobre y nonce
    /// aunque el valor en claro no cambie: el nonce es aleatorio por valor, y eso es lo
    /// correcto —repetirlo con la misma clave rompería el AEAD—. Consecuencia práctica:
    /// el texto almacenado cambia en cada actualización, y quien compare bytes verá una
    /// diferencia donde no la hay en el secreto.
    pub async fn actualizar(&self, repositorio: &Repositorio) -> Result<bool, DbError> {
        exigir_no_vacio("secret_access_key", &repositorio.secret_access_key)?;
        exigir_no_vacio("passphrase", &repositorio.passphrase)?;

        let secret_access_key = self.clave.cifrar(&repositorio.secret_access_key)?;
        let passphrase = self.clave.cifrar(&repositorio.passphrase)?;

        let resultado = sqlx::query(SQL_ACTUALIZAR)
            .bind(&repositorio.name)
            .bind(&repositorio.endpoint)
            .bind(&repositorio.bucket)
            .bind(&repositorio.region)
            .bind(&repositorio.access_key_id)
            .bind(&secret_access_key)
            .bind(&passphrase)
            .bind(repositorio.id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ACTUALIZAR))?;

        Ok(resultado.rows_affected() == 1)
    }

    /// Borra la fila y devuelve si existía.
    ///
    /// `false` no es un error: quien pidió borrar algo que ya no está ha conseguido lo
    /// que quería. Es el mismo trato que [`Repositorios::leer`] da al «no está».
    pub async fn borrar(&self, id: &Uuid) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_BORRAR)
            .bind(id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_BORRAR))?;

        Ok(resultado.rows_affected() == 1)
    }

    /// Anota en la fila el desenlace de una comprobación de salud y devuelve si existía.
    ///
    /// Escribe el estado, el motivo y la fecha —esta última la pone la base—, y **solo**
    /// esas tres columnas: los campos mutables del repositorio no se tocan, porque una
    /// comprobación no edita la configuración. Es el método que la comprobación llama
    /// después de mirar el destino, así que recibe el desenlace ya resuelto y no vuelve
    /// a mirarlo.
    ///
    /// El motivo es `Option<&str>` y no `Option<String>`: quien llama ya tiene el texto
    /// —o no tiene ninguno— y aquí solo se enlaza. Un estado `Ok` no tiene motivo, y
    /// `None` deja la columna a nulo, que es lo que significa «no hay nada que contar».
    ///
    /// `false` es «la fila no está», con el mismo trato que [`Repositorios::borrar`] y
    /// [`Repositorios::actualizar`]: quien comprobó un repositorio que ya se borró no ha
    /// fallado, se ha quedado sin sitio donde anotarlo.
    pub async fn anotar_salud(
        &self,
        id: &Uuid,
        salud: SaludDeRepositorio,
        detalle: Option<&str>,
    ) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_ANOTAR_SALUD)
            // El estado va por su tipo de `sqlx`, que es el que conoce el texto de la
            // columna: el `snake_case` de las variantes es el mismo que el `CHECK` admite,
            // y así el literal no se repite aquí.
            .bind(salud)
            .bind(detalle)
            .bind(id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ANOTAR_SALUD))?;

        Ok(resultado.rows_affected() == 1)
    }
}

/// Envuelve un fallo de `sqlx` con **la misma** sentencia que se ejecutó.
///
/// La sentencia entra por parámetro en vez de repetirse en cada `map_err`: así la que
/// se ejecuta y la que viaja en el error son una sola cadena —la constante—, y no dos
/// literales que pueden separarse.
fn error_de_consulta(consulta: &'static str) -> impl FnOnce(sqlx::Error) -> DbError {
    move |causa| DbError::Consulta { consulta, causa }
}

/// Rechaza un secreto vacío, nombrando el **campo** y nunca el valor.
///
/// El descifrado ya falla en alto y no devuelve nunca un secreto vacío; ésta es la mitad
/// simétrica, la de la escritura. Sin ella, quien construya la entidad desde fuera y
/// deje el campo en blanco sustituiría la credencial por el cifrado de nada, sin señal,
/// y el fallo aparecería más tarde, en el motor, hablando de S3.
fn exigir_no_vacio(campo: &'static str, secreto: &SecretString) -> Result<(), DbError> {
    if secreto.expose().is_empty() {
        return Err(DbError::SecretoVacio { campo });
    }

    Ok(())
}

/// Lee una columna de la fila, conservando la causa si falta o no tiene el tipo.
fn columna<'fila, T>(fila: &'fila SqliteRow, nombre: &str) -> Result<T, DbError>
where
    T: sqlx::Decode<'fila, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
{
    fila.try_get::<T, _>(nombre)
        .map_err(|causa| DbError::Consulta {
            consulta: CONSULTA_MAPEO,
            causa,
        })
}

/// Mapea una fila de `repositories` a la entidad, **descifrando** sus dos secretos.
///
/// Es el único sitio del crate que ve el sobre almacenado: el resto de la aplicación
/// solo conoce valores en claro dentro de un [`SecretString`].
///
/// Espera las columnas del `SELECT` de lectura y de listado, salud incluida: es la
/// puerta de las filas que ya existen. La del alta es [`desde_fila_sin_salud`].
fn desde_fila(fila: &SqliteRow, clave: &Clave) -> Result<Repositorio, DbError> {
    Ok(Repositorio {
        // La salud se lee por el tipo del enum, no por texto suelto: un valor que no sea
        // uno de los cinco del `CHECK` falla aquí, en el mapeo, en vez de colarse como
        // un `None` silencioso que diría que la fila no se ha comprobado.
        health: columna(fila, "health")?,
        health_checked_at: columna(fila, "health_checked_at")?,
        health_detail: columna(fila, "health_detail")?,
        ..desde_fila_sin_salud(fila, clave)?
    })
}

/// Mapea una fila **sin** las columnas de la salud, dejando las tres en «desconocido».
///
/// Es el mapeo del `RETURNING` del alta, y por eso existe: la fila que acaba de nacer
/// **no tiene salud** —la escribe una comprobación, y a una fila recién creada no se le
/// ha hecho ninguna—, así que el `RETURNING` sigue siendo el del §3 y no nombra esas
/// columnas. Poner el `None` aquí, en la puerta por la que entra esa fila, es lo que
/// hace que la ausencia sea explícita y no un campo que se rellena por accidente.
///
/// La salud se descarta de la fila con el `..` de [`desde_fila`]: un `RETURNING` que
/// algún día las trajera no cambiaría este desenlace —una fila que se acaba de insertar
/// no puede traerlas—.
fn desde_fila_sin_salud(fila: &SqliteRow, clave: &Clave) -> Result<Repositorio, DbError> {
    // El `id` viaja como `TEXT` con guiones, así que la conversión a `Uuid` es del
    // mapeo y no de `sqlx`. Su error **no** es un `sqlx::Error`, y envolverlo en
    // `sqlx::Error::Decode` es lo que lo hace viajar por `DbError::Consulta` con la
    // causa intacta, sin estrenar una variante para un fallo que ya tiene sitio.
    let id = columna::<String>(fila, "id")?;
    let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    Ok(Repositorio {
        id,
        name: columna(fila, "name")?,
        endpoint: columna(fila, "endpoint")?,
        bucket: columna(fila, "bucket")?,
        region: columna(fila, "region")?,
        access_key_id: columna(fila, "access_key_id")?,
        secret_access_key: clave.descifrar(&columna::<String>(fila, "secret_access_key")?)?,
        passphrase: clave.descifrar(&columna::<String>(fila, "passphrase")?)?,
        created_at: columna(fila, "created_at")?,
        health: None,
        health_checked_at: None,
        health_detail: None,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use sqlx::SqlitePool;
    use time::macros::format_description;
    use time::{OffsetDateTime, PrimitiveDateTime};
    use uuid::Uuid;

    use super::{AltaDeRepositorio, Repositorio, Repositorios, SaludDeRepositorio};
    use crate::db::DbError;
    use crate::db::entities::execution::{AltaDeEjecucion, Ejecuciones, Estado};
    use crate::db::entities::job::{AltaDeTarea, Tareas};
    use crate::db::entities::target::{AltaDeTarget, Targets, TipoDeTarget};
    use crate::error::AppError;
    use crate::secret::SecretString;
    use crate::test_support::{
        DirectorioDePrueba, base_con_clave, body_text, capturar_log, nombre_de_tarea_de_prueba,
    };

    /// Secreto de acceso distinguible y largo: si aparece en una salida, la salida
    /// es el fallo.
    const SECRETO_ACCESO: &str = "AKIAIOSFODNN7EXAMPLE-repositories-1";
    /// Passphrase distinguible y larga, por el mismo motivo.
    const PASSPHRASE: &str = "passphrase-distinguible-0000000000000000000000001";
    /// Contraseña de base distinguible y larga: es el tercer secreto del §3.
    const PASSWORD_DE_TARGET: &str = "db-password-distinguible-000000000000000000000049";

    /// Un `SecretString` con el valor dado.
    fn secreto(valor: &str) -> SecretString {
        SecretString::new(valor.to_owned())
    }

    /// Un alta con todos los campos informados, distinguible por su nombre.
    fn alta(nombre: &str) -> AltaDeRepositorio {
        AltaDeRepositorio {
            name: nombre.to_owned(),
            endpoint: format!("https://s3.example.test/{nombre}"),
            bucket: format!("cubo-{nombre}"),
            region: "eu-west-1".to_owned(),
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
            secret_access_key: secreto(SECRETO_ACCESO),
            passphrase: secreto(PASSPHRASE),
        }
    }

    /// La capa de acceso sobre una base de prueba migrada.
    async fn capa() -> (DirectorioDePrueba, SqlitePool, Repositorios) {
        let (directorio, pool, clave) = base_con_clave().await;
        let repositorios = Repositorios::nuevo(pool.clone(), clave);

        (directorio, pool, repositorios)
    }

    /// 1.18 — Alta y lectura por identificador, con todos los campos.
    #[tokio::test]
    async fn el_alta_y_la_lectura_por_id_devuelven_todos_los_campos() {
        let (_directorio, _pool, repositorios) = capa().await;

        let dado_de_alta = repositorios
            .alta(alta("repo-1-18"))
            .await
            .expect("el alta no puede fallar");

        // El identificador lo generó la capa de acceso: quien llama no lo informó.
        let leido = repositorios
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila recién dada de alta debe estar");

        assert_eq!(leido.id, dado_de_alta.id, "el id leído no es el del alta");
        assert_eq!(leido.name, "repo-1-18");
        assert_eq!(leido.endpoint, "https://s3.example.test/repo-1-18");
        assert_eq!(leido.bucket, "cubo-repo-1-18");
        assert_eq!(leido.region, "eu-west-1");
        assert_eq!(leido.access_key_id, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(leido.secret_access_key.expose(), SECRETO_ACCESO);
        assert_eq!(leido.passphrase.expose(), PASSPHRASE);
        assert_eq!(leido.created_at, dado_de_alta.created_at);
    }

    /// 1.19 — El identificador se almacena como **`TEXT` con guiones**, no como BLOB.
    ///
    /// Es el escenario que caza la trampa del `Uuid` como BLOB: `sqlx` codifica un
    /// `Uuid` en 16 bytes y el §3 dice `TEXT`.
    #[tokio::test]
    async fn el_identificador_se_almacena_como_texto_con_guiones() {
        let (_directorio, pool, repositorios) = capa().await;

        let repositorio = repositorios
            .alta(alta("repo-1-19"))
            .await
            .expect("el alta no puede fallar");

        let almacenado = repositorio.id.hyphenated().to_string();

        let tipo: String = sqlx::query_scalar("SELECT typeof(id) FROM repositories WHERE id = ?1")
            .bind(&almacenado)
            .fetch_one(&pool)
            .await
            .expect("el tipo del identificador debe poder leerse");

        assert_eq!(
            tipo, "text",
            "el identificador no está almacenado como TEXT, sino como {tipo}"
        );

        let longitud: i64 = sqlx::query_scalar("SELECT length(id) FROM repositories WHERE id = ?1")
            .bind(&almacenado)
            .fetch_one(&pool)
            .await
            .expect("la longitud del identificador debe poder leerse");

        assert_eq!(
            longitud, 36,
            "el identificador no tiene la forma canónica con guiones (36 caracteres): {longitud}"
        );

        let leido: String = sqlx::query_scalar("SELECT id FROM repositories WHERE id = ?1")
            .bind(&almacenado)
            .fetch_one(&pool)
            .await
            .expect("el identificador debe poder leerse");

        assert_eq!(
            leido, almacenado,
            "el id guardado no es la forma con guiones"
        );
    }

    /// 1.20 — Leer por un identificador inexistente devuelve `Ok(None)`, no un error.
    #[tokio::test]
    async fn leer_por_un_id_inexistente_devuelve_none() {
        let (_directorio, _pool, repositorios) = capa().await;

        let leido = repositorios
            .leer(&Uuid::new_v4())
            .await
            .expect("un id inexistente no puede dar error de base");

        assert!(
            leido.is_none(),
            "un id inexistente debe dar `None`, no una fila: {leido:?}"
        );
    }

    /// 1.21 — El listado con tres filas, y el orden es **idéntico** entre dos
    /// llamadas seguidas.
    #[tokio::test]
    async fn el_listado_devuelve_las_tres_filas_en_un_orden_estable() {
        let (_directorio, _pool, repositorios) = capa().await;

        let mut esperados = Vec::new();

        for nombre in ["repo-1-21-a", "repo-1-21-b", "repo-1-21-c"] {
            esperados.push(
                repositorios
                    .alta(alta(nombre))
                    .await
                    .expect("el alta no puede fallar")
                    .id,
            );
        }

        let primera = repositorios
            .listar()
            .await
            .expect("el listado no puede fallar");
        let segunda = repositorios
            .listar()
            .await
            .expect("el listado no puede fallar");

        assert_eq!(primera.len(), 3, "el listado debe traer las tres filas");

        let ids_primera: Vec<Uuid> = primera.iter().map(|fila| fila.id).collect();
        let ids_segunda: Vec<Uuid> = segunda.iter().map(|fila| fila.id).collect();

        assert_eq!(
            ids_primera, ids_segunda,
            "el orden cambió entre dos llamadas seguidas: `created_at` a secas no desempata"
        );

        for esperado in &esperados {
            assert!(
                ids_primera.contains(esperado),
                "el listado se dejó la fila {esperado}: {ids_primera:?}"
            );
        }
    }

    /// 1.22 — La actualización: el campo tocado cambia y **los demás conservan su
    /// valor**, secretos y `created_at` incluidos.
    #[tokio::test]
    async fn la_actualizacion_cambia_lo_tocado_y_conserva_lo_demas() {
        let (_directorio, _pool, repositorios) = capa().await;

        let original = repositorios
            .alta(alta("repo-1-22-antes"))
            .await
            .expect("el alta no puede fallar");

        let nuevo_secreto = "AKIAIOSFODNN7EXAMPLE-rotado-1-22";

        let mut cambiado = original.clone();
        cambiado.name = "repo-1-22-despues".to_owned();
        cambiado.secret_access_key = secreto(nuevo_secreto);

        assert!(
            repositorios
                .actualizar(&cambiado)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let leido = repositorios
            .leer(&original.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe seguir existiendo");

        assert_eq!(leido.name, "repo-1-22-despues", "el campo tocado no cambió");
        assert_eq!(
            leido.secret_access_key.expose(),
            nuevo_secreto,
            "el secreto tocado no cambió"
        );
        assert_eq!(leido.id, original.id, "el id cambió");
        assert_eq!(
            leido.endpoint, original.endpoint,
            "el endpoint no debía cambiar"
        );
        assert_eq!(leido.bucket, original.bucket, "el bucket no debía cambiar");
        assert_eq!(leido.region, original.region, "la región no debía cambiar");
        assert_eq!(
            leido.access_key_id, original.access_key_id,
            "el access_key_id no debía cambiar"
        );
        assert_eq!(
            leido.passphrase.expose(),
            original.passphrase.expose(),
            "la passphrase no debía cambiar"
        );
        assert_eq!(
            leido.created_at, original.created_at,
            "el created_at no debía cambiar"
        );
    }

    /// 1.23 — El borrado por identificador: `true`, y la lectura posterior da `None`.
    #[tokio::test]
    async fn el_borrado_quita_la_fila() {
        let (_directorio, _pool, repositorios) = capa().await;

        let repositorio = repositorios
            .alta(alta("repo-1-23"))
            .await
            .expect("el alta no puede fallar");

        assert!(
            repositorios
                .borrar(&repositorio.id)
                .await
                .expect("el borrado no puede fallar"),
            "la fila existía, así que el borrado debe decir que sí"
        );

        let leido = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar");

        assert!(leido.is_none(), "la fila debía desaparecer: {leido:?}");
    }

    /// 1.2 (salud por repositorio) — La entidad lee la salud de la fila: «desconocido»
    /// cuando no se ha comprobado, y el estado, la fecha y el motivo cuando sí.
    ///
    /// El «desconocido» es la **ausencia** del estado —`None`, el `NULL` de la columna—, no
    /// una variante más del enumerado: lo que se afirma es que una fila sin comprobar no
    /// finge un desenlace. La lectura con salud se escribe por SQL directo porque la
    /// comprobación no vive en esta capa; lo que se mide aquí es que el mapeo fila→struct
    /// la traiga, y que el texto almacenado —el del `CHECK`— sea el que corresponde al
    /// enumerado.
    #[tokio::test]
    async fn la_entidad_lee_la_salud_con_y_sin_anotar() {
        let (_directorio, pool, repositorios) = capa().await;

        let repositorio = repositorios
            .alta(alta("repo-1-2"))
            .await
            .expect("el alta no puede fallar");

        // Recién dada de alta: nadie la ha comprobado, así que no hay estado, ni fecha, ni
        // motivo. El alta tampoco los escribe.
        assert_eq!(
            repositorio.health, None,
            "una fila recién nacida no puede traer un desenlace de salud"
        );

        let sin_salud = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila recién dada de alta debe estar");

        assert_eq!(
            sin_salud.health, None,
            "sin comprobar, la salud tiene que ser «desconocido», no un estado inventado"
        );
        assert_eq!(
            sin_salud.health_checked_at, None,
            "no hay fecha sin comprobación"
        );
        assert_eq!(
            sin_salud.health_detail, None,
            "no hay motivo sin comprobación"
        );

        // La salud anotada, escrita donde la escribe la comprobación: en las columnas.
        let almacenada = "2026-05-06 07:08:09";

        sqlx::query(
            "UPDATE repositories SET health = 'sin_permiso', health_checked_at = ?1, \
             health_detail = 'el cubo no deja listar' WHERE id = ?2",
        )
        .bind(almacenada)
        .bind(repositorio.id.hyphenated().to_string())
        .execute(&pool)
        .await
        .expect("la anotación directa de la salud no puede fallar");

        let con_salud = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            con_salud.health,
            Some(SaludDeRepositorio::SinPermiso),
            "el estado almacenado no se leyó como su variante"
        );
        assert_eq!(
            con_salud.health_detail.as_deref(),
            Some("el cubo no deja listar"),
            "el motivo no llegó a la entidad"
        );

        // La fecha se descodifica como la fecha UTC que declara el texto, igual que el
        // `created_at` que puso la base.
        let esperada = PrimitiveDateTime::parse(
            almacenada,
            &format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        )
        .expect("el texto almacenado debe ser una fecha")
        .assume_utc();

        assert_eq!(
            con_salud.health_checked_at,
            Some(esperada),
            "la fecha de la comprobación no es la del texto almacenado: {almacenada}"
        );

        // Y el listado, que es el otro `SELECT` que nombra las columnas, trae lo mismo.
        let listado = repositorios
            .listar()
            .await
            .expect("el listado no puede fallar");

        assert_eq!(listado.len(), 1, "el listado tiene que traer la única fila");
        assert_eq!(
            listado[0].health,
            Some(SaludDeRepositorio::SinPermiso),
            "el listado no trae la salud de la fila"
        );
        assert_eq!(
            listado[0].health_detail.as_deref(),
            Some("el cubo no deja listar"),
            "el listado no trae el motivo de la fila"
        );
    }

    /// 1.3 (salud por repositorio) — La comprobación anota su desenlace —estado, motivo y
    /// fecha—, y **editar el repositorio no lo pisa**.
    ///
    /// Las dos mitades del contrato: `anotar_salud` escribe las tres columnas de la salud y
    /// la fecha la pone la base —UTC, como el `created_at`—, y `actualizar`, que reemplaza
    /// los campos mutables, **no nombra** esas columnas: la salud no es un campo que el
    /// operador edite. Se afirma llevando la salud incluso dentro de la fila que se pasa a
    /// `actualizar`, que es el caso en el que una implementación descuidada la reescribiría.
    #[tokio::test]
    async fn la_salud_se_anota_y_editar_el_repositorio_no_la_pisa() {
        let (_directorio, pool, repositorios) = capa().await;

        let repositorio = repositorios
            .alta(alta("repo-1-3"))
            .await
            .expect("el alta no puede fallar");

        assert!(
            repositorios
                .anotar_salud(
                    &repositorio.id,
                    SaludDeRepositorio::Passphrase,
                    Some("la passphrase no abre el repositorio"),
                )
                .await
                .expect("anotar la salud no puede fallar"),
            "la fila existía, así que anotar debe decir que sí"
        );

        let leido = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            leido.health,
            Some(SaludDeRepositorio::Passphrase),
            "el estado anotado no se leyó de vuelta"
        );
        assert_eq!(
            leido.health_detail.as_deref(),
            Some("la passphrase no abre el repositorio"),
            "el motivo anotado no se leyó de vuelta"
        );

        // Y el texto almacenado es el del `CHECK`, que es el del `snake_case` del enumerado:
        // la pareja columna/enum se afirma contra el literal, no contra el propio enum.
        let almacenado: String =
            sqlx::query_scalar("SELECT health FROM repositories WHERE id = ?1")
                .bind(repositorio.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el estado almacenado debe poder leerse");

        assert_eq!(
            almacenado, "passphrase",
            "la columna no guarda el texto que el CHECK admite"
        );

        let fecha = leido
            .health_checked_at
            .expect("la anotación tiene que dejar la fecha de la comprobación");

        assert!(
            fecha.offset().is_utc(),
            "la fecha la pone la base, en UTC: {fecha}"
        );

        let fecha_almacenada: String =
            sqlx::query_scalar("SELECT health_checked_at FROM repositories WHERE id = ?1")
                .bind(repositorio.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("la fecha almacenada debe poder leerse");

        let esperada = PrimitiveDateTime::parse(
            &fecha_almacenada,
            &format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        )
        .expect("el texto almacenado debe ser una fecha")
        .assume_utc();

        assert_eq!(
            fecha, esperada,
            "la fecha devuelta no es la que anotó la base: {fecha_almacenada}"
        );

        // Un desenlace sin motivo lo deja a nulo: `ok` no tiene nada que contar, y el `None`
        // es lo que lo dice —la cadena vacía diría otra cosa—.
        assert!(
            repositorios
                .anotar_salud(&repositorio.id, SaludDeRepositorio::Ok, None)
                .await
                .expect("anotar la salud no puede fallar"),
            "la fila existía, así que anotar debe decir que sí"
        );

        let con_ok = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(con_ok.health, Some(SaludDeRepositorio::Ok));
        assert_eq!(
            con_ok.health_detail, None,
            "un `ok` no tiene motivo, y la columna tiene que quedar a nulo"
        );

        let fecha_ok = con_ok.health_checked_at;

        // La edición del repositorio, con la salud **dentro** de la fila que se pasa.
        let mut cambiado = con_ok.clone();
        cambiado.name = "repo-1-3-editado".to_owned();

        assert!(
            repositorios
                .actualizar(&cambiado)
                .await
                .expect("la actualización no puede fallar"),
            "la fila existía, así que la actualización debe decir que sí"
        );

        let tras_editar = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            tras_editar.name, "repo-1-3-editado",
            "la edición no cambió lo que tenía que cambiar"
        );
        assert_eq!(
            tras_editar.health,
            Some(SaludDeRepositorio::Ok),
            "la edición pisó el estado de salud"
        );
        assert_eq!(
            tras_editar.health_checked_at, fecha_ok,
            "la edición pisó la fecha de la comprobación"
        );
        assert_eq!(
            tras_editar.health_detail, None,
            "la edición escribió un motivo donde no lo había"
        );

        // Y anotar sobre una fila que no está dice que no, como `actualizar` y `borrar`.
        assert!(
            !repositorios
                .anotar_salud(&Uuid::new_v4(), SaludDeRepositorio::Ok, None)
                .await
                .expect("anotar sobre una fila que no existe no es un error"),
            "anotar sin fila debe decir que no"
        );
    }

    /// 1.4 (salud por repositorio) — Los **cinco** estados del `CHECK` van y vuelven por la
    /// columna: `anotar_salud` escribe el texto y la relectura devuelve la variante.
    ///
    /// Es la mitad que las pruebas de arriba no cubrían: allí solo se ejercitaban `passphrase` y
    /// `ok`, así que una variante cuyo `snake_case` de `sqlx` no coincidiera con el `CHECK`
    /// —`sin_repositorio`, `inaccesible`, `sin_permiso`— habría pasado la suite. El texto esperado
    /// se afirma contra el **literal** que el `CHECK` admite, no contra el propio enumerado: si la
    /// derivación y la columna se separaran, la prueba lo vería.
    #[tokio::test]
    async fn los_cinco_estados_de_salud_van_y_vuelven_por_la_columna() {
        let (_directorio, pool, repositorios) = capa().await;

        let repositorio = repositorios
            .alta(alta("repo-1-4"))
            .await
            .expect("el alta no puede fallar");

        let estados = [
            (SaludDeRepositorio::Ok, "ok"),
            (SaludDeRepositorio::SinRepositorio, "sin_repositorio"),
            (SaludDeRepositorio::Passphrase, "passphrase"),
            (SaludDeRepositorio::Inaccesible, "inaccesible"),
            (SaludDeRepositorio::SinPermiso, "sin_permiso"),
        ];

        for (salud, texto) in estados {
            assert!(
                repositorios
                    .anotar_salud(&repositorio.id, salud, Some("motivo de la comprobación"))
                    .await
                    .expect("anotar la salud no puede fallar"),
                "la fila existía, así que anotar debe decir que sí"
            );

            let almacenado: String =
                sqlx::query_scalar("SELECT health FROM repositories WHERE id = ?1")
                    .bind(repositorio.id.hyphenated().to_string())
                    .fetch_one(&pool)
                    .await
                    .expect("el estado almacenado debe poder leerse");

            assert_eq!(
                almacenado, texto,
                "la columna no guarda el texto que el CHECK admite para {salud:?}"
            );

            let leido = repositorios
                .leer(&repositorio.id)
                .await
                .expect("la lectura no puede fallar")
                .expect("la fila debe estar");

            assert_eq!(
                leido.health,
                Some(salud),
                "el estado `{texto}` no volvió como su variante"
            );
        }
    }

    /// 1.24 — Una fila con `created_at` rellenado por `CURRENT_TIMESTAMP` se
    /// **descodifica** como la fecha UTC que declara el texto.
    #[tokio::test]
    async fn el_created_at_que_puso_la_base_se_descodifica_como_utc() {
        let (_directorio, pool, clave) = base_con_clave().await;

        let id = Uuid::new_v4();
        let secreto_cifrado = clave
            .cifrar(&secreto(SECRETO_ACCESO))
            .expect("cifrar no puede fallar");
        let passphrase_cifrada = clave
            .cifrar(&secreto(PASSPHRASE))
            .expect("cifrar no puede fallar");

        // SQL directo, **sin** informar `created_at`: lo pone el `CURRENT_TIMESTAMP`
        // del §3, que es lo que hay que saber descodificar.
        sqlx::query(
            "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
             secret_access_key, passphrase) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(id.hyphenated().to_string())
        .bind("repo-1-24")
        .bind("https://s3.example.test/repo-1-24")
        .bind("cubo-repo-1-24")
        .bind("eu-west-1")
        .bind("AKIAIOSFODNN7EXAMPLE")
        .bind(&secreto_cifrado)
        .bind(&passphrase_cifrada)
        .execute(&pool)
        .await
        .expect("la inserción directa no puede fallar");

        let almacenado: String =
            sqlx::query_scalar("SELECT created_at FROM repositories WHERE id = ?1")
                .bind(id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("la fecha almacenada debe poder leerse");

        let repositorio = Repositorios::nuevo(pool.clone(), clave)
            .leer(&id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert!(
            repositorio.created_at.offset().is_utc(),
            "el desplazamiento debe ser cero (UTC): {}",
            repositorio.created_at
        );

        let esperada = PrimitiveDateTime::parse(
            &almacenado,
            &format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        )
        .expect("el texto de la base debe ser una fecha")
        .assume_utc();

        assert_eq!(
            repositorio.created_at, esperada,
            "la fecha descodificada no es la del texto almacenado: {almacenado}"
        );
    }

    /// 1.35 — Un nombre con comillas simples y con `); DROP TABLE backup_jobs; --`
    /// llega **íntegro** a la fila, y las cuatro tablas del §3 siguen existiendo.
    #[tokio::test]
    async fn un_nombre_con_comillas_y_sql_llega_integro_y_no_rompe_el_esquema() {
        let (_directorio, pool, repositorios) = capa().await;

        let hostil = "O'Reilly ); DROP TABLE backup_jobs; --";

        let repositorio = repositorios
            .alta(alta(hostil))
            .await
            .expect("el alta no puede fallar");

        let leido = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(leido.name, hostil, "el nombre no llegó íntegro a la fila");

        for tabla in [
            "repositories",
            "backup_targets",
            "backup_jobs",
            "job_executions",
        ] {
            let existe: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            )
            .bind(tabla)
            .fetch_one(&pool)
            .await
            .expect("la consulta de tablas no puede fallar");

            assert_eq!(existe, 1, "la tabla {tabla} desapareció");
        }
    }

    /// 1.36 — El error de la capa de acceso, traducido a respuesta, no publica la
    /// causa: lleva el mensaje **genérico**.
    ///
    /// Es **exigente** a propósito: la causa **sí** lleva el texto de la consulta,
    /// así que queda demostrado que es la envoltura la que protege y no que el caso
    /// no se dé. El mensaje genérico se compara con el de otro `5xx` en vez de con un
    /// literal: la constante vive privada en `src/error.rs`, que no se toca.
    #[tokio::test]
    async fn el_error_de_acceso_no_filtra_ni_la_consulta_ni_el_secreto() {
        let (_directorio, pool, repositorios) = capa().await;

        // Se rompe el esquema por debajo: la consulta de la capa falla de verdad.
        sqlx::query("DROP TABLE repositories")
            .execute(&pool)
            .await
            .expect("la tabla debe poder borrarse para provocar el fallo");

        let error = repositorios
            .alta(alta("repo-1-36"))
            .await
            .expect_err("con la tabla borrada el alta no puede dar una fila");

        // La causa sí lleva el texto de la consulta: es lo que hace útil el registro.
        let causa = error.to_string();
        assert!(
            causa.contains("INSERT INTO repositories"),
            "la causa debe llevar el texto de la consulta para el registro: {causa}"
        );

        let respuesta = AppError::Database(error).into_response();

        assert_eq!(respuesta.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let texto = body_text(respuesta).await;

        assert!(
            !texto.contains("INSERT INTO repositories"),
            "el 500 publicó el texto de la consulta: {texto}"
        );
        assert!(
            !texto.contains(SECRETO_ACCESO),
            "el 500 publicó el secreto de acceso: {texto}"
        );
        assert!(
            !texto.contains(PASSPHRASE),
            "el 500 publicó la passphrase: {texto}"
        );

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 500 debe traer JSON en el cuerpo");

        let testigo = AppError::Other(String::from("testigo del mensaje genérico")).into_response();
        let cuerpo_testigo: serde_json::Value =
            serde_json::from_str(&body_text(testigo).await).expect("el testigo debe traer JSON");

        assert_eq!(
            cuerpo["message"], cuerpo_testigo["message"],
            "el fallo de base debe traer el mismo mensaje genérico que otro 5xx: {cuerpo}"
        );
    }

    /// 1.48 — El `Debug` de un repositorio **leído de la base** no publica ninguno
    /// de sus dos secretos, y dice que están redactados.
    #[tokio::test]
    async fn el_debug_de_un_repositorio_no_publica_los_secretos() {
        let (_directorio, _pool, repositorios) = capa().await;

        let dado_de_alta = repositorios
            .alta(alta("repo-1-48"))
            .await
            .expect("el alta no puede fallar");

        let leido = repositorios
            .leer(&dado_de_alta.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        let impreso = format!("{leido:?}");

        assert!(
            !impreso.contains(SECRETO_ACCESO),
            "el `Debug` publicó el secreto de acceso: {impreso}"
        );
        assert!(
            !impreso.contains(PASSPHRASE),
            "el `Debug` publicó la passphrase: {impreso}"
        );
        assert!(
            impreso.to_lowercase().contains("redact"),
            "el `Debug` no dice que los secretos están redactados: {impreso}"
        );

        // Y el valor solo sale por el método explícito.
        assert_eq!(leido.secret_access_key.expose(), SECRETO_ACCESO);
        assert_eq!(leido.passphrase.expose(), PASSPHRASE);
    }

    /// 1.33 — Borrar un **repositorio** deja sin tareas sus `backup_jobs` y sin
    /// ejecuciones esas tareas, **con el target intacto**.
    ///
    /// Es la cascada del otro lado de la 1.32: `backup_jobs` tiene dos claves foráneas
    /// con `ON DELETE CASCADE`, una por cada padre, y borrar cualquiera de los dos se
    /// lleva la tarea. El target no era de este repositorio, así que sigue ahí.
    #[tokio::test]
    async fn borrar_un_repositorio_arrastra_sus_tareas_y_sus_ejecuciones_y_deja_el_target() {
        let (_directorio, pool, clave) = base_con_clave().await;

        let repositorios = Repositorios::nuevo(pool.clone(), clave.clone());
        let targets = Targets::nuevo(pool.clone(), clave.clone());
        let tareas = Tareas::nuevo(pool.clone());
        let ejecuciones = Ejecuciones::nuevo(pool.clone(), clave);

        let repositorio = repositorios
            .alta(alta("repo-1-33"))
            .await
            .expect("el alta del repositorio no puede fallar");

        let target = targets
            .alta(alta_target_de_directorio("target-1-33"))
            .await
            .expect("el alta del target no puede fallar");

        let tarea = tareas
            .alta(alta_tarea(&target.id, &repositorio.id, "0 3 * * *"))
            .await
            .expect("el alta de la tarea no puede fallar");

        let ejecucion = ejecuciones
            .alta(alta_ejecucion(&tarea.id))
            .await
            .expect("el alta de la ejecución no puede fallar");

        assert!(
            repositorios
                .borrar(&repositorio.id)
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
            "las tareas del repositorio sobrevivieron; el `CASCADE` no ocurrió"
        );

        let ejecuciones_restantes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_executions")
            .fetch_one(&pool)
            .await
            .expect("el recuento de ejecuciones no puede fallar");

        assert_eq!(
            ejecuciones_restantes, 0,
            "las ejecuciones sobrevivieron a sus tareas"
        );

        assert!(
            tareas
                .leer(&tarea.id)
                .await
                .expect("la lectura no puede fallar")
                .is_none(),
            "la tarea sobrevivió al repositorio"
        );

        assert!(
            ejecuciones
                .leer(&ejecucion.id)
                .await
                .expect("la lectura no puede fallar")
                .is_none(),
            "la ejecución sobrevivió a su tarea"
        );

        // El target **no** era de este repositorio: sigue ahí.
        let target_sigue = targets
            .leer(&target.id)
            .await
            .expect("la lectura no puede fallar");

        assert!(
            target_sigue.is_some(),
            "el target no debía desaparecer: el que se borró fue el repositorio"
        );
    }

    /// 1.49 — Con `RUST_LOG=trace`, el alta, la lectura y la actualización no dejan en
    /// el registro ninguno de los tres secretos del §3 **ni su forma cifrada**.
    ///
    /// La forma cifrada se lee con SQL directo, que es la que está almacenada, y se busca
    /// literalmente en la salida capturada. Usa el arnés de captura **por hilo** de
    /// `src/test_support.rs`, nunca `set_default` ni `with_default`: un despachante de
    /// hilo deja un callsite cacheado como `Interest::never` para todo el proceso y el
    /// registro desaparece **en silencio**.
    #[tokio::test]
    async fn el_registro_de_una_lectura_no_publica_los_secretos() {
        let (buffer, _guard) = capturar_log();

        let (_directorio, pool, clave) = base_con_clave().await;

        let repositorios = Repositorios::nuevo(pool.clone(), clave.clone());
        let targets = Targets::nuevo(pool.clone(), clave);

        // Entre el repositorio y el target están los tres secretos del §3.
        let repositorio = repositorios
            .alta(alta("repo-1-49"))
            .await
            .expect("el alta del repositorio no puede fallar");

        let target = targets
            .alta(alta_target_con_password("target-1-49"))
            .await
            .expect("el alta del target no puede fallar");

        // Y su lectura y su actualización.
        let leido = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        let mut cambiado = leido.clone();
        cambiado.secret_access_key = secreto("AKIAIOSFODNN7EXAMPLE-rotado-1-49");
        repositorios
            .actualizar(&cambiado)
            .await
            .expect("la actualización no puede fallar");

        let leido_target = targets
            .leer(&target.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        targets
            .actualizar(&leido_target)
            .await
            .expect("la actualización no puede fallar");

        // La forma **almacenada** de los tres secretos: el escenario prohíbe registrarla
        // a ella también.
        let secreto_guardado: String =
            sqlx::query_scalar("SELECT secret_access_key FROM repositories WHERE id = ?1")
                .bind(repositorio.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el secreto almacenado debe poder leerse");

        let passphrase_guardada: String =
            sqlx::query_scalar("SELECT passphrase FROM repositories WHERE id = ?1")
                .bind(repositorio.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("la passphrase almacenada debe poder leerse");

        let password_guardado: String =
            sqlx::query_scalar("SELECT db_password FROM backup_targets WHERE id = ?1")
                .bind(target.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("la contraseña almacenada debe poder leerse");

        // El candado de ausencia solo vale si la captura está viva.
        tracing::trace!("captura en pie para la prueba 1.49");

        let log = buffer.texto();

        assert!(
            !log.is_empty(),
            "el buffer de captura está vacío, así que la ausencia no prueba nada"
        );
        assert!(
            log.contains("captura en pie para la prueba 1.49"),
            "la captura por hilo no recibió el registro, así que no prueba nada"
        );

        for (nombre, valor) in [
            ("el secreto de acceso en claro", SECRETO_ACCESO),
            ("la passphrase en claro", PASSPHRASE),
            ("la contraseña de base en claro", PASSWORD_DE_TARGET),
            ("el sobre del secreto de acceso", secreto_guardado.as_str()),
            ("el sobre de la passphrase", passphrase_guardada.as_str()),
            (
                "el sobre de la contraseña de base",
                password_guardado.as_str(),
            ),
        ] {
            assert!(!log.contains(valor), "el registro publica {nombre}: {log}");
        }
    }

    /// Un alta de target de **directorio** mínima, para las pruebas que cruzan tablas.
    fn alta_target_de_directorio(nombre: &str) -> AltaDeTarget {
        AltaDeTarget {
            name: nombre.to_owned(),
            target_type: TipoDeTarget::Directory,
            container_name: None,
            db_user: None,
            db_password: None,
            db_name: None,
            host_path: Some(format!("/srv/{nombre}")),
            db_path: None,
        }
    }

    /// El mismo target, pero de base de datos: es el que aporta el tercer secreto.
    fn alta_target_con_password(nombre: &str) -> AltaDeTarget {
        AltaDeTarget {
            target_type: TipoDeTarget::Mariadb,
            container_name: Some(format!("contenedor-{nombre}")),
            db_user: Some("usuario-de-base".to_owned()),
            db_password: Some(secreto(PASSWORD_DE_TARGET)),
            db_name: Some(format!("base-{nombre}")),
            host_path: None,
            db_path: None,
            ..alta_target_de_directorio(nombre)
        }
    }

    /// Un alta de tarea mínima, para las pruebas que cruzan tablas.
    fn alta_tarea(target_id: &Uuid, repository_id: &Uuid, cron: &str) -> AltaDeTarea {
        AltaDeTarea {
            target_id: *target_id,
            repository_id: *repository_id,
            cron_expression: cron.to_owned(),
            keep_daily: 7,
            keep_weekly: 4,
            keep_monthly: 12,
            enabled: true,
            name: nombre_de_tarea_de_prueba(),
        }
    }

    /// Un alta de ejecución mínima, para las pruebas que cruzan tablas.
    fn alta_ejecucion(job_id: &Uuid) -> AltaDeEjecucion {
        AltaDeEjecucion {
            job_id: *job_id,
            status: Estado::Pending,
            snapshot_id: None,
            bytes_processed: 0,
            bytes_uploaded: 0,
            error_message: None,
            started_at: OffsetDateTime::now_utc(),
            finished_at: None,
        }
    }

    /// `actualizar` y `borrar` sobre una fila que **no existe** devuelven `false`, no error.
    ///
    /// Es el contrato documentado de las dos: `false` significa «no había nada que hacer»,
    /// que no es un fallo. Es el mismo trato que [`Repositorios::leer`] da al «no está».
    #[tokio::test]
    async fn actualizar_y_borrar_sin_fila_devuelven_false() {
        let (_directorio, _pool, repositorios) = capa().await;

        let fantasma = Uuid::new_v4();

        assert!(
            !repositorios
                .actualizar(&Repositorio {
                    id: fantasma,
                    name: "repo-sin-fila".to_owned(),
                    endpoint: "https://s3.example.test/repo-sin-fila".to_owned(),
                    bucket: "cubo-sin-fila".to_owned(),
                    region: "eu-west-1".to_owned(),
                    access_key_id: "AKIAIOSFODNN7EXAMPLE".to_owned(),
                    secret_access_key: secreto(SECRETO_ACCESO),
                    passphrase: secreto(PASSPHRASE),
                    created_at: OffsetDateTime::now_utc(),
                    health: None,
                    health_checked_at: None,
                    health_detail: None,
                })
                .await
                .expect("actualizar una fila que no existe no es un error"),
            "actualizar sin fila debe decir que no"
        );

        assert!(
            !repositorios
                .borrar(&fantasma)
                .await
                .expect("borrar una fila que no existe no es un error"),
            "borrar sin fila debe decir que no"
        );
    }

    /// Un secreto **vacío** se rechaza al escribir y **no deja fila**.
    ///
    /// Es la mitad simétrica del «nunca un secreto vacío» del descifrado: sin esta guarda,
    /// un campo en blanco sustituiría la credencial por el cifrado de nada, sin señal, y
    /// el fallo aparecería más tarde, en el motor, hablando de S3.
    #[tokio::test]
    async fn un_secreto_vacio_se_rechaza_y_no_deja_fila() {
        let (_directorio, pool, repositorios) = capa().await;

        let error = repositorios
            .alta(AltaDeRepositorio {
                secret_access_key: secreto(""),
                ..alta("repo-t4-vacio")
            })
            .await
            .expect_err("un secreto de acceso vacío no puede darse de alta");

        assert!(
            matches!(error, DbError::SecretoVacio { campo } if campo == "secret_access_key"),
            "el fallo debe nombrar el campo vacío: {error}"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories")
            .fetch_one(&pool)
            .await
            .expect("el recuento no puede fallar");

        assert_eq!(filas, 0, "la fila se escribió pese al secreto vacío");

        let error = repositorios
            .alta(AltaDeRepositorio {
                passphrase: secreto(""),
                ..alta("repo-t4-vacio-dos")
            })
            .await
            .expect_err("una passphrase vacía no puede darse de alta");

        assert!(
            matches!(error, DbError::SecretoVacio { campo } if campo == "passphrase"),
            "el fallo debe nombrar el campo vacío: {error}"
        );

        // Y por el camino de la actualización: la fila existe y el rechazo no la toca.
        let repositorio = repositorios
            .alta(alta("repo-t4-ok"))
            .await
            .expect("el alta no puede fallar");

        let mut vacio = repositorio.clone();
        vacio.secret_access_key = secreto("");

        let error = repositorios
            .actualizar(&vacio)
            .await
            .expect_err("la actualización no puede guardar un secreto vacío");

        assert!(
            matches!(error, DbError::SecretoVacio { campo } if campo == "secret_access_key"),
            "el fallo debe nombrar el campo vacío: {error}"
        );

        let leido = repositorios
            .leer(&repositorio.id)
            .await
            .expect("la lectura no puede fallar")
            .expect("la fila debe estar");

        assert_eq!(
            leido.secret_access_key.expose(),
            SECRETO_ACCESO,
            "el rechazo tocó la fila: el secreto original se perdió"
        );
    }

    /// El descifrado que falla **atravesando la capa de acceso**.
    ///
    /// Es el camino que describen los escenarios del `secrets/spec.md` —«cuando se lee la
    /// fila **por la capa de acceso**»— y el único que ejerce [`DbError::Cifrado`]: una
    /// fila con el sobre roto entra por `desde_fila`, el `?` encadena `ErrorDeCifrado →
    /// DbError`, y el resultado es un error **tipado**, no un `Ok` con un secreto vacío.
    ///
    /// La fila se escribe con **SQL directo**, que es la única forma de dejar un sobre
    /// roto: la capa de acceso no produce uno. Se cubren los dos casos que el contrato
    /// nombra, el sobre corrupto y el valor en claro sin sobre.
    #[tokio::test]
    async fn el_descifrado_que_falla_llega_como_error_tipado() {
        let (_directorio, pool, clave) = base_con_clave().await;
        let repositorios = Repositorios::nuevo(pool.clone(), clave.clone());

        // La passphrase sí es un sobre legítimo: lo que se rompe es el otro secreto, para
        // que el fallo sea del caso que se prueba y no de la fila entera.
        let passphrase_legitima = clave
            .cifrar(&secreto(PASSPHRASE))
            .expect("cifrar no puede fallar");

        // El `'v1:AAAA:BBBB'` de la comprobación de operador 4.8, y un valor en claro.
        for (marca, secreto_guardado) in [
            ("roto", "v1:AAAA:BBBB"),
            ("claro", "AKIAIOSFODNN7EXAMPLE-en-claro-en-la-columna"),
        ] {
            let id = Uuid::new_v4();

            sqlx::query(
                "INSERT INTO repositories (id, name, endpoint, bucket, region, access_key_id, \
                 secret_access_key, passphrase) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(id.hyphenated().to_string())
            .bind(format!("repo-1-t3-{marca}"))
            .bind("https://s3.example.test/repo-1-t3")
            .bind("cubo-1-t3")
            .bind("eu-west-1")
            .bind("AKIAIOSFODNN7EXAMPLE")
            .bind(secreto_guardado)
            .bind(&passphrase_legitima)
            .execute(&pool)
            .await
            .expect("la inserción directa no puede fallar");

            let error = repositorios
                .leer(&id)
                .await
                .expect_err("un secreto ilegible no puede dar un repositorio");

            assert!(
                matches!(error, DbError::Cifrado(_)),
                "el fallo debe llegar como error de cifrado, no como otro ({marca}): {error}"
            );
        }
    }
}
