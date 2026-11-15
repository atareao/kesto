# Tareas — batería e2e de copia y restauración

Issue: #86 · Plan: `plans/PLAN-023.md`

## 1. RED — escenarios que deben fallar primero

- [ ] 1.1 Arnés `tests/e2e/mod.rs`: resolución de `DOCKER_HOST`, comprobación de socket y de S3; falla con motivo explícito si faltan (falla en voz alta).
- [ ] 1.2 Levantar SeaweedFS desde el arnés (contenedor, puerto S3, credenciales fijas) y esperar disponibilidad con plazo.
- [ ] 1.3 Escenario mariadb: sembrar, copiar, restaurar en contenedor limpio, comparar filas.
- [ ] 1.4 Escenario postgres: ídem con tablas y filas.
- [ ] 1.5 Escenario sqlite: fichero sembrado, copia, restauración, comparación de filas.
- [ ] 1.6 Escenario directorio: árbol sembrado, copia, restauración, comparación byte a byte.
- [ ] 1.7 Escenario de diferencia de datos: alterar una fila tras restaurar y comprobar que la comparación falla y nombra la diferencia.
- [ ] 1.8 Aislamiento del S3 entre ejecuciones: dos ciclos seguidos no comparten snapshots.
- [ ] 1.9 Restauración hacia contenedor e2e (camino S3 → `volcar_a` → `LectorDelVolcado` → `stdin`).
- [ ] 1.10 Contenedor que no drena `stdin`: error acotado por plazo, sin cuelgue.
- [ ] 1.11 Sobrescritura: directorio existente con confirmación se sustituye; sin confirmación se rechaza y no se toca.
- [ ] 1.12 Retención y purga contra S3 real: queda lo conservado, desaparece el resto.
- [ ] 1.13 Credencial S3 incorrecta: falla y el motivo no publica la credencial.
- [ ] 1.14 Ejecutar con `cargo test --test e2e -- --ignored` y confirmar **RED** (los escenarios nuevos fallan; los unitarios siguen verdes).

## 2. GREEN — mínimo necesario para que pasen

- [ ] 2.1 Receta `just e2e` en `.justfile`: levanta el S3, exporta `DOCKER_HOST`, ejecuta la batería.
- [ ] 2.2 Job `e2e` en `.github/workflows/ci.yml`: motor de contenedores del runner, SeaweedFS, imágenes de origen fijadas por versión, `cargo test -- --ignored` y la batería; aislado del job `backend`.
- [ ] 2.3 Corregir en `src/` solo los defectos que la batería descubra (esperado: ramas de `restaurar_en_el_contenedor` y del camino `volcar_a` → `stdin`).
- [ ] 2.4 Confirmar **GREEN**: batería completa verde en local contra podman.

## 3. REFACTOR

- [ ] 3.1 Extraer del arnés los ayudantes repetidos (sembrar, comparar, esperar disponibilidad) sin cambiar comportamiento.
- [ ] 3.2 Acotar plazos y nombres de imágenes en un único sitio; documentar los valores.
- [ ] 3.3 `cargo fmt`, `cargo clippy --all-targets -- -D warnings` y `cargo test --locked` sin warnings.

## 4. Documentación y cierre

- [ ] 4.1 `docs/FEATURE-023-bateria-e2e-de-copia-y-restauracion.md`: qué es, ficheros, cómo se ejecuta en local y en CI, decisiones y follow-ups.
- [ ] 4.2 Actualizar `docs/COMANDOS.md` (receta `just e2e`) y `docs/ENTORNO.md` (requisitos de la batería: podman y SeaweedFS).
- [ ] 4.3 `just check-spec` y `openspec validate bateria-e2e-de-copia-y-restauracion --strict` en verde.
- [ ] 4.4 PR a `development` con gitmoji; renombrar el plan a `plans/PLAN-023-COMPLETED.md`.