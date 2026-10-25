# PLAN-004 — `api-endpoints`

> **Estado:** en curso (change proposal aprobado el 2026-10-25; implementación TDD en marcha).
> **Rama:** `feature/api-endpoints`, desde `development`.
> **Change de OpenSpec:** `openspec/changes/api-endpoints/` — **16 requisitos y 48 escenarios** (`api-resources` 11/33, capability **nueva**; `api-shell` 2/7, `db` 1/3 y `secrets` 2/5, que crecen), `openspec validate api-endpoints --strict` válido. `tasks.md`: **99 tareas** (7 de preparación, 48 de RED, 15 de GREEN, 10 de REFACTOR, 10 de operador y 9 de cierre).
> **Especificación de origen:** `docs/DEFINITION.md` §5 (endpoints y el evento SSE) y §6 (lo que el panel necesita). El motor del §4, Docker y Rustic **no** entran.
> **Cambio anterior:** `db-schema` (`PLAN-003-COMPLETED.md`, `docs/FEATURE-003-db-schema.md`, 156 tests, fusionado en `development` como `203638c`).

## Objetivo

Que el orquestador deje de ser una biblioteca. Hoy la capa de acceso del §3 está escrita, probada y auditada —con sus secretos cifrados— y **nadie la alcanza desde el proceso**: `actualizar` y `borrar` de las cuatro tablas solo los ejercitan las pruebas, y `/api/v1/*` tiene la salud y el flujo de entrada y nada más. Al terminar, `/api/v1` sirve los recursos del §3 con sus códigos, sus DTO, su validación y su paginación, y el pool llega a los manejadores por el estado compartido.

Es además el change que estrena el **primer consumidor** de la capa de acceso, y cierra **tres deudas declaradas** en `docs/FEATURE-003-db-schema.md`: el pool sin traspasar al `AppState`, la violación de clave foránea que colapsa en `500` y el `SecretoVacio` que también colapsa en `500`.

Y corrige **un error de recuento**: el §5 declara **diez** endpoints, no nueve. Lo dicen mal `AGENTS.md`, `plans/PLAN-003-COMPLETED.md` y `docs/FEATURE-003-db-schema.md`.

## Alcance

**Dentro:**

1. El **estado de base** dentro del `AppState`: el pool, la `Clave` de cifrado y las cuatro capas de acceso, construido una vez y **no opcional** —un Kesto sin base no es un estado válido—, con la base **ya abierta** para que `AppState::new` siga sin ser asíncrona.
2. La **superficie de los tres recursos**: `GET` y `POST` de `/targets`, `/repositories` y `/jobs` —los seis endpoints del §5— más `GET /:id`, `PUT /:id` y `DELETE /:id` en cada uno, declarados por escrito **como añadido al SRS**, con el precedente de los cuatro índices de `db-schema`. Quince rutas.
3. Los **tipos del cable** propios de la API, nunca las entidades: identificadores como texto con guiones, fechas como cadena RFC3339 formateada por nosotros, y los nombres de los campos los del §3.
4. La **validación** que hoy no existe en ningún sitio: coherencia del target con su tipo, referencias que existen antes de escribir, campos desconocidos rechazados y secretos vacíos rechazados nombrando el campo.
5. El **listado de ejecuciones** con filtros, paginación acotada (`limit` 50 por defecto, **200 de tope**) y orden estable `started_at DESC, id`.
6. La **traducción de los errores de base que causó la petición**: `SecretoVacio` → `400` nombrando el campo, clave foránea violada → `409`, y **todo lo demás sigue en `500`** con el mensaje genérico.
7. El **rechazo del cuerpo y de los parámetros** traducido a `400` con la envoltura y con mensaje **propio**, sin el texto del intérprete ni en el cuerpo ni en el registro.

**Fuera:** `POST /api/v1/jobs/:id/run` y `GET /api/v1/events` (el **motor**: sin nada que ejecute una copia, una ejecución `pending` es prometer algo que no ocurre; y sin productor, el flujo SSE abriría mudo y sus pruebas fabricarían un publicador que en producción no existe); `GET /api/v1/snapshots` (**Rustic**: hay que listar snapshots en S3 y `src/rustic/` está vacío); motor, planificador, Docker, Rustic, `ui/` y `Dockerfile`; agregados o `/stats`; filtro por repositorio en `/executions`; `PATCH`; ordenación configurable, `ETag`, caché y versionado de la API.

## Módulos

El change se reparte en tres módulos entregables, cada uno con su ciclo RED → GREEN → auditoría independiente → correcciones:

