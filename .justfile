# ═══════════════════════════════════════════════════════════════════════════
# Kesto — recetas de desarrollo
# ═══════════════════════════════════════════════════════════════════════════
#
# `just --list` es el punto de entrada. Lo que hay que saber:
#
#   * Cada LÍNEA de una receta corre en su PROPIO shell: encadena con `&&` o usa
#     una receta con shebang (`#!/usr/bin/env bash`) si necesitas varias.
#   * `just check` es exactamente lo que corre el CI. Si pasa en local, pasa en
#     el PR, porque el workflow ejecuta los mismos tres comandos.
#   * Las recetas del panel abortan con un mensaje claro mientras no exista
#     `ui/package.json`; las de imagen, mientras no exista `Dockerfile`.
#   * Podman-first: en esta máquina no hay `docker` ni `/var/run/docker.sock`.
#     Ver AGENTS.md.

registry := "ghcr.io"
user := "atareao"
name := "kesto"
version := `vampus show`
image_ref := registry + "/" + user + "/" + name
port := "8080"

# Base de datos local. El contenedor usa sqlite:///app/data/kesto.db (SRS §8).
database_url := env_var_or_default("DATABASE_URL", "sqlite://data/kesto.db")
db_file := env_var_or_default("KESTO_DB_FILE", "data/kesto.db")

# Socket del daemon de podman, donde bollard habla en desarrollo. En producción
# es /var/run/docker.sock (SRS §8).
podman_socket := `echo "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock"`

# Podman construye en formato OCI por defecto, que NO admite la instrucción
# HEALTHCHECK del Dockerfile y la descarta en silencio. Forzar el formato
# `docker` conserva la sonda de salud en la imagen construida en local.
export BUILDAH_FORMAT := 'docker'

# ── Punto de entrada ────────────────────────────────────────────────────────

# Sin argumentos: lista las recetas disponibles
default:
    @just --list

# ── Calidad (paridad exacta con el CI) ─────────────────────────────────────

# Los tres checks del CI, en su orden. Ejecútalo antes de cada commit
check: fmt clippy test

# Los tres del backend más los cuatro del panel
check-all: check ui-check

# Formato: solo comprueba
fmt:
    cargo fmt --all -- --check

# Formato: aplica los cambios. Si toca algo, commit aparte (🎨 style: apply formatting)
fmt-fix:
    cargo fmt --all

# Clippy con cero warnings, con los mismos flags que el CI
clippy:
    cargo clippy --all-targets -- -D warnings

# Tests unitarios (#[cfg(test)]) y de integración (tests/)
test:
    cargo test --locked

# Rustdoc: falla con cualquier warning, enlaces rotos incluidos
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# Comprueba el formato del propio .justfile
justfile-check:
    @just --fmt --check

# Aplica formato al .justfile
justfile-fmt:
    @just --fmt

# ── Panel (React + Antd, en ui/) ───────────────────────────────────────────
# Todavía no existe: estas recetas abortan solas hasta que aparezca ui/.

# (privada) Aborta si el panel aún no está creado
_require-ui:
    @test -f ui/package.json || (echo "❌ ui/package.json no existe todavía: el panel React aún no está creado (SRS §6-§7)." && exit 1)

# Instala las dependencias del panel
ui-install: _require-ui
    cd ui && pnpm install

# Servidor de desarrollo del panel, con recarga en caliente
ui-dev: _require-ui
    cd ui && pnpm run dev

# Type-check del panel
ui-typecheck: _require-ui
    cd ui && npx tsc --noEmit

# Lint del panel
ui-lint: _require-ui
    cd ui && pnpm run lint

# Tests del panel
ui-test: _require-ui
    cd ui && npx vitest run

# Build de producción del panel (genera ui/dist)
ui-build: _require-ui
    cd ui && pnpm run build

# Los cuatro pasos del CI para el panel
ui-check: _require-ui ui-typecheck ui-lint ui-test ui-build

# ── Desarrollo ─────────────────────────────────────────────────────────────

