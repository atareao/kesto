# FEATURE-013 — Hardening del despliegue: HSTS opcional + backup del volumen de datos

**Issue:** [#44](https://github.com/atareao/kesto/issues/44) · **Plan:** `plans/PLAN-013-COMPLETED.md` · **Change OpenSpec:** `openspec/changes/archive/2026-11-14-deploy-endurecimiento/`

## Qué es

Cierra los dos follow-ups que quedaron abiertos en la feature de despliegue (FEATURE-008):

1. **HSTS opcional:** en el despliegue estándar el TLS lo termina un proxy, así que la cabecera `Strict-Transport-Security` es **del proxy**. Esta feature permite, además, que la emita la propia aplicación de forma **opcional y configurable**, para el caso en que el operador lo quiera.
2. **Backup del volumen de datos:** el volumen nombrado `kesto_data` (la SQLite) persiste, pero no había un procedimiento reproducible para respaldarlo y recuperarlo. Ahora hay una receta `just backup-datos` y su recuperación documentada.

## Ficheros implicados

- `src/config.rs` — constante `ENV_KESTO_HSTS_MAX_AGE`, campo `hsts_max_age: Option<u64>` en `Config` y su parseo.
- `src/api/mod.rs` — capa `SetResponseHeaderLayer` (tower-http, feature `set-header`) que añade la cabecera en el router raíz.
- `.justfile` — receta `backup-datos`.
- `.env.example` · `docs/ENTORNO.md` — documentación de la variable.

## Cómo funciona

### HSTS

- `KESTO_HSTS_MAX_AGE` (segundos, entero) es **opcional**:
  - **Ausente** → `hsts_max_age = None` → **ninguna** respuesta lleva la cabecera (defecto).
  - **Presente con un entero ≥ 0** → toda respuesta lleva `Strict-Transport-Security: max-age=<n>`.
  - **Presente pero vacía o no numérica** → **error de arranque** (`ConfigError::InvalidValue`), coherente con `BIND_ADDRESS` y `RUST_LOG`.
- La cabecera se añade con `SetResponseHeaderLayer::overriding` en el **router raíz**, no en la mitad protegida: así alcanza también a los `fallbacks` de `404` (ruta no declarada) y `405` (método no declarado). El valor se calcula una vez al construir el router a partir de `state.config.hsts_max_age`; con `None`, la capa no inserta nada.

### Backup del volumen

- `just backup-datos [destino]` (destino por defecto `respaldos/`):
  1. Localiza el volumen por sufijo `kesto_data` (cubre `kesto_data` de `just run` y `<proyecto>_kesto_data` de `podman compose`).
  2. Si un contenedor lo monta, lo **para** (SQLite y su WAL deben quedar quietos durante la copia).
  3. `podman volume export | gzip > <destino>/kesto-data-<sello>.tar.gz`.
  4. **Rearranca** el contenedor si estaba en marcha.
- El contenedor de Kesto atiende `SIGTERM` y se para limpiamente; un proceso que lo ignore recibirá `SIGKILL` tras 10 s (comportamiento de `podman stop`).

## Variables y configuración

| Variable | Obligatoria | Defecto | Para qué |
|---|---|---|---|
| `KESTO_HSTS_MAX_AGE` | no | desactivada | Segundos de la cabecera HSTS. Ausente → sin cabecera; entero ≥ 0 → `max-age=<n>` en toda respuesta; vacía/no numérica → error de arranque. |

## Cómo verificar

- `just check` — fmt + clippy + tests (incluye los 4 tests de config y los 2 de cabecera).
- `just backup-datos /tmp/resp` — produce un `.tar.gz` y deja el contenedor en marcha.

## Recuperación del respaldo

```sh
# Con el contenedor parado (la SQLite no debe estar en uso):
podman stop kesto-app
podman volume create kesto_data    # si no existe
gunzip -c kesto-data-<sello>.tar.gz | podman volume import kesto_data -
podman start kesto-app
```

## Decisiones

- **HSTS opcional, no incondicional:** sobre HTTP los navegadores ignoran la cabecera, y cuando hay un proxy que termina el TLS la HSTS es suya. Emitirla siempre desde la app sería ruido; el defecto es **apagada**.
- **La cabecera en el router raíz:** para que alcance a los `404`/`405` (que no pasan por la mitad protegida ni por su *gate*).
- **`podman volume ls -q` / `ps -q`** en la receta, en vez de `--format '{{.Name}}'`: evita el choque con la sintaxis `{{ }}` de `just`.
- **Parar el contenedor antes de exportar:** una copia del volumen con la SQLite a media escritura (o con WAL pendiente) no es un respaldo fiable.

## Follow-ups

- La rotación automática de respaldos y su subida a un remoto quedan fuera de alcance.