| # | Módulo | Contenido | Ficheros |
|---|---|---|---|
| 1 | Estado, tipos del cable y errores | El `Deserialize` manual de `SecretString`, el estado de base en el `AppState`, el arnés ajustado y la traducción de los errores de base y del rechazo del cuerpo | `src/secret.rs`, `src/state.rs`, `src/main.rs`, `src/test_support.rs`, `src/error.rs` |
| 2 | Los tres recursos | Los DTO de petición y respuesta, las quince rutas, la validación y la conservación del secreto en la edición | `src/api/mod.rs`, `src/api/{targets,repositories,jobs}.rs` |
| 3 | El listado de ejecuciones | Los filtros, la paginación acotada, el orden estable y el `Query` propio | `src/api/executions.rs`, `src/api/extract.rs`, `src/api/mod.rs`, `src/api/particion.rs`, `src/db/entities/execution.rs` |

El **orden de entrega** es 1 → 2 → 3. El 1 va primero porque **nada se puede probar sin él**: los manejadores reciben las capas por el estado, así que sin el estado no hay una sola ruta que ejecutar, y la traducción de los errores es lo que permite contestar `400` y `409` desde cualquiera de los tres recursos. El 3 va último aunque sea independiente del 2: los dos declaran rutas en `src/api/mod.rs`, y escribirlos a la vez sería dos escritores sobre el mismo fichero.

## Decisiones tomadas

### Alcance: los siete entregables hoy, más tres verbos declarados como añadido

`GET /:id`, `PUT /:id` y `DELETE /:id` **no** están en el §5 y se declaran como añadido, igual que `db-schema` declaró sus cuatro índices en vez de colarlos. El motivo es que la capa de acceso **ya implementa** `leer`, `actualizar` y `borrar` y hoy solo las pruebas las ejercitan: sin verbos de edición, un repositorio con una errata en la clave de acceso no se puede corregir, y el §6 pide «crear/editar tarea». El §5 tampoco tiene `GET /:id`, y sin él la edición se hace a ciegas.

### El SSE se difiere al motor

Sin un productor de eventos el flujo abriría y se quedaría mudo. Va con el motor, que trae a la vez el transporte y su primer evento real. Lo que **sí** queda escrito ahora es el esquema del evento del §5 —`event: execution_progress` con su payload literal—, porque `AGENTS.md` manda no inventar otro.

### Los códigos del recurso

`POST` contesta **`201`** con el recurso creado, y su `id` lo genera la capa de acceso: quien llama no lo informa. `GET`, `GET /:id`, `PUT` y `DELETE` contestan **`200`**; `GET /:id`, `PUT` y `DELETE` sobre una fila que no existe, **`404`**. Y **`DELETE` contesta `200` con `data` nulo, no `204`**: el contrato de `api-shell` exige que toda respuesta de la API sea JSON con `status` y `message`, y un `204` no tiene cuerpo —sería la única respuesta capaz de romper la envoltura que la API se impone—.

### `409` para la clave foránea, y no `400`

La petición está bien formada; lo que ocurre es que contradice una relación que existe. Un `400` diría que está mal escrita, y no lo está. Y el `409` es **una red de seguridad, no el camino normal**: el manejador valida antes de escribir y da un mensaje preciso, así que solo se llega a él por una carrera —otra petición borró la fila referenciada entre la comprobación y el `INSERT`—.

### Los secretos no vuelven, y el que no se envía se conserva

La respuesta de un repositorio **no** trae `secret_access_key` ni `passphrase`, ni la de un target `db_password`: trae un **booleano** por credencial diciendo si está configurada. El panel no necesita el valor para pintar el formulario, y devolverlo convertiría cada `GET` en una fuga. En un `PUT`, un secreto **ausente** conserva el almacenado —el manejador lee la fila y lo reutiliza— y uno **vacío** se rechaza con `400` nombrando el campo: sin lo primero, un formulario que no rellena la contraseña la sustituiría por nada; sin lo segundo, ese mismo formulario recibiría el error de la base.

### El `Deserialize` de `SecretString` es manual, y `Serialize` no se implementa

`SecretString` guarda su valor en un `Zeroizing<String>`, y el `Deserialize` de `Zeroizing` vive tras la feature `serde` de `zeroize` —dependencia **opcional**, hoy apagada—. Encenderla resolvería el `derive`, pero unificaría esa feature para **todo** el grafo que usa `zeroize` —`crypto-bigint`, `curve25519-dalek`, `der`, `ed25519-dalek`, `elliptic-curve`, `generic-array`, `jsonwebtoken`, `num-bigint-dig`, `rsa`, `rustls`, `rustls-pki-types`, `sec1`, `p256`, `p384`, `ecdsa`, `primeorder`— por una necesidad local. La implementación manual pasa por `SecretString::new` y deja la invariante donde se audita. Y **`Serialize` no se implementa**: es lo que hace que una respuesta que llevara una credencial **no compile**, así que la fuga deja de depender de que nadie se equivoque.