# Levanta el backend en local. El panel va aparte, en otra terminal: `just ui-dev`
dev:
    RUST_LOG=debug cargo run

# ── Imagen y contenedor (podman-first) ─────────────────────────────────────
# El Dockerfile todavía no existe; estas recetas abortan solas mientras falte.

# (privada) Aborta si no hay Dockerfile
_require-image:
    @test -f Dockerfile || (echo "❌ Todavía no hay Dockerfile. Orden de etapas del SRS §7: UI → Rust → runtime, sin root." && exit 1)

# Construye la imagen local con los tags de versión y latest
build: _require-image
    podman build --format docker --tag {{ image_ref }}:{{ version }} --tag {{ image_ref }}:latest .

# Construye la imagen local sin caché (útil para cazar capas mentirosas)
build-nocache: _require-image
    podman build --format docker --no-cache --tag {{ image_ref }}:{{ version }} .

# `--userns=keep-id:uid=10001,gid=10001` mapea el uid del contenedor al tuyo en
# el host: sin eso el proceso no puede leer el socket, porque el socket es tuyo
# (rootless) y dentro del contenedor correrías como otro uid.
#
# Monta el socket en /var/run/docker.sock y un volumen para /app/data.
#
# Ejecuta la imagen en un contenedor de prueba.
run: _require-image
    podman run --rm -d --name {{ name }} \
        -p {{ port }}:8080 \
        --userns=keep-id:uid=10001,gid=10001 \
        -v {{ podman_socket }}:/var/run/docker.sock \
        -v kesto_data:/app/data \
        -e RUST_LOG=debug \
        -e DATABASE_URL='sqlite:///app/data/kesto.db' \
        -e BIND_ADDRESS='0.0.0.0:8080' \
        {{ image_ref }}:latest
    @echo "→ http://127.0.0.1:{{ port }}"

# Sigue los logs del contenedor de prueba
logs:
    podman logs -f {{ name }}

# Para y borra el contenedor de prueba
stop:
    -podman rm -f {{ name }}

# Empuja la imagen local a GHCR (en las releases lo hace el CI, esto es manual)
push: _require-image
    podman push {{ image_ref }}:{{ version }}
    podman push {{ image_ref }}:latest

# ── Despliegue ─────────────────────────────────────────────────────────────

# (privada) Aborta si no hay fichero compose
_require-compose:
    @test -f compose.yml -o -f docker-compose.yml -o -f compose.yaml || (echo "❌ No hay compose.yml. El despliegue de referencia está en el SRS §8." && exit 1)

# Despliega una imagen ya publicada de GHCR sin compilar nada
deploy tag="latest": _require-compose
    podman pull {{ image_ref }}:{{ tag }}
    podman compose up -d --force-recreate
    @echo "✅ Desplegado {{ image_ref }}:{{ tag }}"

# Comprueba que el servicio responde en el puerto publicado
health:
    #!/usr/bin/env bash
    set -uo pipefail
    url="http://127.0.0.1:{{ port }}/"
    for i in $(seq 1 30); do
        code="$(curl -s -o /dev/null -w '%{http_code}' "${url}" || true)"
        if [ "${code}" != "000" ]; then
            echo "✅ Responde ${code} tras ${i} intento(s)."
            exit 0
        fi
        sleep 2
    done
    echo "❌ Sin respuesta HTTP en ${url} tras 60s."
    exit 1

# ── Gitflow (SIEMPRE: nada directo en main ni en development) ──────────────

# Crea una rama feature desde development
gf-feature feature_name:
    git checkout development
    git pull --ff-only
    git checkout -b feature/{{ feature_name }}
    @echo "✅ feature/{{ feature_name }} creada desde development"

# Crea una rama hotfix desde main
gf-hotfix desc:
    git checkout main
    git pull --ff-only
    git checkout -b hotfix/{{ desc }}
    @echo "✅ hotfix/{{ desc }} creada desde main"

# Trae development a la rama actual
gf-sync:
    git fetch origin
    git merge origin/development

