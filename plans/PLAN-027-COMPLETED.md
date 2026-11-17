# PLAN-027 — Salud por repositorio (deuda D12/B6)

- **Issue:** [#111](https://github.com/atareao/kesto/issues/111)
- **Rama:** `feature/salud-por-repositorio` (desde `development`)
- **Change SDD:** `openspec/changes/salud-por-repositorio/`
- **Estado:** en curso

## 1. Objetivo

Cerrar la forma **completa** de la visibilidad de salud por repositorio (deuda **D12/B6**, diferida desde PLAN-025): que el panel sepa y muestre el estado de cada repositorio S3 sin tener que intentar una copia. Tres piezas, decididas con el operador:

1. **Persistencia** — columnas de salud en `repositories`.
2. **Comprobación** — endpoint que pone en marcha el repositorio (con el tope de 10 s ya fijado) y **lista sus instantáneas**, persiste el desenlace y lo devuelve.
3. **Indicador** — estado de salud por repositorio en **Ajustes** (con acción de comprobar) y visibilidad del estado persistido en el **listado del Dashboard**.

## 2. Desenlaces de salud (contrato)

| Estado | Significado | Origen |
|---|---|---|
| `ok` | El destino responde, abre y lista | poner en marcha = conectado + listado correcto |
| `sin_repositorio` | Responde pero no hay repositorio todavía | no hay fichero de configuración (sin inicializar) |
| `passphrase` | Existe y no abre | passphrase que no abre |
| `inaccesible` | El destino no se alcanza o agota el tope | destino inalcanzable |
| `sin_permiso` | Abre pero el listado falla | listado fallido |
| `desconocido` | Nunca comprobado | `health IS NULL` |

## 3. Entregables por capa

### Backend
- `migrations/0008_salud_de_repositorio.sql` — `ALTER TABLE repositories` con `health TEXT CHECK(...)`, `health_checked_at DATETIME`, `health_detail TEXT` (anulables).
- `src/db/entities/repository.rs` — enum `SaludDeRepositorio` + campos nuevos en `Repositorio`, `desde_fila`, y método `anotar_salud(id, salud, detalle)`.
- `src/rustic/` — primitiva **read-only** `salud_del_repositorio(acceso)` (poner en marcha **sin inicializar** + listar instantáneas, bajo el tope ya existente).
- `src/engine/mod.rs` — envoltura `Motor::comprobar_salud(acceso)`.
- `src/api/repositories.rs` — `POST /api/v1/repositories/{id}/health` y `POST /api/v1/repositories/health`; `RepositorioDelCable` con `health`, `health_checked_at` y `health_detail` (saneado).
- `src/api/mod.rs` — registrar las rutas nuevas.

### Frontend
- `ui/src/services/api/types.ts` — `SaludDeRepositorio` y campos de salud en `Repository`.
- `ui/src/api/repositorios.ts` — `comprobarSaludDeRepositorio(id)` y `comprobarSaludDeTodos()`.
- `ui/src/hooks/` — mutación de comprobación + invalidación del listado.
- `ui/src/pages/ajustes.tsx` — columna «Salud» con texto + fecha, acción de comprobar por fila y botón global.
- Listado del **Dashboard** — indicador desde el estado persistido, **sin** disparar comprobaciones.

### Docs (al cerrar)
- `docs/FEATURE-027-salud-por-repositorio.md`.
- Archivar el change (`openspec archive salud-por-repositorio`) y volcar los deltas a `openspec/specs/`.
- Renombrar a `plans/PLAN-027-COMPLETED.md` (`git mv`).

## 4. TDD (orden obligatorio)

1. **RED** — tests Rust (migración + capa de acceso + endpoint) y UI (columna, hook) que fallen.
2. **GREEN** — implementación mínima hasta verde.
3. **REFACTOR** — `cargo clippy -- -D warnings` y `pnpm run lint` limpios.

## 5. Verificación

- `just check` (fmt + clippy + test) y `just check-all` (incluye `ui-check`).
- Prueba manual en el contenedor del lab con un repositorio sano y uno caído.

## 6. Riesgos

- **Inicializar por accidente**: la comprobación DEBE ser read-only; un check que cree el repositorio es un fallo.
- **Fuga de secretos** en `health_detail`: sanear y cubrirlo con un test.
- **Coste del endpoint global**: N repositorios × tope de 10 s; documentarlo
---
