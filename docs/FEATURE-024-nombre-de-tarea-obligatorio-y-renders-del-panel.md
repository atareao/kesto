# FEATURE-024 — Nombre de tarea obligatorio y único, y renders del panel

- **Issues**: #87, #89, #93 · **Plan**: `plans/PLAN-024.md` · **Change SDD**: `openspec/changes/archive/2026-11-16-contratos-y-renders-del-panel/` (archivado; cuatro deltas: `db`, `api-resources`, `ui` y `restauracion`)
- **Rama**: `feature/panel-contratos-de-tareas`

## Qué es

Cierra tres defectos del panel que comparten raíz. **#87**: la columna «Nombre» de Tareas salía vacía. **#89**: en Snapshots, «Tarea» y «Etiqueta» mostraban lo mismo. **#93**: la poda de un directorio de destino no era alcanzable desde la UI. El primer par se arregla cortando el problema por la raíz —`backup_jobs.name` pasa a **obligatorio y único**—; el tercero es la puerta que faltaba en la confirmación de restauración.

## Por qué

- **#87 — «Tareas: la columna Nombre sale vacía».** `backup_jobs.name` nació **anulable y no único** a propósito (`migrations/0005_nombre_de_tarea.sql`): «una tarea sin nombre es un estado válido». Como el panel tenía que decidir en cada sitio qué enseñar cuando no lo hay, decidía distinto cada vez: la celda «Nombre» de Tareas dejaba el hueco en blanco (`tarea.name ?? ''`), y la de Snapshots caía al identificador de la tarea. El usuario aprobó cerrar la raíz: el nombre pasa a ser obligatorio y único global, y las tareas ya existentes reciben un nombre derivado **dentro de la migración**.
- **#89 — «Snapshots: «Tarea» y «Etiqueta» muestran lo mismo».** La columna «Tarea» caía a `label` cuando faltaba `job_name`, y `label` es el identificador de la tarea: las dos columnas acababan diciendo el mismo valor. Con el nombre obligatorio, el único `null` que queda en `job_name` es una instantánea **huérfana** (sin vínculo con ninguna ejecución), y se pinta con un marcador propio.
- **#93 — «La poda de un directorio no es alcanzable desde la UI».** El backend ya la autorizaba —`sobrescribir` viaja para todos los tipos y el motor la aplica como `RestoreOptions::delete(podar)`—, pero el panel abría la confirmación **solo** cuando el target era de base. Un directorio caía a un `Popconfirm` y llamaba con `sobrescribir = false`: el destino solo recibía y no había manera de pedir la poda.

## Ficheros implicados

### Esquema y migración

| Fichero | Papel |
|---|---|
| `migrations/0007_nombre_de_tarea_obligatorio.sql` | **Reconstruye** `backup_jobs` con `name TEXT NOT NULL` y `UNIQUE(name)`; rellena las filas sin nombre, desambigua los homónimos y recrea los dos índices |
| `src/db/migrate.rs` | `aplicar` corre las migraciones con `foreign_keys` **apagado** (fuera de transacción), lo reenciende y comprueba que no queden filas huérfanas; tests de la `0007` (relleno, ensanchado, desambiguación, blanco, grupo grande) |
| `docs/DEFINITION.md` §3 | La columna `name` de `backup_jobs` pasa a `TEXT NOT NULL` con `UNIQUE(name)` |

### API (backend)

| Fichero | Papel |
|---|---|
| `src/db/entities/job.rs` | `Tarea.name`/`AltaDeTarea.name` dejan de ser `Option<String>`; `error_de_escritura` traduce la violación de `UNIQUE(name)` a `DbError::NombreDeTareaDuplicado` |
| `src/db/mod.rs` | La variante tipada `NombreDeTareaDuplicado` |
| `src/error.rs` | Esa variante se convierte en `AppError::Conflict` → **`409`** con la envoltura del §5 y el mensaje «el campo `name` ya está en uso: elige otro nombre para la tarea» (nombra el campo, **no** repite el valor) |
| `src/api/jobs.rs` | `nombre_del_cuerpo` (falta/`null`/blanco → `400` que nombra `name`; tope de 120 caracteres en caracteres, no bytes; recorta); `TareaDelCable.name` no anulable |

### Frontend

