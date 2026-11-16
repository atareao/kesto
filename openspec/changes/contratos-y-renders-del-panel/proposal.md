# Proposal — contratos-y-renders-del-panel

## Why

Tres defectos del panel tienen la misma raíz —el **contrato** de la tarea y lo que las pantallas hacen con él— y se arreglan de una vez, porque el orden importa: mientras el nombre siga siendo anulable, cualquier celda que lo pinte tiene que inventarse un sustituto, y ésos son los sustitutos que hoy se contradicen entre pantallas.

**Parte A — el nombre de la tarea (el núcleo).** `backup_jobs.name` nace anulable a propósito (`migrations/0005_nombre_de_tarea.sql`): el nombre era **opcional y no único**, y «sin nombre» era un estado válido. Con eso, el panel tiene que decidir en cada sitio qué enseñar cuando no lo hay, y no lo decide igual dos veces: la celda «Nombre» de Tareas deja el hueco en blanco (`tareas.tsx:96`), y la de Snapshots cae al identificador de la tarea (`snapshots.tsx:441`). El resultado es el que reportan #87 y #89: una vista con un hueco y otra con dos columnas que dicen lo mismo. La decisión aprobada es cortar el problema por la raíz: el nombre pasa a ser **obligatorio y único global**, y las tareas que ya existen reciben un nombre derivado y único **dentro de la migración**. Es la única forma de que «Nombre» signifique lo mismo en Tareas, en el Histórico y en Snapshots.

**Parte B — los renders #87 y #89.** Son los dos síntomas visibles. `ui/src/pages/tareas.tsx:91-97` pinta `tarea.name ?? ''`, que es un hueco en blanco indistinguible de «no se pintó»; `ui/src/pages/snapshots.tsx:441-445` cae a `label` cuando falta el nombre, y como `label` es el identificador de la tarea, «Tarea» y «Etiqueta» acaban mostrando lo mismo —literalmente lo que denuncia #89—. Los tests que hoy pasan **codifican el defecto** (`tareas.test.tsx:185`, `snapshots.test.tsx:236-238`), así que primero hay que corregirlos.

