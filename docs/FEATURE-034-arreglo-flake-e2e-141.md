# FEATURE-034 — Arreglo del flake e2e: `POST /api/v1/repositories` → 502 bajo carga

- **Issue:** #141
- **Plan:** `plans/PLAN-034-COMPLETED.md`
- **Change OpenSpec:** archivado (`openspec/changes/archive/2026-11-20-arreglo-flake-e2e-141/`)
- **Capability:** `arnes-de-pruebas`

## Qué es

La batería e2e fallaba de forma intermitente con `502` en el alta de repositorio cuando los escenarios corrían en paralelo: el `init` de rustic contra un SeaweedFS recién arrancado (Argon2 + escritura de config/keys/índice) supera el tope fijo `TOPE_DEL_DESTINO = 10 s` bajo carga. No era un fallo de producto —el `502` es la respuesta correcta ante un destino inalcanzable— sino una calibración del arnés: reintentar el alta es seguro porque un `502` deja **cero** filas escritas (la fila se escribe después de abrir el destino). El arreglo vive **solo** en el arnés e2e.

## Ficheros implicados

- `tests/e2e/arnes.rs` — único código de producción tocado: constante `INTENTOS_DEL_ALTA` y `ESPERA_ENTRE_ALTA`, helper puro `reintentar(...)`, uso en `crear_repositorio`, y cinco `#[tokio::test]` del helper.
- `openspec/specs/arnes-de-pruebas/spec.md` — requisito nuevo *«El alta de un repositorio tolera un destino aún no listo bajo carga»* (3 escenarios), fusionado al archivar el change.
- `openspec/changes/archive/2026-11-20-arreglo-flake-e2e-141/` — propuesta, tareas y delta archivados.

## Cómo funciona

- `reintentar(intentos, espera, intento)` es un bucle **puro** que recibe el «intento» como clausura async `Fn() -> Fut` y devuelve `(StatusCode, Value)`. Devuelve en cuanto el código **no** sea `502`; si es `502` y quedan intentos, espera con **backoff exponencial** (`espera`, luego `espera × 2`, …) y reenvía la misma petición; agotados los intentos devuelve el **último** `(502, cuerpo)` sin panic, para que el llamador falle en voz alta.
- `INTENTOS_DEL_ALTA = 3` y `ESPERA_ENTRE_ALTA = 2 s` (las esperas reales son 2 s y 4 s).
- `crear_repositorio` construye el cuerpo una vez, envuelve `pedir(POST, "/api/v1/repositories", …)` en la clausura, y al agotar los intentos hace `assert!` nombrando contexto, código y cuerpo.
- El bucle es testeable sin HTTP ni contenedores: las pruebas inyectan un stub con `Rc<Cell<usize>>` como contador.

## Variables y configuración

Ninguna nueva. Son constantes internas del arnés (`INTENTOS_DEL_ALTA`, `ESPERA_ENTRE_ALTA`); no hay variables de entorno ni cambios de despliegue.

## Cómo verificar

- Tests del helper (rápidos, sin contenedores): `cargo test --locked --features e2e --test e2e 'arnes::tests'` → 5 passed.
- Reproducción del flake (paralelo): `cargo test --locked --features e2e --all-targets` → **0 fallos** (verificado: e2e 28 passed / 0 failed).
- Suite oficial (serie): `just e2e` → **0 fallos** (verificado: 28 passed / 0 failed, `--test-threads=1`).
- Puerta habitual: `just check` (fmt + clippy + tests) → sin error.

## Decisiones

- **Reintento solo ante `502` exacto** (`StatusCode::BAD_GATEWAY`); cualquier otro código (incluidos otros `5xx`) devuelve de inmediato.
- **Seguro por diseño:** un `502` equivale a cero filas (`abrir_repositorio` precede al `INSERT`), así que un segundo intento no duplica nada.
- **Backoff exponencial** (2 s → 4 s) en vez de espera fija: acorta el caso bueno y cumple el SHALL «espera con backoff» del spec.
- **Descartado:** endurecer `esperar_el_s3` (no era el problema), subir `TOPE_DEL_DESTINO` (política de producto del #100) y un semáforo de contenedores (más invasivo).
- El test de backoff usa umbral `ESPERA × 3` (60 ms) para distinguir backoff real de una espera constante (40 ms), con aserción de cota inferior para no ser flaky por carga.

## Follow-ups

- **#142** — flake `engine::tests::el_porcentaje_es_nulo_sin_total_y_numerico_con_total` (siguiente).
- Los tests del helper viven tras `required-features = ["e2e"]`: solo los compila/corre el job e2e del CI, no el job `backend`.
