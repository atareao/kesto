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

# sccache: cachea la compilación en la caché de GitHub Actions. El binario se
# instala desde el release oficial, precompilado por arquitectura.
ARG SCCACHE_VERSION=0.19.0
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && arch="$(uname -m)" \
    && curl -fsSL "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-${arch}-unknown-linux-musl.tar.gz" \
       | tar -xz -C /usr/local/bin --strip-components=1 \
         "sccache-v${SCCACHE_VERSION}-${arch}-unknown-linux-musl/sccache"

# sccache envuelve a rustc. El backend GHA solo se enciende si llegan las
# credenciales de la caché de Actions (secretos de BuildKit); en local queda
# apagado y el build no se rompe.
ENV RUSTC_WRAPPER=sccache \
    CARGO_INCREMENTAL=0

# Caché de dependencias: se compilan las dependencias contra un `main.rs` vacío,
# de modo que esta capa se reutiliza mientras Cargo.toml/Cargo.lock no cambien.
COPY Cargo.toml Cargo.lock ./
RUN --mount=type=secret,id=actions_cache_url \
    --mount=type=secret,id=actions_runtime_token \
    set -eux; \
    export ACTIONS_CACHE_URL="$(cat /run/secrets/actions_cache_url 2>/dev/null || true)"; \
    export ACTIONS_RUNTIME_TOKEN="$(cat /run/secrets/actions_runtime_token 2>/dev/null || true)"; \
    export SCCACHE_GHA_ENABLED="$([ -n "${ACTIONS_CACHE_URL}" ] && echo true || echo false)"; \
    mkdir -p src; \
    echo 'fn main() {}' > src/main.rs; \
    cargo build --release --locked; \
    rm -rf src

# `migrations/` es imprescindible: `sqlx::migrate!()` (src/db/migrate.rs) lee
# el directorio en tiempo de compilación y lo incrusta en el binario.
COPY src/ ./src/
COPY migrations/ ./migrations/
RUN --mount=type=secret,id=actions_cache_url \
    --mount=type=secret,id=actions_runtime_token \
    set -eux; \
    export ACTIONS_CACHE_URL="$(cat /run/secrets/actions_cache_url 2>/dev/null || true)"; \
    export ACTIONS_RUNTIME_TOKEN="$(cat /run/secrets/actions_runtime_token 2>/dev/null || true)"; \
    export SCCACHE_GHA_ENABLED="$([ -n "${ACTIONS_CACHE_URL}" ] && echo true || echo false)"; \
    cargo build --release --locked

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
