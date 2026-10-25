# PLAN-003 — `db-schema`

> **Estado:** **cerrado** (implementado, auditado y verificado el 2026-10-25; change archivado en `openspec/changes/archive/2026-10-25-db-schema/`).
> **Rama:** `feature/db-schema`, desde `development`.
> **PR:** [#8 — `feature/db-schema` → `development`](https://github.com/atareao/kesto/pull/8) · CI en verde (run [`40162918376`](https://github.com/atareao/kesto/actions/runs/40162918376), `attempt 1`, `Backend (fmt, clippy, test)` ✅; `Frontend` saltado por diseño — `ui/package.json` no existe todavía). **Sin release a `main`**: se publica solo hasta `development`.
> **Change de OpenSpec:** `openspec/changes/archive/2026-10-25-db-schema/` — **11 requisitos y 50 escenarios** (`db` 7/34, capability **nueva**; `secrets` 4/16, que crece con el cifrado en reposo), `openspec validate db-schema --strict` válido.
> **Especificación de origen:** `docs/DEFINITION.md` §3 (modelo de datos) y §7 (layout). La retención del §4.B es del motor y **no** entra aquí.
> **Cambio anterior:** `auth-capability` (`PLAN-002-COMPLETED.md`, `docs/FEATURE-002-auth-capability.md`, 93 tests).

## Objetivo

Que `DATABASE_URL` deje de ser una cadena decorativa. Hoy el proceso arranca con un DSN que no lleva a ninguna parte: no hay `migrations/`, ni pool, ni entidades, ni cifrado, y los tres secretos del §3 se guardarían **en claro** en el fichero que los custodia. Al terminar, `src/db/` existe con el esquema del §3 aplicado por migraciones embebidas, el pool abierto con WAL y claves foráneas encendidas, una capa de acceso por tabla y el cifrado en reposo de `secret_access_key`, `passphrase` y `db_password`.

Es además **la raíz del grafo**: los cinco módulos del §7 que siguen vacíos —`docker`, `engine`, `rustic`, `scheduler` y el propio `db`— no pueden empezar sin él. El motor necesita targets y repositorios que leer, el planificador tareas que agendar, y el contenedor ejecuciones que registrar.

Y cierra **cuatro cabos sueltos** del SRS anotados como deuda desde `FEATURE-001`: los secretos en `TEXT` plano, las claves foráneas sin índice (SQLite no las indexa, así que cada `ON DELETE CASCADE` recorre la tabla hija entera), `job_executions` sin forma razonable de filtrarse por repositorio, y `db_name` inaplicable con los comandos del §4.A.

## Alcance

**Dentro:**

1. `migrations/` con el esquema del §3 y los índices añadidos, aplicado con `sqlx::migrate!()` al arrancar: un fallo de migración es error de arranque.
2. El pool de SQLite con **WAL**, `busy_timeout` y **`foreign_keys=ON` en las opciones de conexión** (no en un `PRAGMA` suelto: es una opción por conexión).
3. Las cuatro entidades del §3 y una capa de acceso por tabla: alta, lectura por id, listado, actualización y borrado, con prepared statements y el «no está» distinto del «no se pudo leer».
4. El cifrado en reposo de los tres secretos: AEAD **AES-256-GCM** con clave derivada de `KESTO_MASTER_KEY` por **HKDF-SHA256** (`info` = `kesto/secrets/v1`), **nonce aleatorio por valor** y sobre versionado `v1:<nonce>:<cifrado>`.
5. El descifrado que **falla en alto**: prefijo desconocido, nonce de longitud mala, texto manipulado y valor en claro son errores, nunca un secreto vacío.
6. La semántica de `db_name`: informado ⇒ esa sola base; `NULL` ⇒ dump global. Se fija aquí, en el esquema; la ejecuta el change del motor.
7. `main.rs` abre la base y la migra **antes** de escuchar, una sola vez. El traspaso del pool al `AppState` queda **aplazado** al change de los endpoints, y está declarado abajo como deuda.

**Fuera:** los nueve endpoints del §5 (change propio, con su contrato y sus escenarios); motor, Docker, Rustic y planificador; cifrado del repositorio Rustic; rotación de `KESTO_MASTER_KEY` o clave de datos envuelta; SQLCipher; `VACUUM` programado y retención de `job_executions`; `Dockerfile` y volumen del `compose`.

## Módulos

El change se reparte en tres módulos entregables, cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones:

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| 1 | Apertura y esquema | Pool, pragmas, migraciones embebidas, error de base | `migrations/0001_init.sql`, `migrations/0002_indexes.sql`, `src/db/mod.rs`, `src/error.rs` |
| 2 | Entidades y acceso | Las cuatro entidades, su mapeo y su capa de acceso, con la cascada a tres niveles | `src/db/entities/{mod,repository,target,job,execution}.rs` |
| 3 | Cifrado en reposo | El sobre versionado, la derivación por HKDF y el descifrado que falla en alto | `src/db/crypto.rs` |

El **orden de entrega** es 1 → 3 → 2, y no el de la numeración: la capa de acceso **descifra** —el escenario «la lectura por la capa de acceso devuelve el valor original» y los cuatro del «descifrado que falla en alto» pasan por ella—, así que las entidades dependen del módulo de cifrado. Escribirlas antes obligaría a escribirlas dos veces: una con el valor guardado en claro y otra cuando llegara el sobre. La numeración de la tabla es la del reparto de contenido, no la del calendario.

## Decisiones tomadas

### Alcance: persistencia, sin HTTP

Los nueve endpoints del §5 **no** entran: van a un change propio. Aquí se escribe el esquema, su cifrado y la capa que el motor y el planificador consumirán. Es la decisión que mantiene cada change auditable y con una sola responsabilidad.

### Cifrado en reposo

El cifrado vive en la **capa de acceso**, no en la columna: la columna sigue siendo `TEXT` y lo que cambia es qué texto. El valor guardado es `v1:<nonce en base64>:<cifrado en base64>`. El prefijo de versión permite que un esquema posterior conviva con las filas ya escritas sin un `UPDATE` masivo que exigiría la llave vieja a mano. El nonce es **aleatorio por valor**, así que guardar dos veces el mismo secreto produce dos textos cifrados distintos. No es SQLCipher: SQLCipher cifraría el fichero entero y obligaría a un `libsqlite3-sys` distinto, y lo que hay que proteger son tres columnas, no el histórico de ejecuciones.

### Versiones fijadas por la caché

El repositorio compila **sin red**, así que las versiones no son preferencia sino consecuencia, y las tres se comprobaron con `cargo tree --offline` **antes** de escribir el proposal:

- **`sqlx` 0.8.6**, no 0.9.0. Las dos resuelven sin red; 0.9 exige **MSRV 1.94** y su feature `sqlite` deja de ser un alias de «bundled» para expandirse a cuatro features que arrastran `libsqlite3-sys` a rango abierto. Y en las dos, **`default-features = false` no es cosmético**: sin él entra `any`, que mete los drivers de MySQL y PostgreSQL que este proyecto no usa. Features: `runtime-tokio`, `sqlite`, `migrate`, `macros`, `time`, `uuid`.
- **Generación RustCrypto 0.10/0.12** —`aes-gcm` 0.10.3, `hkdf` 0.12.4, `sha2` 0.10.9—, que es la que **ya está en el árbol** por `jsonwebtoken` con `rust_crypto`. La generación nueva (0.11/0.13/0.11) resuelve, pero mete **nueve bifurcaciones** nuevas (`digest`, `crypto-common`, `hmac`, `rand_core`, `cpufeatures`, `const-oid`…) porque cambia `generic-array` por `hybrid-array`.
- **`base64` 0.22.1 se promueve** de `[dev-dependencies]` a `[dependencies]`, conservando su comentario y su pin: `^0.22` es la única mayor que ya está en el árbol.

### Fecha e identificadores

- **`time::OffsetDateTime`, no `chrono`**: `time` **ya está en el árbol activo** —lo traen `cookie` y `simple_asn1`—, así que `chrono` sería una segunda familia de fechas para lo mismo. En `sqlx` 0.8.6 la fecha es una dependencia **opcional**, así que la feature `time` de `sqlx` es imprescindible para encender las impls `Encode`/`Decode`. Y hay una razón de corrección: su impl **descodifica** el formato `YYYY-MM-DD HH:MM:SS` que produce `CURRENT_TIMESTAMP`, asumiéndolo UTC, así que el `DEFAULT` del §3 sigue siendo legible sin tocar el SRS.
- **El `id` es `TEXT` con el UUID en forma canónica con guiones**, generado en la capa de acceso. Ver «trampas», abajo.

### El directorio del DSN

Si el fichero de la base no existe, **se crea**. Si **el directorio** del DSN no existe, el arranque **falla nombrando la ruta** y no lo crea: crearlo escondería un volumen mal montado, y el fallo se manifestaría más tarde y en otro sitio. Consecuencia práctica que queda escrita en la fase 4 del `tasks.md`: `just dev` con el `DATABASE_URL` por defecto del SRS (`sqlite:///app/data/kesto.db`) **no arranca** en una máquina sin `/app/data` —eso es correcto, ese directorio es el del volumen del `compose`—, así que el desarrollo exporta su propio `DATABASE_URL`.

### Pragmas de SQLite

`journal_mode=WAL` (propiedad del fichero: permite leer mientras se escribe, que es lo que hace el panel mientras el motor registra progreso), `busy_timeout` finito (SQLite serializa las escrituras y sin espera una escritura que coincide con otra **falla** en vez de esperar) y `foreign_keys=ON` **en las opciones de conexión del pool**. WAL crea `-wal` y `-shm` junto a la base, así que **el directorio tiene que ser escribible**: es un requisito del volumen del §8, declarado aquí porque lo impone el pragma.

### Índices añadidos al SRS

`idx_backup_jobs_target_id`, `idx_backup_jobs_repository_id`, `idx_job_executions_job_id` —las tres columnas hijas de FK— y `idx_job_executions_status_started_at` para el listado del §6. Se declaran como **añadido al SRS**, no se cuelan: el §3 no tiene ningún índice. Y **no** se desnormaliza `repository_id` en `job_executions`: el `JOIN` por `backup_jobs` ya va por índice y duplicar la columna crearía una segunda verdad que se puede desincronizar.

## Trampas encontradas y candadeadas

- **`sqlx` escribe `Uuid` como BLOB de 16 bytes** y lo descodifica con `Uuid::from_slice`, así que un `TEXT` de 36 caracteres **no** descodifica como `Uuid`. Con el §3 diciendo `id TEXT`, mapear la columna a `Uuid` contradice el esquema y deja los identificadores **ilegibles para `sqlite3`**. La capa de acceso usa `uuid::fmt::Hyphenated`, y la trampa es requisito, no comentario.
- **`foreign_keys` es una opción *por conexión*** y está apagada por defecto en SQLite. Un `PRAGMA foreign_keys=ON` suelto solo afectaría a una conexión del pool: las demás seguirían sin claves foráneas, el `ON DELETE CASCADE` del §3 **no ocurriría y nada lo delataría** —el borrado simplemente no borraría—.
- **`default-features = false` en `sqlx`** decide si entran los drivers de MySQL y PostgreSQL que no se usan.
- **`CURRENT_TIMESTAMP` no se sustituye** por una expresión con formato propio: la impl de `OffsetDateTime` ya lo descodifica, y las filas insertadas a mano con `sqlite3` siguen siendo legibles.

## Auditado y corregido

*(Se rellena al cerrar, con los hallazgos que devuelva la auditoría independiente y lo que se corrigió.)*

## Verificación

*(Se rellena al cerrar. Son las puertas del repositorio —`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` y `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`— más las diez comprobaciones de operador de la fase 4 del `tasks.md`, que van contra el binario real y contra `sqlite3`: fichero nuevo con las cuatro tablas, directorio inexistente que detiene el arranque, WAL y `foreign_keys`, secretos buscados **literalmente en el fichero**, doble arranque idempotente, alta huérfana rechazada, cascada a mano, cifrado manipulado que **no** se lee como vacío, `RUST_LOG=trace` sin secretos e identificadores legibles.)*

## Deuda declarada

- **Rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados.** No hay clave de datos envuelta por una clave maestra rotable: sería un segundo sistema de claves que nadie ha pedido. Es un límite aceptado, y queda escrito para que quien rote sepa que la clave anterior hace falta para leer lo que ya está guardado.
- **El fichero `.db` no se cifra.** Lo que no aparece en claro son las tres columnas de secreto; el histórico de ejecuciones es legible a propósito, que es lo que hace útil abrir el `.db` con `sqlite3`.
- **No hay `VACUUM` programado ni retención de `job_executions`.** El esquema lo soportará; quien borra por antigüedad es el planificador, y todavía no existe.
- **El pool no llega todavía al `AppState`.** Se abre y se migra al arrancar y vive el resto del proceso, pero su traspaso al estado compartido es la **primera** tarea del change que estrene el primer consumidor —los endpoints del §5—, que hoy no existe. Adelantarlo obligaría a elegir entre un campo no opcional, que llevaría una base de datos a las más de treinta construcciones de `AppState` de las pruebas, o uno opcional, que declararía válido un Kesto sin base de datos, y no lo es.

## Cierre

- [x] `tasks.md` completo (obligatorio: `openspec archive` se niega si queda una casilla sin marcar). → 98 tareas tildadas; las puertas del repositorio en verde con `just check` (`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked`): **156 tests** (155 en el binario + 1 de integración). `just check-all` **no aplica**: `ui/` no existe todavía y la propia receta aborta diciéndolo.
- [x] `docs/FEATURE-003-db-schema.md` con lo que realmente quedó construido. → escrito, con las comprobaciones de operador 4.1–4.10 y las cifras de tests.
- [x] `AGENTS.md`: la fila de `DATABASE_URL` deja de decir «Todavía no se usa» y se describe el esquema de cifrado en reposo con la rotación como límite aceptado. → hecho: la fila de `DATABASE_URL` ya no dice «Todavía no se usa» y hay sección «Cifrado en reposo».
- [x] `README.md`: el estado deja de decir que no hay base de datos. → hecho: el estado ya no dice que no hay base de datos.
- [x] Redactar el `Purpose` de `openspec/specs/db/spec.md`: es capability **nueva** y no puede quedarse en `TBD`. → hecho: se redactó al archivar.
- [x] `openspec archive db-schema`. → `just spec-validate db-schema` pasó en estricto, comprobado **antes** del archivado (el change ya archivado, el comando deja de encontrarlo); archivado en `openspec/changes/archive/2026-10-25-db-schema/`, 11 requisitos aplicados (`db` 7 creados, `secrets` 4 añadidos).
- [x] Renombrar este fichero a `plans/PLAN-003-COMPLETED.md` con `just plan-done 3`. → hecho.
- [x] PR `feature/db-schema` → `development` con el CI en verde. **Sin release a `main`**: se publica solo hasta `development`. → [#8](https://github.com/atareao/kesto/pull/8), abierto contra `development`; **sin release**: no se abre PR a `main` ni se etiqueta nada. CI en verde (run [`40162918376`](https://github.com/atareao/kesto/actions/runs/40162918376), `attempt 1`): `Preflight` y `Backend (fmt, clippy, test)` en verde, y `Frontend` **saltado por diseño** —`ui/package.json` no existe todavía, así que el workflow se lo salta solo; no es un job olvidado—.
