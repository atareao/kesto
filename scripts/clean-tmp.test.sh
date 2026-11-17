#!/usr/bin/env bash
# Pruebas de scripts/clean-tmp.sh, con base inyectable (nunca toca /tmp real).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="${here}/clean-tmp.sh"

base="$(mktemp -d)"
trap 'rm -rf "${base}"' EXIT

vivo="$$"
heredado="$(cat /proc/sys/kernel/random/uuid 2>/dev/null || echo "resto-heredado")"

# Busca un pid que no exista (para no depender de un número concreto).
muerto=""
for candidato in 999999 999998 999997 999996 999995; do
  if [ ! -e "/proc/${candidato}" ]; then
    muerto="${candidato}"
    break
  fi
done
if [ -z "${muerto}" ]; then
  echo "FAIL no se encontró un pid muerto para la prueba"
  exit 1
fi

mkdir -p "${base}/kesto-prueba-${vivo}"
mkdir -p "${base}/kesto-prueba-${muerto}"
mkdir -p "${base}/kesto-prueba-${heredado}"
mkdir -p "${base}/no-kesto-prueba-x"
mkdir -p "${base}/otra-cosa"

"${script}" "${base}" >/dev/null

fail=0
check() {
  local descripcion="$1" ruta="$2" debe_existir="$3"
  if [ "${debe_existir}" = "si" ]; then
    if [ -d "${ruta}" ]; then
      echo "OK   ${descripcion} (existe)"
    else
      echo "FAIL ${descripcion}: debería existir y no existe -> ${ruta}"
      fail=1
    fi
  else
    if [ -e "${ruta}" ]; then
      echo "FAIL ${descripcion}: debería haberse borrado -> ${ruta}"
      fail=1
    else
      echo "OK   ${descripcion} (borrado)"
    fi
  fi
}

check "pid propio (proceso vivo)"            "${base}/kesto-prueba-${vivo}"       si
check "pid muerto (${muerto})"                "${base}/kesto-prueba-${muerto}"     no
check "resto heredado (${heredado})"          "${base}/kesto-prueba-${heredado}"   no
check "sin prefijo (no-kesto-prueba-x)"       "${base}/no-kesto-prueba-x"          si
check "ajeno (otra-cosa)"                     "${base}/otra-cosa"                  si

exit "${fail}"
