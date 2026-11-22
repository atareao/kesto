# Kesto — Instrucciones para agentes

> Las directivas globales de este entorno (OpenSpec SDD + TDD, tags de estado, tabla de subagentes) se inyectan ya en cada sesión desde `~/.config/opencode/AGENTS.md`. No se repiten aquí: lo de abajo es solo lo específico de este repositorio.

**Fuente de verdad: `docs/DEFINITION.md`** (SRS completo, en español). Manda sobre este fichero cuando haya conflicto. Secciones que hay que leer antes de tocar código:

| § | Contenido |
|---|---|
| §3 | Schema SQLite exacto: 4 tablas, `CHECK` de enums, `ON DELETE CASCADE` |
| §4 | Flujo de extracción por streaming y lógica de retención/prune |
| §5 | Endpoints `/api/v1/*` y el JSON exacto del evento SSE |
| §6–§8 | Pantallas Antd, layout del repo, compose de producción |

## Comandos de cada ciclo

**Empieza por el `.justfile` (`just --list` tiene todas las recetas); el resto está en `docs/COMANDOS.md`.**

| Receta | Qué hace |
|---|---|
| `just check` | Los tres checks del CI, en su orden y sobre el **workspace** entero: `cargo fmt --all -- --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --locked --workspace`. Ejecútalo antes de cada commit. |
| `just check-all` | `just check` + la puerta de dependencias (`just deny` = `cargo deny --workspace check`) + las pruebas de `scripts/` + los cuatro pasos del panel (`just ui-check`). Es el análogo local del CI entero. |
| `just check-spec` | Puerta del protocolo SDD: falla si no hay change proposal activo en `openspec/changes/`. |

## Reglas de git flow (obligatorio)

**Este proyecto usa gitflow siempre: nada se commitea directamente en `main` ni en `development`.**

- Mensajes: conventional commits con gitmoji (`✨ feat:`, `🐛 fix:`, `🚑️ hotfix:`, `📝 docs:`, `✅ test:`, `🔧 chore:`…). Flujo completo y tabla de gitmoji en `GIT_FLOW.md`; automatización en la skill `gitflow`.
- Formateo **antes** de cada commit: `cargo fmt --all` → `cargo clippy -- -D warnings` → `cargo test`; en `ui/`: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Si el formateo toca ficheros, commit aparte (`🎨 style: apply formatting`).
- Nunca hagas `git commit` directo en `main`: ahí solo entran PRs de release desde `development`.

**Pipeline:** **Un merge a `main` ES una release.** No se etiqueta a mano: `release-prepare` decide el bump desde los conventional commits (`feat` → minor, `!:` / `BREAKING CHANGE` → major, resto → patch) y empuja el tag, que es lo que dispara `release.yml`. El CI corre lo mismo que `just check` **sobre el workspace entero** (`--workspace`), más la puerta de dependencias `cargo deny --workspace check` (en local, `just deny`): repítelo antes de cada push. Workflows, secretos y ramas, en `GIT_FLOW.md`.

## Planes y documentación — regla fija

**1) `plans/`** — un fichero por plan, correlativo de tres dígitos y sin huecos (el siguiente número = mayor existente + 1; `plans/` es la única fuente del contador):

| Estado | Nombre |
|---|---|
| En curso | `plans/PLAN-001.md` |
| Terminado | `plans/PLAN-001-COMPLETED.md` |
| Aparcado | `plans/PLAN-001-PENDING.md` |

Los planes **se renombran, nunca se borran** (`git mv`). El renombrado al cerrar o aparcar lo haces tú al terminar, no el subagente que escribe el plan.

**2) `docs/`** — un documento por feature entregada, correlativo y emparejado con su plan: `docs/FEATURE-001-<slug>.md` (slug en kebab-case). `docs/DEFINITION.md` es el SRS de referencia y no se mezcla con las features.

