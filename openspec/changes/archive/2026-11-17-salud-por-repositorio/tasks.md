# Tasks

## 1. Backend — persistencia

- [x] 1.1 Migración `0008_salud_de_repositorio.sql` con `health`, `health_checked_at`, `health_detail` (anulables, `CHECK` del enum de estados) → verificación: test de migración que comprueba que las filas existentes quedan intactas y en «desconocido».
- [x] 1.2 Enum `SaludDeRepositorio` + campos nuevos en `Repositorio` + lectura de la fila → verificación: test de lectura con y sin salud anotada.
- [x] 1.3 Método `anotar_salud` en la capa de acceso → verificación: test de anotar+releer y de que la actualización del repositorio no pisa la salud.

## 2. Backend — comprobación

- [x] 2.1 Primitiva de comprobación de solo lectura (puesta en marcha sin inicializar + listado de instantáneas) → verificación: tests de los cinco desenlaces con destino simulado.
- [x] 2.2 Envoltura del motor `comprobar_salud` → verificación: test de mapeo de errores tipados a estado.
- [x] 2.3 Traductor estado/motivo saneado → verificación: test de que el motivo no filtra credenciales.

## 3. Backend — API

- [x] 3.1 `POST /api/v1/repositories/{id}/health` (comprueba, persiste, devuelve) → verificación: test de integración con el router.
- [x] 3.2 `POST /api/v1/repositories/health` (todos) → verificación: test con varios repositorios.
- [x] 3.3 Campos de salud en el recurso de repositorio (listado y lectura) → verificación: test de que el listado no comprueba y que «desconocido» llega a nulo.
- [x] 3.4 La salud no es escribible en alta/actualización → verificación: test.

## 4. Frontend

- [x] 4.1 `SaludDeRepositorio` y campos en `types.ts` → verificación: `tsc --noEmit`.
- [x] 4.2 `comprobarSaludDeRepositorio` / `comprobarSaludDeTodos` en el cliente → verificación: test del cliente.
- [x] 4.3 Hook de mutación con invalidación del listado → verificación: test.
- [x] 4.4 Columna «Salud» con texto+fecha y acciones en Ajustes → verificación: test de componente.
- [x] 4.5 Indicador en el listado del Dashboard sin disparar comprobación → verificación: test que comprueba que no se lanza la petición de comprobación.

## 5. Cierre

- [x] 5.1 `just check-all` verde.
- [x] 5.2 `docs/FEATURE-027-salud-por-repositorio.md` escrito.
- [x] 5.3 Change archivado y plan renombrado a `-COMPLETED`.
