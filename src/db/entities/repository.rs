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
const SQL_LEER: &str = "SELECT id, name, endpoint, bucket, region, access_key_id, secret_access_key, \
     passphrase, created_at FROM repositories WHERE id = ?1";

/// Listado de todos los repositorios.
///
/// `ORDER BY created_at, id` y **no** `created_at` a secas: las filas dadas de alta en
/// el mismo segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y el orden
/// entre dos llamadas seguidas dejaría de ser estable. El `id` es el desempate.
const SQL_LISTAR: &str = "SELECT id, name, endpoint, bucket, region, access_key_id, secret_access_key, \
     passphrase, created_at FROM repositories ORDER BY created_at, id";

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

        desde_fila(&fila, &self.clave)
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
    /// Los dos secretos se **vuelven a cifrar**, así que la fila estrena sobre y nonce
    /// aunque el valor en claro no cambie: el nonce es aleatorio por valor, y eso es lo
    /// correcto —repetirlo con la misma clave rompería el AEAD—. Consecuencia práctica:
    /// el texto almacenado cambia en cada actualización, y quien compare bytes verá una
    /// diferencia donde no la hay en el secreto.
    pub async fn actualizar(&self, repositorio: &Repositorio) -> Result<bool, DbError> {
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

/// Mapea una fila de `repositories` a la entidad, **descifrando** sus dos secretos.
///
/// Es el único sitio del crate que ve el sobre almacenado: el resto de la aplicación
/// solo conoce valores en claro dentro de un [`SecretString`].
fn desde_fila(fila: &SqliteRow, clave: &Clave) -> Result<Repositorio, DbError> {
    // El `id` viaja como `TEXT` con guiones, así que la conversión a `Uuid` es del
    // mapeo y no de `sqlx`. Su error **no** es un `sqlx::Error`, y envolverlo en
    // `sqlx::Error::Decode` es lo que lo hace viajar por `DbError::Consulta` con la
    // causa intacta, sin estrenar una variante para un fallo que ya tiene sitio.
    let id = columna::<String>(fila, "id")?;
    let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO.to_owned(),
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
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use sqlx::SqlitePool;
    use time::PrimitiveDateTime;
    use time::macros::format_description;
    use uuid::Uuid;

    use super::{AltaDeRepositorio, Repositorios};
    use crate::error::AppError;
    use crate::secret::SecretString;
    use crate::test_support::{DirectorioDePrueba, base_con_clave, body_text};

    /// Secreto de acceso distinguible y largo: si aparece en una salida, la salida
    /// es el fallo.
    const SECRETO_ACCESO: &str = "AKIAIOSFODNN7EXAMPLE-repositories-1";
    /// Passphrase distinguible y larga, por el mismo motivo.
    const PASSPHRASE: &str = "passphrase-distinguible-0000000000000000000000001";

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
}
