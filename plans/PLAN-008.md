---
# Plan 008 — Despliegue en producción: Dockerfile y compose (`despliegue-produccion`)

- **Issue:** #17
- **SRS:** `docs/DEFINITION.md` §8 (compose de producción), §2 (componentes), §7 (estructura)
- **Change OpenSpec:** `openspec/changes/despliegue-produccion/`
- **Doc:** `docs/FEATURE-008-despliegue-produccion.md`

## Objetivo

Empaquetar el binario Kesto y el panel `ui/dist` en una imagen reproducible (multi-stage) y desplegarlo con Docker Compose según el §8, cerrando los cabos que el ejemplo del SRS deja sueltos (sobre todo `KESTO_MASTER_KEY`, que es obligatoria siempre y el §8 omite).

## Decisiones de partida (fase 0 las confirma)

- **Dockerfile multi-stage:** etapa `builder` (imagen Rust + toolchain) que compila `cargo build --release --locked`; etapa de panel (Node + pnpm) que produce `ui/dist`; etapa final mínima (`debian:bookworm-slim`), usuario no root, sin toolchain. Cacheo por capas (copiar manifiestos y `ui/pnpm-lock.yaml` antes del código).
- **`.dockerignore`:** excluir `target/`, `ui/node_modules`, `ui/dist`, `.git`, `assets/`, documentación no necesaria.
- **`compose.yaml` de producción** siguiendo §8: servicio `kesto`, `container_name: kesto-app`, `restart: always`, `8080:8080`, volúmenes (socket Docker `:ro`, fuentes de respaldo de ejemplo `:ro`, volumen `kesto_data:/app/data`).
- **Configuración y secretos:** `KESTO_MASTER_KEY` obligatoria (añadir al compose, nunca en claro: `.env`/compose `secrets`); en modo OIDC, `KESTO_SECRET` + las tres `OIDC_*` «todo o nada». `RUST_LOG`, `BIND_ADDRESS`, `DATABASE_URL` se leen tal cual (sin prefijo); vacío ≠ defecto.
- **Entornos:** Podman en local, Docker en producción; bollard fija la versión de API en la llamada; `DOCKER_HOST` configurable.
- **Sin CORS / mismo origen:** el binario sirve la API y el panel.

## Entregables del change

- `Dockerfile`, `.dockerignore`, `compose.yaml`.
- Documentación de despliegue (variables, volúmenes, socket, modo `disabled` vs OIDC) en `docs/FEATURE-008-despliegue-produccion.md` y enlace desde el README.

## Fases y módulos

### Fase 0 — Preparación y contratos
- Confirmar versiones de las imágenes base, el layout (§7: `src/` + `ui/` en la raíz) y el contrato de variables de entorno (`docs/ENTORNO.md`).

### Fase 1 — Dockerfile y contexto de build
- `Dockerfile` multi-stage (backend + panel + runtime) y `.dockerignore`.
- Verificación: build reproducible y arranque en modo `disabled` solo con `KESTO_MASTER_KEY`.

### Fase 2 — Compose de producción
- `compose.yaml` según §8 con los cabos cerrados; comprobación de `up` con volúmenes y DB persistida.

### Fase 3 — Verificación y documentación
- Arrancar la imagen, comprobar `/api/v1/health` (`auth_mode: "disabled"`) y el panel servido.
- `docs/FEATURE-008-despliegue-produccion.md`.

## Riesgos y cabos

- El socket Docker montado `:ro` y la versión de API de bollard en producción.
- Secretos en el compose: no deben quedar en claro en el YAML ni en el historial.
- Rutas de respaldo del §8 son de ejemplo del host; no fijarlas como obligatorias.
- Tamaño y tiempo de build de la imagen (panel Antd).
- `BIND_ADDRESS`/`RUST_LOG` presentes pero vacíos son error de arranque.

---
Fin del contenido.
---