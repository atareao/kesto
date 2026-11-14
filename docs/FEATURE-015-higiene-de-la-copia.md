# FEATURE-015 — Higiene de la copia

**Issues:** [#52](https://github.com/atareao/kesto/issues/52) y [#53](https://github.com/atareao/kesto/issues/53) · **Plan:** `plans/PLAN-015.md` · **Change OpenSpec:** `openspec/changes/higiene-de-la-copia/`

## Qué es

Cierra dos defectos encontrados al probar Kesto **contra un banco de pruebas real** (RustFS como repositorio S3, fuentes MariaDB/PostgreSQL/SQLite/directorio, copias y restauración con `restic`):

1. **Sin residuos (#52).** Una copia que no se da por buena **no deja** su instantánea en el repositorio.
2. **Volcados legibles (#53).** El nodo del volcado declara modo `0o644`, así que un `.sql` restaurado se puede leer sin `chmod`.

## Ficheros implicados

- `src/engine/ejecucion.rs` — `copiar_el_volcado`: las **tres** ramas de error posteriores al archivado olvidan la instantánea antes de devolver el cierre `failed`; helper puro `descartar_copia_fallida` y `olvidar_la_instantanea`.
- `src/rustic/repositorio.rs` — nuevo método `Repositorio::olvidar_instantanea(&self, id: &str) -> Result<(), Error>`.
- `src/rustic/error.rs` — variante `Error::OlvidoFallido { destino, motivo }`.
- `src/rustic/origen.rs` — el nodo del volcado se construye con `Metadata { mode: Some(0o644), ..Default::default() }`.
- `src/engine/mod.rs` — dos pruebas e2e `#[ignore]` que recorren el cableado con un contenedor de verdad.
- `docs/DEFINITION.md` (§4.B) — nota de la retirada y del modo del volcado restaurado.

## Cómo funciona

### #52 — La instantánea huérfana

La biblioteca de archivado (`rustic_core`) **silencia** el error del lector y sigue, devolviendo `Ok` y **guardando la instantánea igualmente** —sin el volcado dentro—. El motor ya no hereda ese `Ok`: comprueba `EntregaDelVolcado::completa()` y marca la ejecución `failed`. Lo que antes no hacía era **retirar** la instantánea que la biblioteca ya había escrito.

Ahora, en cuanto la copia **no** se da por buena **después** de que el archivado escribiera la instantánea, el motor la olvida:

- **Tres desenlaces** cubiertos: fallo del productor (`Ok(Err(motivo))`), pánico del hilo del productor (`Err(_)`) y entrega incompleta (`el_volcado_se_entrego` → `Err`).
- La retirada es de **mejor esfuerzo**: va en `descartar_copia_fallida`, que llama a `olvidar_la_instantanea`, registra un `warn!` si falla y devuelve **siempre** `Cierre::fallida(motivo)` con el motivo del volcado. El fallo de la limpieza **no** sustituye al de la copia —el que el operador tiene que leer— ni tumba al worker.
- El camino `directory` **no** se toca: no tiene señal de completitud y no escribe instantánea antes de decidir.

`Repositorio::olvidar_instantanea` recibe el hex **completo** (el de `CopiaHecha::id`, que es lo que publica el listado), lo convierte a `SnapshotId` y llama a `delete_snapshots`. Un identificador que no sea un hex de 64 caracteres sale como `Error::OlvidoFallido`, nunca como pánico.

### #53 — El modo del nodo del volcado

El volcado **no existe** como fichero en ningún sistema de ficheros de Kesto —llega en streaming a la biblioteca—, así que su nodo se construye a mano y su `Metadata` por defecto dejaba `mode: None`. Ese modo queda grabado en la instantánea y lo hereda el fichero al restaurarlo. Con **restic** (que aplica el modo del nodo tal cual) un `mode: None` da un fichero en modo `000`; con rustic, `None` no da `000` —hereda el modo de creación—, pero tampoco es una garantía. El arreglo fija `mode: Some(0o644)` explícito, que es el modo correcto para ambas herramientas.

> **Ojo con el valor:** `0o644`, **no** `0o100644`. Restic interpreta el bit `S_IFREG` como bit de tipo de Go y el fichero saldría con un modo equivocado.

## Limitaciones conocidas

- **`node.meta.size = 0`.** Un volcado en streaming no conoce su longitud hasta el final y `rustic_core` no rellena el tamaño del nodo, así que la instantánea declara `size = 0` para un fichero con contenido. No impide el restore (el contenido se escribe igual), pero el atajo «fichero vacío ya existe» de un restore puede saltarse un destino de 0 bytes con el mismo nombre. Es una limitación de la biblioteca, no arreglable desde Kesto.
- El residuo de un olvido que **falle** (destino inaccesible) queda hasta la próxima purga: `delete_snapshots` retira la **entrada** de la instantánea, no sus paquetes; el espacio no vuelve hasta el `prune` de una retención posterior.

## Cómo habilitar y desplegar

Sin cambios de API, de esquema ni de variables de entorno. Es un arreglo de comportamiento del motor.

## Verificación

- `just check-all` verde (fmt, clippy, 445 pruebas de unidad + 5 de integración, panel: 128 pruebas y build).
- Pruebas e2e (requieren un motor de contenedores, marcadas `#[ignore]`):
  - `una_copia_que_no_se_da_por_buena_no_deja_su_instantanea` — `mariadb-dump` que sale con 1; la fila queda `failed` y el repositorio con **0** instantáneas.
  - `una_copia_con_un_contenedor_que_no_esta_no_deja_su_instantanea` — contenedor inexistente; cubre la rama del productor.
  - Ambas **muerden** contra la mutación: quitar el olvido deja la instantánea y las pruebas fallan.
  - Ejecución: `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock cargo test -- --ignored una_copia`.

## Decisiones

- **Retirada de mejor esfuerzo, no bloqueante:** si el `forget` falla, se registra y la ejecución sigue `failed` por lo que de verdad pasó. No se convierte «no pude limpiar» en un error que oculta «no pude copiar».
- **Solo el camino del volcado:** el `directory` no tiene señal de completitud.
- **Modo `0o644`**, sin uid/gid (dejar `None` es lo correcto: el fichero queda a nombre de quien restaura y es legible).

## Follow-ups

- El residuo de un olvido fallido se limpia en la próxima purga. Si se quisiera cerrar antes, el motor podría reintentar el olvido o exponer una purga manual de instantáneas huérfanas.