| Fichero | Papel |
|---|---|
| `ui/src/components/ModalDeTarea/ModalDeTarea.tsx` | «Nombre» obligatorio (`required` con `whitespace: true`); deja de omitir la clave en blanco; reparte los rechazos por el campo que nombran (`400`/`409` del nombre al campo «Nombre», `400` del cron al cron) sin cerrar el formulario |
| `ui/src/services/api/types.ts` | `Job.name: string` (no `string \| null`); `AltaJob.name` obligatorio; comentarios del contrato viejo retirados |
| `ui/src/pages/tareas.tsx` | La celda «Nombre» pinta `tarea.name` y, como red de seguridad, un marcador atenuado («sin nombre»); nunca el nombre del target |
| `ui/src/pages/snapshots.tsx` | La columna «Tarea» pinta `job_name` y **deja de caer a `label`**; un `null` se pinta atenuado («sin tarea»). «Etiqueta» queda tal cual. Una sola rama de confirmación para base y directorio |
| `ui/src/api/snapshots.ts` | El cuerpo del `POST` lleva `sobrescribir` como booleano explícito; los comentarios dejan de afirmar que un directorio no tiene nada que sobrescribir |

Tests tocados: `ui/src/pages/tareas.test.tsx`, `ui/src/pages/snapshots.test.tsx`, `ui/src/components/ModalDeTarea/ModalDeTarea.test.tsx`, `src/api/jobs.rs` (módulo de tests) y `src/db/migrate.rs` (módulo de tests).

## Cómo funciona

### El contrato del nombre

- **Esquema**: `backup_jobs.name` es `TEXT NOT NULL` con `UNIQUE(name)` y `CHECK (length(trim(name)) > 0)`. La unicidad es **global** —toda la tabla, no por repositorio— y **sensible a mayúsculas**: sin colación declarada, SQLite usa `BINARY`, así que «Copia» y «copia» son dos nombres distintos. El tope de **120 caracteres** no se declara en la base: lo impone el borde HTTP, el único que puede contestar un `400` nombrando el campo (como decidió la `0005`).
- **API**: en el cable `name` sigue siendo `Option<String>` para que `None` y el blanco lleguen al validador y no al extractor de JSON. `nombre_del_cuerpo` recorta primero y luego:
  - sin la clave, a `null`, o en blanco tras recortar → **`400`** que nombra `name`;
  - más de 120 caracteres → **`400`** que nombra `name` y **no repite el valor**;
  - el nombre recortado es el que se guarda.
- **Unicidad en la base, no en un pre-chequeo**: `UNIQUE(name)` es la garantía; el `409` sale de violarla y `error_de_escritura` la traduce a un error **tipado** (`NombreDeTareaDuplicado`), que `src/error.rs` convierte en `409`. Los mensajes del `400` por falta y del `409` por duplicado son distintos, para que el operador sepa si tiene que **escribir** un nombre o **cambiarlo**. Una edición que **conserva** el nombre propio de la tarea no choca: solo choca contra el de *otra*.
- **Respuestas**: `GET /api/v1/jobs` y `GET /api/v1/jobs/:id` publican `name` como cadena, nunca `null`.

### La migración `0007`

SQLite no sabe reescribir con `ALTER TABLE` la nulabilidad de una columna ya declarada ni añadir una restricción de tabla a una que existe, así que la tabla se **reconstruye**: crear la nueva, copiar, borrar la vieja y renombrar. Es el procedimiento que ya usó la `0003`.

- **Claves foráneas apagadas, fuera de transacción.** `backup_jobs` es **padre** de `job_executions`; su `DROP TABLE` con las claves encendidas ejecutaría el `ON DELETE CASCADE` y se llevaría el histórico de copias de **todas** las tareas sin que nada lo delatara. `db::migrate::aplicar` apaga `foreign_keys` **en la conexión y fuera de transacción** (`PRAGMA foreign_keys` es un no-op dentro de una), lo reenciende después y comprueba que no quedaran filas huérfanas.
- **Nombre en blanco tratado como ausente.** El `CHECK` de no-vacío rechaza un blanco, así que la migración convierte antes cualquier `''` o `'   '` a `NULL`: un nombre que no dice nada es un nombre ausente y recibe el mismo relleno.
- **Relleno con nombre derivado.** Las filas sin nombre se rellenan **sobre la tabla vieja** (que todavía no tiene `UNIQUE`, así que un intento a medias no aborta) con `tarea <primer octeto del id>` —catorce caracteres—. Si ese tramo ya está ocupado (por un nombre que ya existía o por otra fila sin nombre con el mismo octeto), se ensancha al **siguiente tramo del mismo id** (16, 24 y 36 caracteres); una red de seguridad final con el `rowid` hace que la migración **termine siempre**.
- **Desambiguación de homónimos preexistentes.** El contrato anterior era **no único**, así que una instalación real puede tener dos tareas llamadas igual; el `INSERT ... SELECT` de la reconstrucción chocaría con `UNIQUE(name)` y abortaría la migración entera —Kesto no arrancaría y sin pantalla donde arreglarlo—. De cada grupo de homónimos, la fila de **menor `rowid`** conserva su nombre y las demás reciben un sufijo ` (2)`, ` (3)`, … que **avanza hasta dar con uno libre en toda la tabla** (no solo en su grupo). El reparto es determinista y va en una sentencia con CTEs `MATERIALIZED` (necesarias: la sentencia lee la tabla que está actualizando) apoyada en un índice temporal sobre `name`, que se borra antes de reconstruir.
- **Reconstrucción final.** La tabla nueva declara `name` en **última** posición —como la dejó el `ADD COLUMN` de la `0005`—, `NOT NULL`, `UNIQUE(name)` y el `CHECK` de no-vacío; las filas se copian **columna a columna, nombradas** (con `SELECT *` la copia dependería del orden de la tabla vieja); se borra la vieja, se renombra la nueva y se recrean `idx_backup_jobs_target_id` e `idx_backup_jobs_repository_id`.

