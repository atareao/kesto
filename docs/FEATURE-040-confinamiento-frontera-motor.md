# FEATURE-040 — Confinamiento de la frontera «operador → motor» (#160, #161, #162)

- **Issues:** #160 (SEC-04), #161 (SEC-05) y #162 (SEC-06), hijos de la auditoría de seguridad **#156** (bloque B de la campaña). **Change OpenSpec:** `confinamiento-frontera-motor`. **Plan:** `plans/PLAN-040.md`.
- **Tipo:** producto (Rust + arnés/e2e + documentación). **No** toca el panel, ni el esquema, ni la forma del contrato HTTP de los targets.
- **Incompatible hacia atrás en tres puntos**, documentados abajo y en `docs/ENTORNO.md`.

## Qué es

Tres campos que el operador escribe en un target llegan al motor con la única guarda de su **forma**. Los tres están bien formados y aun así alcanzan lo que no deben, porque la frontera que faltaba no era de forma sino de **alcance**:

1. **`host_path`** (targets `directory`) se lee y se escribe en el **sistema de ficheros de Kesto**, sin confinar. Un `host_path: "/"` archiva a S3 todo lo que Kesto ve —incluida `/app/data/kesto.db`, con los tokens y los secretos cifrados—, y un `host_path: "/app/data"` permite sobrescribirla al restaurar.
2. **`db_path`** (targets `sqlite`) se ejecuta y se **borra** dentro del contenedor del target —habitualmente como `root`— con la sola guarda de no empezar por `-`: una cadena con `..` o con un enlace simbólico lee y borra ficheros ajenos del contenedor —y del host, si hay un montaje—.
3. **`container_name`** no pasa por ninguna lista: el motor crea el `exec` en **cualquier** contenedor en marcha, incluido el propio `kesto-app`. Combinado con lo anterior, un target `sqlite` puede apuntar al contenedor propio y subir `kesto.db` a S3.

La frontera se dibuja ahora en **dos capas** —el borde HTTP comprueba la forma; el motor canonicaliza— y se gobierna con una variable nueva, `KESTO_BACKUP_ROOT`, y una etiqueta de contenedor, `io.kesto.backup-source=true`.

## Ficheros implicados

- `src/engine/frontera.rs` — **módulo nuevo**, puro (no habla con Docker, ni con la red, ni con la base). Contiene la única definición de «qué es una ruta aceptable»: `forma_de_host_path` y `forma_de_db_path` (comprobación **textual**, sin tocar el disco), `confinar_host_path` (canonicalización del motor) y `coincide_con_su_forma_canonica` (la comparación exacta de `db_path`), más el enum `Campo` (`host_path` / `db_path`) y `FronteraError`. La comparación se hace con `Path::starts_with` (**por componentes**) y el `Display` de cada error nombra el campo y **nunca** una ruta.
- `src/config.rs` — `ENV_KESTO_BACKUP_ROOT`, `DEFAULT_BACKUP_ROOT = "/backup_sources"` y la variante `ConfigError::InvalidBackupRoot`. La raíz se guarda **tal como se declaró** (textual, sin canonicalizar) y `Config` la expone al motor.
- `src/api/targets.rs` — `exigir_host_path_bajo_la_raiz` y `exigir_db_path_sin_ambiguedad`, llamadas desde `alta_desde`, el punto que ya comparten `POST` y `PUT`. El fallo se traduce con `fuera_de_alcance` a `AppError::InvalidInput`, que responde **`400`** nombrando el campo.
- `src/engine/extraccion.rs` — `confinar_host_path` antes de leer un directorio; `readlink -f <db_path>` **antes** del volcado de un `sqlite` y la comparación exacta con el declarado.
- `src/engine/restauracion.rs` — `confinar_host_path` sobre el **destino** antes de escribir ni podar; el `readlink -f` antes de la primera orden; y el `rm -f` de la retirada **acotado** al `<db_path>` declarado y a sus hermanos `-wal` y `-shm`.
- `src/docker/cliente.rs` — `exigir_fuente_de_copia`, que inspecciona el contenedor con `inspect_container` **antes** de crear el `exec` (tanto en `ejecutar` como en `ejecutar_con_entrada`, el único punto por el que pasan todos los `exec`), y `exigir_la_etiqueta_de_fuente`, su mitad pura. Nuevo `Error::ContenedorNoEsFuenteDeCopia`.
- `src/docker/mod.rs` — las constantes `ETIQUETA_DE_FUENTE_DE_COPIA` (`io.kesto.backup-source`) y `VALOR_DE_FUENTE_DE_COPIA` (`true`).
- `src/engine/mod.rs` — el motor recibe la raíz de copias por inyección (nunca la lee del entorno: en las pruebas no se toca el entorno del proceso).
- `src/test_support.rs` — el arnés declara `KESTO_BACKUP_ROOT` y la etiqueta donde crea contenedores de origen.
- `tests/e2e/arnes.rs`, `tests/e2e/escenarios.rs` — la `Receta` gana etiquetas, los contenedores de origen se etiquetan y la raíz de copias apunta a la raíz temporal de la batería; escenarios nuevos de la frontera.
- `docs/DEFINITION.md` (§4 y §8), `docs/ENTORNO.md`, `docs/FEATURE-005-backup-engine.md` y este documento.
- `openspec/changes/confinamiento-frontera-motor/` · `plans/PLAN-040.md`.
- `ui/`: **sin cambios** (los campos del contrato HTTP son los mismos; solo cambia qué valores se aceptan).

