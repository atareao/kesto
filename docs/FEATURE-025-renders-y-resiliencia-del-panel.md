# FEATURE-025 — Renders y resiliencia del panel

- **Issues**: #99, #100, #101 · **Plan**: `plans/PLAN-025.md` · **Change SDD**: `renders-y-resiliencia-del-panel`

## Qué es

Tres defectos de producto que aparecieron tras desplegar la **0.10.0**. Se corrigen en un mismo change porque comparten la misma superficie —el panel y lo que el backend le contesta— aunque tengan causas distintas: un render que pintaba el campo equivocado (#99), un destino S3 que podía dejar la vista «cargando» durante minutos (#100) y una consola que salía vacía para ejecuciones ya terminadas (#101).

## Por qué

### A (#99) — Dashboard: la columna «Tarea» pintaba el target

La tabla «Tareas activas» del Dashboard enseñaba en la columna «Tarea» el nombre del **target**, no el de la tarea. Es el mismo error de raíz que ya se cerró en Tareas y Snapshots en FEATURE-024, pero esta pantalla se quedó atrás: el operador leía un destino donde esperaba una tarea, y dos tareas sobre el mismo target salían indistinguibles. Ahora pinta `tarea.name`; ante `null` o blanco pinta un marcador atenuado «sin nombre» con el componente compartido `MarcadorSinNombre`. **Nunca** el target. De paso se retiró un `useTargets` que ya no alimentaba nada.

### B (#100) — Si el S3 no responde, el panel se quedaba «cargando»

Con un destino S3 caído o inalcanzable, las vistas que lo consultan podían quedarse girando minutos. Eran dos problemas sumados:

- **Backend**: las operaciones que **abren o listan** el destino no tenían tope propio. La biblioteca reintentaba por dentro con su política por defecto (`retry = 5` con backoff), lo que daba entre **36,9 y 38,4 s** antes de fallar, y un pánico de la biblioteca se traducía en un `500 TrabajoInterrumpido` en vez de un error de destino.
- **Frontend**: `useEspacioEnS3` trataba el fallo como todo-o-nada, así que un solo repositorio inaccesible borraba también las cifras que no dependen del S3; y las consultas que sí dependen de él heredaban la política de reintentos global del `QueryClient`, que multiplica la espera.

La corrección va en las dos capas: un **tope de 10 s** en Kesto alrededor de la apertura y el listado del destino, `retry=1` en el mapa de opciones del destino y, en el panel, fallo **por repositorio**, resumen que conserva lo que no depende del S3 y `retry` acotado en las consultas afectadas.

### C (#101) — Histórico: «Ver consola» de una ejecución terminada salía vacía

La consola de una ejecución **terminada** enseñaba un panel en blanco. No había nada que replayar: los eventos del stream no se persisten, así que una ejecución que ya acabó no tiene stream que leer. Ahora la consola enseña un **resumen**: el `error_message` si la ejecución terminó en `failed`; un texto claro si no hay eventos; y para `pending`/`running` mantiene el «todavía no hay eventos». Además la selección guarda el **id** y deriva la fila de la query viva, de modo que `status` y `error_message` se actualizan al refrescar. **No** se persiste ni se hace replay del stream: queda fuera de alcance.

## Ficheros implicados

### Backend

| Fichero | Papel |
|---|---|
| `src/rustic/repositorio.rs` | `con_tope` (hilo + `recv_timeout`), `TOPE_DEL_DESTINO = 10 s`, `REINTENTOS_DEL_DESTINO = 1`; las operaciones que abren o listan el destino pasan por el tope |
| `src/rustic/mod.rs` | Reexporta lo anterior |
| `src/engine/mod.rs` | `Motor::listar_snapshots` y `Motor::tiene_la_instantanea` usan las operaciones con tope |
| `src/api/repositories.rs` | Rustdocs: el tope y el `502` |
| `src/api/snapshots.rs` | Rustdocs: el peor caso 2×TOPE y el `502` |
| `src/test_support.rs` | Utilidades de test para las rutas con tope |

### Frontend

| Fichero | Papel |
|---|---|
| `ui/src/components/TablaDeTareasEnCurso/` | La columna «Tarea» pinta `tarea.name` y el marcador ante `null`/blanco |
| `ui/src/components/MarcadorSinNombre/` | Componente compartido del marcador atenuado |
| `ui/src/components/TarjetasDeResumen/` | Resumen del Dashboard que no es todo-o-nada; «al menos» cuando la cifra es un mínimo |
| `ui/src/components/ConsolaDeEjecucion/` | Resumen de la ejecución terminada (`error_message`, texto claro sin eventos) |
| `ui/src/hooks/useEspacioEnS3.ts` | `repositoriosCaidos`: fallo por repositorio |
| `ui/src/hooks/useSnapshots.ts` | `retry` acotado (`REINTENTOS_DE_S3 = false`) |
| `ui/src/pages/dashboard.tsx` | Aviso en `warning` que **nombra** el repo caído; usa las tarjetas resilientes |
| `ui/src/pages/historico.tsx` | La selección guarda el id y deriva la fila de la query viva |
| `ui/src/pages/tareas.tsx`, `ui/src/pages/snapshots.tsx` | `retry` acotado en las consultas que dependen del S3 |

Tests tocados en `ui/src/components/TablaDeTareasEnCurso/`, `MarcadorSinNombre/`, `TarjetasDeResumen/`, `ConsolaDeEjecucion/` y en los `*.test.tsx` de `dashboard`, `historico`, `tareas` y `snapshots`; en backend, los módulos de tests de `src/rustic/repositorio.rs` y `src/engine/mod.rs`.

## Cómo funciona

### El tope del destino (#100)

- **Dónde**: el tope se impone **en la capa de Kesto** (`con_tope`), no en la biblioteca. `con_tope` ejecuta la operación en un hilo y espera con `recv_timeout`; si el plazo se agota, el hilo se descarta y la llamada devuelve `Error::DestinoNoAlcanzable`. Un hilo que muere también cae en ese error.
- **Qué cubre**: las operaciones que **abren o listan** el destino —la apertura, el listado de snapshots y la pregunta por una instantánea del restore—. Las que solo usan un `Operator` ya construido no pasan por el tope.
- **Cuánto**: `TOPE_DEL_DESTINO = 10 s`. Es una constante de implementación, **no configurable** por entorno.
- **Reintentos**: el mapa de opciones del destino lleva `retry=1` (`REINTENTOS_DEL_DESTINO`). Antes el defecto de la biblioteca era `5` con backoff, que es lo que producía los **36,9–38,4 s**.
- **Desenlace**: un tope agotado o un hilo caído dan `Error::DestinoNoAlcanzable`, que el borde HTTP traduce a **`502`**. Antes, un pánico de la biblioteca daba `500 TrabajoInterrumpido`.
- **Peor caso de `GET /api/v1/snapshots`**: **2×TOPE** (apertura + listado), es decir ~20 s. En el caso real —destino caído— falla en la **apertura**, así que se queda en un tope.

### El panel resiliente (#100)

- **`useEspacioEnS3` modela el fallo por repositorio** (`repositoriosCaidos`): un repositorio caído no invalida la cifra de los demás.
- **El resumen del Dashboard no es todo-o-nada**: conserva las cifras que no dependen del S3 y avisa en `warning` **nombrando** el repositorio caído, para que el operador sepa cuál mirar.
- **La cifra de espacio se declara mínimo** («al menos») cuando la respuesta viene con `truncated` o cuando algún repositorio es ilegible: lo que se enseña es un suelo, no el total.
- **Las consultas que dependen del S3 llevan `retry` acotado** con la constante única `REINTENTOS_DE_S3 = false`, **sin** tocar la política global del `QueryClient` —que sigue sirviendo a las consultas que no hablan con el destino—.
- **Snapshots** ya tenía `EstadoDeError` con «Reintentar»; el «cargando sin fin» lo cierra el `retry` acotado, no un componente nuevo.

### El resumen de la consola (#101)

- **Ejecución terminada**: la consola pinta `error_message` si `status === 'failed'`; si no hay eventos y no hay error, un texto claro que dice que no hay nada que mostrar. Nunca un panel vacío.
- **Ejecución viva**: para `pending`/`running` se mantiene el «todavía no hay eventos».
- **Selección por id**: el estado de selección guarda el **id** y la fila se deriva de la query viva, así que `status` y `error_message` se actualizan al refrescar en vez de quedarse con la copia del primer render.
- **Sin persistencia ni replay**: los eventos no se guardan y no se reconstruye el stream. El resumen es lo que se puede contar con lo que hay.

## Variables y configuración

**Ninguna nueva.** `TOPE_DEL_DESTINO` y `REINTENTOS_DEL_DESTINO` son constantes de implementación, no variables de entorno; `REINTENTOS_DE_S3` es una constante del panel. La tabla completa de variables sigue en `docs/ENTORNO.md`.

## Decisiones

- **D2/B1 — el tope es de 10 s.** Lo fijó el operador: por debajo se cortarían destinos lentos pero vivos; por encima la vista vuelve a parecer colgada. Es un equilibrio de producto, no un número medido contra un S3 concreto.
- **D6 revisada — el tope va en la capa de Kesto.** `opendal 0.58.2` y `rustic_backend 0.7.0` **no admiten un tope como opción** y construyen el `Operator` por dentro. Con el build **offline** y las versiones **pinadas**, no hubo bump ni fork: se envuelve la llamada con `con_tope` (hilo + `recv_timeout`) en vez de tocar la biblioteca.
- **D12/B6 — la salud por repositorio, en forma completa, va a otro change.** Aquí la visibilidad llega hasta lo que el panel necesita para no mentir: el fallo por repositorio y el aviso que lo nombra. El campo dedicado, el endpoint y la pantalla de Ajustes quedan para un change aparte.
- **Parte C — resumen, no log completo.** Persistir los eventos o hacer replay del stream sería otro diseño (almacenamiento, retención, tamaño); el defecto era una consola vacía, y un resumen lo cierra sin abrir ese frente.
- **Los reintentos del panel se acotan por consulta, no en el `QueryClient`.** Bajar el `retry` global habría cambiado el comportamiento de todas las consultas para arreglar unas pocas; la constante `REINTENTOS_DE_S3` deja explícito cuáles son las que hablan con el destino.

## Follow-ups

1. **Change aparte para la salud por repositorio completa** (D12/B6): campo, endpoint y Ajustes.
2. **#88 (columna «Tarea» del Histórico) y #90–#94 siguen abiertos.** Este change no los toca.
3. **Unificar el marcador «sin nombre»/«sin tarea» en un solo componente.** Hoy `MarcadorSinNombre` cubre el Dashboard y Tareas; Snapshots usa su propio «sin tarea». Son el mismo patrón.
4. **Si algún día se quiere un deadline de cliente**, añadirlo con su test. Hoy no lo hay: la vista no se cuelga porque el backend acota a 10 s.

## Referencias

- Issues: **#99** (Dashboard: «Tarea» pintaba el target), **#100** (el panel se quedaba cargando si el S3 no responde), **#101** (Histórico: «Ver consola» vacía en ejecuciones terminadas).
- Change SDD: `openspec/changes/renders-y-resiliencia-del-panel/`.
- Plan: `plans/PLAN-025.md`.

## Verificación

- **Puertas**: `just check-all` (Rust + panel) en verde.
- **Destino caído**: con un repositorio S3 inaccesible, `GET /api/v1/snapshots` debe fallar en **~10 s** (no en 36,9 s) y la vista Snapshots debe enseñar el error con «Reintentar».