### El formato del cable

Los identificadores viajan como **texto** con guiones (`uuid` está declarado sin `serde`, así que un `Uuid` no se puede serializar y la trampa del BLOB del §3 no puede reaparecer en el cable), las fechas como **cadena RFC3339** formateada por nosotros, y los nombres de los campos son **los del §3** en `snake_case`. No se enciende la feature `serde-human-readable` de `time`: formatear por nuestra cuenta deja el formato del cable como parte del contrato en vez de como consecuencia de una feature, y hay una prueba que fija el literal.

### Sin dependencias nuevas

Medido: `axum` 0.8.9 trae `json` y `query` en sus features por defecto y este `Cargo.toml` no las desactiva; `serde` con `derive` ya está; y `tower` y `http-body-util` ya están en `[dev-dependencies]`. `Cargo.toml` y `Cargo.lock` quedan **sin tocar**, y la tarea 0.2 lo verifica con `cargo tree --offline`: si hiciera falta una dependencia, se para y se reporta antes de seguir.

## Trampas encontradas y candadeadas

- **`SecretString` no implementa `Deserialize`**, y su `Zeroizing` interno tampoco sin encender una feature de `zeroize` que se unificaría en dieciséis paquetes del árbol. La implementación es **manual** y pasa por `SecretString::new`.
- **El `Serialize` de `time` no produce una cadena** en este crate: `serde-human-readable` es una feature **aparte** en `time` 0.3.55 y no está activada. Se formatea con `OffsetDateTime::format(&Rfc3339)`, y el literal medido para UTC es `2026-10-25T12:34:56Z` —emite `Z` y **omite la fracción** cuando los nanosegundos son cero—, así que la prueba fija ese literal en vez de suponerlo.
- **El texto del rechazo de `serde` puede ecoar el valor del campo**: `{"keep_daily": "AKIA…"}` produce un mensaje con la credencial dentro. Por eso el rechazo se traduce a un mensaje **propio** y el texto del intérprete no entra ni en el cuerpo **ni** en el registro.
- **Con la autenticación apagada el router no monta la mitad de negocio**: hoy devuelve solo la mitad pública, así que las quince rutas nuevas contestarían `404` y `just dev` no serviría para nada. Se montan públicas y sin capa en ese modo, que es lo que `api-shell` ya dice del modo de desarrollo.
- **El orden de las dos líneas del router es semántico**: `method_not_allowed_fallback` va **antes** de colgar el *gate*, o un `405` sale sin haber exigido sesión y revela qué métodos existen.
- **De una violación de clave foránea no se sabe cuál de las dos referencias falló**: `backup_jobs` tiene dos y SQLite contesta `FOREIGN KEY constraint failed` sin nombrar ninguna. El mensaje del `409` no adivina.
- **La guarda del directorio de prueba borra al caer y el pool la sobrevive**: si el ayudante del arnés construye la base y no devuelve con el estado la guarda que la mantiene viva, el fichero desaparece con la conexión abierta. La guarda tiene que viajar con el estado hasta el final de la prueba.
- **`PUT` es reemplazo completo**: lo que no venga en el cuerpo vuelve a su valor por defecto, con la salvedad de los secretos. No hay `PATCH`.
- **El `DELETE` de un repositorio arrastra sus tareas y con ellas sus ejecuciones** por el `ON DELETE CASCADE` del §3: el histórico desaparece con el repositorio. Queda escrito para que nadie lo descubra vaciando la tabla.

## Módulo 3 — el plan de `/executions`

### Qué entra

`GET /api/v1/executions` — la séptima entrega del §5 y la última del change. Es un recurso **de solo lectura**: el §5 no le da `POST` ni `/:id`, y no se le añaden. El histórico lo escribe el **motor** por la capa de acceso —`alta`, `actualizar` y `borrar` ya existen desde `db-schema` y siguen sin ruta—, no un cliente HTTP: una ejecución no se «crea» desde el panel, la crea una copia que corre.

