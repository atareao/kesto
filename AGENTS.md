# Kesto — Instrucciones para agentes

> Las directivas globales de este entorno (OpenSpec SDD + TDD, tags de estado, tabla de subagentes) se inyectan ya en cada sesión desde `~/.config/opencode/AGENTS.md`. No se repiten aquí: lo de abajo es solo lo específico de este repositorio.

## Estado del repositorio (2026-10-24)

Repositorio público en `github.com/atareao/kesto` con la base cerrada: ramas `main` (producción) y `development` (integración), pipeline de GitHub Actions en verde, `GIT_FLOW.md`, `openspec/` (con `config.yaml`, las especificaciones ya fusionadas en `specs/` —`config`, `secrets`, `api-shell` y `auth`— y los changes archivados en `changes/archive/`), `.justfile` y los directorios `plans/` y `docs/`, con sus `PLAN-001-COMPLETED.md`, `PLAN-002-COMPLETED.md`, `docs/FEATURE-001-bootstrap-core.md` y `docs/FEATURE-002-auth-capability.md`. Los dos primeros changes están **implementados y verificados**: `bootstrap-core` —arranque, configuración desde el entorno, redacción de secretos, envoltura JSON única, salud, `404`/`405` y apagado ordenado con `SIGTERM`/`SIGINT`— y `auth-capability` —sesión en cookie `HttpOnly`, flujo OIDC contra un proveedor externo, puerta sobre toda la superficie de negocio y modo de desarrollo sin capa—, con **93 tests en verde** y los cuatro portales limpios (`cargo fmt`, `cargo clippy -D warnings`, `cargo test` y `cargo doc`). Lo que **sigue sin existir** es lo que más importa: `ui/`, `Dockerfile` y los cinco módulos del §7 todavía sin contenido (`db`, `docker`, `engine`, `rustic`, `scheduler`), porque el change `db-schema` está sin empezar: no hay base de datos, ni SSE, ni motor, ni planificador. De `src/` existen ya `api`, `auth` y los módulos de nivel superior `config`, `secret`, `error` y `state`; ojo con `src/auth/`: es un módulo propio que **no** está en el §7, porque se separa el HTTP (`src/api/auth.rs`) del flujo, la sesión y el *gate*. Todo lo que sigue describe el sistema **a construir**, no lo que hay.

**Fuente de verdad: `docs/DEFINITION.md`** (SRS completo, en español). Manda sobre este fichero cuando haya conflicto. Secciones que hay que leer antes de tocar código:

| § | Contenido |
|---|---|
| §3 | Schema SQLite exacto: 4 tablas, `CHECK` de enums, `ON DELETE CASCADE` |
| §4 | Flujo de extracción por streaming y lógica de retención/prune |
| §5 | Endpoints `/api/v1/*` y el JSON exacto del evento SSE |
| §6–§8 | Pantallas Antd, layout del repo, compose de producción |

## Qué es Kesto

Orquestador ligero de backups dentro de un contenedor: extrae dumps de MariaDB/PostgreSQL (vía `docker exec` en el contenedor de la base de datos) y directorios del host, los mete en repositorios Rustic sobre S3 y aplica retención. Panel web React + Ant Design con progreso en vivo por SSE.

- Backend: Axum 0.7+, SQLite vía `sqlx` (WAL + migraciones embebidas), `bollard` (API Docker por socket UNIX), `tokio-cron-scheduler`, `rustic_core` + `rustic_backend`.
- Frontend (en `ui/`): React + TypeScript (el SRS dice 18+, el entorno usa 19 en repos nuevos) + Antd v5 + TanStack Query + `EventSource`.

## Variables de entorno