# Empuja la rama actual y abre el PR (a development; a main si es hotfix/*)
gf-pr:
    #!/usr/bin/env bash
    set -euo pipefail
    branch="$(git rev-parse --abbrev-ref HEAD)"
    case "${branch}" in
        feature/*|chore/*) base="development" ;;
        hotfix/*)          base="main" ;;
        *) echo "❌ Estás en '${branch}'. gf-pr solo actúa en feature/*, chore/* o hotfix/*."; exit 1 ;;
    esac
    git push -u origin "${branch}"
    gh pr create --base "${base}" --head "${branch}" --fill

# Fusiona en su base el PR de la rama actual, borra la rama y deja development al día
gf-merge:
    #!/usr/bin/env bash
    set -euo pipefail
    branch="$(git rev-parse --abbrev-ref HEAD)"
    case "${branch}" in
        feature/*|chore/*) base="development" ;;
        hotfix/*)          base="main" ;;
        *) echo "❌ Estás en '${branch}'. Solo se fusionan PRs de feature/*, chore/* o hotfix/*."; exit 1 ;;
    esac
    # Fusión condicionada al CI: `gh pr checks` sale distinto de cero si algún
    # check falla o sigue pendiente.
    gh pr checks >/dev/null 2>&1 || { echo "❌ El PR no tiene todos los checks en verde:"; gh pr checks; exit 1; }
    gh pr merge --merge --delete-branch
    git checkout "${base}"
    git pull --ff-only
    # Un hotfix entra en main y hay que devolverlo a la integración.
    if [ "${base}" = "main" ]; then
        git checkout development
        git pull --ff-only
        git merge origin/main --no-edit
        git push origin development
    fi
    echo "✅ PR de ${branch} fusionado en ${base}."

# No etiquetes a mano: mergear este PR dispara la release automática (bump + tag
# + imagen a GHCR + crate a crates.io).
#
# Abre el PR de release development → main.
gf-release:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! gh secret list -R {{ user }}/{{ name }} | grep -q '^GH_PAT'; then
        echo "❌ Falta el secreto GH_PAT en {{ user }}/{{ name }}."
        echo "   Sin él, el tag que empuja release-prepare no dispara release.yml"
        echo "   (un tag empujado con el GITHUB_TOKEN por defecto no dispara workflows)."
        echo "   Créalo con: gh secret set GH_PAT -R {{ user }}/{{ name }}"
        exit 1
    fi
    gh pr create --base main --head development \
        --title "release: $(vampus show)" \
        --body "Release automática: al mergear, release-prepare sube la versión, genera el CHANGELOG, etiqueta v$(vampus show) y release.yml publica imagen y crate."

# Árbol de ramas
gf-graph:
    git log --oneline --graph --all --decorate=short -30

# ── Versión y release ──────────────────────────────────────────────────────

# Muestra la versión actual (la fuente es .vampus.yml)
version:
    @vampus show

# La versión vive en TRES sitios que deben ir sincronizados: Cargo.toml, la
# entrada [[package]] de Cargo.lock y .vampus.yml:current_version.
#
# Normalmente no hace falta: release-prepare lo hace solo en cada release.
#
# Sube la versión y resincroniza Cargo.lock.
bump type="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    before="$(vampus show)"
    vampus upgrade --{{ type }}
    cargo update --workspace
    after="$(vampus show)"
    if [ "${before}" = "${after}" ]; then
        echo "❌ vampus no cambió la versión (revisa los 'replaces' de .vampus.yml)."
        exit 1
    fi
    echo "✅ ${before} → ${after}"
    echo "   Commitea los tres ficheros: Cargo.toml, Cargo.lock y .vampus.yml"

# Vista previa del CHANGELOG sin escribir nada
changelog:
    @git-cliff --unreleased --strip header

# ── Pipeline (GitHub Actions) ──────────────────────────────────────────────

# Últimos runs del CI
pipeline:
    @gh run list -R {{ user }}/{{ name }} --limit 8

# Espera al run de la rama actual y falla si no acaba en verde
pipeline-watch:
    #!/usr/bin/env bash
    set -euo pipefail
    branch="$(git rev-parse --abbrev-ref HEAD)"
    id="$(gh run list -R {{ user }}/{{ name }} --branch "${branch}" --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
    if [ -z "${id}" ]; then
        echo "❌ No hay ningún run para la rama ${branch}."
        exit 1
    fi
    gh run watch "${id}" -R {{ user }}/{{ name }} --exit-status

# Lista los secretos del repo (los releases necesitan GH_PAT y CARGO_REGISTRY_TOKEN)
secrets:
    @gh secret list -R {{ user }}/{{ name }} || true

# ── OpenSpec (protocolo SDD) ───────────────────────────────────────────────

# En este repo NO se escribe código ni tests antes de tener un change proposal
# aprobado en openspec/changes/.
#
# Puerta del protocolo: falla si no hay ningún change proposal activo.
check-spec:
    @ls openspec/changes/*/proposal.md >/dev/null 2>&1 || (echo "❌ No hay ningún change proposal activo en openspec/changes/." && echo "   Crea uno con: just spec-new <feature>" && echo "   Y después rellena openspec/changes/<feature>/{proposal,tasks}.md y specs/<capability>/spec.md" && exit 1)
    @echo "✅ Hay change proposal activo."

