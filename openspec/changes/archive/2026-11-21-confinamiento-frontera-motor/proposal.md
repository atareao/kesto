# Proposal — confinamiento de la frontera «operador → motor»

Issues **#160 (SEC-04)**, **#161 (SEC-05)** y **#162 (SEC-06)**, hijos de la auditoría de seguridad **#156** (bloque B de la campaña).

## Why

Los tres hallazgos son la misma frontera mal dibujada. La API de targets valida **presencia** y **coherencia de campos** —`src/api/targets.rs:265-355`, un único punto compartido por el alta y la edición—, pero no **alcance**: tres campos que el operador escribe llegan al motor con la única guarda de su forma.

1. **SEC-04 (#160)** — `host_path` (targets `directory`) se acepta no-vacío (`src/api/targets.rs:273`) y el motor lo usa como ruta del **sistema de ficheros de Kesto**, sin canonicalizar ni confinar: la extracción lo lee en `src/engine/extraccion.rs:249-262` (`LocalDestination`, sin pasar por Docker) y la restauración lo usa en `src/engine/restauracion.rs:114-120` como destino de escritura y, con la bandera, de poda. Un `host_path: "/"` archiva a S3 todo lo que Kesto ve —incluida `/app/data/kesto.db`, con los tokens y los secretos— y un `host_path: "/app/data"` permite sobrescribirla. El confinamiento `/backup_sources` que documenta `docs/FEATURE-005-backup-engine.md:76` **solo aparece en los tests**.
2. **SEC-05 (#161)** — `db_path` (targets `sqlite`) solo se guarda contra el `-` inicial (`sin_ser_bandera`: `src/engine/extraccion.rs:406-419` y `src/engine/restauracion.rs:312-325`). La restauración ejecuta `rm -f <db_path> <db_path>-wal <db_path>-shm` y `sqlite3 <db_path>` (`src/engine/restauracion.rs:176-209`) **dentro del contenedor**, habitualmente como `root`: una cadena con `..` o con un enlace simbólico lee y **borra** ficheros arbitrarios del contenedor —y del host, si hay un montaje—.
3. **SEC-06 (#162)** — `container_name` no pasa por ninguna lista: el motor resuelve el nombre y crea el `exec` en **cualquier** contenedor (`src/engine/extraccion.rs:376-395`, `src/engine/restauracion.rs:283-302`, `src/docker/cliente.rs:143`), incluido el propio `kesto-app`. Combinado con SEC-05, un target `sqlite` puede apuntar al contenedor propio y subir `kesto.db` a S3.

Ninguno es un fallo de forma: los tres campos **están** bien formados y aun así alcanzan lo que no deben. La frontera tiene que ser de **alcance**, y hoy no existe ni en el borde HTTP ni en el motor.

## What Changes

### El directorio a copiar vive bajo una raíz declarada (SEC-04)

- Nueva variable opcional **`KESTO_BACKUP_ROOT`**: ruta **absoluta**, por defecto `/backup_sources` —que es donde el `compose` del §8 monta los directorios a respaldar—. Presente pero no absoluta SHALL ser error de arranque que nombra la variable.
- El alta y la edición de un target `directory` SHALL exigir que `host_path` sea absoluto y quede **bajo** la raíz, comparando **por componentes** —no por prefijo de cadena, para que `/backup_sources_x` no cuele—. Si no, `400` nombrando el campo y sin escribir fila. La comprobación del borde es **textual**: no exige que la ruta exista.
- El motor SHALL volver a comprobarlo **canonicalizando** la ruta —resolviendo enlaces simbólicos, también los de los directorios padre— y rechazando lo que escape de la raíz, en las dos direcciones. Es la defensa en profundidad: hay filas anteriores a la regla y la API no es el único camino al motor.

### El `.db` es una ruta canónica dentro de un contenedor permitido (SEC-05)

- `db_path` SHALL ser **absoluto** y SHALL **coincidir con su propia forma canónica** dentro del contenedor: el motor lo comprobará con `readlink -f` **antes** de tocar nada y rechazará la ejecución si difiere —hay un enlace simbólico o un `..` en la cadena—. La frontera efectiva es el contenedor permitido: no se inventa una raíz que no existe en un contenedor ajeno.
- El alta SHALL rechazar además la forma no absoluta, con `..` o con segmento vacío, para que el fallo se vea en el formulario y no en la primera copia.
- El `rm -f` de la restauración SHALL quedar acotado al fichero declarado y a sus hermanos `-wal` y `-shm`, con la ruta ya comprobada.
- El contenedor del target SHALL disponer de `readlink` además de `sqlite3`; su ausencia SHALL reportarse como el fallo de la ejecución, no silenciarse.

### Solo se hace `exec` en contenedores que se declaran fuentes de copia (SEC-06)

- El motor SHALL inspeccionar el contenedor del target **antes** de crear ningún `exec` y SHALL exigir la etiqueta **`io.kesto.backup-source=true`**. Sin ella, la ejecución SHALL fallar nombrando el contenedor, y NO SHALL crearse el `exec`.
- Es **fail-closed**: sin etiqueta no se ejecuta nada, y no hay configuración que olvidar. La declaración viaja con el contenedor —el operador etiqueta el suyo al crearlo—, y `kesto-app` queda denegado por sí solo, porque nadie lo etiqueta.
- La comprobación SHALL ir en el **único punto** por el que pasan todos los `exec` de bollard, no en cada camino del motor, para que no haya forma de saltársela.

### Arnés y documentación

- El arnés de pruebas y el e2e etiquetan sus contenedores de origen con la etiqueta de opt-in y apuntan `KESTO_BACKUP_ROOT` a su raíz temporal.
- `docs/DEFINITION.md` §8 (la variable nueva y la etiqueta), `docs/ENTORNO.md` (tabla de variables) y `docs/FEATURE-005-backup-engine.md` (el confinamiento deja de ser una promesa de los tests).

## Non-goals

- **No se añade separación de privilegio al motor** (ejecutar los `exec` con un usuario sin privilegios, un daemon aparte): es la otra mitad de la propuesta de SEC-06 y exige un diseño de despliegue propio. Se abre issue de seguimiento.
- **No se toca el esquema**: no se añaden `CHECK` ni restricciones a `backup_targets`. Las filas ya escritas no se migran; el motor las rechaza al usarlas.
- **No se cambia la forma del contrato HTTP** de los targets: siguen siendo los mismos campos del §3. Solo cambia **qué valores** se aceptan.
- **No se confina la raíz de `db_path`** con una variable propia: el contenedor permitido es la frontera (ver `design.md`).

## Impact

- **Capacidad nueva**: `confinamiento` (las tres reglas de alcance, en las dos direcciones).
- **Deltas**: `config` (la variable), `api-resources` (el borde HTTP), `extraction` y `restauracion` (la regla en el motor), `verificacion-e2e` (la frontera probada de punta a punta).
- **Cambios incompatibles**, documentados en `docs/ENTORNO.md`:
  - un target `directory` cuyo `host_path` esté fuera de la raíz deja de copiarse;
  - un target de base cuyo contenedor no lleve la etiqueta deja de copiarse y de restaurarse —hay que **recrear el contenedor** con la etiqueta—;
  - un target `sqlite` cuyo `db_path` no sea la forma canónica —enlace simbólico o `..` en la cadena— deja de copiarse; se declara la ruta canónica.
- **Sin dependencias nuevas.** `readlink` es la única herramienta añadida al requisito del contenedor, presente en busybox y en coreutils.
- Bump esperado: **minor** (`0.18.0`), con los commits `feat:`/`fix:` sin `!`.
