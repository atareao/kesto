# Manifest de cambios pre-backup para deduplicación segura de volcados

## Contexto

Kesto archiva los volcados de base de datos con `rustic_core` leyendo el `stdout` de los
clientes (`pg_dump`, `mysqldump`, …) como un stream en memoria, sin ficheros temporales.
Ese flujo no aporta metadatos reales: el contenido llega con tamaño `0`, sin `mtime` ni
`inode`, así que `rustic_core` no puede distinguir dos volcados por su identidad. Como
consecuencia, dos volcados con contenido distinto pero «misma forma» a ojos del archivador
se consideran iguales y la segunda snapshot apunta a los chunks de la primera: se pierden
datos de forma silenciosa.

Para evitar esa corrupción, `archivar_volcado` fuerza hoy `parent_opts.force(true)` y
desactiva de facto la deduplicación: cada snapshot escribe todos los bytes de nuevo. Es
seguro, pero desperdicia espacio y ancho de banda, y no hay forma de saber si un volcado
cambió realmente respecto al anterior.

## Objetivo

Deduplicar de forma **segura** los volcados de base de datos. Antes de archivar se detecta
si el contenido de la BD ha cambiado:

- **Sin cambios** → no se suben bytes y se crea una snapshot «sin cambios» que reutiliza el
  contenido del último volcado real.
- **Con cambios** → se sube el volcado completo y se etiqueta con un *manifest de cambios*
  que identifica de forma única ese contenido.

## Alcance

### Detección de cambios (Fase 1 — PostgreSQL)

- `pg_dump --schema-only` para capturar la estructura (tablas, columnas, índices,
  restricciones).
- Checksum de los datos por tabla para detectar cambios de contenido sin volcar filas.
- Combinación de esquema + checksums de datos en un hash estable: el **manifest**.

### Detección de cambios (Fase 2 — MySQL / MariaDB)

- `mysqldump --no-data` para capturar la estructura.
- `CHECKSUM TABLE` para el checksum de datos por tabla.
- Misma composición de hash → manifest.

### Almacenamiento y etiquetado

- La snapshot archiva el volcado con la etiqueta `kesto.change_manifest=<hash>`.
- Snapshots con el mismo manifest se consideran **duplicados lógicos** del mismo contenido.

### Retención

- La retención trata las snapshots que comparten `kesto.change_manifest` como duplicados
  lógicos: no se libera tamaño al retirar una mientras queden otras con ese manifest
  (comparten los mismos chunks), y solo al retirar la última se liberan los chunks.

### Panel

- La vista de snapshots muestra una columna **«Cambios»** que refleja el manifest de
  cambios (por ejemplo, si hubo cambios respecto al volcado anterior y el identificador
  abreviado del manifest).

## No alcance

- **No** se modifica `rustic_core` ni se parchea su comportamiento interno.
- **No** se altera el algoritmo de chunking ni el formato de snapshot de rustic.
- **No** se cambia el formato del evento SSE ni el esquema de mensajes existente.
- **No** se escriben volcados a disco ni a `/tmp`: el streaming en memoria se mantiene.

## Impacto

| Fichero | Cambio |
|---|---|
| `src/engine/extraccion.rs` | Nuevo trait `ChangeDetector` (detección de manifest por motor) e implementaciones PostgreSQL (Fase 1) y MySQL/MariaDB (Fase 2). |
| `src/rustic/repositorio.rs` | `archivar_volcado` acepta un `change_manifest` opcional y etiqueta la snapshot con `kesto.change_manifest=<hash>`; el `force(true)` pasa a depender de la detección. |
| `src/rustic/retencion.rs` | Lógica de duplicados lógicos: agrupa snapshots por manifest y ajusta el cálculo de espacio liberado. |
| `ui/` | Columna «Cambios» en la vista de snapshots. |

## Riesgos y decisiones

- **Compatibilidad hacia atrás**: las snapshots sin `kesto.change_manifest` se tratan como
  contenido único (comportamiento actual); la retención no las agrupa.
- **Coste de la detección**: `--schema-only` / `--no-data` y `CHECKSUM TABLE` son más
  baratos que un volcado completo, pero añaden una consulta previa por backup. Se acepta el
  coste a cambio de dedup segura.
- **Fallo de detección**: si el manifest no se puede calcular, se degrada al comportamiento
  actual (`force(true)`, snapshot completa sin etiqueta) en lugar de abortar el backup.
