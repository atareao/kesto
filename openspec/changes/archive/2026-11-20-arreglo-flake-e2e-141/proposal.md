# Proposal

## Why

El escenario de alta de repositorio de la batería e2e falla de forma intermitente con `502` cuando la suite corre en paralelo (issue #141). Bajo carga —16 hilos— el `init` de rustic contra el SeaweedFS recién arrancado (Argon2 + escritura de config/keys/índice) tarda más de los 10 s del tope fijo `TOPE_DEL_DESTINO` y agota el plazo con `Error::DestinoNoAlcanzable` → `502`. No es un fallo de producto: el destino responde `404` a las sondas de readiness y el `init` solo se queda corto de tiempo. El CI no lo reproduce porque la suite oficial corre con `--test-threads=1`; el flake aparece al correr los escenarios en paralelo.

La fila no queda a medias: en el alta la fila de base de datos se escribe DESPUÉS de abrir el destino, así que un `502` equivale a cero filas escritas y **reintentar es seguro**.

## What Changes

- **Solo el arnés e2e** (`tests/e2e/arnes.rs`, `crear_repositorio`): el alta de repositorio reintenta de forma acotada ante un `502` —respuesta transitoria de un destino aún no listo bajo carga—, con backoff entre intentos, y devuelve los datos del alta en cuanto llega un `2xx`.
- El reintento se implementa como un bucle **puro y testeable** —recibe el «intento» como clausura y solo conoce el número de intentos, la espera y el `StatusCode`—, de modo que una prueba unitaria pueda simular un `502` seguido de un `200` sin levantar HTTP ni contenedores.

## Non-goals

- NO se endurece la sonda `esperar_el_s3` (`tests/e2e/arnes.rs:870`): no es el problema —el `stat` de `config` responde bien—.
- NO se sube `TOPE_DEL_DESTINO` (`src/rustic/repositorio.rs:1247`): es política de producto fijada en #100 y el plazo del cliente no debe usarse como red para una calibración de este arnés.
- NO se cambia el código de producto, el contrato HTTP, la base de datos ni el panel. El `502` sigue siendo la respuesta correcta ante un destino inalcanzable.
- NO se introduce un semáforo que limite contenedores concurrentes (alternativa más invasiva); el reintento acotado basta.

## Capabilities

### New Capabilities

_(ninguna)_

### Modified Capabilities

- `arnes-de-pruebas`: el alta de repositorio del arnés pasa a tolerar el `502` transitorio de un destino recién arrancado bajo carga mediante un reintento acotado con backoff, seguro porque un `502` deja cero filas escritas.