| Fichero | Qué se le hace |
|---|---|
| `src/api/executions.rs` | **nuevo**: el DTO de la consulta, el DTO del cable, el manejador, la validación y las ocho pruebas |
| `src/api/extract.rs` | el `Query` propio, con el rechazo traducido a la envoltura |
| `src/db/entities/execution.rs` | `Estado::texto`/`Estado::desde_texto`, `FiltroDeEjecuciones`, `SQL_LISTAR_FILTRADO`, `listar_filtrado` y su prueba |
| `src/api/mod.rs` | `pub mod executions;` y el `merge` en las **dos** ramas |
| `src/api/particion.rs` | una prueba: ejecuciones sigue la partición del resto |

### Decisiones

**1) El filtro vive en la capa de acceso, y su SQL es una sola cadena estática.** El `WHERE` de dos filtros opcionales se escribe `WHERE (?1 IS NULL OR job_id = ?1) AND (?2 IS NULL OR status = ?2)` con los valores **enlazados**, y no componiendo la consulta en piezas: la tarea 3.2 prohíbe que una consulta se construya concatenando, y una capa que decidiera el `WHERE` según los filtros presentes sería exactamente eso. Los cuatro parámetros van siempre al mismo sitio de la **misma** sentencia.

**2) El orden es `started_at DESC, id`.** El descendente es el del contrato —lo último primero, que es lo que el panel pinta—, y el `id` desempata hacia arriba. **No se toca `SQL_LISTAR`**, que sigue en ascendente: su orden es el del change anterior y hay pruebas que afirman lo suyo. La sentencia nueva es otra constante.

**3) El vocabulario del estado viaja en la entidad, no en el DTO.** `Estado` gana `texto()` y `desde_texto(...)`, la pareja que `TipoDeTarget` ya tiene con `texto_del_tipo`/`tipo_del_cable`. Sin ellas, el DTO tendría que conocer los cuatro literales —`pending`, `running`, `success`, `failed`— y sería el segundo sitio que los escribe: el `CHECK` del §3 es el primero y `Estado::texto` el único otro.

**4) `deny_unknown_fields` en la consulta, igual que en el cuerpo.** Un `?limits=5` que se ignorara en silencio devolvería el listado por defecto —cincuenta filas— y quien llamó creería haber acotado. Medido antes de decidirlo: `serde_urlencoded` 0.7.1 **sí** lo respeta, porque el `deserialize_identifier` de la clave va a `visit_borrowed_str`, y es la clave desconocida la que revienta.

**5) Los parámetros de consulta también tienen mensaje propio.** El `Query` propio de `src/api/extract.rs` cierra la mitad que le faltaba a la tarea 2.11: `QueryRejection` es `#[non_exhaustive]` con una sola variante —`FailedToDeserializeQueryString`—, así que la traducción lleva su brazo comodín, y el aviso al registro dice **que** no se pudieron interpretar los parámetros y nunca su texto, por la misma razón medida que en el cuerpo: el `Display` del rechazo lleva el valor de dentro.

**6) El rango de `limit` es `1..=200`, y el de `offset` es `>= 0`.** Fuera de rango es un `400` que **nombra el campo y dice el rango**, y no un recorte silencioso. Un `limit=0` se rechaza: un listado vacío pedido a propósito y un recorte son indistinguibles para quien lo lee.

**7) `?limit=` —presente y vacía— es `400`, y no el valor por defecto.** Es la misma regla que la del `BIND_ADDRESS` y el `RUST_LOG` del arranque: presente-y-vacía no es lo mismo que ausente, y caer al defecto escondería un formulario mal construido. Lo hace cumplir el intérprete, porque `serde_urlencoded` deserializa el valor vacío como `visit_some` y `""` no parsea a `i64`.

**8) `error_message` sale por el cable tal cual, y es deuda del motor.** El DTO publica las nueve columnas del §3; `error_message` es una de ellas y el panel la necesita para decir por qué falló una copia. Quien la escribe es el motor, y **el motor no debe volcar credenciales ahí**: queda declarado.

### Las ocho pruebas del listado

| Tarea | Prueba | Qué fija |
|---|---|---|
| 1.38 | `el_listado_sin_filtros_devuelve_las_ejecuciones_con_sus_metricas` | `200`, las tres filas y las cuatro claves de métricas |
| 1.39 | `los_filtros_por_job_id_y_por_status_acotan_el_listado` | cada filtro por su lado y los dos a la vez |
| 1.40 | `el_limite_acota_y_el_desplazamiento_avanza` | `limit=1` y `limit=1&offset=1` dan filas **distintas** |
| 1.41 | `un_limite_fuera_de_rango_o_no_numerico_contesta_400_sin_recortar` | `limit=201` y `limit=abc` → `400`, y no hay recorte en silencio |
| 1.42 | `dos_llamadas_seguidas_dan_el_mismo_orden_con_filas_empatadas` | dos `started_at` iguales, mismo orden, desempate por `id` |
| 1.47 | `las_fechas_de_una_fila_de_ejecucion_son_cadenas_rfc3339` | las dos son cadena, se releen y ninguna es número |
| 1.25 | `un_parametro_de_consulta_mal_formado_contesta_400_con_la_envoltura` | `?limit=abc` → envoltura y **sin** el texto del intérprete |
| — | `un_parametro_de_consulta_desconocido_se_rechaza` | `?limits=5` → `400`: es la decisión 4 |

