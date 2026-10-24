# Kesto

**Kesto** ("caja" en esperanto) es un orquestador ligero de copias de seguridad
que vive dentro de un contenedor. Extrae dumps de MariaDB y PostgreSQL, y
directorios del host, los guarda en repositorios [rustic](https://github.com/rustic-rs/rustic)
sobre S3 (MinIO, RusticFS, AWS) y aplica retención con `keep-daily`,
`keep-weekly` y `keep-monthly`. Todo se controla desde un panel web reactivo que
recibe el progreso en vivo por SSE.

## Estado

Proyecto **en construcción**: la base (especificación, gitflow y CI) está lista,
el código todavía no. La especificación técnica completa está en
[docs/DEFINITION.md](docs/DEFINITION.md) y manda sobre cualquier otra cosa.

## Arquitectura

| Capa | Tecnología |
|---|---|
| API | Rust + Axum (`src/`) |
| Estado | SQLite con SQLx (WAL, migraciones embebidas) |
| Docker | `bollard` sobre el socket del daemon |
| Repositorios | `rustic_core` + `rustic_backend` hacia S3 |
| Planificador | `tokio-cron-scheduler` + un worker por repositorio |
| Panel | React + TypeScript + Ant Design (`ui/`), progreso por SSE |

## Instalación

La imagen multi-arquitectura se publica en GHCR
(`ghcr.io/atareao/kesto`) en cuanto exista un `Dockerfile`; hasta entonces el
job de imagen del pipeline se salta solo:

```bash
docker pull ghcr.io/atareao/kesto:latest
```

El despliegue de referencia (socket del daemon, directorios a respaldar y
volumen para la SQLite) está en [docs/DEFINITION.md](docs/DEFINITION.md) §8.

## Desarrollo

```bash
cargo run            # backend
cargo test           # tests
cd ui && pnpm dev    # panel (cuando exista ui/)
```

Las instrucciones para agentes (layout, podman-first y las trampas del SRS)
están en [AGENTS.md](AGENTS.md); el flujo de ramas y releases, en
[GIT_FLOW.md](GIT_FLOW.md).

## Documentación

- [docs/DEFINITION.md](docs/DEFINITION.md) — SRS del proyecto
- [docs/](docs/) — un documento por feature (`FEATURE-NNN-<slug>.md`)
- [plans/](plans/) — planes de implementación (`PLAN-NNN.md`)

## Licencia

MIT — ver [LICENSE](LICENSE).
