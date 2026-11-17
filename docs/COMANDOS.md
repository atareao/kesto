# Recetas `just` y comandos

> Extracto **literal** del `AGENTS.md` de Kesto (poda v2, 2026-10-31). Tabla de recetas y avisos (`just dev`, `openspec init`). La fuente de verdad sigue siendo el `.justfile` (`just --list`).

## Comandos

**Empieza por el `.justfile`: `just --list` tiene todas las recetas.** Las que importan:

| Receta | Qué hace |
|---|---|
| `just check` | Los tres checks del CI, en su orden: `cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test --locked`. Ejecútalo antes de cada commit. |
| `just check-all` | `just check` + los cuatro pasos del panel (`just ui-check`). |
| `just doc` | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`. No está en el CI, pero caza enlaces rotos del rustdoc. |
| `just e2e` | Batería **e2e** de copia y restauración (issue #86): levanta su propio servidor S3 (SeaweedFS) y exige un motor de contenedores, para los cuatro tipos de origen. Es la feature `e2e`: el job `backend` del CI no la corre. Cuesta minutos. |
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
| `just clean-tmp` | Borra a mano los restos `/tmp/kesto-prueba-*` que dejan las pruebas: directorios por pid de procesos ya muertos y restos `<uuid>` heredados. El arnés los recoge solo al arrancar (`src/test_support.rs`), pero si `/tmp` se llena de inodos hay que poder limpiar a mano. No toca nada que no lleve el prefijo ni los procesos vivos. |

Los comandos crudos siguen siendo la fuente de verdad del CI y `just check` los encadena en su orden exacto, así que no hay dos listas que mantener.

⚠️ `just dev` necesita **su propio `DATABASE_URL`**: el defecto apunta al volumen del `compose` del §8 (`sqlite:///app/data/kesto.db`) y en una máquina de desarrollo ese directorio no existe, así que el arranque falla nombrando la ruta. Exporta uno tuyo antes de la receta (`mkdir -p /tmp/kesto && export DATABASE_URL=sqlite:///tmp/kesto/kesto.db`).

⚠️ `just e2e` necesita un **motor de contenedores**: toma el socket de `DOCKER_HOST` y, si no está puesto, cae al de podman de esta máquina (`unix://${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock`). Sin un socket que responda, la batería **falla** nombrando la variable y el socket esperado —no se salta ningún escenario—. En el CI lo corre el job `e2e` con Docker; en local, contra el podman del socket.

⚠️ `openspec init` configura los ficheros de instrucciones de la herramienta que le pases, y `--tools opencode` **escribe en `AGENTS.md`**, que aquí es un documento vivo. Por eso `just spec-init` usa `--tools none` y exige el árbol limpio, para que puedas revisar el diff antes de nada.