Más una prueba en `particion.rs` —`el_listado_de_ejecuciones_sigue_la_particion_del_resto`— y una en la capa de acceso —`el_listado_filtrado_acota_y_ordena_por_la_fecha_descendente`— que fija el SQL directamente.

### El ciclo, medido

**RED**: `src/api/executions.rs` con los DTO, el router declarado y los manejadores en `todo!()`, con el `Query` **de axum** y no el propio. Las ocho pruebas del listado y la de partición en rojo, y las dos del parámetro por un motivo que vale la pena medir: con el extractor de axum, `?limit=abc` contesta `400` en **texto plano** y con el texto del intérprete dentro —que es el defecto que la 1.25 mide—.

**GREEN**: el `Query` propio, el `listar_filtrado` de la capa de acceso y el manejador. Las **dos** pruebas de la capa de acceso no pueden ser rojas de compilar —el método todavía no existe y no hay binario que medir—, así que acompañan al verde: se dice aquí para que no parezca que se escribieron después para que pasaran.

### Esperado

203 pruebas hoy. Con las diez nuevas: **213 passed; 0 failed** en el binario y **1 passed** en la de integración, con las cuatro puertas limpias. Casillas: **1.25, 1.38–1.42, 1.47, 2.8 y 2.11** —**nueve**—, que dejan **62 de 99**. Las `0.1–0.7` y la `2.15` quedan para la fase de cierre, donde se verifican con `git diff` y con las cuatro puertas.

## Auditado y corregido

### Módulo 1 — el estado, los tipos del cable y la traducción de los errores

RED medido: **157 passed; 5 failed** de 162 en el binario —los dos rojos de `src/api/repositories.rs` y los tres de `src/error.rs`— y **1 passed** en la de integración. GREEN: **162 passed; 0 failed** más la de integración. Tras el incremento correctivo: **164 passed; 0 failed** más la de integración.

La auditoría independiente encontró **un defecto de seguridad bloqueante** y lo midió **en vivo** contra el binario: el extractor `axum::Json` rechaza **antes** de entrar al manejador y su rechazo por defecto sale en `text/plain`, con `422` y con el valor del campo **ecoado dentro**. El director lo reprodujo sobre su propio binario y en un puerto limpio, con el cuerpo entero igual a la credencial —`Failed to deserialize the JSON body into the target type: invalid type: string "AKIAIOSFODNN7EXAMPLE", expected struct …`— y con la credencial como nombre de un campo de más. El registro del servidor **no** la llevaba: la fuga era al cliente.

Corrección: un extractor propio, `src/api/extract.rs`, que envuelve a `axum::Json` para quedarse con su camino real de extracción —tipo de contenido, lectura del cuerpo y deserialización— y traduce el rechazo a la envoltura: `400` con `{status, message}` y un mensaje **nuestro** por clase de fallo, más un `tracing::warn!` que deja constancia de **que** una petición no se pudo interpretar, **sin** su texto. Lo usan los dos manejadores con cuerpo. Cerrado con las pruebas 1.23 y 1.24 y verificado otra vez en el cable real: `400` con `application/json`, cero apariciones de la credencial en el cuerpo **y** en el registro, y el alta válida intacta en `201`.

Los demás hallazgos de esa auditoría, en corto:

- **Una laguna de cobertura** que explicaba el bloqueante —ninguna prueba miraba el **tipo de contenido** de un rechazo, así que un `400` en texto plano pasaba inadvertido—: cerrada por 1.23 y 1.24.
- **Una justificación falsa** en el doc-comment del booleano por credencial —decía que el formulario, el cuerpo y la columna se llaman igual, y el cuerpo informa `secret_access_key`, no `has_secret_access_key`—: reescrita, y la tensión entre el requisito que manda llamar a los campos como las columnas del §3 y el booleano del requisito 8 queda **declarada como añadido al SRS** en el `proposal` y en la spec, en vez de quedar como una contradicción.
- **La precisión del doc-comment de `contenido`**: afirmaba que la causa de `serde_json` no lleva el valor, y eso no es una garantía del crate sino de estos tipos de respuesta; ahora lo dice así.
- **Un hallazgo rechazado en el módulo 1 y revertido en el módulo 2, con el motivo escrito**: en el módulo 1 rechacé registrar los `400` de validación del manejador —«un formulario rellenado mal es el desenlace normal de un `400`»—, y era un error de lectura del contrato: el escenario «Un rechazo de validación no pone el cuerpo en el registro» del delta `secrets` pide por escrito que **quede la traza del campo que no cumplía**, además de que no quede la credencial ni el cuerpo. Lo destapó la prueba 1.22 al nacer en rojo con el registro vacío, y se revirtió: hay traza del campo en `AppError::InvalidInput` y en `traducir_base`. Lo que sigue rechazado es lo que de verdad pedía mi argumento —registrar el **valor** o el **cuerpo**—, y eso no se hace en ningún punto. La lección queda escrita porque es la que se repite: la decisión se tomó leyendo un requisito y no el escenario completo, y quien la corrigió fue una prueba, no una relectura.

### Módulo 2 — los tres recursos, la validación y la partición

RED y GREEN medidos, recurso a recurso: `/repositories` **164 passed; 7 failed** → **171 passed; 0 failed**; `/targets` **171/9** → **180/0**; `/jobs` **182/6** → **188/0**. Después, el incremento de cobertura transversal: las pruebas del cable llevaron el binario a **194/0**; las de secretos nacieron con **196/2** —las dos rojas por la segunda mitad de su escenario, el registro vacío— y quedaron en **198/0**; la partición, en **202/0**; y el `Debug` del DTO del cable, en **203/0**. Más **1** prueba de integración, siempre verde, y las cuatro puertas limpias en cada paso.

**El hallazgo de este módulo, y es el que más ha valido la fase de pruebas**: el delta `secrets` exige por escrito que el registro diga **qué** ocurrió —con la identidad del recurso— y nunca el contenido del cuerpo, con dos escenarios: que un alta deje la traza de que **se atendió**, y que un rechazo de validación deje la del **campo que no cumplía**. Medido contra el binario: el registro estaba **vacío**. Los manejadores no registraban nada, así que la red que el `SecretString` tiende sobre el camino deserializado no cubría el registro en absoluto —ni la mitad que exige que quede constancia, ni la que prohíbe el cuerpo—. Cerrarlo costó tres cambios: un middleware `from_fn` sobre el router que registra método, ruta y código de la respuesta a `debug` y **nada** del cuerpo —en un solo sitio y no en cada manejador, porque son quince rutas y quince llamadas a mano son quince sitios donde olvidarse—; la traza del motivo en `AppError::InvalidInput`; y la del campo en `traducir_base`, que **no** era un extra: el `400` del secreto vacío en `/repositories` no pasa por el manejador —lo rechaza la capa de acceso con `DbError::SecretoVacio`— así que instrumentar solo la primera rama dejaba sin traza el rechazo más probable de un formulario. Esto lo descubrió el subagente al medir, no al leer: la tanda quedó en 197/1 y el fallo decía, con el registro en la mano, que la traza del campo no estaba.

Los demás hallazgos del módulo, en corto:

- **La asimetría del secreto vacío entre los dos recursos**, que conviene tener escrita: en `/targets` el validador del manejador rechaza el vacío y contesta `InvalidInput`; en `/repositories` no lo valida y baja hasta la capa de acceso, que lo rechaza con `SecretoVacio`. El desenlace para el cliente es **el mismo** —`400` con el mismo texto, que nombra el campo—, pero el camino por dentro no lo es, y eso es justo lo que hizo que la instrumentación tuviera que estar en los dos sitios. No se «unifica» porque cada sitio defiende lo suyo: el manejador, que el tipo del target sea coherente; la capa de acceso, que no entre un secreto vacío por ningún camino.
- **La búsqueda en los bytes ya existía** para el camino de la capa de acceso (`src/db/mod.rs`); la del módulo 2 la extiende al camino **completo**, con la credencial entrando por el `POST` —el camino real del panel—, y reutiliza la misma idea de control: se comprueba que el control **sí** se encuentra antes de afirmar que la credencial no.
- **La conservación del secreto se prueba descifrando**, nunca comparando sobres: `Targets::actualizar` vuelve a cifrar y el nonce es aleatorio por valor, así que el sobre **cambia en cada escritura** aunque el valor no cambie. Comparar bytes habría dado un rojo falso.

### Módulo 3 — el listado de ejecuciones, la ventana y el `Query` propio

