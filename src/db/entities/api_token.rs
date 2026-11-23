//! La tabla `api_tokens` del change `mcp`: la entidad, el alta y la capa de acceso.
//!
//! Es la tabla de la credencial de máquina que permite llamar a la API sin pasar por el
//! navegador. La decisión que la condiciona: **solo se guarda el hash** del token, nunca su
//! valor en claro —eso lo hace quien lo emite, que lo entrega una única vez—, así que aquí
//! no hay ningún secreto cifrado en reposo como en `repositories` o `backup_targets`: no hay
//! nada que descifrar porque no hay nada que valga la pena descifrar.
//!
//! Por eso esta capa **no recibe la clave**: no tiene ninguna columna secreta, y pasarle una
//! que no usa sugeriría que aquí se cifra algo.
//!
//! `scopes` es el vocabulario cerrado del permiso —`read` y `run`— y viaja como el texto que
//! el `CHECK` del DDL acepta: `read`, `run` o `read,run`, en ese orden canónico. La capa
//! escribe siempre el texto canónico y este `CHECK` es el que decide si lo es.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteRow;
use time::{OffsetDateTime, UtcOffset};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::db::DbError;

/// Un permiso del vocabulario cerrado de un token.
///
/// Son dos y no más: `read` cubre las lecturas y `run`, las lecturas **y** las acciones que
/// disparan trabajo. No hay ningún scope que habilite restaurar ni borrar: esas operaciones
/// quedan fuera del alcance de cualquier token por diseño, así que no tienen palabra aquí.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Consultar el estado sin modificarlo.
    Read,
    /// Disparar trabajo: lanzar una copia y comprobar la salud de un repositorio.
    Run,
}

impl Scope {
    /// El texto con el que el scope viaja por el cable y se guarda en la columna.
    pub fn texto(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Run => "run",
        }
    }

    /// El scope que corresponde a un texto, o `None` si está fuera del vocabulario.
    pub fn desde_texto(texto: &str) -> Option<Self> {
        match texto {
            "read" => Some(Scope::Read),
            "run" => Some(Scope::Run),
            _ => None,
        }
    }
}

/// El texto canónico con el que se guarda un conjunto de scopes, o `None` si no hay ninguno.
///
/// El orden es **siempre** el mismo —la lectura primero— y las repeticiones no caben: el
/// `CHECK` del DDL acepta `read`, `run` y `read,run`, así que el texto canónico es lo que
/// hace que un conjunto se escriba de una sola forma y que dos filas con los mismos permisos
/// no se distingan por cómo se escribieron.
///
/// Un conjunto **vacío** no tiene texto canónico: devolver `""` sería devolver un valor que el
/// `CHECK` rechaza, y dejar que lo escriba la base obligaría a que el rechazo dependiera de la
/// validación de la capa HTTP. El `None` lo dice aquí, donde el error habla del campo.
pub fn scopes_a_texto(scopes: &[Scope]) -> Option<String> {
    let mut partes = Vec::new();

    if scopes.contains(&Scope::Read) {
        partes.push(Scope::Read.texto());
    }
    if scopes.contains(&Scope::Run) {
        partes.push(Scope::Run.texto());
    }

    if partes.is_empty() {
        return None;
    }

    Some(partes.join(","))
}

/// El conjunto de scopes de un texto almacenado.
///
/// El `CHECK` del DDL garantiza que el texto es uno de los tres admitidos, así que esto solo
/// puede fallar ante una fila escrita por fuera; aun así falla en alto y con su causa, en vez
/// de interpretar un valor desconocido como «sin permisos», que abriría una fila corrupta en
/// silencio.
fn scopes_desde_texto(texto: &str) -> Result<Vec<Scope>, DbError> {
    let mut scopes = Vec::new();

    for parte in texto.split(',') {
        let scope = Scope::desde_texto(parte).ok_or_else(scope_desconocido)?;

        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }

    Ok(scopes)
}

/// El fallo de mapeo de un `scopes` que no es del vocabulario cerrado.
fn scope_desconocido() -> DbError {
    DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "`scopes` trae un valor fuera del vocabulario cerrado",
        ))),
    }
}

