# Proposal — recogida-de-temporales-de-prueba

## Why

Las pruebas de Kesto crean un directorio temporal por prueba y **no lo recogen todo**: medido en este repositorio, el directorio temporal del sistema llegó a tener **17.375 directorios `/tmp/kesto-prueba-*` huérfanos**, con `df -i /tmp` al **100 %** de inodos (1.048.115 usados de 1.048.576, 461 libres) mientras el espacio en disco seguía sobrando. El fallo es **acumulativo y miente**: no aparece hasta que se agota, y mientras tanto se persigue en el sitio equivocado —`panicked at src/test_support.rs:135: el directorio temporal de la prueba debe crearse: Os { code: 28, kind: StorageFull }`, y los fallos **crecen con cada corrida** (45 en la primera pasada, 149 en la siguiente, sobre 531 tests)—. En el CI de GitHub no se nota (runner limpio cada vez); solo golpea en local. Hoy, tras limpiar a mano una vez, vuelve a haber **884** restos.

La causa está **medida, no supuesta**. `directorio_de_prueba()` (`src/test_support.rs:133-139`) crea `/tmp/kesto-prueba-<uuid>` y su `Drop` (`src/test_support.rs:123-129`) lo borra... **solo en el camino feliz**. Hay dos vías por las que ese `Drop` no pasa:

- **El arnés retiene guardas en un `static`.** `GUARDAS_DEL_ARNES: OnceLock<Mutex<Vec<DirectorioDePrueba>>>` (`src/test_support.rs:749-758`) guarda los directorios del arnés —los que entrega `destino_local_de_prueba` (`:402`) y `base_del_arnes` (`:770`)— para que el pool que los usa no se quede sin su directorio a mitad de una prueba. Un `static` **nunca ejecuta `Drop`**, y el propio comentario lo asume: «los directorios del arnés **no se borran** … quedan en el directorio temporal del sistema» (`src/test_support.rs:744-748`). Es decir: **el grueso de los restos se filtra incluso en el camino feliz** (≈64 guardas retenidas por corrida).
- **La muerte abrupta del proceso.** Si un test **panica**, si se **mata** la suite (un `--ignored` abortado, un `Ctrl-C`, un `timeout`), o si el proceso muere, los `Drop` pendientes no corren y los directorios se quedan.

El arnés **e2e** ya resolvió exactamente este problema para sus contenedores y su patrón está probado en este repositorio: etiqueta al creador (`ETIQUETA_DEL_CREADOR = "kesto-e2e-pid"`, `tests/e2e/arnes.rs:90`), barrido al arrancar de lo que no está vivo (`recoger_los_restos`, `tests/e2e/arnes.rs:401-445`) y comprobación de proceso vivo por `/proc/<pid>` (`proceso_vivo`, `tests/e2e/arnes.rs:833-839`). Este change lleva ese mismo patrón al arnés de las pruebas unitarias.

El issue #94 propone cuatro vías; este change decide cuáles se implementan (ver Decisiones) y deja escrito el porqué.

## What Changes

- **Una raíz única por ejecución.** Los directorios temporales de prueba SHALL colgar de `/tmp/kesto-prueba-<pid>/` (el pid del proceso de pruebas), y NO SHALL crearse entradas `kesto-prueba-*` sueltas en el primer nivel del directorio temporal. Cada prueba SHALL seguir teniendo su **propio subdirectorio** bajo esa raíz, para no perder el aislamiento.
- **Barrido de restos al arrancar.** Al arrancar la batería, el arnés SHALL barrer las raíces `/tmp/kesto-prueba-*` de ejecuciones anteriores cuyo proceso **ya no esté vivo**, preguntando por `/proc/<pid>` (el mismo criterio que el e2e), y NO SHALL tocar las raíces de un proceso vivo. El barrido SHALL correr **una sola vez por proceso**.
- **La limpieza NO SHALL depender del `Drop`.** La retención de guardas en `GUARDAS_DEL_ARNES` (`src/test_support.rs:749-758`) se **mantiene** —existe para no reintroducir un `404` intermitente—, pero deja de ser la única limpieza: la raíz por ejecución y el barrido al arrancar acotan la acumulación a, como mucho, la raíz de la ejecución en curso. Un `Drop` no cubre ni un `static` retenido ni una muerte abrupta, y esos son los caminos que producen los restos.
- **Receta `just clean-tmp`.** `.justfile` SHALL ofrecer una receta que borre los restos `kesto-prueba-*`, y `docs/COMANDOS.md` SHALL explicarla.
- **Aviso de inodos bajos, no bloqueo.** Si al arrancar quedan pocos inodos libres en el directorio temporal, el arnés SHALL dejar constancia —sin fallar por ello—, de modo que un fallo en masa se persiga donde está la causa.
- **Sin dependencias nuevas.** El arnés usa solo `std` (`std::process`, `std::path`, `std::fs`, `/proc`): el build es **offline** y las versiones están pinadas a la caché local, así que no se añade ni una caja (ni `ctor`, ni `atexit`, ni `libc`).