RED y GREEN medidos: **203 passed; 9 failed** → **213 passed; 0 failed** en el binario, más **1 passed** en la de integración, y las cuatro puertas limpias en los dos extremos. Los nueve rojos, en dos grupos que conviene no confundir: **seis** —`1.38` a `1.42` y `1.47`— por el `todo!()` del manejador, que en el binario de pruebas revienta como el `panic!` de la propia prueba; y **tres** —`1.25`, la del parámetro desconocido y la de la partición— por **el defecto que la `1.25` existe para medir**: con el `Query` **de axum** el rechazo no sale por la envoltura, sino en `text/plain` y con el texto del intérprete dentro, así que leerlo como JSON revienta. El RED de esta fase, por tanto, no fue un `todo!()` disfrazado: fue el rechazo real, medido contra el extractor equivocado.

**El `Query` propio cierra la mitad que le faltaba a la `2.11`.** La tarea habla del cuerpo **y de los parámetros**, y los parámetros llegaban con este módulo: `src/api/extract.rs` gana un `Query<T>` que envuelve al de axum y traduce su rechazo a la envoltura, con un mensaje nuestro y sin el texto del intérprete —ni en el cuerpo ni en el registro—. Dos hechos medidos antes de escribirlo, y los dos importan: `QueryRejection` es `#[non_exhaustive]` con **una sola** variante, así que la traducción lleva un brazo comodín que hoy no cubre nada y está para que una variante nueva de axum caiga por el mismo camino en vez de romper la compilación; y `serde_urlencoded` 0.7.1 **sí** respeta `deny_unknown_fields` —el `deserialize_identifier` de la clave va a `visit_borrowed_str`—, que es lo que permite rechazar un `?limits=5` en vez de ignorarlo en silencio y devolver el listado por defecto mientras quien llamó cree haber acotado.

**Un fallo de este módulo fue mío, y lo destapó la verificación, no la implementación.** La prueba de la capa de acceso que yo dicté —`el_listado_filtrado_acota_y_ordena_por_la_fecha_descendente`— salió **flaky**: medida diez veces seguidas, falló **siete**. La causa no estaba en el código —el SQL y el orden de la sentencia eran correctos— sino en la aserción: la prueba siembra dos filas **con la misma** `started_at` para ejercitar el desempate, el orden entre ellas lo decide el `id` —que es **aleatorio** en cada ejecución—, y yo afirmé `todas[0].bytes_processed == 300`, que es medir el azar del UUID y no el contrato. Corregido afirmando el **conjunto** de las empatadas y dejando el desempate a las aserciones por identificador, que ya comparaban contra el orden ordenado y eran correctas. Y la corrección se verificó como se verifica un fallo intermitente —con **doce** pasadas seguidas de la prueba sola, todas verdes, y la suite entera verde **dos** veces—, porque una pasada verde suelta no prueba nada cuando el fallo es de uno de cada tres. Queda escrito porque el error es de **diseño de la prueba** y no del código: una prueba que siembra un empate no puede afirmar cuál de las dos empatadas va primero cuando el desempate es aleatorio.

Los demás hallazgos del módulo, en corto:

- **El `?limit=` presente y vacío contesta `400` y no cae al valor por defecto**, y lo hace cumplir el intérprete: `serde_urlencoded` deserializa el valor vacío como `visit_some` y `""` no parsea a `i64`. Es la misma regla que el `BIND_ADDRESS` y el `RUST_LOG` del arranque —presente y vacía no es lo mismo que ausente—, y aquí sale gratis.
- **La ventana no se recorta.** Un `limit` fuera de `1..=200` y un `offset` negativo contestan `400` nombrando el campo, y el mensaje dice el rango con el tope y **nunca** repite el valor que llegó. Recortar habría dejado a quien pidió creyendo que recibió lo que pidió.
- **El contrato no pide escritura para `/executions`, y no se le añade.** El requisito de la superficie nombra los **tres** recursos con cinco rutas —`/targets`, `/repositories` y `/jobs`—; las ejecuciones tienen su propio requisito, que habla del listado. El histórico lo escribe el **motor** por la capa de acceso, cuyas tres escrituras siguen sin ruta, así que la única ruta del recurso es la del §5 y no hay añadido que declarar aquí.
- **`error_message` se publica tal cual, y es deuda del motor**: queda declarada abajo, con su motivo.

Contabilidad: **nueve** casillas —`1.25`, `1.38`–`1.42`, `1.47`, `2.8` y `2.11`—, que dejan **62 de 99**. Las `0.1`–`0.7` y la `2.15` siguen abiertas para el cierre, donde se comprueban con `git diff` y con las cuatro puertas.

## Verificación

