#!/usr/bin/env bash
# latest-tag-guard.sh <version> <tag...>
#
# Imprime "true" si <version> es la mayor de los tags por orden semver
# (sort -V), o "false" en caso contrario. Acepta con o sin prefijo "v".
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "uso: $(basename "$0") <version> <tag...>" >&2
  exit 2
fi

version="${1#v}"
shift

# Normaliza los tags y calcula el mayor con orden de versión.
greatest="$(printf '%s\n' "${version}" "$@" | sed 's/^v//' | sort -V | tail -n1)"

if [ "${version}" = "${greatest}" ]; then
  echo "true"
else
  echo "false"
fi
