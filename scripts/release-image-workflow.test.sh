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
if grep -qE '^[[:space:]]*matrix:' "${wf}" \
   && grep -qE '^[[:space:]]*runner: ubuntu-26\.04$' "${wf}" \
   && grep -qE '^[[:space:]]*runner: ubuntu-26\.04-arm$' "${wf}"; then
  ok "matriz con runners nativos amd64 (ubuntu-26.04) y arm64 (ubuntu-26.04-arm)"
else
  bad "release.yml no declara la matriz {ubuntu-26.04, ubuntu-26.04-arm}"
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

exit "${fail}"