⚠️ **`@plan-writer` escribe `PLAN.md` en la raíz por defecto.** En este repo los planes van a `plans/PLAN-NNN.md`: pásale la ruta de destino explícita y el contenido completo del plan en el prompt.

## Relevo de sesión (`HANDOFF.md`) — regla fija

`HANDOFF.md` en la raíz es el **relevo local de la sesión**: se escribe y se actualiza en disco, pero **NUNCA se versiona**.

- Está en `.gitignore`; no lo añadas con `git add` ni lo metas en un PR.
- Un `git rm --cached` **no basta** para conservarlo: si el borrado ya está fusionado, el siguiente `git pull` elimina el fichero del working tree. Si desaparece, recréalo localmente.
- El estado del proyecto va en ficheros **versionados** (`plans/`, `docs/`, `openspec/`); el handoff solo sirve para retomar la sesión.

## Layout (SRS §7 — respétalo)

```text
src/{main.rs,api,db,docker,engine,rustic,scheduler}
ui/{src/{components,pages,services},vite.config.ts}
```

`src/` + `ui/` en la raíz, **no** `backend/` + `frontend/`. El resto de repos Rust+React de este entorno usan la otra convención: no la apliques aquí.

## Variables de entorno — reglas de arranque

Tabla completa (nombres, defectos, obligatoriedad) en `docs/ENTORNO.md`. Lo que **no** se puede olvidar:

- Los nombres que se leen **tal cual**, sin prefijo `KESTO_`, son los tres del §8 (`RUST_LOG`, `BIND_ADDRESS`, `DATABASE_URL`) y `DOCKER_HOST`. El resto lleva prefijo `KESTO_`, salvo `OIDC_*`, que conserva el nombre que ya usa shuul.
- Las tres variables `OIDC_*` que gobiernan el modo (`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`) son **un todo o nada**: unas sí y otras no es **error de arranque**, no un modo degradado.
- `KESTO_MASTER_KEY` es obligatoria **siempre**.
- `KESTO_SECRET` es obligatoria **en modo OIDC** —es la clave con la que se firma la sesión— y no hace falta en modo `Disabled`. **Rotarla invalida todas las sesiones vivas**.
- Una credencial —`KESTO_MASTER_KEY` siempre, `KESTO_SECRET` en modo OIDC— de menos de **32 caracteres**, **vacía incluida**, **impide el arranque** nombrando la variable y el mínimo. Ya **no** basta con avisar: la clave vacía hace determinista y pública la derivación del cifrado en reposo.
- El modo sin autenticación **no es lo que pasa por omisión**: sin las tres `OIDC_*` hace falta el permiso explícito `KESTO_ALLOW_INSECURE=1` (`0` o ausente lo niega; cualquier otro valor es error de arranque) **y** una dirección de escucha de loopback, así que ese modo no se puede publicar. En modo OIDC el permiso ni se consulta. La gestión de tokens (`/api/v1/tokens*`) **no se monta** en ese modo.
- `BIND_ADDRESS` sin puerto es error de arranque, y **el valor vacío no cae al defecto**: `BIND_ADDRESS=` mata el arranque. Su defecto es `127.0.0.1:8080`, **no** el `0.0.0.0:8080` del §8: escuchar en todas las interfaces es una decisión explícita del operador. Si usas `compose` con interpolación de una variable que no definiste, acuérdate de esto.
- Lo mismo vale para `RUST_LOG`: presente pero vacía es **error de arranque**, no «sin registro».
- `OIDC_REDIRECT_URL` es **obligatoria** en modo OIDC y tiene que ser `https`: con las tres `OIDC_*` puestas, ausente o sin `https` es **error de arranque** que nombra la variable. Configurada **sola**, sin las otras tres, se ignora en silencio.
- `KESTO_ALLOW_INSECURE_ENDPOINT` — `1` admite el `endpoint` `http` y el loopback; `0` o ausente lo niega; cualquier otro valor es error de arranque. Se lee **siempre**, también con OIDC, porque el `endpoint` de un repositorio se valida en los dos modos.

