# Relevo — higiene de la copia entregada y publicada en v0.6.1

`main` y `development` en **`dffb9cf`** (`chore: release v0.6.1`), versión **0.6.1**, tag **`v0.6.1`**. **Sin PRs ni issues abiertos.**

## Lo hecho en esta sesión

### FEATURE-015 — Higiene de la copia (#52, #53)

Ciclo SDD + TDD completo; **PR #54** mergeado. Change `higiene-de-la-copia` archivado (`openspec/changes/archive/2026-11-14-higiene-de-la-copia/`) y `plans/PLAN-015-COMPLETED.md`.

- **#52 — sin instantáneas huérfanas.** Cuando una copia acaba en `failed` **después** de que el archivado escribiera su instantánea, el motor la **olvida** (`Repositorio::olvidar_instantanea` → `delete_snapshots`). Cubre las **tres** ramas de `copiar_el_volcado`: fallo del productor, pánico del hilo y entrega incompleta. De **mejor esfuerzo**, sin sustituir el motivo original (helper puro `descartar_copia_fallida`); el camino `directory` no se toca.
- **#53 — volcados legibles.** El nodo del volcado declara `Metadata { mode: Some(0o644), .. }` en vez de `Metadata::default()`, que dejaba el `.sql` restaurado en modo `000`.
- **Nuevos:** `Error::OlvidoFallido` (`src/rustic/error.rs`), `Repositorio::olvidar_instantanea` (`src/rustic/repositorio.rs`), `descartar_copia_fallida`/`olvidar_la_instantanea` (`src/engine/ejecucion.rs`).
- **Docs:** `docs/FEATURE-015-higiene-de-la-copia.md` y nota en `docs/DEFINITION.md` §4.B. `lab/` añadido a `.gitignore`.
- Code review (rust-reviewer): sin bloqueantes; los 5 hallazgos cerrados antes de archivar (incl. el ALTO: cobertura del cableado, con dos pruebas e2e `#[ignore]` que muerden contra la mutación).

### Release v0.6.1

- PR de release **#56** (`development → main`) mergeado. `Prepare Release` subió **0.6.0 → 0.6.1** (bump **patch**, por los `🐛 fix`) y etiquetó `v0.6.1`.
- El tag disparó `release.yml` (**success**): imagen en **ghcr.io**, crate `kesto` en **crates.io** (`max_version: 0.6.1`) y GitHub Release **"kesto v0.6.1"**. `development` resincronizado con `main`.

## Verificación

- `just check-all` verde: **445 + 5** tests backend, **128** tests UI, `tsc`, `eslint`, `vite build`.
- CI de los PR #54, #55 y #56 en verde; workflows `Prepare Release` y `Release` de v0.6.1 en verde.
- **Verificación real en el banco (RustFS + `restic`)**, confirmada también con la imagen **publicada 0.6.1**: copia fallida (`sqlite` con ruta inexistente) → `failed` y el repositorio **sin** instantánea nueva (7 → 7; antes 7 → 8 con una de 0 bytes); restauración → `.sql` en modo **`644`** (legible), 41 814 B.
- Issues **#52** y **#53** cerrados.

## Pendiente / siguiente

1. **Backlog:** vacío (sin issues abiertos). Follow-ups anotados en `docs/FEATURE-015-higiene-de-la-copia.md`: el residuo de un olvido que **falle** se limpia en la próxima purga → mejora posible: reintento de olvido o purga manual de instantáneas huérfanas. Heredados de FEATURE-014: copia binaria con `.backup` (API `sqlite3_backup`) frente al `.dump` lógico, y Litestream como replicación continua.
2. Al abrir una feature nueva: empezar por su **GitHub Issue**, luego `plans/PLAN-016.md` y el change SDD.

## Lab (dejado arriba)

Red podman `kesto-lab`, todo alcanzable por nombre:

- `rustfs-lab` — RustFS (S3), puertos 9000/9001, creds `rustfsadmin`/`rustfsadmin`, bucket **`kesto`**. Datos en `lab/rustfs/`.
- `mariadb-src` (`pruebas`), `postgres-src` (`pruebas`), `sqlite-src` (`/datos/app.db`, WAL).
- `kesto` — imagen **`ghcr.io/atareao/kesto:0.6.1`**, red `kesto-lab`, `-p 8080:8080`, `--userns=keep-id:uid=10001,gid=10001`, volumen `kesto_data:/app/data`, socket podman en `/var/run/docker.sock`, `-v /data/rust/kesto/lab/directorio:/datos:ro`. Clave maestra en `/tmp/opencode/kesto-master-key`. Panel: `http://127.0.0.1:8080`.
- Parar el lab: `podman rm -f kesto rustfs-lab mariadb-src postgres-src sqlite-src`.
- **Recordatorio:** credenciales de Docker Hub del host caducadas → usar `podman pull --authfile /tmp/opencode/anon.json <img>`.

## Recordatorios heredados

- El CI **no** levanta daemon de contenedores: los e2e van `#[ignore]` (`cargo test -- --ignored`). `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock` en esta máquina.
- El binario del host (glibc nueva) **no** corre en la imagen `bookworm`: para probar contra RustFS, usar la imagen publicada del tag.
