# Arquitectura e invariantes de diseño

> Extracto **literal** del `AGENTS.md` de Kesto (poda v2, 2026-10-31). Detalle del cifrado en reposo, del modo de desarrollo sin OIDC y justificación de las restricciones de diseño. El `AGENTS.md` conserva la regla imperativa de cada una.

### Cifrado en reposo

Los tres secretos del §3 —`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`— se guardan **cifrados**: el valor almacenado es el sobre `v1:<nonce>:<cifrado>` con AES-256-GCM y **nonce aleatorio por valor**, y la clave se deriva de `KESTO_MASTER_KEY` con HKDF-SHA256 y la etiqueta constante `kesto/secrets/v1` (que **no** es configurable: una etiqueta que se puede tocar por variable de entorno es una forma de perder los secretos sin haber rotado nada). Consecuencia que hay que saber: **rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados** —no hay clave de datos envuelta por una clave rotable—, así que la rotación es una operación con plan y no un cambio de variable.

### Modo de desarrollo (sin OIDC)

Basta `KESTO_MASTER_KEY` para arrancar sin proveedor de identidad: sin ninguna de las tres `OIDC_*` el modo es `Disabled`. En ese modo `GET /api/v1/health` contesta `auth_mode: "disabled"` —eso es lo que hay que consultar para comprobar qué modo está activo— y el arranque lo anuncia con un `WARN`.

⚠️ Ese modo **no pone ninguna capa**: la mitad protegida no se monta, todo `/api/v1/*` queda abierto y `/api/v1/auth/*` ni siquiera existe —contesta `404` con la envoltura, **no** `401`—. Es para desarrollo, no para un despliegue expuesto. Con las tres `OIDC_*` configuradas, en cambio, todo `/api/v1/*` exige sesión salvo la salud y las dos rutas públicas del flujo de entrada.


## Restricciones de diseño que un agente rompería por defecto

- **Un worker secuencial por repositorio S3**, no un task de Tokio por job. `rustic_core` toma un lock exclusivo del repositorio: paralelizar backups del mismo repo los bloquea entre sí. Cola MPSC + un worker por `repository_id`.
- **Streaming en memoria, sin ficheros temporales**: `stdout` del `exec` de bollard → `AsyncRead` en un buffer de Tokio → `NodeWriter`/`TreeBuilder` de `rustic_core`. No escribas el dump a disco ni a `/tmp`.
- **Un único stream SSE global** en `GET /api/v1/events`, con `event: <nombre>` (`execution_progress`, …) y el payload literal de §5. No inventes otro esquema de mensaje.
- **Las migraciones se nombran con un prefijo numérico entero** (`0001_init.sql`), no con la forma `V001__init.sql` del estilo de Flyway: `sqlx::migrate!()` parte el nombre por el primer `_` y hace `parts[0].parse::<i64>()`, así que la forma con `V` **no compila**. Y `migrations/` tiene que existir antes del primer `cargo check` que incluya la macro, porque la resuelve en tiempo de compilación.
- **Nunca enlaces un `Uuid` de `sqlx` contra una columna `TEXT`**: `sqlx` lo escribe como un **BLOB de 16 bytes** y lo descodifica con `Uuid::from_slice`, así que deja los identificadores ilegibles para `sqlite3`. La capa de acceso escribe y lee la forma canónica con guiones (`uuid::fmt::Hyphenated`).
- **`foreign_keys` va en las opciones de conexión del pool**, nunca en un `PRAGMA` suelto: es una opción *por conexión* y está apagada por defecto, así que un `PRAGMA` ejecutado una vez solo afecta a una conexión del pool y el `ON DELETE CASCADE` **no ocurre sin que nada lo delate**.
- **El arnés de captura de registro (`src/test_support.rs`) instala un suscriptor global y elige el buffer por hilo**: nunca `set_default` ni `with_default`. Un despachante de hilo deja un callsite cacheado como `Interest::never` para todo el proceso y su registro desaparece **en silencio** —el síntoma es «el log está vacío», no «el arnés está roto»—, y solo se manifiesta en CI (2 CPUs), no en local.