| Variable | Obligatoria | Valor por defecto | Para qué |
|---|---|---|---|
| `RUST_LOG` | no | `info` | Filtro de `tracing-subscriber`. Nombre del §8, sin prefijo |
| `BIND_ADDRESS` | no | `0.0.0.0:8080` | Dirección de escucha. Nombre del §8, sin prefijo |
| `DATABASE_URL` | no | `sqlite:///app/data/kesto.db` | DSN de SQLite. Nombre del §8, sin prefijo. **Todavía no se usa**: llega con `db-schema` |
| `KESTO_MASTER_KEY` | **sí, siempre** | — | Clave maestra del cifrado en reposo. Aún no cifra nada, pero se exige desde el primer día |
| `KESTO_SECRET` | **sí, si el modo es OIDC** | — | Clave de firma del JWT de sesión (`HS256`). Sin ella en modo OIDC el arranque falla nombrándola |
| `OIDC_ISSUER_URL` | sí, si el modo es OIDC | — | Emisor del proveedor |
| `OIDC_CLIENT_ID` | sí, si el modo es OIDC | — | Identificador del cliente |
| `OIDC_CLIENT_SECRET` | sí, si el modo es OIDC | — | Secreto del cliente |
| `OIDC_REDIRECT_URL` | no | `http://localhost:8080/api/v1/auth/callback` | URL de retorno del callback |

- Los tres nombres del §8 (`RUST_LOG`, `BIND_ADDRESS`, `DATABASE_URL`) se leen **tal cual**, sin prefijo `KESTO_`, para que un `compose` copiado de `docs/DEFINITION.md` funcione sin editar una línea. El resto lleva prefijo `KESTO_`, salvo `OIDC_*`, que conserva el nombre que ya usa shuul.
- Las tres variables `OIDC_*` que gobiernan el modo (`OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET`) son **un todo o nada**: las tres presentes encienden la autenticación; ninguna presente la apaga y lo anuncia con un aviso; unas sí y otras no es **error de arranque**, no un modo degradado. Degradar a «apagada» dejaría el panel abierto sin que nadie se entere.
- `KESTO_MASTER_KEY` es obligatoria **siempre**, aunque todavía no haya nada que cifrar.
- `KESTO_SECRET` es obligatoria **en modo OIDC** —es la clave con la que se firma la sesión— y no hace falta en modo `Disabled`. **Rotarla invalida todas las sesiones vivas**: no hay tabla de sesiones ni estado en el servidor, cada petición se verifica por firma contra la clave que tenga el proceso en memoria.
- Una credencial de menos de 32 caracteres **avisa y arranca igualmente**: Kesto es el orquestador de las copias, y negarse a arrancar por una variable floja convertiría un error de configuración en ausencia de copias.
- `BIND_ADDRESS` sin puerto es error de arranque, y **el valor vacío no cae al defecto**: `BIND_ADDRESS=` mata el arranque. Si usas `compose` con interpolación de una variable que no definiste, acuérdate de esto.
- Lo mismo vale para `RUST_LOG`: presente pero vacía es **error de arranque**, no «sin registro». Si dejas `RUST_LOG=` en un `compose` con la variable sin definir, el proceso no arranca —que es mucho mejor que un servicio ciego—. Quitarla del todo sí cae al valor por defecto, `info`. Los espacios sobrantes se recortan: `RUST_LOG=" info "` funciona igual que `RUST_LOG=info`.
- Un `OIDC_REDIRECT_URL` configurado **solo**, sin las otras tres, se ignora en silencio: los tres desenlaces se deciden sobre las tres variables de la autenticación. Quien configure solo el retorno creerá que encendió algo.

### Modo de desarrollo (sin OIDC)

Basta `KESTO_MASTER_KEY` para arrancar sin proveedor de identidad: sin ninguna de las tres `OIDC_*` el modo es `Disabled`. En ese modo `GET /api/v1/health` contesta `auth_mode: "disabled"` —eso es lo que hay que consultar para comprobar qué modo está activo— y el arranque lo anuncia con un `WARN`.

⚠️ Ese modo **no pone ninguna capa**: la mitad protegida no se monta, todo `/api/v1/*` queda abierto y `/api/v1/auth/*` ni siquiera existe —contesta `404` con la envoltura, **no** `401`—. Es para desarrollo, no para un despliegue expuesto. Con las tres `OIDC_*` configuradas, en cambio, todo `/api/v1/*` exige sesión salvo la salud y las dos rutas públicas del flujo de entrada.

## Layout (SRS §7 — respétalo)

