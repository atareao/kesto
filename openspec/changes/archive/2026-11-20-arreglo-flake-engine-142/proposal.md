# Proposal

## Why

`engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total` falla de forma intermitente —una vez cada muchas corridas, y con más probabilidad en la suite completa bajo carga— aunque el test pasa 5/5 en aislamiento (issue #142). La aserción que cae es la del caso «sin total»: `bytes_processed` vale `0` cuando se esperaban `500`.

La causa está en el test, no en el producto. El trabajador escribe la fila terminal (`src/engine/ejecucion.rs:201`) y **después** publica el evento final (`:206`); ese orden es deliberado y lo fija el §5 («la fila queda constancia antes del aviso»). Pero la prueba se sincroniza con la **fila** —`esperar_el_final` sondea la base cada 10 ms (`src/engine/mod.rs:2380-2411`)— y acto seguido vacía el receptor con `recoger` (`:3116-3133`, un `try_recv` que no espera). Entre la escritura de la fila y la publicación del evento hay una ventana: si el test lee en esa ventana, `eventos.last()` es el último evento `running` —con `percentage=None` y `bytes_processed=0`— y no el evento final. No es que el porcentaje se calcule mal: `subidos=500`, `total=2000` → `25.0` es determinista (`src/engine/ejecucion.rs:1222-1227`); lo que falla es **cuándo** se lee el stream.

La fila y el stream son **dos caminos con orden no garantizado entre sí para el observador**: ver la fila terminal no implica haber recibido el evento final. La prueba asume lo contrario.

## What Changes

- **Solo pruebas** (`src/engine/mod.rs`, módulo `#[cfg(test)]`): un ayudante que espera en el **stream** —no en la fila— hasta observar un evento con `status ∈ {Success, Failed}`, con plazo acotado, y solo entonces recolecta. Las dos llamadas a `recoger` que aseveran sobre el evento final (`:3233` y `:3293`) pasan a usar ese ayudante, de modo que `eventos.last()` es siempre el evento terminal.
- El ayudante es reutilizable por cualquier prueba que hoy lea el stream tras sincronizarse con la fila, y su espera tiene plazo para que un fallo se manifieste como un mensaje claro y no como un cuelgue.
- Se fija por especificación el invariante del que depende el arreglo: el trabajador escribe la fila terminal **antes** de publicar el evento final, y ambos coinciden en `status` y cifras.

## Non-goals

- NO se toca el producto: el orden «fila, luego evento» se conserva tal cual lo fija el §5. Reordenarlo para publicar el evento antes de la fila cambiaría esa semántica y se descarta.
- NO se cambia el cálculo del porcentaje (`src/engine/ejecucion.rs:1222-1227`): es determinista y correcto.
- NO se sube ningún plazo de producto ni se retira ninguna aserción: se conserva la cobertura actual y solo se corrige el punto de sincronización.
- NO se introduce ninguna dependencia nueva (build offline y versiones pinadas).

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `engine`: se añade el requisito que hace explícito el invariante de cierre —la fila terminal se escribe antes de publicar el evento final y ambos coinciden en `status` y cifras—, y del que se deriva que quien mida el evento final debe **esperar en el stream, no en la fila**.
