//! Las entidades del §3 y la capa de acceso que las lee y las escribe.
//!
//! Cada tabla tiene su módulo —`repository`, y después `target`, `job` y
//! `execution`— con tres piezas: la **entidad** (una fila), el **payload de alta**
//! (lo que quien llama informa) y la **capa de acceso** (las consultas). Los tipos
//! se nombran en español y los campos conservan el nombre de la columna en inglés,
//! porque el §3 es el contrato y el mapeo fila→struct tiene que leerse de un
//! vistazo.
//!
//! Tres reglas valen para todas las tablas:
//!
//! * El **identificador** es un `Uuid` en la entidad, pero viaja como `TEXT` con la
//!   forma canónica con guiones: `sqlx` codifica un `Uuid` como un BLOB de 16 bytes
//!   y el §3 dice `TEXT`, así que se escribe `uuid::fmt::Hyphenated` y se lee con
//!   `Uuid::parse_str`. Un `Uuid` directo en un `bind` contradiría el esquema y
//!   dejaría los identificadores ilegibles para `sqlite3`.
//! * Las **fechas** son `time::OffsetDateTime` —el tipo que ya está en el árbol—, y
//!   las columnas que el §3 declara con `CURRENT_TIMESTAMP` las rellena la base en
//!   UTC.
//! * Los **secretos** entran y salen como [`SecretString`](crate::secret::SecretString),
//!   cifrados en reposo por [`crate::db::crypto`]: el mapeo fila→struct es el único
//!   sitio donde se descifra, y por eso la capa de acceso recibe la clave al
//!   construirse.
//!
//! Todas las consultas van con `sqlx::query` y valores **enlazados**: ninguna
//! consulta se construye concatenando cadenas, ni siquiera en piezas.

pub mod repository;

pub use repository::{AltaDeRepositorio, Repositorio, Repositorios};