# `openspec init` configura los ficheros de instrucciones de la herramienta que
# le pases, y `--tools opencode` escribe en AGENTS.md, que aquí es un documento
# vivo del repo. Con `--tools none` solo crea la estructura openspec/.
#
# Inicializa OpenSpec (una sola vez), exigiendo el árbol limpio a propósito.
spec-init:
    @test -z "$(git status --porcelain)" || (echo "❌ Hay cambios sin commitear. Commitea antes: openspec init puede reescribir ficheros." && exit 1)
    openspec init --tools none --language es

# Crea el directorio de un change proposal nuevo
spec-new change_name:
    openspec new change {{ change_name }}

# Lista los change proposals activos
spec-list:
    @openspec list

# Valida un change proposal en modo estricto
spec-validate change_name:
    openspec validate {{ change_name }} --strict

# Estado de los artefactos de un change (qué falta por escribir)
spec-status change_name:
    openspec status --change {{ change_name }}

# Archiva un change completado y funde su delta en openspec/specs/
spec-archive change_name:
    openspec archive --yes {{ change_name }}

# ── Planes y documentación (convención fija del repo) ─────────────────────
# Un plan por trabajo en plans/PLAN-NNN.md; al cerrarlo se RENOMBRA (nunca se
# borra) a -COMPLETED o -PENDING. Un documento por feature en
# docs/FEATURE-NNN-<slug>.md. Ver AGENTS.md.

# Lista los planes con su estado
plans:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    files=(plans/PLAN-*.md)
    if [ ${#files[@]} -eq 0 ]; then
        echo "No hay planes todavía. La ruta del primero la da: just plan-new"
        exit 0
    fi
    printf '%-30s %s\n' "FICHERO" "ESTADO"
    for f in "${files[@]}"; do
        base="$(basename "${f}" .md)"
        case "${base}" in
            *-COMPLETED) state="terminado" ;;
            *-PENDING)   state="aparcado" ;;
            *)           state="en curso" ;;
        esac
        printf '%-30s %s\n' "$(basename "${f}")" "${state}"
    done