*(Se rellena al cerrar. Son las puertas del repositorio —`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked` y `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`— más las diez comprobaciones de operador de la fase 4 del `tasks.md`, que esta vez van con `curl` **de verdad** contra el binario —ya hay endpoints— y con `sqlite3` sobre el fichero: los altas y las lecturas de los tres recursos, la columna del secreto guardando el sobre, el `NULL` del `db_name` de un directorio, la `passphrase` que **no** cambia cuando no se envía, las credenciales buscadas literalmente en los bytes del fichero, el `limit` fuera de rango, la cascada del borrado y el `401` sin cookie en modo `Oidc`.)*

## Deuda declarada

- **Los tres endpoints del §5 que no se entregan**: `POST /jobs/:id/run` y `GET /events` llegan con el **motor**, y `GET /snapshots` con **Rustic**. Cada uno con su motivo escrito en el `proposal` y en la spec.
- **No hay agregados ni `/stats`**: «el historial **y métricas**» del §5 se lee como que cada fila lleva sus métricas —los dos contadores de bytes y sus dos fechas—, y quien sume totales es el panel. Un `/stats` sería una segunda fuente de verdad que mantener en sincronía con los listados.
- **No se filtra por repositorio en `/executions`**: el índice existe desde `db-schema`, pero el `JOIN` con `backup_jobs` y su filtro los añade el change que lo pida.
- **El listado de los recursos no tiene paginación**: solo `/executions` la lleva, que es la tabla que crece. Con muchas filas, `GET /targets` devuelve todo, y acotarlo es una tarea de quien lo necesite.
- **`PUT` es reemplazo completo, sin `PATCH`**: el panel siempre tiene el objeto entero porque lo ha leído antes.
- **Los ayudantes internos de la capa de acceso siguen copiados en las cuatro capas** (heredado de `db-schema`).
- **Las más de treinta construcciones de `AppState`** que hereda el arnés se ajustan a la firma nueva, y los siete ayudantes pasan a ser asíncronos: es un cambio ancho, mecánico y verificado por las 156 pruebas que ya existen, que tienen que seguir verdes afirmando lo mismo.
- **Los tres manejadores que siguen sin implementar —`listado`, `actualizar` y `borrar` de `/repositories`— revientan si se alcanzan por la red**: son `todo!()` y llegan con el módulo 2. Medido: sin `CatchPanicLayer`, el `panic!` deja caer la tarea de la conexión, el cliente ve la conexión cerrada sin respuesta (`curl` contesta `000`) y el pánico sale en el registro del servidor. Queda declarado mientras dure la fase, para que nadie lo confunda con un fallo del servidor: ningún binario entregado debe tener un camino declarado y sin implementar.
- **Los `debug!` de la instrumentación solo se ven con `RUST_LOG=debug` o `trace`** y el valor por defecto es `info`, así que en un despliegue sin tocar la variable no queda rastro de las peticiones atendidas. Es deliberado —una línea por clic del panel es ruido de operación a `info`— pero conviene que sea una decisión y no una sorpresa: quien depure un problema en producción tendrá que subir el nivel, y eso está escrito aquí.
- **`error_message` sale por el cable tal cual, y el motor no debe volcar credenciales ahí.** Es una de las nueve columnas del §3 y el panel la necesita para decir por qué falló una copia, así que el listado la publica. Quien la escribe es el **motor**, y el §4.A manda la contraseña del volcado por la línea de comandos del `exec` —el cuarto cabo suelto del SRS, todavía abierto—, así que la salida de un volcado fallido puede traerla dentro: cuando llegue el motor, `error_message` es un sitio donde una credencial **no** puede acabar. Se declara aquí para que no se descubra con una fuga.

## Cierre

- [ ] `tasks.md` completo (obligatorio: `openspec archive` se niega si queda una casilla sin marcar).
- [ ] `docs/FEATURE-004-api-endpoints.md` con lo que realmente quedó construido.
- [ ] La cuenta corregida a **diez** endpoints en `AGENTS.md`, `plans/PLAN-003-COMPLETED.md` y `docs/FEATURE-003-db-schema.md`.
- [ ] `AGENTS.md`: el estado —los módulos que siguen vacíos son cuatro— y la superficie HTTP nueva.
- [ ] Redactar el `Purpose` de `openspec/specs/api-resources/spec.md`: es capability **nueva** y no puede quedarse en `TBD`.
- [ ] `openspec archive api-endpoints`.
- [ ] Renombrar este fichero a `plans/PLAN-004-COMPLETED.md` con `just plan-done 4`.
- [ ] PR `feature/api-endpoints` → `development` con el CI en verde. **Sin release a `main`**: se publica solo hasta `development`.
