# Kesto

**Orquestador de copias de seguridad de bases de datos y volúmenes a S3, dentro de un contenedor.**

## Qué es

Kesto ("caja" en esperanto) es un contenedor único que:

- ejecuta `mariadb-dump` / `pg_dumpall` **dentro del contenedor de tu base de datos** (vía `exec` por la API del daemon) y transmite la salida **en streaming, sin pasar por disco**;
- guarda el resultado en un **repositorio [rustic](https://github.com/rustic-rs/rustic) sobre S3** (MinIO, RusticFS, AWS);
- aplica **retención** tras cada copia (`keep-daily`, `keep-weekly`, `keep-monthly`);
- lo programa con **cron** y lo muestra en un **panel web** con el progreso en vivo por SSE.

## Estado

Proyecto **en construcción**, pero el código ya ha empezado y el primer bloque está cerrado. La base está lista —especificación, gitflow, CI y `.justfile`— y sobre ella ya se ha implementado `bootstrap-core`: arranque, configuración desde el entorno, redacción de credenciales, envoltura JSON única y apagado ordenado, con 25 tests en verde. Lo siguiente es la autenticación. La especificación técnica ([docs/DEFINITION.md](docs/DEFINITION.md)) manda sobre todo lo demás.

| Pieza | Estado |
|---|---|
| Arranque y API | Cerrado: configuración desde el entorno, envoltura JSON única, salud, `404`/`405` y apagado ordenado (25 tests) |
| CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) | Verde: `cargo fmt --check`, `cargo clippy -- -D warnings` y `cargo test --locked` |
| Imagen | Pendiente de `Dockerfile`; el job del pipeline se salta solo hasta que exista |
| Crate | `publish = false` a propósito, para no dejar un placeholder permanente en crates.io |
| Panel | `ui/` todavía no existe; el job de frontend del CI se salta solo |

## Arquitectura

```mermaid
flowchart LR
  subgraph K["Contenedor Kesto"]
    UI["Panel React + Ant Design"]
    API["Axum: REST + SSE"]
    DB[("SQLite")]
    CRON["Scheduler tokio-cron"]
    ENG["Motor de ejecucion"]
  end
  SOCK["Socket del daemon"]
  PG[("MariaDB / PostgreSQL")]
  S3[("S3")]
  UI -.->|"SSE"| API
  API --> DB
  API --> CRON
  CRON --> ENG
  ENG -->|"bollard: exec"| SOCK
  SOCK --> PG
  PG -->|"stdout en streaming"| ENG
  ENG -->|"rustic_core"| S3
```

| Capa | Tecnología |
|---|---|
| API | Rust + Axum (`src/`) |
| Estado | SQLite con SQLx (WAL, migraciones embebidas) |
| Docker | `bollard` sobre el socket del daemon |
| Repositorios | `rustic_core` + `rustic_backend` hacia S3 |
| Planificador | `tokio-cron-scheduler` + un worker por repositorio |
| Panel | React + TypeScript + Ant Design (`ui/`), progreso por SSE |

## Requisitos

- **Rust 1.85+** (`edition = "2024"`; desarrollado con 1.97).
- **Podman** en desarrollo y Docker en producción: el motor habla con la API del daemon a través del socket UNIX.
- **Node 24 + pnpm 12** solo si vas a tocar el panel.

## Instalación

La imagen multi-arquitectura (amd64 + arm64) se publica en `ghcr.io/atareao/kesto` en cuanto exista un `Dockerfile`:

```bash
docker pull ghcr.io/atareao/kesto:latest
```

El despliegue de referencia —socket del daemon, directorios del host a respaldar y volumen para la SQLite— está en [docs/DEFINITION.md](docs/DEFINITION.md) §8.

## Desarrollo

Todo pasa por el `.justfile`:

```bash
just --list     # todas las recetas disponibles
just check      # los tres checks del CI, en su orden exacto
just dev        # backend en local
just ui-dev     # panel con recarga en caliente (cuando exista ui/)
```

`just dev` **necesita `KESTO_MASTER_KEY` en el entorno** o el proceso no arranca. Sin variables `OIDC_*` arranca en modo desarrollo con la autenticación apagada; usa `RUST_LOG=debug` para ver el detalle.

Las recetas cubren además el ciclo gitflow (`gf-*`), los planes y las features (`plan-*`, `feature-new`), la imagen local (`build`, `run`) y OpenSpec (`check-spec`, `spec-*`).

## Pipeline

**Un merge a `main` es una release**, sin pasos manuales: [`release-prepare.yml`](.github/workflows/release-prepare.yml) deduce el bump de los conventional commits, sube la versión con vampus, genera el CHANGELOG con git-cliff y etiqueta `vX.Y.Z`. El tag dispara [`release.yml`](.github/workflows/release.yml), que publica la imagen multi-arquitectura en GHCR y el crate en crates.io. Detalles en [GIT_FLOW.md](GIT_FLOW.md).

## Estructura

```text
src/     backend Rust (Axum)
  api/       endpoints: hoy solo salud (GET /api/v1/health) y la envoltura JSON
  config.rs  configuración desde el entorno
  secret.rs  redacción de credenciales
  error.rs   errores tipados + IntoResponse
  state.rs   estado compartido
  # db, docker, engine, rustic y scheduler llegan con los changes siguientes
ui/      panel React + TypeScript + Ant Design (pendiente)
docs/    SRS y un documento por feature (FEATURE-NNN-<slug>.md)
plans/   planes de implementación (PLAN-NNN.md)
```

## Documentación

- [docs/DEFINITION.md](docs/DEFINITION.md) — SRS: esquema SQLite, endpoints, flujos y despliegue
- [AGENTS.md](AGENTS.md) — instrucciones para agentes: layout, podman-first y las trampas del SRS
- [GIT_FLOW.md](GIT_FLOW.md) — ramas, commits convencionales y releases

## Licencia

MIT — ver [LICENSE](LICENSE). También declarada en `Cargo.toml` (`license = "MIT"`), que es lo que exige crates.io para publicar.
