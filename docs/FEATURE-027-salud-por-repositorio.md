# FEATURE-027 — Salud por repositorio

**Issue:** [#111](https://github.com/atareao/kesto/issues/111) · **Plan:** `plans/PLAN-027-COMPLETED.md` · **Change SDD:** archivado como `openspec/changes/archive/2026-11-17-salud-por-repositorio/`

## Qué es

Cierra la forma **completa** de la visibilidad de salud por repositorio (deuda **D12/B6**, diferida desde PLAN-025). El panel pasa a saber y a mostrar el estado de cada repositorio S3 sin tener que intentar una copia: un **estado persistido**, una **comprobación bajo demanda** que pone a prueba el destino de verdad y un **indicador** en Ajustes y en el listado del Dashboard.

## Ficheros implicados

**Backend**
- `migrations/0008_salud_de_repositorio.sql` — columnas `health`, `health_checked_at`, `health_detail` (anulables, `CHECK` del enum de estados).
- `src/db/entities/repository.rs` — enum `SaludDeRepositorio`, campos en `Repositorio` y método `anotar_salud`.
- `src/rustic/repositorio.rs` — `LoQueContestaElDestino`, `salud_del_repositorio` y `medir_la_salud` (comprobación de solo lectura).
- `src/engine/mod.rs` — `ComprobadorDeSalud` (punto de inyección) y `Motor::comprobar_salud` (traduce el desenlace a estado).
- `src/api/repositories.rs` — endpoints de salud y campos de salud en el recurso.

**Frontend**
- `ui/src/services/api/types.ts` — `SaludDeRepositorio`, `SaludComprobada` y campos en `Repository`.
- `ui/src/services/salud.ts` — etiqueta y color por estado (con caída en tiempo de ejecución).
- `ui/src/api/repositorios.ts` — `comprobarSaludDeRepositorio` y `comprobarSaludDeTodos`.
- `ui/src/hooks/useRepositorios.ts` — mutaciones de comprobación.
- `ui/src/components/IndicadorDeSalud/` y `ui/src/components/SaludDeRepositorios/`.
- `ui/src/pages/ajustes.tsx` y `ui/src/pages/dashboard.tsx`.

## Cómo funciona

1. **Persistencia.** La fila del repositorio guarda el último estado, su fecha y un motivo acotado. Las filas anteriores a la migración quedan en «desconocido» (nulo). La salud **no** es un campo editable: el alta y la actualización no la tocan.
2. **Comprobación.** `POST /api/v1/repositories/{id}/health` y `POST /api/v1/repositories/health` (todos). Cada comprobación es **de solo lectura**: mira el destino **sin inicializarlo**, abre con la passphrase de la fila y **lista las instantáneas**, todo bajo el tope de 10 s ya existente. El desenlace se anota en la fila y se devuelve.
3. **Indicador.** `GET /api/v1/repositories` y `GET /:id` sirven el estado persistido **sin comprobar nada**. Ajustes pinta el estado con su texto y su fecha y ofrece «Comprobar» (por fila) y «Comprobar todos»; el Dashboard muestra el estado persistido de cada repositorio sin disparar comprobaciones.

## Estados

| Estado | Significado |
|---|---|
| `ok` | El destino responde, abre y lista |
| `sin_repositorio` | Responde pero todavía no hay repositorio (no se inicializa al comprobar) |
| `passphrase` | Existe y la passphrase de la fila no abre |
| `inaccesible` | El destino no se alcanza o agota el tope |
| `sin_permiso` | Abre pero el listado falla |
| `desconocido` | Nunca comprobado (nulo en la fila) |

## Variables / configuración

Ninguna nueva. El tope de la comprobación es el ya existente (10 s) y el endpoint global cuesta, en el peor caso, N × 10 s.

## Cómo se despliega

Sin pasos especiales: la migración `0008` se aplica al arrancar y es aditiva (columnas anulables). No hay cambios incompatibles en la API.

## Decisiones

- **D1 — Estado persistido**, no calculado al vuelo: el panel no depende de comprobar para pintar.
- **D2 — La comprobación es de solo lectura**: no inicializa el repositorio (a diferencia de la puesta en marcha normal).
- **D3 — Profundidad abrir + listar**: distingue «responde» de «es utilizable».
- **D4 — `POST` para comprobar y `GET` que no comprueba**: la comprobación persiste, luego es una acción.
- **D5 — Motivo saneado** con un único traductor: sin credenciales ni texto crudo de la dependencia.

## Follow-ups

- Comprobación periódica en segundo plano (scheduler) e histórico de comprobaciones: fuera de alcance.
- Coste del endpoint global (N × tope): documentado, no optimizado.