## Cómo funciona

### El borde HTTP: forma, sin tocar el disco

Al dar de alta o editar un target, `alta_desde` comprueba la **forma** de los dos campos de ruta:

- `host_path`: absoluto, sin `..` y **bajo** `KESTO_BACKUP_ROOT`, comparando por componentes. La raíz misma cuenta como «bajo la raíz» —copiar la raíz entera es una configuración legítima—. No se exige el segmento vacío: una barra final en un directorio (`/backup_sources/var_www/`) es legítima, a diferencia de `db_path`, donde sí se exige porque el valor ha de ser el nombre exacto de un fichero.
- `db_path`: absoluto, sin `..` y sin segmento vacío, para que el fallo se vea en el formulario y no en la primera copia.

Es la comprobación **textual**: **no exige que la ruta exista** —al dar de alta un directorio puede no estar montado todavía, y exigir su existencia convertiría el alta en una comprobación del despliegue—. Si no pasa, la API contesta `400` nombrando el campo y **no escribe fila**.

### El motor: canonicalización, también para las filas anteriores a la regla

El borde HTTP no es el único camino al motor —hay targets escritos antes de que la regla existiera—, así que el motor **vuelve a comprobarlo canonicalizando** antes de leer (extracción) y antes de escribir ni podar (restauración):

- Se resuelven los enlaces simbólicos de **todos** los componentes, no solo el último: es por donde escapa un `host_path` textualmente correcto —un directorio **padre** enlazado—.
- El destino de una restauración puede **no existir**; se canonicaliza el **anteproyecto existente más profundo** y se recompone el resto textualmente (el mismo patrón que ya usaba la clave de reserva).
- La comparación final vuelve a ser **por componentes** contra la raíz **también canonicalizada**.
- Si la ruta escapa, la ejecución acaba en **`failed`** con un motivo que nombra el campo, y **no se toca el sistema de ficheros**: ni se lee un byte, ni se escribe, ni se poda.

### El `db_path`: `readlink -f` y un `rm -f` acotado

Antes de ejecutar **ninguna** orden contra el contenedor de un target `sqlite`, el motor ejecuta `readlink -f <db_path>` **dentro del contenedor** y exige que la salida coincida **exactamente** con el declarado (se admite solo el salto de línea final de `readlink`, `\n` o `\r\n`). Si difiere, la ejecución acaba en `failed` **sin volcar** (extracción) y **sin retirar nada** (restauración). Cubre los tres vectores de una vez: un `..`, un enlace en el **último** componente y —el peligroso— un enlace en un directorio **padre**, que un `rm -f` seguiría hasta borrar un fichero ajeno.

En la restauración, el `rm -f` que retira el fichero vivo queda **acotado** al `<db_path>` declarado y a sus hermanos `-wal` y `-shm`, con la ruta ya comprobada: un vecino del mismo directorio sobrevive.

La comprobación se hace **sin `sh -c`**: el repositorio construye los `argv` del `exec` sin intérprete a propósito (la credencial viaja por el entorno y no por la línea de órdenes), así que se captura el `stdout` de `readlink -f` y la comparación la hace Kesto.

### La etiqueta de fuente de copia

El motor inspecciona el contenedor del target con `inspect_container` **antes de crear ningún `exec`** y exige la etiqueta `io.kesto.backup-source` con el valor **exacto** `true`. Sin la etiqueta —ausente, o con `"1"`, `"yes"`, `"false"`— no se crea el `exec` y la ejecución acaba en `failed` nombrando el contenedor. La comprobación vive en el **único punto** que crea `exec` en el cliente del daemon, así que ningún camino nuevo del motor puede saltársela. La etiqueta se exige **en las dos direcciones**: extracción y restauración.

## Variables y configuración

| Pieza | Defecto | Regla |
|---|---|---|
| `KESTO_BACKUP_ROOT` | `/backup_sources` | Raíz bajo la que ha de quedar el `host_path` de un target `directory`. Ruta **absoluta**: presente pero relativa o vacía → **error de arranque** que nombra la variable (en la línea de `RUST_LOG=""` y `BIND_ADDRESS=""`); ausente → defecto. El defecto es exactamente donde el `compose` del §8 monta los directorios a respaldar, así que la plantilla de referencia no la declara. |
| Etiqueta `io.kesto.backup-source=true` | — | La ha de llevar **cada contenedor del que se copia** (los orígenes de base de datos), puesta al crearlo (`--label io.kesto.backup-source=true`). Es **fail-closed**: sin ella no se ejecuta nada. El propio `kesto-app` queda denegado por sí solo, porque nadie lo etiqueta. |
| `readlink` en el contenedor | — | Un contenedor del que se copia un `sqlite` ha de traer **`readlink`** —busybox y coreutils lo traen— **además de `sqlite3`**. Su ausencia se reporta como el fallo de la ejecución, con el error del contenedor leído, no silenciado. |

