# Relevo — FEATURE-016 entregada: identidad visual y tema claro/oscuro

`development` y `main` con la base **v0.6.1** (tag `v0.6.1`). **Sin issues abiertos** salvo lo que se anote como follow-up. El laboratorio sigue **levantado** (ver §Lab).

## Hecho en la sesión anterior

### FEATURE-016 — Identidad visual y tema (issue #60)

Ciclo **SDD + TDD** completo. Change archivado en `openspec/changes/archive/2026-11-14-identidad-visual-y-tema/` (delta fusionada en `openspec/specs/ui/spec.md`), plan cerrado en `plans/PLAN-016-COMPLETED.md`, doc en `docs/FEATURE-016-identidad-visual-y-tema.md`.

- **Assets de marca**: favicons + `site.webmanifest` + Open Graph en `ui/public/`; logos en `ui/src/assets/`; `ui/index.html` con iconos, manifest, `theme-color` y meta OG, más un script previo al pintado que evita el destello.
- **Paleta de marca** en tokens de Antd (primario cyan `#2194AA`; fondos `#0D1B2A`/`#F8FAFC`).
- **Tema claro/oscuro con oscuro por defecto** y conmutador accesible persistido en `localStorage` (`kesto-tema`). No sigue al sistema.
- **Contraste WCAG AA** verificado token a token por modo (test con helper de luminancia relativa).
- Verificación: `just check-all` verde (**162** tests UI + `tsc` + `eslint` + `build`); revisado con `@react-reviewer` (2 hallazgos ALTO y varios MEDIO/BAJO, corregidos).

## Pendiente / siguiente

- **Backlog funcional:** vacío. Follow-ups de FEATURE-016: `useCallback`/`useMemo` redundantes en `tema.tsx`; `"types": ["node"]` en `ui/tsconfig.json` acotable a un tsconfig de tests; bundle del panel en un único chunk (code-splitting de Antd).
- **Heredados:** copia binaria `.backup` de SQLite (API `sqlite3_backup`) y Litestream; reintento/purga del «olvido fallido» de FEATURE-015.
- Al abrir una feature nueva: empezar por su **GitHub Issue**, luego `plans/PLAN-017.md` y el change SDD.

## Lab (dejado arriba)

Red podman `kesto-lab`, todo alcanzable por nombre:

- `rustfs-lab` — RustFS (S3), puertos 9000/9001, creds `rustfsadmin`/`rustfsadmin`, bucket **`kesto`**. Datos en `lab/rustfs/`.
- `mariadb-src` (`pruebas`), `postgres-src` (`pruebas`), `sqlite-src` (`/datos/app.db`, WAL).
- `kesto` — imagen **`ghcr.io/atareao/kesto:0.6.1`** con el **panel nuevo montado**: `-v /data/rust/kesto/ui/dist:/app/ui/dist:ro` (el `release` no recompila el binario por un cambio de panel). Red `kesto-lab`, `-p 8080:8080`, `--userns=keep-id:uid=10001,gid=10001`, volumen `kesto_data:/app/data`, socket podman en `/var/run/docker.sock`, `-v /data/rust/kesto/lab/directorio:/datos:ro`. Clave maestra en `/tmp/opencode/kesto-master-key`. Panel: `http://127.0.0.1:8080`.
  - **Cuidado:** si recreas `kesto` sin el montaje de `ui/dist`, vuelve al panel **viejo** de la imagen. Para el panel nuevo, reconstruye `ui/dist` (`pnpm run build`) y añade el montaje; o construye una imagen completa con `just build`.
- Parar el lab: `podman rm -f kesto rustfs-lab mariadb-src postgres-src sqlite-src`.
- **Recordatorio:** credenciales de Docker Hub del host caducadas → usar `podman pull --authfile /tmp/opencode/anon.json <img>`.

## Recordatorios heredados

- El CI **no** levanta daemon de contenedores: los e2e van `#[ignore]` (`cargo test -- --ignored`). `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock` en esta máquina.
- El binario del host (glibc nueva) **no** corre en la imagen `bookworm`: para probar contra RustFS, usar la imagen publicada del tag.
- Antes de cada commit: `just check` (backend) y, en `ui/`, `pnpm run lint` → `npx tsc --noEmit` → `pnpm test`. Git flow con gitmoji; nada directo a `main`/`development`.