```text
src/{main.rs,api,db,docker,engine,rustic,scheduler}
ui/{src/{components,pages,services},vite.config.ts}
```

`src/` + `ui/` en la raíz, **no** `backend/` + `frontend/`. El resto de repos Rust+React de este entorno usan la otra convención: no la apliques aquí.

## Entorno de desarrollo (verificado en esta máquina)

| Herramienta | Estado |
|---|---|
| Rust / Cargo | 1.97 · `edition = "2024"` (exige ≥ 1.85) |
| Node / pnpm | v24.22.1 · pnpm 12.15.0 (npm 12.2.1) |
| Contenedores | **no hay `docker` ni `/var/run/docker.sock`**; Podman 6.1.4 con API Docker-compatible en `unix:///run/user/1000/podman/podman.sock` (`/_ping` → 200, `/containers/json` responde) |
| CLI | `just` 1.59.0 · `openspec` 1.13.2 · `codegraph` 0.7.9 · `gh` 2.102.1 (autenticado como `atareao`) · `vampus` 0.4.6 · `git-cliff` 2.14.2 |

- **Podman en local, Docker en producción** (decisión tomada el 2026-10-24). En dev, bollard apunta al socket de podman con la ruta configurable (`DOCKER_HOST`); nunca al de Docker. El compose de producción (§8) sigue montando `/var/run/docker.sock`.
- Cualquier comando `docker ...` falla en esta máquina: usa `podman ...`.
- Antes de cablear bollard, comprueba la negociación de versión de API contra podman; si se atasca, fíjala explícitamente en vez de dejar que la deduzca.

## Comandos

**Empieza por el `.justfile`: `just --list` tiene todas las recetas.** Las que importan:

| Receta | Qué hace |
|---|---|
| `just check` | Los tres checks del CI, en su orden: `cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test --locked`. Ejecútalo antes de cada commit. |
| `just check-all` | `just check` + los cuatro pasos del panel (`just ui-check`). |
| `just doc` | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`. No está en el CI, pero caza enlaces rotos del rustdoc. |
| `just dev` | Backend en local con `RUST_LOG=debug`. El panel va aparte, en otra terminal: `just ui-dev`. |
| `just plans` · `just plan-new` | Lista los planes con su estado · imprime la ruta del siguiente `plans/PLAN-NNN.md` (contador sin huecos). |
| `just plan-done N` · `just plan-pending N` | Renombra el plan N con `git mv` a `-COMPLETED` / `-PENDING`. |
| `just feature-new <slug>` | Crea el stub del siguiente `docs/FEATURE-NNN-<slug>.md`. |
| `just check-spec` | Puerta del protocolo SDD: falla si no hay change proposal activo en `openspec/changes/`. |
| `just spec-new <c>` · `just spec-validate <c>` · `just spec-archive <c>` | `openspec new change` (sí existe en 1.13.2) · `openspec validate <c> --strict` · `openspec archive <c>`. |
| `just gf-feature <n>` · `just gf-hotfix <n>` · `just gf-pr` | Ramas y PR según gitflow (`gf-pr` abre el PR a `development`, y a `main` desde `hotfix/*`). |
| `just gf-release` | PR `development` → `main`. Comprueba antes que exista el secreto `GH_PAT`: mergearlo dispara la release automática. |
| `just build` · `just run` | Imagen local y contenedor de prueba con el socket de podman montado (abortan mientras no haya `Dockerfile`). |
| `just bump <patch\|minor\|major>` | `vampus upgrade` + `cargo update --workspace`. La versión vive en tres sitios: `Cargo.toml`, `Cargo.lock` y `.vampus.yml`. |
| `just pipeline` | Últimos runs del CI en GitHub. |

Los comandos crudos siguen siendo la fuente de verdad del CI y `just check` los encadena en su orden exacto, así que no hay dos listas que mantener.

⚠️ `openspec init` configura los ficheros de instrucciones de la herramienta que le pases, y `--tools opencode` **escribe en `AGENTS.md`**, que aquí es un documento vivo. Por eso `just spec-init` usa `--tools none` y exige el árbol limpio, para que puedas revisar el diff antes de nada.

## Git Flow — obligatorio

**Este proyecto usa gitflow siempre: nada se commitea directamente en `main` ni en `development`.**

| Rama | Base | Uso |
|---|---|---|
| `main` | — | Producción. Un merge aquí **dispara una release**. |
| `development` | `main` | Integración (rama activa de trabajo) |
| `feature/<kebab-case>` | `development` | Nuevas funcionalidades |
| `hotfix/<kebab-case>` | `main` | Correcciones urgentes sobre producción |

```bash
git checkout development && git pull --ff-only
git checkout -b feature/<nombre>
# ... trabajo ...
git push -u origin feature/<nombre>
gh pr create --base development --head feature/<nombre>
```

- Mensajes: conventional commits con gitmoji (`✨ feat:`, `🐛 fix:`, `🚑️ hotfix:`, `📝 docs:`, `✅ test:`, `🔧 chore:`…). Flujo completo y tabla de gitmoji en `GIT_FLOW.md`; automatización en la skill `gitflow`.
- **Ojo con la skill `gitflow`**: sus pasos de formateo asumen `backend/` y `frontend/`. Aquí el layout es `src/` + `ui/`: adapta los comandos, no los directorios.
- Formateo **antes** de cada commit: `cargo fmt --all` → `cargo clippy -- -D warnings` → `cargo test`; en `ui/`: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Si el formateo toca ficheros, commit aparte (`🎨 style: apply formatting`).
- Nunca hagas `git commit` directo en `main`: ahí solo entran PRs de release desde `development`.

## Pipeline (GitHub Actions)

| Workflow | Se dispara con | Hace |
|---|---|---|
| `ci.yml` | PR y push a `development`/`main` | `preflight` detecta el árbol → `backend` (`fmt --check`, `clippy -D warnings`, `test --locked`) → `frontend` (npm o pnpm según el lockfile) |
| `release-prepare.yml` | push a `main` (solo eso) | bump con vampus, CHANGELOG con git-cliff, tag `vX.Y.Z`, sincroniza `development` |
| `release.yml` | tag `v*` | imagen multi-arquitectura a `ghcr.io/atareao/kesto` + crate a crates.io + GitHub Release |

Lo que hay que saber antes de tocarlo:

- **Un merge a `main` ES una release.** No se etiqueta a mano: `release-prepare` decide el bump desde los conventional commits (`feat` → minor, `!:` / `BREAKING CHANGE` → major, resto → patch) y empuja el tag, que es lo que dispara `release.yml`.
- **Dos guardas deliberadas** para no publicar basura, ambas auto-activables: la imagen se salta mientras no existan `Dockerfile` y `ui/package.json`; el crate se salta mientras `Cargo.toml` diga `publish = false`. **Quita `publish = false` cuando el crate merezca publicarse**: crates.io no permite borrar versiones, solo yank.
- **El job `frontend` del CI se salta solo** mientras no exista `ui/package.json`, y elige npm o pnpm por el lockfile presente. No hace falta tocarlo cuando llegue la UI.
- **Secretos necesarios antes de la primera release**: `GH_PAT` (PAT con `contents: write`) y `CARGO_REGISTRY_TOKEN`. Sin `GH_PAT` el tag no dispara `release.yml`, porque un tag empujado con el `GITHUB_TOKEN` por defecto no dispara workflows.
- La versión vive en **tres sitios** que deben ir sincronizados: `Cargo.toml`, la entrada `[[package]]` de `Cargo.lock` y `.vampus.yml:current_version`. `release-prepare` los resincroniza y falla en alto si vampus no sube nada; si lo tocas a mano, toca los tres.
- El CI corre exactamente `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked`: replica esos comandos antes de cada push para no descubrir el fallo en el PR.

## Planes y documentación — regla fija

Dos directorios en la raíz, siempre:

**1) `plans/`** — un fichero por plan, correlativo de tres dígitos y sin huecos (el siguiente número = mayor existente + 1; `plans/` es la única fuente del contador):

| Estado | Nombre |
|---|---|
| En curso | `plans/PLAN-001.md` |
| Terminado | `plans/PLAN-001-COMPLETED.md` |
| Aparcado | `plans/PLAN-001-PENDING.md` |

Los planes **se renombran, nunca se borran** (`git mv`). El renombrado al cerrar o aparcar lo haces tú al terminar, no el subagente que escribe el plan.

**2) `docs/`** — un documento por feature entregada, correlativo y emparejado con su plan: `docs/FEATURE-001-<slug>.md` (slug en kebab-case). `docs/DEFINITION.md` es el SRS de referencia y no se mezcla con las features.

Ciclo de vida de una feature: change proposal de OpenSpec aprobado → `plans/PLAN-NNN.md` → implementación TDD → al cerrar, `docs/FEATURE-NNN-<slug>.md` con lo que realmente quedó construido (contratos, endpoints, esquema, decisiones y deuda pendiente) → `plans/PLAN-NNN-COMPLETED.md`.

⚠️ **`@plan-writer` escribe `PLAN.md` en la raíz por defecto.** En este repo los planes van a `plans/PLAN-NNN.md`: pásale la ruta de destino explícita y el contenido completo del plan en el prompt.

## Restricciones de diseño que un agente rompería por defecto

- **Un worker secuencial por repositorio S3**, no un task de Tokio por job. `rustic_core` toma un lock exclusivo del repositorio: paralelizar backups del mismo repo los bloquea entre sí. Cola MPSC + un worker por `repository_id`.
- **Streaming en memoria, sin ficheros temporales**: `stdout` del `exec` de bollard → `AsyncRead` en un buffer de Tokio → `NodeWriter`/`TreeBuilder` de `rustic_core`. No escribas el dump a disco ni a `/tmp`.
- **Un único stream SSE global** en `GET /api/v1/events`, con `event: <nombre>` (`execution_progress`, …) y el payload literal de §5. No inventes otro esquema de mensaje.
- **Migraciones embebidas**: `sqlx::migrate!()` resuelve su directorio en tiempo de compilación, así que `migrations/` tiene que existir antes del primer `cargo check` que lo incluya.
- **El arnés de captura de registro (`src/test_support.rs`) instala un suscriptor global y elige el buffer por hilo**: nunca `set_default` ni `with_default`. Un despachante de hilo deja un callsite cacheado como `Interest::never` para todo el proceso y su registro desaparece **en silencio** —el síntoma es «el log está vacío», no «el arnés está roto»—, y solo se manifiesta en CI (2 CPUs), no en local.

## Cabos sueltos del SRS (decidir antes de implementar)

1. **No hay autenticación en ninguna capa**: §5 no define login ni tokens y §8 publica el puerto 8080. El panel permite crear repositorios con credenciales S3 y disparar dumps. Hay que cerrarlo antes de exponer el servicio.
2. **Secretos en claro**: `repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password` son `TEXT` plano en §3 (el comentario dice "Encrypted/Plain"). Hay que elegir el cifrado en reposo antes de escribir el schema.
3. **`db_name` es inaplicable con los comandos de §4.A**: `mariadb-dump --all-databases` y `pg_dumpall` vuelcan todo el servidor e ignoran `backup_targets.db_name`. Para soportar dump de una sola base hay que pasar a `mariadb-dump <db>` / `pg_dump -d <db> -U <user>`.
4. Las contraseñas van por línea de comandos (`-p<db_password>`), visibles en el listado de procesos del contenedor: considera `MYSQL_PWD` o fichero de credenciales.
5. `job_executions` solo tiene FK a `backup_jobs`; filtrar ejecuciones por repositorio obliga a un join. Si el dashboard lo va a hacer, añade el índice correspondiente.
6. El compose de §8 monta el socket como `:ro`, pero el motor necesita la API completa (`exec`). Verifica que el `exec` funciona con ese montaje en tu runtime antes de darlo por bueno.
7. Aún no hay `Dockerfile`: hasta que exista, el job de imagen de `release.yml` se salta solo. Escríbelo con el orden de etapas del §7 (UI → Rust → runtime) y sin privilegios de root.