### Los renders

- **Tareas** (`ui/src/pages/tareas.tsx`): la celda «Nombre» pinta `tarea.name`; si llegara `null` o en blanco —valores que el contrato ya no admite pero que un panel que hable con un backend anterior todavía puede recibir—, pinta el marcador atenuado «sin nombre». **Nunca** el nombre del target, que es justo la confusión que el campo viene a resolver.
- **Snapshots** (`ui/src/pages/snapshots.tsx`): la columna «Tarea» pinta `job_name` y **deja de caer a `label`**; un `null` se pinta atenuado («sin tarea»). La columna «Etiqueta» sigue siendo el identificador del snapshot y **no se tocó**: la duplicación desaparece porque «Tarea» pasa a decir otra cosa.

### La confirmación de poda (`#93`)

La bandera deja de deducirse del tipo de target y pasa a ser un parámetro **explícito** de la operación, `alRestaurar(snapshot, sobrescribir)`, con una sola rama de confirmación para los dos tipos:

- Un target de **base** exige **teclear su nombre** antes de aceptar, y entonces manda `sobrescribir: true` (el `428` sin confirmación sigue siendo exclusivo de las bases).
- Un target de **directorio** ofrece una **casilla** que, al marcarla, pide la **poda** —retirar del destino lo que no está en la instantánea— y manda `sobrescribir: true`; sin marcarla, manda `sobrescribir: false` y el destino **solo recibe**.
- `ui/src/api/snapshots.ts` envía siempre el booleano explícito en el cuerpo del `POST`.

**Sin cambio de backend en esta parte**: el motor y el contrato HTTP ya distinguían los dos desenlaces.

## Variables y configuración

**Ninguna nueva.** Este change no añade ni cambia variables de entorno ni configuración: la migración va embebida, el contrato del nombre lo impone el esquema y el borde HTTP, y la parte C no toca el backend. La tabla completa de variables sigue en `docs/ENTORNO.md`.

## Cómo habilitar / desplegar

- **La migración `0007` se aplica sola al arrancar.** Las migraciones van embebidas (`sqlx::migrate!()`) y el arranque las aplica antes de servir, así que una instalación que ya tuviera Kesto recibe `name NOT NULL` + `UNIQUE(name)` en el primer arranque tras actualizar la imagen. El panel va en el bundle, sin pasos.
- **Copia de seguridad recomendada antes de actualizar** una instalación que pueda tener tareas **homónimas** o sin nombre. La migración **ya las desambigua** —las de nombre repetido se renombran con sufijo ` (2)`, ` (3)`…— y **ya rellena** las que no tenían nombre, pero conviene conservar la base por si se quiere consultar el nombre anterior. Para el volumen desplegado, `just backup-datos` detiene el contenedor, exporta el volumen `*kesto_data` a un `tar.gz` y lo vuelve a arrancar. En local, `just db-setup` recrea la base y aplica las migraciones.
- Puertas: `just check-all` y `openspec validate --all --strict` en verde.

## Decisiones

