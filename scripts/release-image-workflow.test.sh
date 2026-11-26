#!/usr/bin/env bash
# Pruebas estructurales del pipeline de release de la imagen (issue #38).
#
# Comprueban que `.github/workflows/release.yml` construye cada arquitectura en
# un runner NATIVO, publica por digest, ensambla el manifest list, conserva el
# guard de `latest`, y que el `Dockerfile` cachea la compilación de Rust
# (capa de dependencias + sccache). No necesitan red ni Docker.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "${here}/.." && pwd)"
wf="${root}/.github/workflows/release.yml"
df="${root}/Dockerfile"

fail=0
ok()  { echo "OK   $*"; }
bad() { echo "FAIL $*"; fail=1; }

# 1. Matriz con runners nativos por arquitectura (nada de emulación).
#    amd64 es obligatorio. arm64 admite dos estados legítimos: ACTIVO (entrada
#    presente, runner nativo `ubuntu-26.04-arm`) o PAUSADO (issue #129: las tres
#    líneas siguen ahí comentadas y el workflow lleva el TODO). Cualquier otra
#    cosa —falta la entrada, o se construye arm64 en un runner x64— es fallo.
amd64_nativo=0
arm_nativo=0
arm_pausado=0
if grep -qE '^[[:space:]]*runner: ubuntu-26\.04$' "${wf}"; then
  amd64_nativo=1
fi
if grep -qE '^[[:space:]]*runner: ubuntu-26\.04-arm$' "${wf}"; then
  arm_nativo=1
elif grep -qE '^[[:space:]]*#[[:space:]]*runner: ubuntu-26\.04-arm$' "${wf}" \
     && grep -q 'TODO(#129)' "${wf}"; then
  arm_pausado=1
fi

if grep -qE '^[[:space:]]*matrix:' "${wf}" \
   && [ "${amd64_nativo}" = 1 ] \
   && { [ "${arm_nativo}" = 1 ] || [ "${arm_pausado}" = 1 ]; }; then
  if [ "${arm_pausado}" = 1 ]; then
    ok "matriz amd64 nativa (ubuntu-26.04) y arm64 PAUSADO con TODO(#129)"
  else
    ok "matriz con runners nativos amd64 (ubuntu-26.04) y arm64 (ubuntu-26.04-arm)"
  fi
else
  bad "release.yml no declara la matriz amd64 + arm64 (activo en ubuntu-26.04-arm o pausado con TODO(#129))"
fi

# 2. Cada arquitectura publica por digest.
if grep -q 'push-by-digest=true' "${wf}"; then
  ok "publicación por digest"
else
  bad "falta 'push-by-digest=true' en release.yml"
fi

# 3. Un job ensambla el manifest list.
if grep -q 'imagetools create' "${wf}"; then
  ok "ensamblado del manifest list (docker buildx imagetools create)"
else
  bad "falta 'docker buildx imagetools create' en release.yml"
fi

# 4. El guard de `latest` sigue presente.
if grep -q 'latest-tag-guard.sh' "${wf}"; then
  ok "guard de latest presente"
else
  bad "se perdió la llamada a scripts/latest-tag-guard.sh"
fi

# 5. sccache activado en el Dockerfile.
if grep -q 'RUSTC_WRAPPER=sccache' "${df}" && grep -q 'SCCACHE_GHA_ENABLED' "${df}"; then
  ok "sccache activado en el Dockerfile"
else
  bad "el Dockerfile no activa sccache (RUSTC_WRAPPER=sccache / SCCACHE_GHA_ENABLED)"
fi

# 6. Caché de dependencias Rust: build dummy + build real (dos 'cargo build').
builds="$(grep -c 'cargo build --release' "${df}" || true)"
if [ "${builds:-0}" -ge 2 ]; then
  ok "caché de dependencias: build dummy + build real (${builds} 'cargo build')"
else
  bad "el Dockerfile no separa la capa de dependencias (se esperaban >=2 'cargo build --release', hay ${builds:-0})"
fi

# 7. El Dockerfile invalida la huella de Cargo tras copiar el código real:
#    'COPY' de Docker preserva el mtime, y el 'src/main.rs' del checkout es más
#    antiguo que el dummy creado durante el build, así que sin adelantar el mtime
#    Cargo considera el crate fresco y no lo recompila (se publica 'fn main() {}').
if grep -qE 'find[[:space:]]+src[[:space:]]+-type[[:space:]]+f[[:space:]]+-exec[[:space:]]+touch' "${df}"; then
  ok "el Dockerfile adelanta el mtime del código real (touch antes del build real)"
else
  bad "el Dockerfile no invalida la huella de Cargo tras 'COPY src' (falta 'find src -type f -exec touch')"
fi

# 8. El smoke test verifica el contrato observable de la imagen: health 200 con
#    cuerpo vacío y `/api/v1/auth/session` -> 401 (modo OIDC; en Disabled sería
#    404). La comprobación antigua de `.data.auth_mode` rompió v0.18.0.
smoke_step="$(
  awk '
    /^[[:space:]]*- name: Smoke test the image[[:space:]]*$/ { inside=1; next }
    inside && /^[[:space:]]*- name:/ { inside=0 }
    inside { print }
  ' "${wf}"
)"

if printf '%s\n' "${smoke_step}" | grep -qF '.data.auth_mode'; then
  bad "el smoke test sigue comprobando '.data.auth_mode' (comprobación obsoleta que rompió v0.18.0)"
else
  ok "el smoke test no usa la comprobación obsoleta '.data.auth_mode'"
fi

if printf '%s\n' "${smoke_step}" | grep -qF 'jq -e' \
   && printf '%s\n' "${smoke_step}" | grep -qF '.status == 200' \
   && printf '%s\n' "${smoke_step}" | grep -qF '.data == {}'; then
  ok "el smoke test exige jq -e con '.status == 200' y '.data == {}'"
else
  bad "el smoke test no comprueba con jq -e que '.status == 200' y '.data == {}'"
fi

if printf '%s\n' "${smoke_step}" | grep -vE '^[[:space:]]*#' | grep -F "curl -sS -o /dev/null -w '%{http_code}'" | grep -qF 'http://127.0.0.1:8080/api/v1/auth/session' \
   && printf '%s\n' "${smoke_step}" | grep -vE '^[[:space:]]*#' | grep -qE '\[[[:space:]]*"\$\{session_code\}"[[:space:]]*!=[[:space:]]*"401"[[:space:]]*\]'; then
  ok "el smoke test exige 401 en /api/v1/auth/session (modo OIDC)"
else
  bad "el smoke test no verifica un 401 en /api/v1/auth/session"
fi

exit "${fail}"