**Parte C — la poda de un directorio (#93).** El backend ya la autoriza: `sobrescribir` viaja en `CuerpoDeLaRestauracion` (`src/api/snapshots.rs:169-170`, con `#[serde(default)]`), un `directory` solo exige `host_path` y el motor la aplica como `RestoreOptions::delete(podar)`. Lo que falta es la puerta: `ui/src/pages/snapshots.tsx:511` abre la confirmación **solo** cuando el target es de base, así que un directorio cae al `Popconfirm` y llama a `alRestaurar(snapshot)` con `sobrescribir = false` —el destino solo recibe y no hay manera de pedir la poda—. Los comentarios de `ui/src/api/snapshots.ts:25-31` afirman que en un directorio «no hay nada que sobrescribir», que es falso: la poda **retira** del destino lo que no está en la instantánea.

## What Changes

### Parte A — `backup_jobs.name` obligatorio y único

- La columna pasa a `TEXT NOT NULL` con `UNIQUE(name)`, por la migración `0007_nombre_de_tarea_obligatorio.sql`, que **reconstruye** la tabla: SQLite no puede reescribir con `ALTER TABLE` una columna ya declarada anulable ni añadir una restricción de tabla a una que existe.
- Las tareas sin nombre se rellenan **dentro de esa migración** con un nombre derivado, único y no vacío —nunca el identificador de la tarea: sería la semilla de la duplicación de #89—, y las que ya tenían nombre se quedan como están.
- La reconstrucción trata las claves foráneas: `backup_jobs` es **padre** de `job_executions`, y su `DROP TABLE` con `foreign_keys` encendido se llevaría por delante las ejecuciones de todas las tareas. Corre con `foreign_keys` apagado, como las migraciones anteriores (`db::migrate::aplicar`), recrea los dos índices de la tabla y no pierde ninguna fila.
- `POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` **exigen** `name`: sin él, o en blanco tras recortar, contestan `400` nombrando `name`; más de 120 caracteres, `400` también; y un nombre que ya tenga otra tarea contesta **`409`**, con un mensaje **distinto** del de la falta de nombre. La unicidad vive en la base y el `409` sale de violarla, no de una lectura previa.
- `GET /api/v1/jobs` y `GET /api/v1/jobs/:id` publican `name` como cadena, nunca `null`.
- El formulario de Tareas hace el campo «Nombre» **obligatorio**, y **deja de señalar sobre la expresión cron** los rechazos que hablan del nombre: el `400` que lo nombra y el `409` del duplicado se muestran en su campo, sin cerrar el formulario.

### Parte B — los renders de #87 y #89

- `ui/src/pages/tareas.tsx`: la celda «Nombre» muestra `tarea.name` y, como red de seguridad, un marcador explícito y atenuado («sin nombre») cuando llegue `null` o en blanco —nunca un hueco y nunca el nombre del target—.
- `ui/src/pages/snapshots.tsx`: la columna «Tarea» muestra `job_name` y **deja de caer a `label`**; cuando llegue `null` —la instantánea no casa con ninguna tarea— pinta el mismo tipo de marcador atenuado. La columna «Etiqueta» **no se toca**: sigue siendo el identificador del snapshot, y la duplicación desaparece porque «Tarea» pasa a decir otra cosa.
- Los tests que hoy fijan el defecto (`tareas.test.tsx:185` y `snapshots.test.tsx:236-238`) se corrigen con el contrato nuevo.

### Parte C — la poda de directorio (#93)

- `ui/src/pages/snapshots.tsx`: la confirmación deja de reservarse a `esTargetDeBase(snapshot.target_type)`; un directorio la ofrece **la misma**, con el aviso de que la operación **retira del destino** lo que no está en la instantánea, y solo al aceptarla se llama a `alRestaurar(snapshot, true)`.
- Sin aceptarla, el directorio se restaura igual que hoy: `sobrescribir: false` y el destino **solo recibe**.
- La bandera deja de deducirse del tipo de target y pasa a ser un **parámetro explícito** de la operación, `alRestaurar(snapshot, sobrescribir)`.
- Los comentarios de `ui/src/api/snapshots.ts` dejan de afirmar que un directorio no tiene nada que sobrescribir.
- **Sin cambio de backend**: para un `directory` la API ya exige solo `host_path`, `sobrescribir` ya está en el contrato para todos los tipos y el `428` sin confirmación sigue siendo **exclusivo de las bases**.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `db` — `backup_jobs.name` pasa a obligatoria y única, y la migración que lo consigue reconstruye la tabla rellenando las filas que no tenían nombre.
- `api-resources` — la tarea lleva un nombre **obligatorio y único**: `400` cuando falta, `409` cuando ya está en uso, y `name` deja de ser anulable en las respuestas. El requisito viejo —«La tarea lleva un nombre legible», que fijaba el nombre como opcional y no único— se **retira** y en su lugar se añade «El nombre de la tarea es obligatorio y único» (decisión D13): no es un matiz, es otro contrato sobre el mismo campo.
- `ui` — Tareas exige el nombre y lo pinta sin huecos, Snapshots deja de confundir «Tarea» con «Etiqueta», y la confirmación de sobrescritura cubre también los targets de directorio.
- `restauracion` — la bandera de sobrescritura queda declarada como lo que ya es para un directorio: la que autoriza la **poda**, explícita y no deducida del tipo de target.

## Decisiones

Aprobadas por el operador (D1, D2, D4, D7 y D8), consecuencia técnica de ellas (D3, D5, D6, D13), o decisiones de diseño de este proposal (D9 a D12):

- **D1 — El nombre es obligatorio y único global.** Aprobada. Un nombre anulable obliga a cada pantalla a inventarse un sustituto, y ésos son los sustitutos que hoy se contradicen; único, el nombre se convierte en identidad de la tarea —la misma cadena en Tareas, en el Histórico y en Snapshots—. El precio es que dos tareas sobre el mismo target ya no pueden llamarse igual, que es justo lo que el operador quiere leer.
- **D2 — Las tareas sin nombre se rellenan dentro de la migración, con un derivado único.** Aprobada. Alternativa descartada: dejar las filas viejas ilegibles (imposible: `NOT NULL`) o borrarlas (una migración que se lleva copias y su histórico). El derivado **no** puede ser el identificador de la tarea: sería exactamente el valor de `label`, y volvería a dejar «Tarea» y «Etiqueta» diciendo lo mismo. La forma aprobada es `tarea <octeto corto del id>` —el primer octeto del UUID en minúsculas, no el UUID entero—: único, no vacío, muy por debajo de los 120 caracteres y distinto del identificador.
- **D3 — Se reconstruye la tabla, no se parchea.** Consecuencia de D1: SQLite no admite `ALTER TABLE` para reescribir la nulabilidad de una columna ya declarada ni para añadir una restricción de tabla, así que el procedimiento canónico es crear la nueva, copiar, borrar y renombrar. `0003` ya fijó el camino y su manejo de las claves foráneas.
- **D4 — `400` para la falta y `409` para el duplicado.** Aprobada. No es un matiz de estilo: el `400` dice «la petición está mal» y el `409` «está bien y choca con lo que ya hay», y los mensajes se distinguen para que el operador sepa si tiene que **escribir** un nombre o **cambiarlo**. El `409` de la API (choque con el estado) no es el `409` del disparo manual (ya hay una ejecución en curso), y el panel los separa.
- **D5 — Los mensajes no repiten el valor.** Convención del repo (`api-resources`, «Los mensajes de error nombran el campo y nunca su valor»): el `409` dice que ese nombre ya está en uso, **sin** eco del nombre.
- **D6 — La unicidad vive en la base.** `UNIQUE(name)` es la garantía; el `409` se deriva de violarla. Comprobar y luego escribir deja la ventana abierta a que dos altas simultáneas pasen las dos.
- **D7 — Un `CHECK` de no-vacío, aprovechando que se reconstruye.** Aprobada. El tope de 120 caracteres se queda donde la `0005` lo puso —el borde HTTP, único que puede contestar un `400` nombrando el campo—, pero el nombre en blanco es una fila sin valor: `CHECK (length(trim(name)) > 0)` cuesta una línea ahora y sólo se puede cerrar mientras se reconstruye. Esa línea entra.
- **D8 — La unicidad es sensible a mayúsculas.** Aprobada. `UNIQUE(name)` con la colación `BINARY` de SQLite —el defecto, sin colación declarada—: «Copia» y «copia» son dos nombres. Es lo predecible y no obliga a decidir qué es «el mismo nombre» en más de un idioma; el `409` del borde cuenta exactamente lo que la base rechaza.
- **D9 — El render no vuelve a caer a otro dato.** Con toda tarea nombrada, el único `null` que queda en `job_name` es una instantánea **huérfana** (sin vínculo con ninguna ejecución); se pinta atenuado y explícito, no con el contenido de `label`.
- **D10 — «Etiqueta» no se toca.** #89 apunta que, si no aporta un dato distinto, se replantee su existencia. Aquí sólo se elimina la duplicación (la «Tarea» deja de valer lo que la etiqueta); replantear la columna —o retirarla— es otro change, porque afecta a lo que el operador ya usa para identificar un snapshot.
- **D11 — La confirmación de un directorio no exige teclear el nombre del target.** Teclear el nombre es lo que convierte un clic en una decisión cuando se **pisan datos vivos**; en un directorio la operación retira lo que no está en la copia, así que el aviso es explícito y la aceptación deliberada, sin pedir además el nombre. Igualar las dos rutas en todo lo demás —una sola llamada con la bandera explícita— sí es obligatorio.
- **D12 — Sin cambio de backend en la parte C.** El motor y el contrato HTTP ya distinguen los dos desenlaces.
- **D13 — El requisito del nombre se retira y se sustituye, en vez de modificarse.** `openspec` no admite que un `MODIFIED` pierda un escenario: dos de los cuatro del requisito viejo —«Un nombre en blanco se guarda como `null`» y «El listado de tareas publica el nombre anulable»— afirman justo lo contrario de lo que este change aprueba, y conservarlos con el nombre puesto y el cuerpo cambiado dejaría el vocabulario de la spec mintiendo. Se retiran con su motivo y su migración, y el contrato nuevo entra como requisito propio con los escenarios que siguen valiendo —el recorte y el tope de 120— más los nuevos. `db` y `ui` sí se modifican en sitio: sus escenarios afectados conservan el nombre y el cuerpo cambia.

## Fuera de alcance

- Renombrar tareas por una ruta aparte o cualquier operación de nombre que no sea el alta y la edición.
- Replantear o retirar la columna «Etiqueta» de Snapshots (D10), y la columna «Target» del Histórico (#88), que es otro issue.
- Cualquier cambio en el motor, el planificador o la restauración más allá de la parte C, que **no** toca backend.
- Los demás defectos del panel detectados en la misma revisión (#90, #91, #92, #94).

## Impact

- **Base de datos**: nueva migración `migrations/0007_nombre_de_tarea_obligatorio.sql`; `docs/DEFINITION.md` §3 (la columna pasa a `NOT NULL` con `UNIQUE`); `src/db/entities/job.rs` (`Option<String>` → `String`) y su capa de acceso, que es la que traduce la violación de la unicidad a un error tipado.
- **API**: `src/api/jobs.rs` (`AltaDeTareaDelCable`, `TareaDelCable`, `nombre_del_cuerpo`, alta y edición), `src/error.rs` (`Conflict` ya existe; lo que se añade es el punto donde nace).
- **Frontend**: `ui/src/components/ModalDeTarea/ModalDeTarea.tsx` (campo obligatorio y destino de los rechazos), `ui/src/pages/tareas.tsx` y `ui/src/pages/snapshots.tsx` (las celdas y la rama de confirmación), `ui/src/api/snapshots.ts` (comentarios y bandera explícita) y `ui/src/services/api/types.ts` (`Job.name` deja de ser anulable; `AltaJob.name` pasa a obligatorio). Tests: `tareas.test.tsx`, `snapshots.test.tsx`, `ModalDeTarea.test.tsx`, `snapshots.test.tsx` (bloque de restauración).
- **SRS**: §3 enmienda la columna `name`; nada más cambia.
- **Riesgo**: la migración `0007` es la primera que reconstruye una tabla **padre** de un histórico (`job_executions`). Es el punto donde un `DROP TABLE` con las claves foráneas encendidas borraría las ejecuciones de todas las tareas sin que nada lo delate, y por eso lleva prueba propia.
- **Riesgo**: el nombre pasa a ser la llave con la que el operador lee una tarea; rechazar la edición de una tarea que **conserva** su propio nombre convertiría el formulario en intocable. Tiene escenario propio.

## Referencias

- Issues **#87** (Tareas: la columna «Nombre» sale vacía), **#89** (Snapshots: «Tarea» y «Etiqueta» muestran lo mismo) y **#93** (el panel no ofrece la confirmación de sobrescritura para targets de directorio). Las tres abiertas, asignadas al operador.
- **El issue de la parte A es #87.** «Tareas: la columna Nombre sale vacía» no es sólo el síntoma que la parte B pinta: es la raíz de la parte A —el nombre anulable es lo que deja el hueco en blanco y obliga a cada pantalla a inventarse un sustituto—, y hacerlo obligatorio y único es cerrarlo por la raíz. No se abre issue nuevo: la decisión de este change se imputa a #87 y lo cierra. #89 y #93 siguen siendo los issues de las partes B y C.
- `plans/PLAN-024.md` cubre sólo la parte C; hay que reescribirlo para las tres partes antes de empezar.
- `openspec/specs/db/spec.md` (las cuatro tablas, la reconstrucción de la `0003`), `openspec/specs/api-resources/spec.md` («La tarea lleva un nombre legible»), `openspec/specs/ui/spec.md` (Tareas, Snapshots y la restauración) y `openspec/specs/restauracion/spec.md` (la confirmación de sobrescritura).
- `migrations/0003_sqlite_target.sql` y `src/db/migrate.rs` (el patrón de reconstrucción y el `foreign_keys` apagado fuera de transacción) como precedente literal de la `0007`.