/// Fila de `api_tokens`, con sus scopes ya resueltos.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenDeApi {
    /// UUIDv4 en forma canónica con guiones.
    pub id: Uuid,
    /// Nombre con el que su dueño reconoce el token.
    pub name: String,
    /// Hash del token: nunca el valor en claro.
    pub token_hash: String,
    /// Permisos concedidos, ya resueltos al vocabulario.
    pub scopes: Vec<Scope>,
    /// Fecha de emisión, en UTC.
    pub created_at: OffsetDateTime,
    /// Cuándo deja de valer; `None` si no caduca.
    pub expires_at: Option<OffsetDateTime>,
    /// Cuándo se revocó; `None` mientras sigue en vigor.
    pub revoked_at: Option<OffsetDateTime>,
    /// Cuándo se usó por última vez; `None` si no se ha usado.
    pub last_used_at: Option<OffsetDateTime>,
}

impl TokenDeApi {
    /// ¿Sigue valiendo este token en el instante dado?
    ///
    /// Un token vale si no se ha revocado y, si caduca, mientras no haya llegado su caducidad.
    /// La comprobación va **en el momento de la petición** y no se cachea nada: el contrato
    /// pide que la revocación surta efecto en la petición siguiente, sin ventana de gracia.
    pub fn en_vigor_en(&self, ahora: OffsetDateTime) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|caduca| caduca > ahora)
    }

    /// ¿Concede este token el scope pedido?
    ///
    /// `run` cubre las lecturas —el contrato lo dice—, así que un token de acción puede
    /// consultar lo que va a accionar sin que haga falta concederle además `read`.
    pub fn tiene(&self, requerido: Scope) -> bool {
        self.scopes.iter().any(|concedido| match concedido {
            Scope::Run => matches!(requerido, Scope::Read | Scope::Run),
            Scope::Read => requerido == Scope::Read,
        })
    }
}

/// El `Debug` **redacta** el hash.
///
/// La entidad nunca guarda el valor en claro, y el hash tampoco tiene por qué acabar en una
/// línea de registro: es el material con el que se verifica un token entrante, y una traza no
/// es sitio para él. El resto de la fila sí se imprime: es lo que un diagnóstico necesita ver.
impl std::fmt::Debug for TokenDeApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenDeApi")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("token_hash", &"redactado")
            .field("scopes", &self.scopes)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("revoked_at", &self.revoked_at)
            .field("last_used_at", &self.last_used_at)
            .finish()
    }
}

/// Los datos de un alta. **Sin `id`**: lo genera la capa de acceso.
#[derive(Clone)]
pub struct AltaDeToken {
    /// Nombre con el que su dueño reconoce el token.
    pub name: String,
    /// Valor en claro del token; se hashea internamente con la clave HMAC de `Tokens`.
    pub en_claro: String,
    /// Permisos concedidos.
    pub scopes: Vec<Scope>,
    /// Cuándo deja de valer; `None` si no caduca.
    pub expires_at: Option<OffsetDateTime>,
}

/// El `Debug` **redacta** el hash, igual que el de [`TokenDeApi`].
///
/// Un alta lleva el valor en claro del token —el que se convierte en el hash de la fila—,
/// así que imprimirlo entero sería una fuga: una traza de un alta fallida no es sitio para
/// él. El resto del alta sí se imprime.
impl std::fmt::Debug for AltaDeToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AltaDeToken")
            .field("name", &self.name)
            .field("en_claro", &"redactado")
            .field("scopes", &self.scopes)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// El hash con el que se guarda y se verifica un token.
