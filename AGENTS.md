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
| `just check` | Los tres checks del CI, en su orden: `cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test --locked`. Ejecútalo antes de cada commit. |
| `just check-all` | `just check` + los cuatro pasos del panel (`just ui-check`). |
| `just check-spec` | Puerta del protocolo SDD: falla si no hay change proposal activo en `openspec/changes/`. |

## Reglas de git flow (obligatorio)

**Este proyecto usa gitflow siempre: nada se commitea directamente en `main` ni en `development`.**

- Mensajes: conventional commits con gitmoji (`✨ feat:`, `🐛 fix:`, `🚑️ hotfix:`, `📝 docs:`, `✅ test:`, `🔧 chore:`…). Flujo completo y tabla de gitmoji en `GIT_FLOW.md`; automatización en la skill `gitflow`.
- Formateo **antes** de cada commit: `cargo fmt --all` → `cargo clippy -- -D warnings` → `cargo test`; en `ui/`: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Si el formateo toca ficheros, commit aparte (`🎨 style: apply formatting`).
- Nunca hagas `git commit` directo en `main`: ahí solo entran PRs de release desde `development`.

**Pipeline:** **Un merge a `main` ES una release.** No se etiqueta a mano: `release-prepare` decide el bump desde los conventional commits (`feat` → minor, `!:` / `BREAKING CHANGE` → major, resto → patch) y empuja el tag, que es lo que dispara `release.yml`. El CI corre lo mismo que `just check`: repítelo antes de cada push. Workflows, secretos y ramas, en `GIT_FLOW.md`.

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
- Una credencial de menos de 32 caracteres **avisa y arranca igualmente**.
- `BIND_ADDRESS` sin puerto es error de arranque, y **el valor vacío no cae al defecto**: `BIND_ADDRESS=` mata el arranque. Si usas `compose` con interpolación de una variable que no definiste, acuérdate de esto.
- Lo mismo vale para `RUST_LOG`: presente pero vacía es **error de arranque**, no «sin registro».
- Un `OIDC_REDIRECT_URL` configurado **solo**, sin las otras tres, se ignora en silencio.

## Cifrado en reposo

Los tres secretos del §3 —`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`— se guardan **cifrados**: el sobre `v1:<nonce>:<cifrado>` con AES-256-GCM y **nonce aleatorio por valor**, y la clave se deriva de `KESTO_MASTER_KEY` con HKDF-SHA256 y la etiqueta constante `kesto/secrets/v1` (que **no** es configurable): **rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados**.

## Modo de desarrollo (sin OIDC)

Basta `KESTO_MASTER_KEY` para arrancar sin proveedor de identidad: sin ninguna de las tres `OIDC_*` el modo es `Disabled`. `GET /api/v1/health` contesta `auth_mode: "disabled"`. Ese modo **no pone ninguna capa**: la mitad protegida no se monta, todo `/api/v1/*` queda abierto y `/api/v1/auth/*` ni siquiera existe —contesta `404` con la envoltura, **no** `401`—. Es para desarrollo, no para un despliegue expuesto.

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

## Dónde está el resto

| Tema | Fichero |
|---|---|
| Estado del repositorio (construido, verificado y pendiente) | `docs/ESTADO.md` |
| Variables de entorno, versiones y recetas `just` | `docs/ENTORNO.md` · `docs/COMANDOS.md` |
| Arquitectura: cifrado, modo dev y diseño | `docs/ARQUITECTURA.md` |
| Workflows de GitHub Actions, gitmoji y ramas | `GIT_FLOW.md` |
| Cabos sueltos del SRS y cómo se cerró cada uno | `docs/DECISIONES-SRS.md` |
| SRS completo (manda sobre este fichero) | `docs/DEFINITION.md` |