## Capabilities

### New Capabilities

- `arnes-de-pruebas` — el arnés de las pruebas de Kesto: los ayudantes que crean, aíslan y recogen lo que una prueba necesita —un directorio temporal, una base SQLite migrada, una clave—, y que al terminar la batería no dejan acumularse en el sistema lo que es suyo. Nace de este change; los dos requisitos nuevos son «Los directorios temporales de prueba no se acumulan en /tmp» y «El arnés no agota los inodos en silencio».

### Modified Capabilities

Ninguna. El comportamiento de producto no cambia: esto es el andamiaje de las pruebas.

## Decisiones

- **D1 — Una raíz por ejecución (`/tmp/kesto-prueba-<pid>/`), con un subdirectorio por prueba.** Decisión de diseño de este change (propuesta 2 del issue #94). Agrupa lo de una corrida en una sola entrada de primer nivel y hace que **un barrido o un borrado sean de un golpe**, sin perder el aislamiento entre pruebas (cada una conserva su directorio, solo que anidado). Alternativa descartada: seguir creando un directorio suelto por prueba —es lo que ya hay, y hace que el número de entradas de primer nivel crezca con cada test—. El pid va en el nombre para poder distinguir un proceso vivo de uno muerto al barrer.
- **D2 — Barrido al arrancar por pid vivo, con `/proc/<pid>`.** Decisión de diseño (propuesta 1 del issue #94). Reutiliza el patrón ya probado del arnés e2e (`kesto-e2e-pid` + `proceso_vivo`) y **ataca la causa raíz**: ni un `static` ni una muerte abrupta ejecutan `Drop`, así que la limpieza no puede depender del `Drop`; tiene que ser **recogida al arrancar**. El barrido corre una sola vez por proceso, igual que el del e2e. Alternativa descartada: confiar en el `Drop` de cada guarda —es el mecanismo que hoy falla en el camino feliz y en el abrupto—.
- **D3 — La retención de guardas se mantiene; deja de ser la única limpieza.** Decisión de diseño. `GUARDAS_DEL_ARNES` existe porque un directorio borrado antes de que su pool termine provoca un `404` sobre una fila que existe —un fallo intermitente invisible—. Se conserva; lo que cambia es que **ya no es el único mecanismo de limpieza**: la raíz por ejecución + el barrido al arrancar garantizan que nada se acumule. Alternativa descartada: quitar la retención —reintroduce el fallo intermitente que la motivó—.
- **D4 — Receta `just clean-tmp` y su entrada en `docs/COMANDOS.md`.** Decisión de diseño (propuesta 3 del issue). Quien se encuentre el agotamiento ha de poder salir de él sin adivinar. Toca `.justfile` y `docs/COMANDOS.md`.
- **D5 — El aviso de inodos bajos avisa, no bloquea.** Decisión de diseño (propuesta 4 del issue). El defecto del issue no es que el disco esté lleno, sino que la suite **no lo dice** y el error aparece disfrazado («No space left on device» en un sitio que no es). El arnés SHALL dejar constancia de la situación y NO SHALL fallar por ella: un umbral que bloquee convertiría un aviso útil en un falso fallo en máquinas con poco `/tmp` pero sanas. Alternativa descartada: no avisar —se deja el diagnóstico a merced de un mensaje que miente—.

## Impact

- **Afecta a:** `src/test_support.rs` (raíz por ejecución, barrido, aviso), `.justfile` (receta `clean-tmp`), `docs/COMANDOS.md` (documentar la receta). Las pruebas que usan `directorio_de_prueba()`/`base_del_arnes()` no cambian de forma: siguen recibiendo una ruta utilizable.
- **Riesgo:** el barrido borra directorios bajo `/tmp`. Ha de acotarse **estrictamente** a entradas `kesto-prueba-*` cuyo pid no esté vivo, sin tocar nada más del directorio temporal; un barrido demasiado ancho sería peor que el problema. La prueba de la raíz viva (pid propio) cubre ese borde.
- **No toca** producto, contrato HTTP, base de datos, panel ni despliegue.