///
/// Es un **HMAC-SHA256** del valor en claro con la clave `kesto/api-tokens/v1`
/// derivada de `KESTO_MASTER_KEY`, codificado en base64: determinista —el mismo
/// token con la misma clave da siempre el mismo hash, que es lo que permite
/// verificar uno entrante sin guardar el original— y de un solo sentido, así que de
/// la columna no se recupera el token. No lleva sal: el valor en claro ya es
/// aleatorio y largo, y una sal por fila obligaría a leer la fila para poder
/// verificar, que es justo lo que el hash evita. La clave HMAC impide que un atacante
/// con acceso de lectura a la tabla pueda precomputar hashes de tokens.
pub fn hash_de_token(en_claro: &str, key: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    // `new_from_slice` **no puede fallar** con HMAC: acepta cualquier longitud de
    // clave. Aun así no se usa `expect` —no hay pánicos en producción—: el brazo
    // de error es inalcanzable y devuelve la cadena vacía, que el `CHECK` de la
    // tabla rechazaría antes de guardarla.
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return String::new();
    };
    mac.update(en_claro.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// La capa de acceso de `api_tokens`.
///
/// **No** recibe la clave de cifrado: no tiene ninguna columna secreta que descifrar.
pub struct Tokens {
    pool: SqlitePool,
    /// Clave HMAC-SHA256 derivada de `KESTO_MASTER_KEY` con etiqueta
    /// `kesto/api-tokens/v1`. Se usa para hashear los tokens al alta y verificar
    /// uno entrante.
    ///
    /// Va en un [`Zeroizing`] porque es material secreto: la política del crate es
    /// borrarlo al caer, igual que la clave de cifrado de `Clave`.
    hmac_key: Zeroizing<[u8; 32]>,
}

// Las sentencias, como constantes: `DbError::Consulta` lleva el texto de la consulta, así que
// la que se ejecuta y la que viaja en el error tienen que ser **la misma cadena**. Los valores
// van **enlazados** (`?1`, `?2`…): ninguna se compone concatenando.

/// Columnas del `SELECT`, en el orden del DDL —el mismo que recorre [`desde_fila`]—.
const SQL_LEER_POR_HASH: &str = "SELECT id, name, token_hash, scopes, created_at, expires_at, \
     revoked_at, last_used_at FROM api_tokens WHERE token_hash = ?1";

/// Listado de todos los tokens.
///
/// `ORDER BY created_at, id` y **no** `created_at` a secas: las filas emitidas en el mismo
/// segundo empatan —`CURRENT_TIMESTAMP` no tiene más resolución— y el `id` es el desempate,
/// para que el orden entre dos llamadas seguidas sea estable.
const SQL_LISTAR: &str = "SELECT id, name, token_hash, scopes, created_at, expires_at, \
     revoked_at, last_used_at FROM api_tokens ORDER BY created_at, id";

/// Alta de un token, devolviendo la fila tal y como quedó.
///
/// El `RETURNING` evita el segundo viaje: el `created_at` es el que puso la base.
const SQL_ALTA: &str = "INSERT INTO api_tokens (id, name, token_hash, scopes, expires_at) \
     VALUES (?1, ?2, ?3, ?4, ?5) \
     RETURNING id, name, token_hash, scopes, created_at, expires_at, revoked_at, last_used_at";

/// Anotación del último uso, sin tocar nada más.
const SQL_ANOTAR_USO: &str = "UPDATE api_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?1";

/// Revocación **idempotente**: la primera vez escribe la fecha; las siguientes la conservan.
///
/// `COALESCE(revoked_at, CURRENT_TIMESTAMP)` es lo que la hace idempotente y **no** un
/// «leer y luego escribir»: la fecha de la primera revocación se queda escrita, y una
/// segunda llamada no la mueve. La fila **no** se borra: se conserva para poder auditar
/// cuándo se emitió y cuándo se revocó.
const SQL_REVOCAR: &str = "UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, CURRENT_TIMESTAMP) \
     WHERE id = ?1";

/// Texto del error cuando falla el **mapeo** de una fila ya traída.
///
/// No es una sentencia ejecutada —la fila ya llegó—, así que no se confunde con las `SQL_*`:
/// dice dónde falló, que es leer o interpretar una columna.
const CONSULTA_MAPEO: &str = "mapeo de la fila de api_tokens";

impl Tokens {
    /// Construye la capa de acceso sobre un pool.
    ///
    /// La clave entra por valor y se envuelve en [`Zeroizing`] aquí: el campo de la
    /// capa —el que vive todo el proceso— queda bajo la política de borrado del
    /// crate.
    pub fn nuevo(pool: SqlitePool, hmac_key: [u8; 32]) -> Self {
        Tokens {
            pool,
            hmac_key: Zeroizing::new(hmac_key),
        }
    }

    /// Da de alta un token y devuelve la fila tal y como quedó.
    pub async fn alta(&self, alta: AltaDeToken) -> Result<TokenDeApi, DbError> {
        // El identificador se genera **aquí**, no en la base: SQLite no tiene `uuid()`.
        let id = Uuid::new_v4();

        // Un conjunto sin scopes no tiene texto canónico: se dice **aquí**, donde el error nombra
        // el campo, en vez de enlazar el vacío y dejar que el `CHECK` lo delate con un mensaje que
        // habla de la tabla. La validación de la capa HTTP no es la que sostiene esta guarda.
        let Some(scopes) = scopes_a_texto(&alta.scopes) else {
            return Err(DbError::ScopesVacios);
        };

        let fila = sqlx::query(SQL_ALTA)
            // El `id` va como texto con guiones: un `bind` de `Uuid` lo escribiría como un
            // BLOB de 16 bytes y el DDL dice `TEXT`.
            .bind(id.hyphenated().to_string())
            .bind(&alta.name)
            .bind(hash_de_token(&alta.en_claro, &self.hmac_key[..]))
            .bind(scopes)
            // La fecha se normaliza a UTC **al enlazarla**: el `Encode` de `time` conserva el
            // desplazamiento del valor y el §3 dice que las fechas son UTC, así que un llamante
            // con `+02:00` no puede dejar la columna fuera de UTC. Es el mismo instante, en otra
            // representación —igual que en `execution.rs`.
            .bind(alta.expires_at.map(|fecha| fecha.to_offset(UtcOffset::UTC)))
            .fetch_one(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ALTA))?;

        desde_fila(&fila)
    }

    /// Busca un token por el hash de su valor en claro: `Ok(None)` si no está.
    ///
    /// Es el camino de la verificación: quien recibe un `Bearer` hashea lo que le llegó y
    /// pregunta por él. «No está» y «no se pudo leer» son cosas distintas: lo primero es
    /// `Ok(None)` —un token que no se emitió, o uno revocado, que sigue estando— y lo segundo,
    /// un error.
    pub async fn buscar_por_hash(&self, hash: &str) -> Result<Option<TokenDeApi>, DbError> {
        let fila = sqlx::query(SQL_LEER_POR_HASH)
            .bind(hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LEER_POR_HASH))?;

        match fila {
            Some(fila) => desde_fila(&fila).map(Some),
            None => Ok(None),
        }
    }

    /// Lista todos los tokens con un orden **estable**.
    pub async fn listar(&self) -> Result<Vec<TokenDeApi>, DbError> {
        let filas = sqlx::query(SQL_LISTAR)
            .fetch_all(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_LISTAR))?;

        filas.iter().map(desde_fila).collect()
    }

    /// Anota el último uso de un token.
    ///
    /// Es informativo —auditar cuándo se usó por última vez— y **no** entra en ninguna
    /// decisión de permiso: quien lo llama no debe hacer depender de esto que la petición
    /// siga o no.
    pub async fn anotar_uso(&self, id: &Uuid) -> Result<(), DbError> {
        sqlx::query(SQL_ANOTAR_USO)
            .bind(id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ANOTAR_USO))?;

        Ok(())
    }

    /// Revoca el token y devuelve si la fila existía.
    ///
    /// Es **idempotente**: revocar un token ya revocado conserva la fecha de la primera
    /// revocación y no falla. La fila no se borra.
    pub async fn revocar(&self, id: &Uuid) -> Result<bool, DbError> {
        let resultado = sqlx::query(SQL_REVOCAR)
            .bind(id.hyphenated().to_string())
            .execute(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_REVOCAR))?;

        Ok(resultado.rows_affected() == 1)
    }
}

