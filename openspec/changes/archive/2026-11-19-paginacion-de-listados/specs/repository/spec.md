# repository Specification

## MODIFIED Requirements

### Requirement: Los snapshots se listan desde el repositorio y no desde la base

`GET /api/v1/snapshots?repository_id=<id>` SHALL obtener los snapshots preguntándole al **repositorio**, y NO SHALL deducirlos de las filas de `job_executions`. SHALL admitir la paginación `limit` y `offset` con los mismos límites y validación que el resto de listados —`200` de tope, `50` por defecto, `400` fuera de rango o no numérico— y SHALL devolver una envoltura con `snapshots`, `total` y `truncated`. Cada snapshot SHALL informar su identificador, su fecha, sus rutas, su etiqueta y su resumen de tamaño y número de ficheros.

`total` SHALL ser el número **exacto** de snapshots del repositorio, no el tamaño de la página ni una aproximación recortada: el paginador del panel pinta el total y el número de páginas a partir de él. Para obtenerlo, la ruta SHALL enumerar el listado del repositorio; esa enumeración es costosa, así que `limit` SHALL acotar **lo que se devuelve** —la página—, no lo que se enumera para contar.

`truncated` SHALL conservarse como aviso: SHALL valer `true` cuando la enumeración se haya topado con un tope alto de seguridad y `total` pueda por eso quedar corto, y `false` en caso contrario. El panel SHALL presentar la lista como completa solo cuando `truncated` sea `false`.

La lectura de la lista SHALL estar acotada también en **tiempo**: cuando el destino no responda, la ruta SHALL fallar rápido con el error tipado de destino inalcanzable en vez de agotar el retroceso exponencial de la biblioteca, y el error SHALL llegar al panel con su motivo legible para que pueda ofrecer reintentar. Acotar la espera es lo que evita que una vista que depende del S3 quede «cargando» durante minutos: un destino caído es un desenlace, no una espera.

El §5 llama a esta ruta «explorador de snapshots en el almacenamiento S3», y esa frase decide de dónde sale la respuesta: del almacenamiento. La base sabe qué copias **se intentaron**; el repositorio sabe qué copias **están**. Son dos listas distintas y la diferencia es justamente lo que hace útil el explorador: un snapshot que está en el repositorio y no en el histórico es una copia que alguien hizo con otra herramienta, y una ejecución en el histórico sin snapshot en el repositorio es una copia que se dio por buena y no está. Deduplicar la segunda lista de la primera escondería ese caso.

Enumerar el repositorio entero es la parte más cara de esta ruta —hay que abrir el repositorio y recorrer su índice—, y es el precio de un `total` exacto: la alternativa, contar solo la página o cortar a una ventana, dejaría al paginador mintiendo sobre cuántas páginas hay. Por eso `limit` acota la respuesta y la enumeración de conteo no: quien pregunte recibe el total real y la página que pidió, o un aviso explícito (`truncated`) si se topó con la guarda de seguridad.

#### Scenario: El listado sale del repositorio
- **GIVEN** un repositorio que contiene snapshots creados por Kesto y alguno creado por otra herramienta
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** SHALL listarse los dos
- **AND** NO SHALL listarse solo los que tienen fila en el histórico

#### Scenario: El listado identifica el repositorio del que sale
- **GIVEN** un identificador de repositorio que no existe
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** SHALL contestar `404` con la envoltura

#### Scenario: El listado va acotado
- **GIVEN** un repositorio con más snapshots que una página
- **WHEN** se pide el listado con `limit`
- **THEN** la página devuelta NO SHALL superar `limit`
- **AND** la respuesta SHALL indicar cuántos hay en total

#### Scenario: La ventana acota el número y no promete antigüedad
- **GIVEN** un repositorio con varios snapshots
- **WHEN** se pide el listado
- **THEN** lo devuelto SHALL venir ordenado de más reciente a más antigua
- **AND** el subconjunto devuelto NO SHALL prometerse como el de los más recientes

#### Scenario: Cada snapshot trae lo que pide el panel
- **GIVEN** un repositorio con al menos un snapshot
- **WHEN** se pide el listado
- **THEN** cada elemento SHALL traer su identificador, su fecha, sus rutas, su etiqueta y su resumen

#### Scenario: Un repositorio que no responde acota la espera del listado
- **GIVEN** un repositorio cuyo destino no responde
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** la petición SHALL fallar en un tiempo **acotado** por el tope, no tras los 36,9 s de reintentos medidos
- **AND** el error SHALL llegar con su motivo legible
- **AND** NO SHALL dejarse la petición colgada hasta agotar el retroceso exponencial de la biblioteca

#### Scenario: El total es exacto y la página va acotada por limit
- **GIVEN** un repositorio con más snapshots que una página
- **WHEN** se pide el listado con `limit=10`
- **THEN** `snapshots` NO SHALL traer más de diez elementos
- **AND** `total` SHALL ser el número exacto de snapshots del repositorio
- **AND** `truncated` SHALL ser `false`

#### Scenario: offset avanza por las páginas
- **GIVEN** un repositorio con más snapshots que una página
- **WHEN** se pide el listado con `limit=10` y después con `limit=10&offset=10`
- **THEN** la segunda página SHALL traer snapshots distintos de los de la primera

#### Scenario: Un limit fuera de rango contesta 400
- **GIVEN** el router con una sesión válida
- **WHEN** se pide el listado con `limit=201` y con `limit=abc`
- **THEN** las dos SHALL contestar `400` con la envoltura
