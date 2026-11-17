# PLAN-028 — Limpieza total del repositorio

- **Issue:** #114
- **Rama:** `chore/limpieza-repo-114` (desde `development`)
- **Tipo:** chore (sin release funcional, sin change SDD)
- **Fecha:** 2026-11-17

## Objetivo

Cerrar la deuda de higiene detectada al publicar la v0.12.0 sin tocar comportamiento de la aplicación.

## Contexto

Tras la release v0.12.0 no queda backlog funcional: `gh issue list --state open` está vacío y `openspec/changes/` solo contiene `archive/` con 27 changes. Lo pendiente es deuda de mantenimiento:

1. `openspec validate --all --strict` falla (`14 passed, 1 failed`) porque `openspec/specs/verificacion-e2e/spec.md` conserva `## Purpose` en `TBD`, texto que dejó `openspec archive` al crear la capability.
2. `docs/ESTADO.md` describe el repositorio a 2026-10-25: 9 especificaciones y planes 001–005. Hoy hay 15 especificaciones y 27 planes.
3. `.gitignore` no cubre `cache/`, `downloading/` ni `models/`: 5,3 GB de artefactos de LocalAI ajenos al proyecto que ensucian `git status` en cada sesión.
4. Quedan 21 ramas remotas fusionadas en `main` y `development` sin borrar (verificado con `git branch -r --merged`; ninguna sin fusionar).

## Tareas

- [ ] T1. Reescribir el `## Purpose` de `openspec/specs/verificacion-e2e/spec.md` describiendo qué verifica la capability, coherente con sus requisitos existentes. Sin cambiar requirements ni escenarios.
- [ ] T2. Actualizar la narrativa de estado de `docs/ESTADO.md` al estado real: 15 especificaciones (`api-resources`, `api-shell`, `arnes-de-pruebas`, `auth`, `config`, `db`, `deploy`, `engine`, `extraction`, `repository`, `restauracion`, `scheduler`, `secrets`, `ui`, `verificacion-e2e`), planes 001–027 completados y features 001–027 con el hueco intencionado de la 017 (fueron follow-ups de FEATURE-016).
- [ ] T3. Añadir `cache/`, `downloading/` y `models/` a `.gitignore` bajo un comentario que explique qué son.
- [ ] T4. Borrar las 21 ramas remotas fusionadas listadas en el issue #114.
- [ ] T5. Verificar: `openspec validate --all --strict` en verde, `just check` en verde y `git status` limpio de artefactos.

## Criterios de aceptación

- `openspec validate --all --strict` → 15 passed, 0 failed.
- `just check` → fmt + clippy + tests en verde (no se toca código Rust, así que deben seguir pasando sin cambios).
- `git status --porcelain` no muestra `cache/`, `downloading/` ni `models/`.
- `git branch -r` sin ramas fusionadas huérfanas.
- Ningún cambio en `src/` ni `ui/`.

## Fuera de alcance

- Lab local: sigue en 0.11.0 con `--restart no`. Es infraestructura fuera del repositorio; se hará aparte.
- Avisos de CI sobre Node 20: obsoletos, las actions ya están en `checkout@v7` / `setup-node@v7` / `upload-artifact@v4`.