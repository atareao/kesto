#!/usr/bin/env bash
# Pruebas de scripts/latest-tag-guard.sh.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
guard="${here}/latest-tag-guard.sh"

fail=0
check() {
  local expected="$1"
  shift
  local actual
  actual="$("${guard}" "$@")"
  if [ "${actual}" = "${expected}" ]; then
    echo "OK   ($*) -> ${actual}"
  else
    echo "FAIL ($*) -> esperado ${expected}, obtenido ${actual}"
    fail=1
  fi
}

check true  0.4.0 0.3.2 0.3.3 0.4.0 0.3.1
check false 0.3.3 0.3.2 0.3.3 0.4.0
check true  0.1.0 0.1.0
check false 0.9.0 0.10.0 0.9.0

exit "${fail}"
