# FEATURE-033 — Paginación con total en Histórico, Snapshots y Restauraciones

**Issue:** [#140](https://github.com/atareao/kesto/issues/140) · **Plan:** `plans/PLAN-033.md` · **Change SDD:** `openspec/changes/paginacion-de-listados/`

## Qué es

Los tres listados del panel —Histórico, Snapshots y Restauraciones— dejan de paginar a ojo y pasan a paginar **en servidor** con un **total** que devuelve el backend. Antes el Histórico y Restauraciones arrastraban botones «Anterior/Siguiente» con heurísticas frágiles (el «siguiente» se apagaba cuando la página venía incompleta) y Snapshots no paginaba en absoluto: cortaba a una ventana fija de 200 y solo avisaba con `truncated`.

La forma nueva es la del paginador típico: **total de elementos**, **número de páginas** y **selector de tamaño** (10 por defecto; 10/20/50), alimentado por `limit` + `offset` + `total` del backend y sin paginar nunca en cliente. El contrato sigue en `limit`/`offset` —no se introduce `page`/`page_size`— y el backend conserva su `limit` por defecto de 50 mientras el panel manda su tamaño de página explícito.

El cambio de contrato del listado de ejecuciones es **BREAKING** (ver *Decisiones* y *Cómo se despliega*).

## Ficheros implicados

**Backend**
- `src/api/executions.rs` — el listado pasa de array pelado a `{ executions, total }`; `total` cuenta las filas que casan con los filtros (`status`, `job_id`), ajeno a `limit`/`offset`.
- `src/db/entities/execution.rs` — `PaginaDeEjecuciones { filas, total }` y las dos consultas gemelas `SQL_LISTAR_FILTRADO` / `SQL_CONTAR_FILTRADO`, ejecutadas en la **misma transacción**.
- `src/db/entities/mod.rs` — reexporta `PaginaDeEjecuciones`.
- `src/api/snapshots.rs` — `ConsultaDelCable` gana `limit`/`offset`; `ListadoDelCable` gana `total`; las constantes `LIMITE_POR_DEFECTO`, `LIMITE_MAXIMO` y `LA_GUARDA`.
- `src/engine/mod.rs` — la firma `listar_snapshots(acceso, guarda)`: el motor enumera el listado del repositorio y lo acota en la guarda que le pasa la ruta. El flujo de streaming del SRS §4 no cambia y `src/rustic/**` no se toca.
- `tests/e2e/arnes.rs` — adaptado al sobre `{ executions, total }`.

**MCP**
- `kesto-mcp/src/server.rs` — `list_snapshots` manda `limit=200` fijo (constante `LIMITE_DE_SNAPSHOTS`).
- `kesto-mcp/tests/herramientas.rs` — fija que la herramienta pide la ventana del tope.

**Frontend**
- `ui/src/components/Paginador/Paginador.tsx` — componente compartido sobre `Pagination` de Antd.
- `ui/src/services/api/types.ts` — los tipos del contrato nuevo.
- `ui/src/api/{ejecuciones,snapshots}.ts` — los servicios mandan `limit`/`offset` y devuelven `total`.
- `ui/src/hooks/{claves,useEjecuciones,useSnapshots}.ts` — los filtros con `limit`/`offset` entran en la **clave de caché** de React Query.
- `ui/src/pages/{historico,snapshots,restauraciones}.tsx` — las tres pantallas pintan el paginador y piden la página al servidor (`pagination={false}` en la tabla: nunca paginan en cliente).
- `ui/src/components/TablaDeTareasEnCurso`, `ui/src/components/TarjetasDeResumen`, Dashboard — consumidores adaptados al contrato nuevo de ejecuciones.
- `ui/src/hooks/useEspacioEnS3.ts` — declara la cifra de espacio como **mínimo** cuando la suma es parcial.

## Cómo funciona

1. **Contratos.** `GET /api/v1/executions` devuelve `{ executions: [...], total: N }`, con `total` = filas que casan con los filtros, ajeno a `limit`/`offset` (una petición con `status=failed` cuenta las fallidas, no todas). `GET /api/v1/snapshots` admite `limit`/`offset`, con `50` por defecto y `200` de tope, y devuelve `{ snapshots, total, truncated }`, con `total` **exacto**. `GET /api/v1/restorations` no cambia: ya publicaba `total`.

2. **El conteo va en la misma transacción que la página.** En ejecuciones, `SQL_CONTAR_FILTRADO` (`COUNT(*)`) y `SQL_LISTAR_FILTRADO` comparten el `WHERE` entero y se ejecutan dentro de un `begin()` común (`design.md`, decisión 2). Sin esa transacción, entre las dos consultas podría entrar una escritura y el `total` y la página contarían conjuntos distintos.

3. **La guarda de 1000 en snapshots.** El `total` exacto exige **enumerar** el listado del repositorio, que es la parte más cara de la ruta. La enumeración se corta en `LA_GUARDA = 1_000` (el motor acota los identificadores **antes** de resolverlos, porque resolver es una lectura por instantánea); `limit` acota **lo que se devuelve** —la página—, no lo que se enumera para contar. Cinco páginas del tope: bastante para que ningún paginador razonable lo alcance, poco para que el peor caso tenga final.

4. **`truncated` cambia de significado.** Ya no significa «la página viene recortada» —eso lo acota `limit`—, sino «la enumeración tocó la guarda», con `total` posiblemente corto. En el caso normal vale `false` y el total es exacto. La validación de `limit`/`offset` es la **primera** sentencia del handler: un parámetro mal formado (`limit` en `{0, -1, 201, no numérico}` o `offset` negativo) contesta `400` **sin** llegar a enumerar nada.

5. **El paginador y el apaño de `totalParaAntd`.** `Paginador.tsx` recibe `total`, `pagina` y `tamanoDePagina`, usa `showSizeChanger` con `10/20/50` (10 por defecto), `showTotal` con plural correcto y `locale` español, y al cambiar de tamaño vuelve a la página **1**. El paginador se pinta cuando `total > 0 || pagina > 1` —para no dejar al operador sin control si la retención o la purga vacían el conjunto estando en una página alta— y va **fuera** de la rama del estado vacío, que usa `TEXTO_DE_PAGINA_VACIA` cuando `pagina > 1`. Detalle no obvio: `rc-pagination` desactiva el item «1» cuando `total === 0` (su guarda `isValid` exige `total > 0`), así que a Antd se le declara `totalParaAntd = total === 0 ? pagina * tamanoDePagina : total`, mientras el recuento visible sigue saliendo del `total` **real** vía `showTotal`.

6. **La clave de caché lleva `limit`/`offset`.** Los servicios y hooks meten la paginación dentro de la clave de React Query (`ui/src/hooks/claves.ts`), de modo que cada página es una entrada distinta y cambiar de página **refetchea** en vez de servir la anterior. Las tres pantallas piden al servidor con `limit` + `offset = (pagina − 1) × tamanoDePagina`.

7. **Consumidores del contrato de ejecuciones.** El MCP consume `total` como campo extra y no lo necesita; el e2e lee `filas["executions"]`. `useEspacioEnS3.ts` declara el espacio como **mínimo** cuando la suma es parcial, y lo mira por **dos** señales: `truncated` del payload **o** `snapshots.length < total` —no solo la comparación, para no depender de que la guarda siga por encima del `limit: 200` que pide el propio hook—.

## Variables / configuración

Ninguna variable de entorno nueva. Los valores que gobiernan la paginación son constantes de código:

| Constante | Valor | Dónde | Para qué |
|---|---|---|---|
| `LIMITE_POR_DEFECTO` | `50` | `src/api/{executions,snapshots}.rs` | `limit` cuando la petición no lo informa. El panel manda `10` explícito y no depende del defecto del backend |
| `LIMITE_MAXIMO` | `200` | `src/api/{executions,snapshots}.rs` | Tope de `limit`. Fuera de rango → `400`, no recorte en silencio |
| `LA_GUARDA` | `1_000` | `src/api/snapshots.rs` | Tope de la **enumeración** que sostiene el `total` exacto de snapshots |
| `LIMITE_DE_SNAPSHOTS` | `200` | `kesto-mcp/src/server.rs` | `limit` fijo que `list_snapshots` manda a la API. **Copia deliberada** de `LIMITE_MAXIMO` |

`LIMITE_DE_SNAPSHOTS` es una copia a propósito: el paquete raíz `kesto` es un **binario sin `src/lib.rs` ni `[lib]`**, así que su `LIMITE_MAXIMO` no es enlazable como dependencia y el crate `kesto-mcp` no puede importarlo. Si cambia el tope del backend hay que **acompañar la constante del MCP a mano**.

## Cómo se despliega

Sin pasos especiales. Entra con el binario: no hay migración de datos ni de esquema, y no hay receta `just` ni variable nueva. El cambio de contrato es visible desde el **primer arranque**: cualquier consumidor del array pelado de `GET /api/v1/executions` —el MCP de una versión anterior, un script propio— deja de parsear la respuesta y tiene que leer `{ executions, total }`. Backend, MCP y panel se despliegan **juntos**; el rollback es revertir el PR (la API vuelve al array pelado y el panel a los botones).

## Decisiones

- **D1 — La misma envoltura que Restauraciones.** `{ executions, total }` en vez de una cabecera `X-Total-Count` o un modo `with_total=true`: restauraciones ya usaba la envoltura y la asimetría era el problema, no la solución. Es un cambio **BREAKING** del contrato de ejecuciones.
- **D2 — El conteo en la misma transacción que la página.** `design.md`, decisión 2: sin transacción común, el `total` puede desincronizarse de la página.
- **D3 — `total` exacto en snapshots a cambio de enumerar.** La alternativa —contar solo la página o cortar a una ventana— deja al paginador mintiendo sobre cuántas páginas hay. `limit` acota la respuesta; la enumeración de conteo acota con `LA_GUARDA`.
- **D4 — `truncated` = «la enumeración tocó la guarda».** Deja de significar «página recortada»; pasa a avisar de que el `total` puede quedar corto. Consecuencia **deliberada y documentada**: con **exactamente** 1000 instantáneas informa `truncated: true` y `total: 1000` aunque no haya más; de los dos errores posibles se eligió el que no esconde copias.
- **D5 — El defecto del backend (50) no es el del panel (10).** El panel manda siempre su tamaño de página explícito; así el contrato del backend no cambia más de lo necesario y quien no pase `limit` sigue viendo 50.
- **D6 — La UI nunca pagina en cliente.** La tabla va con `pagination={false}` y el paginador traduce cada elección a un `limit`/`offset` que pide la página de verdad; un recorte en cliente daría un número de páginas falso con un total correcto.
- **D7 — El MCP pide `limit=200` fijo.** El input de `list_snapshots` es `deny_unknown_fields` sin `limit`/`offset`; con el nuevo defecto de 50 habría pasado de ver 200 a ver 50 sin poder paginar. Se pide el tope para restaurar el comportamiento previo. El **contrato publicado** de la herramienta no cambia.

## Follow-ups

- **`restauraciones` sin la transacción del conteo.** Hace sus dos consultas (página y total) sueltas contra el pool, sin la transacción que sí usa `executions`: asimetría **cosmética**, misma ventana de inconsistencia que la decisión 2 quiso cerrar en ejecuciones.
- **Parpadeo al cambiar de página.** Se evitaría con `placeholderData: keepPreviousData` en los tres hooks; hoy se ve un instante de carga entre páginas.
- **Paginación desde el MCP.** Los agentes MCP no pueden paginar el listado de instantáneas (`list_snapshots` ve la ventana del tope y no ofrece `offset`); queda como issue de seguimiento.
- **Flakes ajenos, ya abiertos como issues.** **#141** (e2e `POST /api/v1/repositories` → `502` bajo carga) y **#142** (`engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total`).