# Imprime la ruta del siguiente plan (PLAN-NNN, contador sin huecos)
plan-new:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    next=1
    for f in plans/PLAN-*.md; do
        n="${f#plans/PLAN-}"
        n="${n%%-*}"
        n="${n%.md}"
        [[ "${n}" =~ ^[0-9]+$ ]] || continue
        if (( 10#${n} >= next )); then
            next=$(( 10#${n} + 1 ))
        fi
    done
    printf 'plans/PLAN-%03d.md\n' "${next}"

# Marca un plan como terminado (renombra con git mv, nunca borra)
plan-done num:
    #!/usr/bin/env bash
    set -euo pipefail
    # 10# evita que bash lea 010 como octal.
    n="$(printf '%03d' "$(( 10#{{ num }} ))")"
    file="plans/PLAN-${n}.md"
    test -f "${file}" || { echo "❌ No existe ${file}"; exit 1; }
    # git mv solo funciona si el plan ya está trackeado; si no, mv a secas.
    if git ls-files --error-unmatch "${file}" >/dev/null 2>&1; then
        git mv "${file}" "plans/PLAN-${n}-COMPLETED.md"
    else
        mv "${file}" "plans/PLAN-${n}-COMPLETED.md"
    fi
    echo "✅ plans/PLAN-${n}-COMPLETED.md"

# Marca un plan como aparcado (renombra con git mv, nunca borra)
plan-pending num:
    #!/usr/bin/env bash
    set -euo pipefail
    # 10# evita que bash lea 010 como octal.
    n="$(printf '%03d' "$(( 10#{{ num }} ))")"
    file="plans/PLAN-${n}.md"
    test -f "${file}" || { echo "❌ No existe ${file}"; exit 1; }
    # git mv solo funciona si el plan ya está trackeado; si no, mv a secas.
    if git ls-files --error-unmatch "${file}" >/dev/null 2>&1; then
        git mv "${file}" "plans/PLAN-${n}-PENDING.md"
    else
        mv "${file}" "plans/PLAN-${n}-PENDING.md"
    fi
    echo "✅ plans/PLAN-${n}-PENDING.md"

# Crea el stub del documento de la siguiente feature entregada
feature-new slug:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    next=1
    for f in docs/FEATURE-*.md; do
        n="${f#docs/FEATURE-}"
        n="${n%%-*}"
        [[ "${n}" =~ ^[0-9]+$ ]] || continue
        if (( 10#${n} >= next )); then
            next=$(( 10#${n} + 1 ))
        fi
    done
    path="$(printf 'docs/FEATURE-%03d-%s.md' "${next}" "{{ slug }}")"
    # `test -e && { ...; exit 1; }` abortaría en silencio con set -e justo en el
    # caso normal (el fichero NO existe), así que va con if.
    if [ -e "${path}" ]; then
        echo "❌ ${path} ya existe."
        exit 1
    fi
    # Sin heredoc: en just una línea sin indentar volvería a nivel superior y
    # rompería la receta, y el terminador de un heredoc no puede indentarse con
    # espacios. De ahí el printf.
    {
        printf '# FEATURE-%03d — %s\n\n' "${next}" "{{ slug }}"
        printf '## Qué entrega\n\n'
        printf '## Contratos (tipos, endpoints, esquema)\n\n'
        printf '## Decisiones\n\n'
        printf '## Deuda pendiente\n'
    } > "${path}"
    echo "✅ ${path}"
    echo "   Plan asociado: $(just plan-new)"

# ── Base de datos (SQLite + SQLx) ─────────────────────────────────────────

# Crea la base de datos local y aplica las migraciones
db-setup:
    sqlx database create --database-url {{ database_url }}
    sqlx migrate run --database-url {{ database_url }}

# Añade una migración reversible vacía (up + down)
migrate-add migration_name:
    sqlx migrate add -r {{ migration_name }}

# Abre un shell sqlite3 sobre la base de datos local
db-shell:
    sqlite3 {{ db_file }}

# Borra la base de datos local (irrecuperable). Recréala con: just db-setup
db-reset:
    rm -f {{ db_file }} {{ db_file }}-wal {{ db_file }}-shm
    @echo "✅ Base de datos local borrada. Recréala con: just db-setup"

# ── Limpieza ───────────────────────────────────────────────────────────────

# Limpia los artefactos de compilación de Rust
clean:
    cargo clean

# Limpia además los del panel y las imágenes locales de Kesto
distclean: clean
    rm -rf ui/dist ui/node_modules
    -podman image rm {{ image_ref }}:latest {{ image_ref }}:{{ version }}
