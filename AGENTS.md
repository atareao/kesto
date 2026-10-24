# Kesto — Instrucciones para agentes

> Las directivas globales de este entorno (OpenSpec SDD + TDD, tags de estado, tabla de subagentes) se inyectan ya en cada sesión desde `~/.config/opencode/AGENTS.md`. No se repiten aquí: lo de abajo es solo lo específico de este repositorio.

## Estado del repositorio (2026-10-24)

Base ya creada: commit inicial en `main`, rama `development` activa, más `GIT_FLOW.md`, `AGENTS.md` y los directorios `plans/` y `docs/`. **Sin remote y sin CI todavía.**

Sigue siendo **greenfield en código**: `src/main.rs` es un `Hello, world!`, `Cargo.toml` no tiene dependencias y no hay `openspec/`, `ui/`, `Dockerfile`, `justfile`, tests ni CI. Todo lo que sigue describe el sistema **a construir**, no lo que hay.

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

Nada está inicializado aún; estos son los que aplican en cuanto exista el andamiaje:

```bash
cargo run                       # backend (único binario por ahora: src/main.rs)
cargo test                      # unitarios en #[cfg(test)] + integración en tests/
cargo clippy -- -D warnings     # cero warnings: obligatorio antes de commit
cargo fmt --check
cd ui && pnpm dev | build | test | lint && npx tsc --noEmit
openspec init                   # el root de OpenSpec todavía no existe
openspec list | openspec new change <feature> | openspec archive <feature>
podman compose up -d --build    # despliegue local
```

Orden antes de commit: `cargo fmt` → `cargo clippy -- -D warnings` → `cargo test`; en `ui/`: `tsc --noEmit` → `vitest run` → `lint`.

## Git Flow — obligatorio

**Este proyecto usa gitflow siempre: nada se commitea directamente en `main` ni en `development`.**

| Rama | Base | Uso |
|---|---|---|
| `main` | — | Producción |
| `development` | `main` | Integración (rama activa de trabajo) |
| `feature/<kebab-case>` | `development` | Nuevas funcionalidades |
| `hotfix/<kebab-case>` | `main` | Correcciones urgentes sobre producción |

- Mensajes: conventional commits con gitmoji (`✨ feat:`, `🐛 fix:`, `🚑️ hotfix:`, `📝 docs:`, `✅ test:`, `🔧 chore:`…). Flujo completo y tabla de gitmoji en `GIT_FLOW.md`; automatización en la skill `gitflow`.
- **Ojo con la skill `gitflow`**: sus pasos de formateo asumen `backend/` y `frontend/`. Aquí el layout es `src/` + `ui/`: adapta los comandos, no los directorios.
- Formateo **antes** de cada commit: `cargo fmt --all` → `cargo clippy -- -D warnings` → `cargo test`; en `ui/`: `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Si el formateo toca ficheros, commit aparte (`🎨 style: apply formatting`).
- **No hay remote configurado**: `git push`, `gh pr create` y todo el flujo de PR fallan hasta que exista. Mientras tanto, trabaja en local con las ramas igualmente.

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

## Cabos sueltos del SRS (decidir antes de implementar)

1. **No hay autenticación en ninguna capa**: §5 no define login ni tokens y §8 publica el puerto 8080. El panel permite crear repositorios con credenciales S3 y disparar dumps. Hay que cerrarlo antes de exponer el servicio.
2. **Secretos en claro**: `repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password` son `TEXT` plano en §3 (el comentario dice "Encrypted/Plain"). Hay que elegir el cifrado en reposo antes de escribir el schema.
3. **`db_name` es inaplicable con los comandos de §4.A**: `mariadb-dump --all-databases` y `pg_dumpall` vuelcan todo el servidor e ignoran `backup_targets.db_name`. Para soportar dump de una sola base hay que pasar a `mariadb-dump <db>` / `pg_dump -d <db> -U <user>`.
4. Las contraseñas van por línea de comandos (`-p<db_password>`), visibles en el listado de procesos del contenedor: considera `MYSQL_PWD` o fichero de credenciales.
5. `job_executions` solo tiene FK a `backup_jobs`; filtrar ejecuciones por repositorio obliga a un join. Si el dashboard lo va a hacer, añade el índice correspondiente.
6. El compose de §8 monta el socket como `:ro`, pero el motor necesita la API completa (`exec`). Verifica que el `exec` funciona con ese montaje en tu runtime antes de darlo por bueno.