## Cifrado en reposo

Los tres secretos del §3 —`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`— se guardan **cifrados**: el sobre `v1:<nonce>:<cifrado>` con AES-256-GCM y **nonce aleatorio por valor**, y la clave se deriva de `KESTO_MASTER_KEY` con HKDF-SHA256 y la etiqueta constante `kesto/secrets/v1` (que **no** es configurable): **rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados**.

## Modo de desarrollo (sin OIDC)

Sin ninguna de las tres `OIDC_*` el modo es `Disabled`, y **no basta con `KESTO_MASTER_KEY`**: hace falta el permiso explícito `KESTO_ALLOW_INSECURE=1` y una dirección de escucha de **loopback**, así que ese modo no se publica. `GET /api/v1/health` contesta `auth_mode: "disabled"`. Ese modo **no pone ninguna capa**: la mitad protegida no se monta, todo `/api/v1/*` queda abierto y `/api/v1/auth/*` ni siquiera existe —contesta `404` con la envoltura, **no** `401`—; la única excepción es la gestión de tokens (`/api/v1/tokens*`), que tampoco se monta. Es para desarrollo, no para un despliegue expuesto.

## Entorno de desarrollo

Versiones y runtime en `docs/ENTORNO.md`. Las que no se negocian:

- **Podman en local, Docker en producción** (2026-10-24). En dev, bollard apunta al socket de podman con la ruta configurable (`DOCKER_HOST`); nunca al de Docker.
- Cualquier comando `docker ...` falla en esta máquina: usa `podman ...`.
- **La versión de API de bollard se fija en la llamada, no se deja deducir**: `Docker::connect_with_socket(socket, segundos, bollard::API_DEFAULT_VERSION)` —`src/docker/cliente.rs:68`— y nunca con `connect_with_defaults()`.

## Restricciones de diseño que un agente rompería por defecto