/// Envuelve un fallo de `sqlx` con **la misma** sentencia que se ejecutó.
fn error_de_consulta(consulta: &'static str) -> impl FnOnce(sqlx::Error) -> DbError {
    move |causa| DbError::Consulta { consulta, causa }
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

/// Mapea una fila de `api_tokens` a la entidad, resolviendo sus scopes.
fn desde_fila(fila: &SqliteRow) -> Result<TokenDeApi, DbError> {
    // El `id` viaja como `TEXT` con guiones, así que la conversión a `Uuid` es del mapeo y no
    // de `sqlx`.
    let id = columna::<String>(fila, "id")?;
    let id = Uuid::parse_str(&id).map_err(|causa| DbError::Consulta {
        consulta: CONSULTA_MAPEO,
        causa: sqlx::Error::Decode(Box::new(causa)),
    })?;

    Ok(TokenDeApi {
        id,
        name: columna(fila, "name")?,
        token_hash: columna(fila, "token_hash")?,
        scopes: scopes_desde_texto(&columna::<String>(fila, "scopes")?)?,
        created_at: columna(fila, "created_at")?,
        expires_at: columna(fila, "expires_at")?,
        revoked_at: columna(fila, "revoked_at")?,
        last_used_at: columna(fila, "last_used_at")?,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::{AltaDeToken, Scope, Tokens, hash_de_token, scopes_a_texto};
    use crate::db::crypto::Clave;
    use crate::secret::SecretString;
    use crate::test_support::{DirectorioDePrueba, MASTER_KEY, base_con_clave};

    /// Valor en claro distinguible y largo: si aparece en una salida, la salida es el fallo.
    const EN_CLARO: &str = "kesto_en-claro-distinguible-0000000000000001";
    /// Otro valor en claro, para comprobar que un hash no casa con un token distinto.
    const OTRO_EN_CLARO: &str = "kesto_otro-en-claro-distinguible-000000000002";

    /// La capa sobre una base de prueba migrada, con la guarda del directorio viva.
    async fn capa() -> (DirectorioDePrueba, SqlitePool, Tokens) {
        let (directorio, pool, clave) = base_con_clave().await;
        let tokens = Tokens::nuevo(pool.clone(), *clave.api_token_key());

        (directorio, pool, tokens)
    }

    /// Un alta de token con el valor en claro, el nombre y los scopes dados.
    ///
    /// El valor en claro entra como parámetro —y no como constante fija— porque el hash es
    /// `UNIQUE`: dos altas con el mismo valor chocarían en la tabla, así que cada token de una
    /// misma prueba tiene que nacer de un valor distinto.
    fn alta(en_claro: &str, nombre: &str, scopes: &[Scope]) -> AltaDeToken {
        AltaDeToken {
            name: nombre.to_owned(),
            en_claro: en_claro.to_owned(),
            scopes: scopes.to_vec(),
            expires_at: None,
        }
    }

    /// 1.1/2.2 — El ciclo de un token: nace con su hash, se busca por él, se anota su uso y
    /// se revoca.
    #[tokio::test]
    async fn el_ciclo_de_un_token_guarda_solo_su_hash() {
        let (_directorio, pool, tokens) = capa().await;

        let dado_de_alta = tokens
            .alta(alta(EN_CLARO, "agente-lector", &[Scope::Read]))
            .await
            .expect("el alta del token no puede fallar");

        // La fila guarda el **hash** y solo el hash: el valor en claro no entra en la capa.
        let guardado: String =
            sqlx::query_scalar("SELECT token_hash FROM api_tokens WHERE id = ?1")
                .bind(dado_de_alta.id.hyphenated().to_string())
                .fetch_one(&pool)
                .await
                .expect("el hash del token debe poder leerse");

        assert_eq!(
            guardado,
            hash_de_token(EN_CLARO, &tokens.hmac_key[..]),
            "la fila no guardó el hash del token"
        );
        assert_ne!(
            guardado, EN_CLARO,
            "la fila guardó el valor en claro en vez de su hash"
        );

        // Se verifica por el hash del token entrante, y un token distinto no casa.
        let encontrado = tokens
            .buscar_por_hash(&hash_de_token(EN_CLARO, &tokens.hmac_key[..]))
            .await
            .expect("la búsqueda por hash no puede fallar")
            .expect("el token recién dado de alta debe encontrarse por su hash");

        assert_eq!(encontrado.id, dado_de_alta.id);
        assert_eq!(encontrado.name, "agente-lector");
        assert_eq!(encontrado.scopes, vec![Scope::Read]);
        assert!(
            encontrado.last_used_at.is_none(),
            "un token recién creado no se ha usado todavía"
        );

        assert!(
            tokens
                .buscar_por_hash(&hash_de_token(OTRO_EN_CLARO, &tokens.hmac_key[..]))
                .await
                .expect("la búsqueda no puede fallar")
                .is_none(),
            "un token que no se ha emitido no puede resolver"
        );

        // El listado trae la fila, sin el valor en claro.
        let listado = tokens.listar().await.expect("el listado no puede fallar");
        assert_eq!(listado.len(), 1, "el listado debe traer el token");
        assert_eq!(listado[0].id, dado_de_alta.id);

        // El uso se anota, y el token sigue en vigor.
        tokens
            .anotar_uso(&dado_de_alta.id)
            .await
            .expect("anotar el uso no puede fallar");

        let usado = tokens
            .buscar_por_hash(&hash_de_token(EN_CLARO, &tokens.hmac_key[..]))
            .await
            .expect("la búsqueda no puede fallar")
            .expect("el token debe seguir");

        assert!(
            usado.last_used_at.is_some(),
            "el uso del token no quedó anotado"
        );
        assert!(usado.revoked_at.is_none(), "anotar el uso no lo revoca");

        // La revocación marca la fecha y **no** borra la fila.
        assert!(
            tokens
                .revocar(&dado_de_alta.id)
                .await
                .expect("la revocación no puede fallar"),
            "revocar una fila que existe tiene que decir que sí"
        );

        let revocado = tokens
            .buscar_por_hash(&hash_de_token(EN_CLARO, &tokens.hmac_key[..]))
            .await
            .expect("la búsqueda no puede fallar")
            .expect("la fila revocada sigue en la base para poder auditarla");

        assert!(
            revocado.revoked_at.is_some(),
            "la revocación no dejó su fecha"
        );

        // Revocar dos veces no es un error y no borra nada.
        assert!(
            tokens
                .revocar(&dado_de_alta.id)
                .await
                .expect("revocar dos veces no puede fallar"),
            "revocar un token ya revocado tiene que ser idempotente"
        );

        let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_tokens")
            .fetch_one(&pool)
            .await
            .expect("el recuento no puede fallar");

        assert_eq!(
            filas, 1,
            "la revocación no puede borrar la fila: se conserva para auditar"
        );
    }

    /// 2.1 — Los scopes se guardan como el texto canónico que el `CHECK` acepta.
    #[tokio::test]
    async fn los_scopes_se_guardan_como_texto_del_vocabulario() {
        let (_directorio, pool, tokens) = capa().await;

        let lector = tokens
            .alta(alta(
                "kesto_lector-000000000000000000000001",
                "lector",
                &[Scope::Read],
            ))
            .await
            .expect("el alta no puede fallar");
        let operador = tokens
            .alta(alta(
                "kesto_operador-000000000000000000000002",
                "operador",
                &[Scope::Run],
            ))
            .await
            .expect("el alta no puede fallar");
        // El token completo se da de alta con `EN_CLARO`: es por el que se relee al final.
        let completo = tokens
            .alta(alta(EN_CLARO, "completo", &[Scope::Run, Scope::Read]))
            .await
            .expect("el alta no puede fallar");

        for (id, esperado) in [
            (lector.id, "read"),
            (operador.id, "run"),
            (completo.id, "read,run"),
        ] {
            let almacenado: String =
                sqlx::query_scalar("SELECT scopes FROM api_tokens WHERE id = ?1")
                    .bind(id.hyphenated().to_string())
                    .fetch_one(&pool)
                    .await
                    .expect("los scopes deben poder leerse");

            assert_eq!(
                almacenado, esperado,
                "los scopes no se guardaron en el texto canónico del vocabulario"
            );
        }

        // La ida y vuelta: el texto vuelve a ser el conjunto del enum.
        let releido = tokens
            .buscar_por_hash(&hash_de_token(EN_CLARO, &tokens.hmac_key[..]))
            .await
            .expect("la búsqueda no puede fallar")
            .expect("el token debe estar");

        assert_eq!(releido.scopes, vec![Scope::Read, Scope::Run]);
    }

    /// 2.1 — Un scope fuera del vocabulario **no entra** en la tabla: el `CHECK` lo rechaza.
    ///
    /// Es lo que hace que la columna sea el vocabulario cerrado y no una cadena libre: una
    /// fila con `scopes = 'admin'` no la puede escribir ni la capa ni un `INSERT` a mano.
    #[tokio::test]
    async fn un_scope_fuera_del_vocabulario_no_entra() {
        let (_directorio, pool, _tokens) = capa().await;

        let resultado = sqlx::query(
            "INSERT INTO api_tokens (id, name, token_hash, scopes) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind("00000000-0000-4000-8000-000000000000")
        .bind("intruso")
        .bind("hash-de-prueba")
        .bind("admin")
        .execute(&pool)
        .await;

        assert!(
            resultado.is_err(),
            "el `CHECK` de `scopes` tiene que rechazar un valor fuera del vocabulario"
        );
    }

    /// 3.1 — El `Debug` de la entidad no publica el hash.
    ///
    /// La entidad nunca guarda el valor en claro —el token se entrega una sola vez y no
    /// entra aquí—, y el hash tampoco sale por `Debug`: es el material con el que se verifica
    /// un token entrante, y no tiene por qué acabar en una línea de registro.
    #[tokio::test]
    async fn el_debug_del_token_no_publica_el_hash() {
        let (_directorio, _pool, tokens) = capa().await;

        let dado_de_alta = tokens
            .alta(alta(EN_CLARO, "agente", &[Scope::Read]))
            .await
            .expect("el alta no puede fallar");

        let impreso = format!("{dado_de_alta:?}");

        assert!(
            !impreso.contains(&hash_de_token(EN_CLARO, &tokens.hmac_key[..])),
            "el `Debug` publicó el hash del token: {impreso}"
        );
        assert!(
            !impreso.contains(EN_CLARO),
            "el `Debug` publicó el valor en claro: {impreso}"
        );
        assert!(
            impreso.contains("redactado"),
            "el `Debug` no dice que el hash está redactado: {impreso}"
        );
    }

    /// 2.1 — Un conjunto sin scopes no tiene texto canónico: lo dice la función, no la base.
    #[test]
    fn un_conjunto_sin_scopes_no_tiene_texto_canonico() {
        assert!(
            scopes_a_texto(&[]).is_none(),
            "un conjunto sin scopes no puede tener texto: el `CHECK` no admite el vacío"
        );
        assert_eq!(scopes_a_texto(&[Scope::Read]), Some("read".to_owned()));
        assert_eq!(scopes_a_texto(&[Scope::Run]), Some("run".to_owned()));
        assert_eq!(
            scopes_a_texto(&[Scope::Run, Scope::Read]),
            Some("read,run".to_owned()),
            "el texto tiene que ser el canónico, con la lectura primero"
        );
    }

    /// 2.2 — Un alta sin scopes se rechaza en la capa, sin depender del `CHECK` de la base.
    #[tokio::test]
    async fn un_alta_sin_scopes_se_rechaza_en_la_capa() {
        let (_directorio, _pool, tokens) = capa().await;

        let resultado = tokens.alta(alta(EN_CLARO, "sin-permisos", &[])).await;

        assert!(
            matches!(resultado, Err(crate::db::DbError::ScopesVacios)),
            "un alta sin scopes tiene que fallar nombrando el campo"
        );
    }

    /// 3.1 — El `Debug` del alta tampoco publica el hash, igual que el de la entidad.
    #[test]
    fn el_debug_del_alta_no_publica_el_hash() {
        let clave = Clave::desde_maestra(&SecretString::new(MASTER_KEY.to_owned()));
        let impreso = format!(
            "{:?}",
            AltaDeToken {
                name: "agente".to_owned(),
                en_claro: EN_CLARO.to_owned(),
                scopes: vec![Scope::Read],
                expires_at: None,
            }
        );

        assert!(
            !impreso.contains(&hash_de_token(EN_CLARO, clave.api_token_key())),
            "el `Debug` del alta publicó el hash: {impreso}"
        );
        assert!(
            impreso.contains("redactado"),
            "el `Debug` del alta no dice que el hash está redactado: {impreso}"
        );
    }
}
