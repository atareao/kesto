#!/usr/bin/env bash
# Pruebas estructurales del cableado de la vigilancia de dependencias (issue #167,
# SEC-11).
#
# Comprueban la FORMA de los cuatro ficheros que sostienen la vigilancia: `deny.toml`
# en la raíz, con sus cuatro familias y sus excepciones justificadas; el trabajo
# `Dependencies (cargo deny)` del CI —con `--workspace` y `--locked`, y con los pasos
# de clippy y de test cubriendo el workspace entero—; `.github/dependabot.yml`, con
# los tres ecosistemas del repositorio; y las recetas `deny` y `scripts-test` del
# `.justfile`. No necesitan red.
#
# Sigue el precedente de `scripts/release-image-workflow.test.sh`, que valida la
# estructura del flujo de publicación: los tres ficheros se leen COMO TEXTO y no se
# ejecuta nada ni se parsea el TOML/YAML de verdad.
#
# Lo que NO prueba, y conviene decirlo:
#
#   * que el trabajo llegue a ejecutarse —eso lo demuestra el CI—;
#   * que la allowlist de licencias esté completa —eso lo demuestra `cargo deny`
#     corriendo de verdad, en local con `just deny` y en el trabajo—.
#
# Lo que atrapa es el modo de fallo que motivó SEC-11: que alguien borre el cableado
# y la vigilancia desaparezca sin que nada lo delate.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/.." && pwd)"
deny="${root}/deny.toml"
ci="${root}/.github/workflows/ci.yml"
dependabot="${root}/.github/dependabot.yml"
justfile="${root}/.justfile"

fail=0
ok()  { echo "OK   $*"; }
bad() { echo "FAIL $*"; fail=1; }

# ── Ayudas de lectura ────────────────────────────────────────────────────────
# Todo lo de abajo es lectura de texto. Basta para saber si una clave está declarada
# DENTRO de la sección o de la entrada que le toca, que es lo único que estas pruebas
# afirman; no es un parser de TOML ni de YAML.

# El texto de una sección de primer nivel del TOML: desde su cabecera hasta la
# siguiente. Las subsecciones (`[licenses.private]`) la cierran, como en TOML.
seccion() {
  awk -v nombre="$2" '
    $0 ~ "^[[:space:]]*\\[" nombre "\\][[:space:]]*$" { dentro = 1; next }
    dentro && $0 ~ "^[[:space:]]*\\[" { dentro = 0 }
    dentro { print }
  ' "$1"
}