- **Unicidad global.** `UNIQUE(name)` sobre toda la tabla, no por repositorio: el nombre se convierte en identidad legible de la tarea —la misma cadena en Tareas, en el Histórico y en Snapshots—. El precio es que dos tareas sobre el mismo target ya no pueden llamarse igual, que es lo que el operador quiere leer.
- **Unicidad sensible a mayúsculas.** `UNIQUE(name)` con la colación `BINARY` por defecto: «Copia» ≠ «copia». Es lo predecible sin decidir qué es «el mismo nombre» en más de un idioma, y el `409` del borde cuenta exactamente lo que la base rechaza.
- **Nombre derivado `tarea <octeto corto del id>`.** Las tareas sin nombre reciben el primer octeto del UUID (`tarea 1a2b3c4d`), **no** el identificador entero ni una copia suya: sería el valor de `label` y volvería a dejar «Tarea» y «Etiqueta» diciendo lo mismo.
- **El marcador «sin nombre» es red de seguridad, no solución.** Con el nombre obligatorio, el único `null` alcanzable es una instantánea huérfana o un backend anterior; el marcador lo hace explícito y atenuado en vez de dejar un hueco indistinguible de «no se pintó».
- **La columna «Etiqueta» no se tocó.** #89 apuntaba que, si no aporta un dato distinto, se replantee su existencia; aquí **solo** se elimina la duplicación (la «Tarea» deja de valer lo que la etiqueta). Replantear o retirar la columna es otro change, porque afecta a lo que el operador ya usa para identificar un snapshot.
- **La confirmación de un directorio no exige teclear el nombre.** Teclear el nombre convierte un clic en una decisión cuando se **pisan datos vivos**; en un directorio la operación **retira** lo que no está en la copia, así que el aviso es explícito y la aceptación deliberada sin pedir además el nombre. Igualar el resto —una sola llamada con la bandera explícita— sí era obligatorio.

## Follow-ups

- **`historico.tsx` sigue respaldando al UUID (#88, fuera de alcance).** La columna «Tarea» del Histórico cae al identificador de la tarea cuando `job_name` es `null` (`ui/src/pages/historico.tsx`). Con el nombre obligatorio ese `null` solo aparece en ejecuciones huérfanas; el issue #88 cubre la revisión de esa pantalla y queda fuera de este change.
- **Clasificación de errores por el campo entre backticks.** El modal decide en qué campo mostrar un rechazo buscando el nombre exacto del campo entre backticks en el mensaje del servidor (para no confundir la palabra «name» suelta de un mensaje que habla de `repository_id`). Es una heurística acoplada al formato de los mensajes: si un día un rechazo cita un campo de otra forma, se señalaría el campo equivocado.
- **Bundle de la UI por encima de los 500 kB.** `pnpm run build` avisa de que un chunk supera los 500 kB (umbral por defecto de Vite); el bundle ronda los **1,3 MB** (≈ 407 kB gzip) por Antd en un único chunk. Candidato a `manualChunks`/code-splitting, ya anotado como deuda desde FEATURE-007 y FEATURE-016.

## Referencias

- Issues: **#87** (Tareas: la columna «Nombre» sale vacía), **#89** (Snapshots: «Tarea» y «Etiqueta» muestran lo mismo), **#93** (la poda de un directorio no es alcanzable desde la UI). El issue de la parte A es **#87**: no se abrió uno nuevo, la decisión se imputa a él.
- Change SDD: `openspec/changes/archive/2026-11-16-contratos-y-renders-del-panel/` (archivado al cerrar). Deltas sobre `db`, `api-resources`, `ui` y `restauracion`.
- Plan: `plans/PLAN-024.md`.
- Commits principales: `✨ feat(tareas): el nombre de la tarea es obligatorio y único (#87)`, `✨ feat(ui): nombre obligatorio, renders del panel y poda de directorio (#87 #89 #93)`, `🐛 fix(db): la 0007 desambigua los nombres repetidos antes de exigir unicidad`, `🐛 fix(db): la migración descarta la conexión si no reenciende las claves foráneas`, `♻️ refactor(ui): una sola condición de confirmación y limpieza de la auditoría (#93)` y `🔀 chore(db): la 0007 desambigua los homónimos y el §3 queda al día`.

## Verificación

- **Backend**: `cargo test --locked` en verde — **545 passed / 0 failed / 27 ignored**.
- **Panel**: `pnpm test` en verde — **228 passed** (30 ficheros).
- **Puertas**: `just check-all` (fmt, clippy, test, typecheck, tests de UI, lint y build) en verde.
- **SDD**: deltas fusionados en `openspec/specs/` y change movido a `openspec/changes/archive/`. `openspec validate --all --strict` sin fallos **de este change** (queda un aviso previo y ajeno a él en `spec/verificacion-e2e`: `## Purpose` es un «TBD» de la feature 023); `openspec list` sin cambios activos y `just check-spec` fallando por su ausencia, que es lo esperado al archivar el último change.