- **Un worker secuencial por repositorio S3**, no un task de Tokio por job. Cola MPSC + un worker por `repository_id`.
- **Streaming en memoria, sin ficheros temporales**: `stdout` del `exec` de bollard → `AsyncRead` en un buffer de Tokio → `NodeWriter`/`TreeBuilder` de `rustic_core`. No escribas el dump a disco ni a `/tmp`.
- **Un único stream SSE global** en `GET /api/v1/events`, con `event: <nombre>` (`execution_progress`, …) y el payload literal de §5. No inventes otro esquema de mensaje.
- **Las migraciones se nombran con un prefijo numérico entero** (`0001_init.sql`), no con la forma `V001__init.sql` del estilo de Flyway: `sqlx::migrate!()` parte el nombre por el primer `_` y hace `parts[0].parse::<i64>()`, así que la forma con `V` **no compila**.
- **Nunca enlaces un `Uuid` de `sqlx` contra una columna `TEXT`**: usa la forma canónica con guiones (`uuid::fmt::Hyphenated`).
- **`foreign_keys` va en las opciones de conexión del pool**, nunca en un `PRAGMA` suelto (el `ON DELETE CASCADE` **no ocurre sin que nada lo delate**).
- **El arnés de captura de registro (`src/test_support.rs`) instala un suscriptor global y elige el buffer por hilo**: nunca `set_default` ni `with_default`.
- **La cookie de sesión va `Secure` incondicional**: `session_cookie` (`src/auth/session.rs`) no lo condiciona a la configuración ni al esquema de la petición, porque una cookie de sesión sin `Secure` viaja en claro.
- **El `endpoint` de un repositorio se valida en el borde HTTP, no en el motor**: `POST`/`PUT /api/v1/repositories` lo juzgan **antes** de escribir la fila y antes de hablar con el destino (`src/rustic/endpoint.rs`; único consumidor, `endpoint_admisible` en `src/api/repositories.rs`), con un `400` que nombra el campo y no repite la URL. `https` siempre, `http` solo con `KESTO_ALLOW_INSECURE_ENDPOINT=1`; enlace local, metadatos, no especificada y credenciales embebidas se rechazan siempre, también con el permiso. La edición **no** conecta con el destino, así que una comprobación en el motor no la alcanzaría; el alta **conserva** su `502` para «el destino no responde».
- **La ruta de entrada tiene dos protecciones en memoria**: tope duro del mapa de `state`s pendientes (`MAX_LOGINS`, `src/state.rs`: purga por caducidad y desalojo de la más antigua, **nunca** rechazo) y límite de tasa por IP en `GET /api/v1/auth/login` (`src/auth/limite.rs`: `VENTANA_DE_LOGIN`, `MAX_PETICIONES_DE_LOGIN`, `429`). La IP sale del **socket** (`ConnectInfo<SocketAddr>`, con `into_make_service_with_connect_info` en `src/main.rs`), **nunca** de una cabecera: `X-Forwarded-For` la falsifica el cliente.
- **El volcado que se restaura es contenido no confiable, y cada cliente se invoca en el modo que ignora sus metacomandos**: `sqlite3 -safe` (SQLite ≥ 3.27), `mariadb --binary-mode` y `psql -X` con el volcado troceado por base y los metacomandos de primer nivel descartados. El volcado sale de un repositorio que **configura el operador**, así que sus metacomandos —`.shell` de `sqlite3`, `\!` de `mariadb` y `psql`— no pueden llegar a ejecutarse como `root` en el contenedor destino. Detalle en `docs/ARQUITECTURA.md`.
- **El volcado de PostgreSQL se trocea por los `\connect` de primer nivel y se restaura base por base**: `\restrict` —el único modo de `psql` que rechaza metacomandos— **bloquea `\connect`**, así que un `psql` único con el volcado global desvía los datos a la base equivocada. El troceador (`src/engine/troceador.rs`) reconoce el estado léxico —comillas, dólares, comentarios anidables y bloques `COPY`— y el metacomando se reconoce por el **primer byte no blanco**, no por la columna cero: `psql` lo ejecuta precedido de **cualquier** blanco de su juego (`[ \t\n\r\f\v]`, `\f` y `\v` incluidos). El descarte de metacomandos es un **puerto fiel** de la detección de `COPY … FROM stdin` de `psql` —letra inicial de los identificadores de primer nivel, tope ocho, sin los de paréntesis, y decisión al `;` a profundidad cero—, **no** una búsqueda de subcadena. Y **`\restrict` es una precondición comprobada en tiempo de ejecución, no una capa opcional**: antes de restaurar se ejecuta `psql --version` con el mismo binario y, si el cliente no lo honra —17.6, o el mínimo de su rama: 16.10, 15.14, 14.19, 13.22—, la restauración **falla** y no restaura nada. Además, el **nombre de base del `\connect` no puede tener forma de cadena de conexión** —un `=`, o un esquema `postgres://`/`postgresql://`, hacen que `psql -d` lea el valor como *conninfo* y desvíe la conexión con la credencial del entorno puesta—: se **rechaza** y la restauración **falla** sin ejecutar esa sección ni las siguientes. Y los errores de SQL **dentro** de una sección aún **no** se detectan (`psql` sin `ON_ERROR_STOP` sale con `0`): **#175**. Detalle en `docs/ARQUITECTURA.md`.

## Dónde está el resto

| Tema | Fichero |
|---|---|
| Estado del repositorio (construido, verificado y pendiente) | `docs/ESTADO.md` |
| Variables de entorno, versiones y recetas `just` | `docs/ENTORNO.md` · `docs/COMANDOS.md` |
| Arquitectura: cifrado, modo dev y diseño | `docs/ARQUITECTURA.md` |
| Workflows de GitHub Actions, gitmoji y ramas | `GIT_FLOW.md` |
| Cabos sueltos del SRS y cómo se cerró cada uno | `docs/DECISIONES-SRS.md` |
| SRS completo (manda sobre este fichero) | `docs/DEFINITION.md` |
