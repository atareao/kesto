//! La tabla `sesiones_revocadas` del change `endurecimiento-sesion-y-frontera`
//! (hallazgo SEC-17): las sesiones cerradas antes de su caducidad.
//!
//! La sesión es un JWT **sin estado**, así que su firma sola no permite cerrarla
//! antes de que su `exp` llegue. El `jti` es el identificador único que la emisión
//! pone en el token, y esta tabla guarda los que ya se han revocado: el *gate*
//! consulta aquí en cada petición y rechaza la cookie cuyo `jti` esté presente.
//!
//! La revocación se guarda **en la base** y no solo en memoria: un reinicio
//! borraría un mapa en memoria y una cookie revocada volvería a servir hasta su
//! `exp`, que es justo el arreglo que se desharía solo y en silencio.
//!
//! `exp` es el `exp` del token y es lo que permite purgar: pasado ese instante la
//! sesión ya no vale por sí sola, así que la revocación no protege nada y la fila
//! se borra. No hace falta un índice sobre `exp`: la purga es un `DELETE` y la
//! lectura es por la clave primaria.

use sqlx::SqlitePool;
use time::OffsetDateTime;

use crate::db::DbError;

/// La capa de acceso de `sesiones_revocadas`.
///
/// **No** recibe la clave de cifrado: no tiene ninguna columna secreta —el `jti` es
/// un identificador, no una credencial—, y pasarle una clave que no usa sugeriría
/// que aquí se cifra algo.
pub struct SesionesRevocadas {
    pool: SqlitePool,
}

// Las sentencias, como constantes: `DbError::Consulta` lleva el texto de la consulta,
// así que la que se ejecuta y la que viaja en el error tienen que ser **la misma
// cadena**. Los valores van **enlazados** (`?1`, `?2`…): ninguna se compone concatenando.

/// Purga las revocaciones ya caducadas: pasado el `exp` del token, la revocación no
/// protege nada y la fila no tiene por qué seguir.
const SQL_PURGAR: &str = "DELETE FROM sesiones_revocadas WHERE exp <= ?1";

/// Revocación **idempotente**: revocar el mismo `jti` dos veces no falla y no duplica.
///
/// `INSERT OR IGNORE` y la clave primaria son lo que la hace idempotente: un alta que
/// ya estaba no es un error, es el estado que se quería. El `exp` que se guarda es el
/// de la primera revocación —la que importa—, y una segunda llamada no lo mueve.
const SQL_REVOCAR: &str = "INSERT OR IGNORE INTO sesiones_revocadas (jti, exp) VALUES (?1, ?2)";

/// ¿Está revocado este `jti`? La lectura va por la clave primaria.
const SQL_ESTA_REVOCADA: &str = "SELECT 1 FROM sesiones_revocadas WHERE jti = ?1";

impl SesionesRevocadas {
    /// Construye la capa de acceso sobre un pool.
    pub fn nuevo(pool: SqlitePool) -> Self {
        SesionesRevocadas { pool }
    }

    /// Revoca la sesión de `jti` hasta su caducidad `exp`.
    ///
    /// Es **idempotente** —revocar dos veces el mismo `jti` no falla— y **purga antes** las
    /// revocaciones ya caducadas (`exp <= ahora`), que son las que ya no protegen nada porque
    /// su sesión dejaría de valer por sí sola. Las dos cosas van en una transacción: la purga
    /// y el alta son una sola operación sobre la tabla.
    pub async fn revocar(&self, jti: &str, exp: u64) -> Result<(), DbError> {
        let ahora = OffsetDateTime::now_utc().unix_timestamp();
        // El `exp` llega como `u64` —es el `exp` de un JWT—, pero la columna es `INTEGER` y
        // SQLite no tiene enteros sin signo, así que se enlaza como `i64`. Un `exp` que no
        // cupiera en `i64` es una fecha imposible; en vez de fallar la revocación por el
        // rango, se acota al máximo y la fila no se purga por caducidad, que es el lado
        // seguro: una revocación de más nunca deja servir una cookie cerrada.
        let exp = i64::try_from(exp).unwrap_or(i64::MAX);

        let mut transaccion = self
            .pool
            .begin()
            .await
            .map_err(error_de_consulta(SQL_REVOCAR))?;

        sqlx::query(SQL_PURGAR)
            .bind(ahora)
            .execute(&mut *transaccion)
            .await
            .map_err(error_de_consulta(SQL_PURGAR))?;
        sqlx::query(SQL_REVOCAR)
            .bind(jti)
            .bind(exp)
            .execute(&mut *transaccion)
            .await
            .map_err(error_de_consulta(SQL_REVOCAR))?;

        transaccion
            .commit()
            .await
            .map_err(error_de_consulta(SQL_REVOCAR))?;

        Ok(())
    }

