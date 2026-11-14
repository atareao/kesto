# PLAN-013 — Hardening del despliegue: HSTS opcional + backup del volumen de datos

- **Issue:** [#44](https://github.com/atareao/kesto/issues/44)
- **Change OpenSpec:** `openspec/changes/deploy-endurecimiento/`
- **Estado:** completado

## Objetivo

Cerrar los dos follow-ups de FEATURE-008: (1) que la aplicación pueda emitir `Strict-Transport-Security` de forma opcional y configurable, y (2) un procedimiento reproducible (receta + documentación) para respaldar y recuperar el volumen `kesto_data`.

## Alcance

1. `src/config.rs`: `KESTO_HSTS_MAX_AGE` (opcional, entero); ausente → desactivada; vacía/no numérica → error de arranque. Campo `hsts_max_age: Option<u64>` en `Config`.
2. `src/api/mod.rs`: capa `SetResponseHeaderLayer` (tower-http, feature `set-header`) que añade la cabecera a toda respuesta cuando está configurada.
3. `.justfile`: receta `backup-datos <destino>`.
4. `compose.yaml` / `.env.example` / `docs/ENTORNO.md`: documentar la variable.
5. `docs/FEATURE-013-*.md`.

## Fuera de alcance

- Terminar TLS en la propia aplicación (sigue siendo del proxy).
- Rotación automática de respaldos o subida a remoto.

## Tareas

- [x] Aprobar el change proposal.
- [x] Rama `feature/deploy-endurecimiento` desde `development`.
- [x] RED: tests de config + cabecera.
- [x] GREEN: config + capa de cabecera.
- [x] Receta `just backup-datos` + documentación.
- [x] `just check` verde.
- [x] `docs/FEATURE-013-*.md`, `docs/ENTORNO.md`; renombrar el plan y archivar el change.

## Criterios de aceptación

- Sin `KESTO_HSTS_MAX_AGE`: ninguna respuesta lleva la cabecera.
- Con `KESTO_HSTS_MAX_AGE=31536000`: toda respuesta lleva `max-age=31536000` (incluidas 404 y 405).
- Valor vacío o no numérico: error de arranque.
- `just backup-datos <destino>` produce un `.tar.gz` del volumen y deja el contenedor en marcha.
- `just check` verde.

## Riesgos

- La cabecera debe alcanzar también los *fallbacks* (404/405): la capa se coloca en el router raíz, no en la mitad protegida.
- El nombre del volumen bajo `podman compose` lleva prefijo del proyecto; la receta debe localizarlo por sufijo `kesto_data`.
