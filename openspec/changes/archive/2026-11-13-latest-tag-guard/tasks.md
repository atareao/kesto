# Tasks

## 0. Preparación

- [x] 0.1 Confirmar que el issue [#32](https://github.com/atareao/kesto/issues/32) existe y describe el alcance. Verificación: `gh issue view 32` devuelve el alcance acordado.
- [x] 0.2 Medir la línea base: `podman manifest inspect ghcr.io/atareao/kesto:latest` resuelve hoy al digest de **0.3.3** (`sha256:99209742…`), no al de 0.4.0 (`sha256:8ccf4ddf…`). Verificación: el digest de `latest` coincide con el de `0.3.3` y difiere del de `0.4.0`.

## 1. Guard de `latest` (TDD)

- [x] 1.1 RED: escribir `scripts/latest-tag-guard.test.sh` con tres casos (tag más alto → `true`; tag antiguo → `false`; única release → `true`) y ejecutarlo. Verificación: el test FALLA porque `scripts/latest-tag-guard.sh` aún no existe o no decide bien.
- [x] 1.2 GREEN: crear `scripts/latest-tag-guard.sh <version> <tag...>` que imprima `true` solo si `<version>` es la mayor por orden semver (`sort -V`), y hacer pasar el test. Verificación: los tres casos en verde.
- [x] 1.3 En `.github/workflows/release.yml`, obtener los tags ya publicados y calcular el `is_latest` con el script; condicionar `type=raw,value=latest` con `enable=${{ ... }}`. Verificación: revisión/`actionlint` deja la expresión condicionada; una release no-máxima no publica `latest`.
- [x] 1.4 Añadir `concurrency` (grupo de release de imagen, `cancel-in-progress: false`) al job de imagen para serializar las construcciones. Verificación: el workflow declara el grupo de concurrencia.

## 2. Remedio inmediato del registro

- [ ] 2.1 Reetiquetar `ghcr.io/atareao/kesto:latest` al digest de 0.4.0 (`sha256:8ccf4ddf…`). Verificación: `podman manifest inspect ghcr.io/atareao/kesto:latest` muestra el mismo digest que `ghcr.io/atareao/kesto:0.4.0`. **Bloqueado:** el token `gho_` de `gh` no tiene el scope `write:packages` y no hay sesión en GHCR en esta máquina; requiere credenciales del usuario o re-ejecutar el job de imagen del tag `v0.4.0`.

## 3. Documentación y cierre

- [x] 3.1 `docs/FEATURE-010-latest-tag-guard.md`: qué es, ficheros implicados, cómo funciona el guard, cómo verificar y follow-ups. Verificación: el documento existe y cubre las cuatro secciones.
- [x] 3.2 `just check` en verde (no cambia código Rust/UI, pero se confirma que nada se rompe). Verificación: fmt, clippy y test pasan. **Hecho:** 425 + 5 en verde.
- [x] 3.3 `openspec validate latest-tag-guard --strict` en verde. Verificación: sale «valid».
- [x] 3.4 Archivar el change (`openspec archive latest-tag-guard`) y renombrar `plans/PLAN-010.md` a `plans/PLAN-010-COMPLETED.md`. Verificación: el delta queda fundido en `openspec/specs/deploy/spec.md`.