    /// ¿Está revocada la sesión de este `jti`?
    pub async fn esta_revocada(&self, jti: &str) -> Result<bool, DbError> {
        let fila = sqlx::query(SQL_ESTA_REVOCADA)
            .bind(jti)
            .fetch_optional(&self.pool)
            .await
            .map_err(error_de_consulta(SQL_ESTA_REVOCADA))?;

        Ok(fila.is_some())
    }
}

/// Envuelve un fallo de `sqlx` con **la misma** sentencia que se ejecutó.
fn error_de_consulta(consulta: &'static str) -> impl FnOnce(sqlx::Error) -> DbError {
    move |causa| DbError::Consulta { consulta, causa }
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::SesionesRevocadas;
    use crate::test_support::{DirectorioDePrueba, base_con_clave};

    /// Un `jti` distinguible de la prueba.
    const JTI: &str = "jti-distinguible-de-la-prueba-000";
    /// Otro `jti`, para comprobar que uno no revocado no resuelve.
    const OTRO_JTI: &str = "jti-otro-distinguible-000000000000";
    /// Un `exp` bien en el futuro: la purga no lo alcanza.
    const EXP_FUTURO: u64 = 4_000_000_000;

    /// La capa sobre una base de prueba migrada, con la guarda del directorio viva.
    async fn capa() -> (DirectorioDePrueba, SqlitePool, SesionesRevocadas) {
        let (directorio, pool, _clave) = base_con_clave().await;

        let sesiones = SesionesRevocadas::nuevo(pool.clone());

        (directorio, pool, sesiones)
    }

    /// Cuántas filas hay en la tabla de revocadas.
    async fn filas(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM sesiones_revocadas")
            .fetch_one(pool)
            .await
            .expect("el recuento no puede fallar")
    }

    /// 2.1 — El ciclo: se revoca un `jti` y `esta_revocada` lo confirma; uno que no se
    /// revocó no resuelve.
    #[tokio::test]
    async fn revocar_y_esta_revocada() {
        let (_directorio, _pool, sesiones) = capa().await;

        assert!(
            !sesiones
                .esta_revocada(JTI)
                .await
                .expect("la consulta no puede fallar"),
            "un `jti` que nadie revocó no puede estar revocado"
        );

        sesiones
            .revocar(JTI, EXP_FUTURO)
            .await
            .expect("la revocación no puede fallar");

        assert!(
            sesiones
                .esta_revocada(JTI)
                .await
                .expect("la consulta no puede fallar"),
            "el `jti` revocado tiene que resolverse como revocado"
        );
        assert!(
            !sesiones
                .esta_revocada(OTRO_JTI)
                .await
                .expect("la consulta no puede fallar"),
            "revocar uno no puede revocar otro"
        );
    }

    /// 2.1 — Revocar dos veces no falla y no duplica la fila: es idempotente.
    #[tokio::test]
    async fn revocar_dos_veces_no_falla() {
        let (_directorio, pool, sesiones) = capa().await;

        sesiones
            .revocar(JTI, EXP_FUTURO)
            .await
            .expect("la primera revocación no puede fallar");
        sesiones
            .revocar(JTI, EXP_FUTURO)
            .await
            .expect("revocar un `jti` ya revocado no puede fallar");

        assert!(
            sesiones
                .esta_revocada(JTI)
                .await
                .expect("la consulta no puede fallar"),
            "el `jti` tiene que seguir revocado"
        );
        assert_eq!(
            filas(&pool).await,
            1,
            "revocar dos veces no puede duplicar la fila"
        );
    }

    /// 2.2 — `revocar` purga **antes** las filas ya caducadas: la tabla no crece con
    /// revocaciones que ya no protegen nada.
    #[tokio::test]
    async fn revocar_purga_las_caducadas() {
        let (_directorio, pool, sesiones) = capa().await;

        // Un `jti` revocado con un `exp` ya pasado es una revocación caducada: su
        // sesión dejaría de valer por sí sola.
        sesiones
            .revocar("jti-caducado-00000000000000000000", 1)
            .await
            .expect("la revocación caducada no puede fallar");

        assert_eq!(filas(&pool).await, 1, "la revocación caducada se guarda");

        // La siguiente revocación purga la caducada antes de insertar.
        sesiones
            .revocar(JTI, EXP_FUTURO)
            .await
            .expect("la revocación en vigor no puede fallar");

        assert_eq!(
            filas(&pool).await,
            1,
            "la purga tiene que haber borrado la revocación caducada: {pool:?}"
        );
        assert!(
            !sesiones
                .esta_revocada("jti-caducado-00000000000000000000")
                .await
                .expect("la consulta no puede fallar"),
            "la revocación caducada no puede sobrevivir a la purga"
        );
        assert!(
            sesiones
                .esta_revocada(JTI)
                .await
                .expect("la consulta no puede fallar"),
            "la revocación en vigor tiene que quedar guardada"
        );
    }
}