# El bloque de una entrada de `.github/dependabot.yml`: desde su
# `package-ecosystem:` hasta la entrada siguiente.
entrada() {
  awk -v ecosistema="$2" '
    /package-ecosystem:/ {
      valor = $0
      sub(/.*package-ecosystem:[[:space:]]*/, "", valor)
      sub(/[[:space:]]*#.*$/, "", valor)
      sub(/[[:space:]]+$/, "", valor)
      dentro = (valor == ecosistema)
      next
    }
    dentro { print }
  ' "$1"
}

# El `directory:` de una entrada, sin comillas ni blanco alrededor. El vacío
# significa que la entrada no lo declara.
directorio_de() {
  awk '
    /^[[:space:]]*directory[[:space:]]*:/ {
      valor = $0
      sub(/^[[:space:]]*directory[[:space:]]*:[[:space:]]*/, "", valor)
      sub(/[[:space:]]*#.*$/, "", valor)
      gsub(/"/, "", valor)
      sub(/[[:space:]]+$/, "", valor)
      print valor
      exit
    }
  ' <<<"$1"
}

# El cuerpo de una receta del `.justfile`: desde su cabecera —`nombre:` a columna
# cero— hasta la primera línea que no esté indentada, que es donde la receta
# termina.
receta() {
  awk -v nombre="$2" '
    $0 ~ "^" nombre ":" { dentro = 1; next }
    dentro && $0 !~ "^[[:space:]]" && $0 !~ "^[[:space:]]*$" { dentro = 0 }
    dentro { print }
  ' "$1"
}

# El contenido de la allowlist de licencias: lo que hay entre `allow = [` y el `]`
# que lo cierra.
allowlist() {
  awk '
    /^[[:space:]]*allow[[:space:]]*=/ { dentro = 1 }
    dentro { print }
    dentro && /\]/ { exit }
  ' <<<"$1"
}

# ¿El texto contiene el patrón (ERE)?
contiene() { grep -qE -- "$2" <<<"$1"; }

# ¿El fichero existe y contiene el texto literal?
tiene() { [ -f "$1" ] && grep -qF -- "$2" "$1"; }

# ¿El fichero existe y contiene el patrón (ERE)?
tiene_re() { [ -f "$1" ] && grep -qE -- "$2" "$1"; }

# Las secciones del `deny.toml`, leídas una sola vez y sin reventar si no existe.
advisories=""
licenses=""
bans=""
sources=""
if [ -f "${deny}" ]; then
  advisories="$(seccion "${deny}" advisories)"
  licenses="$(seccion "${deny}" licenses)"
  bans="$(seccion "${deny}" bans)"
  sources="$(seccion "${deny}" sources)"
fi

# ── A. `deny.toml` en la raíz del repositorio ────────────────────────────────

if [ -f "${deny}" ]; then
  ok "deny.toml existe en la raíz"
else
  bad "falta deny.toml en la raíz del repositorio: sin él no hay puerta de la cadena de suministro"
fi

# Las cuatro familias. Que falte una es una comprobación que no se hace.
for familia in advisories licenses bans sources; do
  if tiene_re "${deny}" "^[[:space:]]*\\[${familia}\\]"; then
    ok "deny.toml declara [${familia}]"
  else
    bad "deny.toml no declara la sección [${familia}]"
  fi
done

# La allowlist de licencias: declarada, no vacía y con las dos licencias que cubren
# la inmensa mayoría del árbol. Que esté COMPLETA no lo dice esto: lo dice
# `cargo deny check` corriendo.
allow="$(allowlist "${licenses}")"
if contiene "${allow}" '"[^"]+"' \
   && contiene "${allow}" '"MIT"' \
   && contiene "${allow}" '"Apache-2.0"'; then
  ok "deny.toml: la allowlist de [licenses] no está vacía y admite \"MIT\" y \"Apache-2.0\""
else
  bad "deny.toml: [licenses] necesita una allowlist no vacía que admita \"MIT\" y \"Apache-2.0\" (falta la lista, está vacía o le falta alguna de las dos)"
fi

# Cada excepción se declara con su motivo: es el único sitio donde un lector futuro
# distingue «no nos alcanza» de «lo silenciamos y ya nadie se acordó». La invariante
# es POR ENTRADA, no un recuento mínimo: CADA línea que declare un
# `id = "RUSTSEC-…"` tiene que llevar también su `reason =`. Un «al menos N» dejaría
# entradas sin motivo en cuanto la lista creciera —hoy hay siete—.
ids="$(grep -cE 'id[[:space:]]*=[[:space:]]*"RUSTSEC-' <<<"${advisories}" || true)"
sin_motivo="$(grep -E 'id[[:space:]]*=[[:space:]]*"RUSTSEC-' <<<"${advisories}" \
  | grep -vcE 'reason[[:space:]]*=' || true)"
if [ "${ids:-0}" -ge 1 ] && [ "${sin_motivo:-1}" -eq 0 ]; then
  ok "deny.toml: las ${ids} excepciones de [advisories] llevan su motivo ('reason =')"
else
  bad "deny.toml: de ${ids:-0} entradas con 'id = \"RUSTSEC-…\"' hay ${sin_motivo:-0} sin 'reason =': CADA excepción tiene que llevar su motivo"
fi

if contiene "${bans}" 'wildcards[[:space:]]*=[[:space:]]*"deny"'; then
  ok "deny.toml: [bans] deniega los comodines (wildcards = \"deny\")"
else
  bad "deny.toml: [bans] tiene que declarar 'wildcards = \"deny\"' (una versión '*' resuelve árboles distintos en cada máquina)"
fi

if contiene "${sources}" 'unknown-registry[[:space:]]*=[[:space:]]*"deny"' \
   && contiene "${sources}" 'unknown-git[[:space:]]*=[[:space:]]*"deny"'; then
  ok "deny.toml: [sources] deniega los registros y los repositorios desconocidos"
else
  bad "deny.toml: [sources] tiene que declarar 'unknown-registry = \"deny\"' y 'unknown-git = \"deny\"'"
fi

# ── B. El trabajo de `cargo deny` y la paridad del workspace en `ci.yml` ──────

if tiene "${ci}" 'name: Dependencies (cargo deny)'; then
  ok "ci.yml declara el trabajo 'Dependencies (cargo deny)'"
else
  bad "ci.yml no tiene ningún trabajo con 'name: Dependencies (cargo deny)'"
fi

# La puerta se corre sobre el workspace ENTERO y con el lockfile congelado.
#
# `--locked`: sin él, el trabajo puede resolver una versión que no es la del PR.
# `--workspace`: el manifiesto raíz es un PAQUETE, no un virtual, así que sin el
# flag el grafo arranca en `kesto` y `kesto-mcp` —y con él `rmcp`— queda fuera.
# En `cargo deny` los dos flags son GLOBALES, así que van ANTES del subcomando
# —`cargo deny --workspace --locked check`—: `cargo deny check --locked` sale con
# código 2. La aserción no fija el orden de los flags, pero exige los DOS en la
# MISMA línea que la invocación.
linea_deny=""
if [ -f "${ci}" ]; then
  linea_deny="$(grep -E '^[[:space:]]*run:.*cargo deny' "${ci}" || true)"
fi

if contiene "${linea_deny}" '--workspace' && contiene "${linea_deny}" '--locked'; then
  ok "ci.yml ejecuta 'cargo deny --workspace --locked check' (workspace entero y lockfile congelado)"
else
  bad "ci.yml: la línea de 'cargo deny … check' tiene que llevar '--workspace' (si no, 'kesto-mcp' y, con él, 'rmcp' quedan fuera del grafo) y '--locked' (si no, se juzga una versión que no es la del PR)"
fi

# `--workspace` en los dos pasos del backend: sin él, Cargo se queda en el paquete
# raíz y `kesto-mcp` no se mira nunca.
if tiene_re "${ci}" 'cargo clippy.*--workspace'; then
  ok "ci.yml: el paso de clippy cubre el workspace (cargo clippy --workspace)"
else
  bad "ci.yml: el paso de clippy no lleva '--workspace' (línea con 'cargo clippy --workspace'): 'kesto-mcp' se quedaría sin analizar"
fi

if tiene_re "${ci}" 'cargo test.*--locked.*--workspace'; then
  ok "ci.yml: el paso de test cubre el workspace (cargo test --locked --workspace)"
else
  bad "ci.yml: el paso de test no lleva '--workspace' (línea con 'cargo test --locked --workspace'): 'kesto-mcp' se quedaría sin probar"
fi

# ── C. `.github/dependabot.yml` ──────────────────────────────────────────────
# Los tres ecosistemas del repositorio. Una entrada de `cargo` en la raíz cubre el
# workspace entero —`kesto-mcp` incluido—, así que no hace falta una por crate.

if tiene_re "${dependabot}" '^version:[[:space:]]*2[[:space:]]*$'; then
  ok ".github/dependabot.yml declara 'version: 2'"
else
  bad ".github/dependabot.yml no declara 'version: 2' (o no existe)"
fi

# (ecosistema, `directory:` esperado). El directorio vacío significa «no aplica»:
# las acciones no viven en una carpeta del repositorio.
for esperada in "cargo:/" "github-actions:" "npm:/ui"; do
  ecosistema="${esperada%%:*}"
  directorio="${esperada#*:}"

  bloque=""
  [ -f "${dependabot}" ] && bloque="$(entrada "${dependabot}" "${ecosistema}")"

  if [ -z "${bloque}" ]; then
    bad ".github/dependabot.yml: falta la entrada 'package-ecosystem: ${ecosistema}'"
    continue
  fi

  if [ -n "${directorio}" ] && [ "$(directorio_de "${bloque}")" != "${directorio}" ]; then
    bad ".github/dependabot.yml: la entrada de '${ecosistema}' tiene que declarar 'directory: \"${directorio}\"'"
    continue
  fi

  if [ -n "${directorio}" ]; then
    ok ".github/dependabot.yml: entrada de '${ecosistema}' con directory \"${directorio}\""
  else
    ok ".github/dependabot.yml: entrada de '${ecosistema}'"
  fi

  if ! contiene "${bloque}" '^[[:space:]]*schedule:' \
     || ! contiene "${bloque}" '^[[:space:]]*interval:'; then
    bad ".github/dependabot.yml: la entrada de '${ecosistema}' necesita 'schedule:' con 'interval:' (sin periodicidad no hay PRs)"
  fi
done

if tiene_re "${dependabot}" '^[[:space:]]*open-pull-requests-limit:'; then
  ok ".github/dependabot.yml declara 'open-pull-requests-limit'"
else
  bad ".github/dependabot.yml no declara 'open-pull-requests-limit:' (sin tope, Dependabot abre PRs sin freno)"
fi

if tiene_re "${dependabot}" '^[[:space:]]*groups:'; then
  ok ".github/dependabot.yml agrupa las actualizaciones ('groups:')"
else
  bad ".github/dependabot.yml no declara ningún 'groups:' (las actualizaciones llegarían sueltas)"
fi

# ── D. La receta local de las pruebas de `scripts/` no enmascara el fallo ────
# `just scripts-test` corre la batería —esta prueba incluida— dentro de un bucle.
# Sin `set -e`, el código de salida de la receta es el del ÚLTIMO script, así que
# un script en rojo se enmascara según el orden alfabético. Una puerta que
# devuelve éxito con una prueba en rojo es un adorno.

cuerpo_scripts_test=""
if [ -f "${justfile}" ]; then
  cuerpo_scripts_test="$(receta "${justfile}" scripts-test)"
fi

if contiene "${cuerpo_scripts_test}" 'set[[:space:]]+-e' \
   || contiene "${cuerpo_scripts_test}" 'bash[[:space:]]+-e'; then
  ok ".justfile: la receta 'scripts-test' propaga el fallo ('set -e')"
else
  bad ".justfile: la receta 'scripts-test' no lleva 'set -e' (ni ejecuta cada script con 'bash -e'): sin eso su código de salida es el del último script y un rojo se enmascara según el orden alfabético"
fi

# ── E. La receta `deny` del `.justfile` cubre el workspace ───────────────────
# El espejo local de la puerta del CI. Si la receta no lleva `--workspace`, la
# comprobación que el desarrollador corre en local mira un grafo más pequeño que
# el del CI —el mismo defecto que la comprobación B vigila en el trabajo—, y su
# verde no dice nada sobre `kesto-mcp` ni sobre `rmcp`.

cuerpo_deny=""
if [ -f "${justfile}" ]; then
  cuerpo_deny="$(receta "${justfile}" deny)"
fi

if contiene "${cuerpo_deny}" 'cargo deny.*--workspace'; then
  ok ".justfile: la receta 'deny' cubre el workspace (cargo deny --workspace)"
else
  bad ".justfile: la receta 'deny' no lleva '--workspace' (línea con 'cargo deny --workspace'): sin él el grafo arranca en el paquete raíz y 'kesto-mcp' —y con él 'rmcp'— quedaría fuera"
fi

exit "${fail}"
