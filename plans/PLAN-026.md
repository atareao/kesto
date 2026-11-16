```markdown
# PLAN-026 — Recogida de temporales de prueba (issue #94)

> **Issue:** #94 — «Los tests dejan directorios temporales huérfanos en /tmp hasta agotar los inodos y tumbar la suite»
> **Change OpenSpec:** `recogida-de-temporales-de-prueba` (capacidad nueva `arnes-de-pruebas`)
> **Doc de cierre:** `docs/FEATURE-026-recogida-de-temporales-de-prueba.md`

## Contexto

Las pruebas de Kesto crean un directorio temporal por prueba y no lo recogen todo. Medido: el
directorio temporal llegó a tener 17.375 directorios `/tmp/kesto-prueba-*` huérfanos y `df -i /tmp`
al 100 % de inodos mientras sobraba espacio. El fallo es acumulativo, miente («No space left on
device» en `src/test_support.rs:135`) y tumba tests que no tienen nada que ver.

Causa medida, no supuesta:

1. El arnés retiene guardas en un `static` (`GUARDAS_DEL_ARNES`, `src/test_support.rs:749-758`), y
   un `static` nunca ejecuta `Drop`: el grueso de los restos se filtra **en el camino feliz**(≈64 guardas por corrida).
2. La muerte abrupta del proceso (panic, `Ctrl-C`, `timeout`, `--ignored` abortado) tampoco ejecuta
   los `Drop` pendientes.

El arnés e2e ya resolvió el mismo problema con etiqueta de pid + barrido por `/proc`
(`tests/e2e/arnes.rs:90,401-445,833-839`). Este plan lleva ese patrón al arnés de las pruebas.

## Objetivo

Que la batería de pruebas **no acumule** directorios temporales entre ejecuciones, que la
acumulación quede acotada como mucho a la raíz de la corrida en curso, que haya una forma
documentada de limpiar los restos, y que el arnés avise —sin bloquear— cuando queden pocos inodos.

## Alcance

- **Dentro:** `src/test_support.rs` (raíz por ejecución, barrido por pid vivo, aviso de inodos),
  `.justfile` (receta `clean-tmp`), `docs/COMANDOS.md` (documentarla).
- **Fuera:** producto, contrato HTTP, base de datos, panel, despliegue. **Sin dependencias nuevas**(build offline, versiones pinadas a la caché): solo `std`.

## Decisiones

- **D1** — Raíz única por ejecución `/tmp/kesto-prueba-<pid>/`, con un subdirectorio por prueba
  (aislamiento intacto, borrado de un golpe).
- **D2** — Barrido al arrancar por pid vivo con `/proc/<pid>`, una sola vez por proceso (patrón e2e).
- **D3** — La retención de guardas en `GUARDAS_DEL_ARNES` se mantiene (evita un `404` intermitente),
  pero deja de ser la única limpieza; la garantía la da el barrido.
- **D4** — Receta `just clean-tmp` documentada en `docs/COMANDOS.md`.
- **D5** — El aviso de inodos bajos **avisa, no bloquea**.

## Tareas (TDD)

### 0. Preparación
- 0.1 `just check` en verde antes de tocar nada.
- 0.2 Medir el estado de partida: número de `/tmp/kesto-prueba-*` y `df -i /tmp`.

### 1. Raíz por ejecución y barrido
- RED 1.1 test del barrido con base inyectada (borra el pid muerto, conserva el vivo).
- RED 1.2 test de que `directorio_de_prueba()` crea bajo la raíz de la ejecución.
- RED 1.3 test de que el barrido no borra entradas ajenas a `kesto-prueba-*`.
- GREEN 1.4 `proceso_vivo(pid)` + `recoger_los_restos_de_prueba(base)`.
- GREEN 1.5 raíz por pid perezosa + `directorio_de_prueba()` bajo ella.
- GREEN 1.6 barrido una vez por proceso (`OnceLock`).
- REFACTOR 1.7 rustdoc del porqué (no se confía en el `Drop`).

### 2. Aviso de inodos y limpieza documentada
- RED 2.1 aviso con umbral inyectable (deja constancia y no falla).
- RED 2.2 test de la receta `just clean-tmp`.
- GREEN 2.3 aviso no bloqueante; 2.4 receta `clean-tmp`; 2.5 entrada en `docs/COMANDOS.md`.

### 3. Verificación
- 3.1 `just check` en verde.
- 3.2 Matar una corrida a mitad y comprobar que los restos **no crecen** entre ejecuciones; `df -i /tmp` no crece.
- 3.3 `cargo fmt --check` y `cargo clippy --all-targets -- -D warnings` limpios.

### 4. Cierre
- 4.1 Marcar tareas y `just spec-archive recogida-de-temporales-de-prueba`.
- 4.2 `docs/FEATURE-026-recogida-de-temporales-de-prueba.md`.
- 4.3 `git mv plans/PLAN-026.md plans/PLAN-026-COMPLETED.md` y PR a `development` con `Closes #94`.

## Criterios de aceptación

- [ ] Matar la suite a mitad **no** deja restos que se acumulen: la siguiente corrida los recoge.
- [ ] `df -i /tmp` no crece entre ejecuciones consecutivas.
- [ ] Documentado cómo limpiar (receta `just clean-tmp`).
- [ ] `just check` en verde y sin dependencias nuevas.
- [ ] Esquiva el barrido: solo borra `kesto-prueba-*` de pids muertos.

## Nota de arranque

La propuesta de cambio (`openspec/changes/recogida-de-temporales-de-prueba/`) ha de estar **aprobada
por el operador** antes de empezar el RED, según el protocolo SDD de este repositorio.
```