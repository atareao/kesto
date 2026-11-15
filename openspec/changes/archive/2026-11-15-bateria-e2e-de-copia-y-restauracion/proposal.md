# Batería e2e de copia y restauración

## Por qué

El ciclo completo **copia → S3 → restauración** no está verificado de punta a punta. Los 27 tests `#[ignore]` que ejercen el motor de contenedores nunca se ejecutan en CI (`ci.yml` solo corre `cargo test --locked`), el almacén de todos los tests es un doble local (`abridor_local`) y el único doble de S3 remoto simula un endpoint caído, no errores 4xx/5xx. Además `Motor::restaurar_en_el_contenedor` (`src/engine/mod.rs:1077`) no tiene ningún test e2e, el camino S3 → `volcar_a` → `LectorDelVolcado` → `stdin` del `exec` no se ejerce, y la sobrescritura de un destino con contenido previo no está cubierta. La única verificación con S3 real es manual (RustFS/SeaweedFS + `lab/`).

Consecuencia: un defecto en el ciclo real puede llegar a `main` sin que ningún test lo delate, y «todo funciona» depende de una prueba manual que no se repite.

## Qué cambia

- Capacidad nueva `verificacion-e2e`: una batería reproducible que ejecuta el ciclo completo por cada tipo de origen —mariadb, postgres, sqlite y directorio— contra un **S3 real** (SeaweedFS) y un **motor de contenedores real**, y **compara los datos restaurados**, no solo el código de salida.
- Arnés que levanta por su cuenta el S3 de la batería y exige un motor inyectado por `DOCKER_HOST`.
- Job de CI dedicado que ejecuta la batería y los 27 `#[ignore]` existentes.
- Receta `just e2e` para reproducirla en local.
- Cobertura de los huecos identificados: restauración hacia contenedor, camino S3→stdin, sobrescritura y retención/purga contra S3.

## Impacto

- **Capacidades**: nueva `verificacion-e2e`; sin cambios en `engine`, `extraction`, `restauracion`, `repository` ni `api-*`.
- **Contrato de API**: sin cambios.
- **Esquema**: sin cambios.
- **Infraestructura**: `.github/workflows/ci.yml` (job nuevo, aislado del job `backend`), `.justfile` (receta `e2e`), `tests/` (arnés nuevo), `docs/` (documento de la feature).
- **Dependencias**: ninguna nueva en `Cargo.toml`; SeaweedFS y los contenedores de origen se usan como imágenes externas, no como crates.
- **Riesgo**: el job de CI depende de descargar imágenes; se mitiga fijando versiones y un plazo explícito.

## Fuera de alcance

- Restauración parcial por rutas, copia previa automática y progreso incremental real (follow-ups de #67: son features).
- Modificar o reescribir los 27 `#[ignore]`: se ejecutan tal cual.
- Sustituir los tests unitarios: la batería es una capa adicional.
- Reescritura del historial de git ni limpieza de `lab/`.

## Referencias

- Issue #86
- `openspec/specs/engine/spec.md`, `openspec/specs/extraction/spec.md`, `openspec/specs/restauracion/spec.md`, `openspec/specs/repository/spec.md`
- `docs/FEATURE-005-backup-engine.md`, `docs/FEATURE-021-restauracion-de-bases-de-datos.md`, `docs/COMANDOS.md`