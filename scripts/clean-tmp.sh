#!/usr/bin/env bash
# clean-tmp.sh [base]
#
# Limpieza manual de los restos que dejan las pruebas: los directorios
# temporales cuyo nombre empieza por "kesto-prueba-" (<temp>/kesto-prueba-<pid>
# y los restos <uuid> heredados). Aplica las mismas reglas que el barrido del
# arnés (src/test_support.rs):
#   - sufijo que parsea como número -> si /proc/<pid> no existe, borra;
#     si existe (proceso vivo), lo conserva y lo dice.
#   - sufijo que no es número (restos <uuid> heredados) -> borra.
# Todo lo que no lleve el prefijo no se toca.
# Sin /proc no se barre nada: no hay forma de saber qué procesos viven.
#
# `base` es inyectable (por defecto ${TMPDIR:-/tmp}) para poder probarlo sin
# tocar el directorio temporal real.
set -euo pipefail

prefijo="kesto-prueba-"

uso() {
  echo "uso: $(basename "$0") [base]" >&2
  echo "  Borra los restos <base>/kesto-prueba-* de procesos muertos." >&2
  echo "  base por defecto: \${TMPDIR:-/tmp}" >&2
}

case "${1:-}" in
  -h | --help)
    uso
    exit 0
    ;;
esac

if [ "$#" -gt 1 ]; then
  uso
  exit 2
fi

base="${1:-${TMPDIR:-/tmp}}"

if [ ! -d "${base}" ]; then
  echo "no existe la base: ${base}"
  echo "resumen: 0 borrados, 0 conservados"
  exit 0
fi

# Sin /proc no hay liveness que consultar: dar por muerto todo pid borraría los
# temporales de procesos vivos. Es la misma regla que el barrido del arnés.
if [ ! -d "/proc" ]; then
  echo "no existe /proc: no se barre nada (no hay forma de saber qué procesos viven)"
  echo "resumen: 0 borrados, 0 conservados"
  exit 0
fi

borrados=0
conservados=0

for ruta in "${base}"/"${prefijo}"*; do
  # Sin coincidencias el glob queda literal: descártalo.
  [ -e "${ruta}" ] || continue

  nombre="${ruta##*/}"
  sufijo="${nombre#"${prefijo}"}"

  if [[ "${sufijo}" =~ ^[0-9]+$ ]]; then
    if [ -e "/proc/${sufijo}" ]; then
      echo "conserva (proceso vivo, pid ${sufijo}): ${ruta}"
      conservados=$((conservados + 1))
      continue
    fi
  fi

  if rm -rf -- "${ruta}"; then
    echo "borra: ${ruta}"
    borrados=$((borrados + 1))
  else
    echo "no se pudo borrar: ${ruta}" >&2
  fi
done

echo "resumen: ${borrados} borrados, ${conservados} conservados"
exit 0