## Decisiones

- **La raíz es una variable, no una constante cableada.** El defecto `/backup_sources` hace que el despliegue de referencia funcione sin declarar nada; la variable permite al operador mover su raíz sin tocar el binario y deja que el e2e y el modo local apunten la raíz a su temporal. Se descarta derivarla de los montajes del propio contenedor: se adaptaría sola, pero en el modo local no hay contenedor de Kesto que inspeccionar y añadiría una llamada al daemon en el arranque para algo que el operador sabe mejor que el daemon.
- **La comparación es por componentes, no por prefijo de cadena.** `/backup_sources_x` **empieza por** `/backup_sources` como texto y **no** está dentro de él. Se compara con `Path::starts_with` (componentes enteros) y no con `str::starts_with`: un confinamiento por prefijo de cadena deja pasar justo lo que quiere cerrar.
- **Dos comprobaciones, dos momentos.** El borde HTTP comprueba la forma (fallo rápido, `400` en el formulario) y el motor canonicaliza (la que de verdad cierra el agujero, porque resuelve enlaces de directorios padre). Las dos hacen falta y no se solapan: la textual no toca el disco, la canónica sí.
- **La raíz se guarda tal como se declaró, y la comparación del borde es textual.** `Config` **no** canonicaliza `KESTO_BACKUP_ROOT`: el borde compara el `host_path` declarado contra la raíz declarada, por componentes y sin tocar el disco, y la canonicalización —que resuelve los enlaces de los dos lados— es cosa del motor. Guardar la raíz resuelta desajustaría el borde: una raíz declarada que es un enlace rechazaría con un `400` desconcertante una ruta textual que el motor sí aceptaría.
- **La ventana TOCTOU entre la comprobación y el uso se acepta.** Entre `canonicalize` / `readlink -f` y la lectura o el `rm -f` hay una ventana en la que la ruta podría cambiar. No es explotable en el modelo de amenaza declarado —el operador controla el contenedor y el host, el `host_path` se lee de un montaje `:ro` y el `rm -f` corre dentro de un contenedor que el atacante ya controla—, así que se declara como límite aceptado y no se aborda aquí.
- **La frontera de `db_path` es el contenedor, no una raíz propia.** `db_path` vive en el sistema de ficheros de **otro** contenedor, del que Kesto no conoce la disposición: una `KESTO_SQLITE_ROOT` obligaría a cada despliegue a declararla y dejaría de poder copiarse cualquier base en otra ruta. La frontera que sí existe y es comprobable es el contenedor, ya restringido por la etiqueta; dentro de él, lo que queda por cerrar es que la ruta no escape por un enlace, y `readlink -f` lo cierra.
- **Una etiqueta de opt-in, no una lista de configuración.** Una lista (`KESTO_ALLOWED_CONTAINERS`) se puede olvidar de poblar o traer un valor por defecto permisivo; la etiqueta es **fail-closed sin configuración**, la declaración **viaja con el contenedor** y `kesto-app` queda denegado por sí solo, sin cablear un nombre que se rompa con cualquier renombrado. Se descarta la variante «lista **o** etiqueta»: dos mecanismos para lo mismo son dos cosas que explicar, dos que probar y una que siempre se olvida.
- **El módulo `frontera` es el único sitio donde se decide qué es una ruta aceptable.** No se duplica por dirección —extracción y restauración comparten la misma regla—, a diferencia de las guardas de banderas, que se duplican a propósito porque sus mensajes hablan de campos distintos.
- **Los errores nombran el campo y nunca el valor.** El `Display` de `FronteraError` y el de `ConfigError::InvalidBackupRoot` no interpolan ninguna ruta: lo único que se lee en el mensaje es el nombre del campo que el operador escribió.

## Follow-ups

- **Separación de privilegio del motor (la otra mitad de SEC-06, #162).** Este change cierra el **alcance** —en qué contenedor y con qué ruta se ejecuta—, pero no el **privilegio**: los `exec` siguen corriendo con el usuario que traiga el contenedor, habitualmente `root`. Ejecutarlos con un usuario sin privilegios o desde un daemon aparte es un cambio de despliegue con su propio diseño y no cabe aquí; seguimiento abierto en **#173** al cerrar este change (tarea 7.6 del plan).
- **Coste aceptado de declarar la ruta canónica.** Una base que viva de verdad bajo un directorio **enlazado** deja de copiarse hasta que el operador declare la ruta **canónica**. Es un coste pequeño y explícito frente a dejar abierto el vector del directorio padre, que es el peligroso; la alternativa —aceptar la ruta declarada sin canonicalizar— lo dejaría abierto.
- **No se migran filas.** El motor rechaza la fila **al usarla**, nombrando el campo, en vez de copiar mal. No se añaden `CHECK` al esquema: la coherencia ya se exige en el borde HTTP y una restricción nueva obligaría a una migración de reconstrucción para filas que el motor ya rechaza.
