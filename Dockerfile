# syntax=docker/dockerfile:1
# ═══════════════════════════════════════════════════════════════════════════
# Kesto — imagen de producción multi-stage (SRS §7)
#
#   etapa 1 (ui):     compila el panel de `ui/` con pnpm  → ui/dist
#   etapa 2 (rust):   compila el binario de release       → target/release/kesto
#   etapa 3 (runtime): solo binario + ui/dist, sin root, sin toolchain
#
# Se construye con Podman y `BUILDAH_FORMAT=docker` (ya fijado en el
# `.justfile`): el formato OCI descarta el HEALTHCHECK en silencio.
# ═══════════════════════════════════════════════════════════════════════════

# ── Etapa 1: panel (React + Antd) ──────────────────────────────────────────
FROM node:24-bookworm-slim AS ui-builder

WORKDIR /app/ui

# pnpm 12 por corepack. Los manifiestos primero, para cachear la instalación.
COPY ui/package.json ui/pnpm-lock.yaml ui/pnpm-workspace.yaml ./
RUN corepack enable && corepack prepare pnpm@12 --activate

# Instalación reproducible: el lockfile manda.
RUN pnpm install --frozen-lockfile

# El resto del panel, y el build de producción (`tsc -b && vite build`).
COPY ui/ ./
RUN pnpm run build

# ── Etapa 2: binario de release de Rust ────────────────────────────────────
FROM rust:1.97-bookworm AS rust-builder

WORKDIR /app

# `migrations/` es imprescindible: `sqlx::migrate!()` (src/db/migrate.rs) lee
# el directorio en tiempo de compilación y lo incrusta en el binario.
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY migrations/ ./migrations/
RUN cargo build --release --locked

# ── Etapa 3: runtime mínimo ────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Solo lo que el HEALTHCHECK necesita: curl + CA raíz para TLS saliente.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# El panel se sirve desde `ui/dist` **relativo al CWD** (src/panel.rs), así que
# el proceso arranca en /app y el panel queda en /app/ui/dist.
WORKDIR /app

COPY --from=rust-builder /app/target/release/kesto /app/kesto
COPY --from=ui-builder /app/ui/dist /app/ui/dist

# Directorio de datos (SQLite) propiedad del usuario sin privilegios.
RUN mkdir -p /app/data && chown -R 10001:10001 /app

USER 10001:10001

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
    CMD curl -fsS http://127.0.0.1:8080/api/v1/health || exit 1

CMD ["/app/kesto"]
