# PLAN-049: SEC-19/SEC-20/SEC-21 — Paginación, CI tags, /health público

## Objetivo

Implementar paginación en los endpoints de listado de targets, repositories, jobs y tokens; fijar las versiones de las acciones de GitHub Actions a SHA de commit concretos; y limitar la información pública expuesta en el endpoint `GET /api/v1/health` (ocultando `version` y `auth_mode`). Estas cambios abordan las SEC-19, SEC-20 y SEC-21 del proyecto Kesto.

## Alcance

Este plan corresponde al change creado para el issue #168 y afecta a los siguientes ámbitos:

- Endpoints de listado en la API pública (`/api/v1/targets`, `/api/v1/repositories`, `/api/v1/jobs`, `/api/v1/tokens`).
- Ficheros de trabajo de GitHub Actions (`.github/workflows/*`).
- Respuesta del health check en `/api/v1/health`.

## Issues referenciados

- #168

## Tareas

- [ ] SEC-19: Implementar paginación en los endpoints de listado de targets, repositories, jobs, tokens (parámetros `limit`/`offset`, wrapper JSON paginado).
- [ ] SEC-19: Añadir tests de integración para endpoints paginados.
- [ ] SEC-20: Fijar todas las acciones de GitHub Actions a SHA de commit en `.github/workflows/*` (reemplazar tags `@v7`, `@v2`, etc.).
- [ ] SEC-20: Verificar que los SHA fijados existan y sean correctos.
- [ ] SEC-21: Limitar información pública en `GET /api/v1/health` (quitar/ocultar `version` y `auth_mode`).
- [ ] SEC-21: Añadir/actualizar tests unitarios/integración para el endpoint /health.
- [ ] Refactor: Asegurar consistencia en respuestas paginadas (usar estructura `PaginatedResponse` o similar).
- [ ] Documentation: Actualizar docs/feature correspondientes tras implementar.

## Decisiones adoptadas

- Utilizar parámetros `limit` y `offset` para la paginación en lugar de `page`/`per_page` para mantener coherencia con el código existente.
- El wrapper JSON paginado contendrá los campos `items`, `total`, `limit`, `offset` y `next_page` (cuando corresponda).
- Los SHA de commit de las acciones de GitHub se fijarán consultando el repositorio correspondiente y quedando registrados en el change log.
- En la respuesta de `GET /api/v1/health` se eliminará del payload los campos `version` y `auth_mode` para la versión pública; internamente seguiran disponibles a través de la ruta de health privada si es necesario.

## Follow-ups / notas

- Tras implementar la paginación, revisar que los endpoints existentes que devuelven listas grandes hagan uso del nuevo wrapper.
- Verificar que los tests de integración cubran casos de `limit=0`, `offset` beyond the total y el campo `next_page`.
- Después de fijar los SHA en GitHub Actions, ejecutar los workflows localmente para confirmar que no hay roturas.
- Actualizar la documentación en `docs/` con secciones sobre la nueva estructura paginada y los cambios en el health check.
- Quedarse atento a posibles ajustes de frontend que consuman los nuevos endpoints paginados.