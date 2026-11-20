# FEATURE-035 — Arreglo del flake `engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total` (#142)

- **Issue:** #142 · **Change OpenSpec:** `arreglo-flake-engine-142` · **Plan:** `plans/PLAN-035.md`.
- **Tipo:** infraestructura de pruebas. **No** toca producto, contrato HTTP, base de datos ni panel.

## Qué es

El test `engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total` fallaba de forma intermitente —una vez cada muchas corridas, con más probabilidad en la suite completa bajo carga— con `bytes_processed == 0` cuando se esperaban `500`. En aislamiento pasaba 5/5. Este cambio lo elimina corrigiendo el punto de sincronización de la prueba, sin tocar el producto.

## Causa raíz

El trabajador escribe la fila terminal de la ejecución (`src/engine/ejecucion.rs:201`) y **después** publica el evento final en el stream (`:206`). Ese orden es deliberado y lo fija el SRS §5 («la fila queda constancia antes del aviso»).

La prueba, sin embargo, se sincronizaba con la **fila** —`esperar_el_final` sondea la base cada 10 ms (`src/engine/mod.rs`)— y acto seguido vaciaba el receptor con `recoger`, un `try_recv` que **no espera**. Entre la escritura de la fila y la publicación del evento hay una ventana: si el test leía en esa ventana, `eventos.last()` era el último evento `running` (con `percentage=None` y `bytes_processed=0`) y no el evento final.

No era un problema de cálculo: `subidos=500`, `total=2000` → `25.0` es determinista (`src/engine/ejecucion.rs:1222-1227`). Fallaba **cuándo** se leía el stream. La fila y el stream son dos caminos con orden no garantizado entre sí para el observador.

## Ficheros implicados

- `src/engine/mod.rs` (módulo `#[cfg(test)] mod tests`): nuevo ayudante `recoger_hasta_el_final` y sustitución de las dos llamadas a `recoger` que aseveraban sobre el evento final.
- `openspec/changes/arreglo-flake-engine-142/`: change SDD (proposal + delta de la capability `engine` + tasks).
- `docs/FEATURE-035-arreglo-flake-engine-142.md` (este documento) y `plans/PLAN-035.md`.

## Cómo funciona

`recoger_hasta_el_final(receptor, plazo)`:

1. Drena sin esperar lo que ya esté en el receptor (reutiliza `recoger`); si el último evento ya es terminal, devuelve la lista.
2. Si no, espera en el stream con `tokio::time::timeout` hasta observar un evento con `status ∈ {Success, Failed}`, saltando los avisos de restauración y los `Lagged` igual que `recoger`.
3. Devuelve la lista de eventos **terminando en el terminal**, de modo que `.last()` es siempre el evento final.
4. Si se agota el plazo sin evento terminal —o el hub se cierra— **falla en voz alta** nombrando lo que no llegó, en vez de devolver una lista sin final.

Las dos llamadas del test del issue pasan a usarlo, así que la prueba espera al **stream** y no a la fila.

## Decisiones

- **Arreglo en la prueba, no en el producto.** Se descartó reordenar el producto (publicar el evento antes de la fila): cambiaría la semántica del §5.
- **`recoger` se conserva.** En vez de borrarlo al quedar sin uso, el ayudante lo reutiliza para el drenaje inicial; así no queda código muerto (`clippy -D warnings`) y se mantiene el ayudante síncrono para quien solo quiera vaciar.
- **Plazo acotado.** Convierte un fallo —algo que nunca llega— en un mensaje, y no en un cuelgue de 90 s.
- **Invariante fijado en la spec.** La capability `engine` gana un requisito: la fila terminal se escribe antes de publicar el evento final, ambos coinciden en `status` y cifras, y quien mida el evento final debe esperar en el stream, no en la fila.

## Cómo verificar

```sh
# El test del issue, aislado
cargo test --locked --bin kesto \
  'engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total' -- --exact

# Los dos tests del ayudante
cargo test --locked --bin kesto 'engine::tests::el_ayudante'

# Anti-flake: 200 pasadas aisladas + módulo engine bajo carga (2 CPUs)
taskset -c 0,1 cargo test --locked --bin kesto 'engine::tests::'

# Los tres checks del CI
just check
```

Resultado medido: **0/200 fallos** en aislamiento y **43 ok / 0 failed** en `engine::tests` bajo carga.

> Nota: este crate es binario (`src/main.rs`); se usa `--bin kesto`, no `--lib`.

## Follow-ups

- Ninguno. El cambio es test-only y no deja deuda abierta.
