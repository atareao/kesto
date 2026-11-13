# FEATURE-012 — La imagen publica el binario real (fix del mtime con COPY)

**Issue:** [#41](https://github.com/atareao/kesto/issues/41) · **Plan:** `plans/PLAN-012.md` · **Change OpenSpec:** `openspec/changes/dockerfile-copy-mtime/`

## Qué es

La feature de imagen rápida (FEATURE-011) introdujo en el `Dockerfile` una **caché de dependencias** con el patrón dummy: se compila el crate contra un `src/main.rs` vacío (`fn main() {}`) para cachear las dependencias, y después se copia el código real y se vuelve a compilar. Ese patrón tenía un **fallo latente**: la imagen publicada contenía el **binario dummy**, no el real.

La release **v0.5.1** lo destapó. La imagen se construyó sin ningún error, pero el **smoke test** (job `smoke`, añadido en FEATURE-011) falló: el contenedor no emitía ni una línea y no escuchaba en `:8080`. Como el smoke test no pasó, el job `manifest` quedó `skipped` y **no se publicaron** `:0.5.1` ni `:latest` (`latest` seguía apuntando a 0.3.3).

Esta feature corrige el `Dockerfile` para que el binario publicado corresponda siempre al código real, y añade un guard estático que impide reintroducir la regresión.

## Ficheros implicados

- `Dockerfile` — etapa `rust-builder`: `RUN find src -type f -exec touch {} +` antes del build real.
- `scripts/release-image-workflow.test.sh` — comprobación nº 7: el `Dockerfile` debe adelantar el mtime de las fuentes.
- `plans/PLAN-012.md` — plan del hotfix.
- `openspec/changes/dockerfile-copy-mtime/` — change proposal (SDD).

## Cómo funciona

El `Dockerfile` separa la compilación en dos capas:

1. **Capa de dependencias (cacheable):** `COPY Cargo.toml Cargo.lock` → `echo 'fn main() {}' > src/main.rs` → `cargo build --release`. Esta capa se reutiliza mientras `Cargo.toml`/`Cargo.lock` no cambien, porque Docker la identifica por el contenido de esos dos ficheros.
2. **Build real:** `COPY src/ ./src/` → `COPY migrations/` → **`find src -type f -exec touch {} +`** → `cargo build --release`.

El problema: **`COPY` de Docker preserva el mtime** de los ficheros del contexto. El `src/main.rs` del checkout es **más antiguo** que el `fn main() {}` dummy que se creó durante el build de la capa de dependencias. Cargo compara mtimes contra su huella (`.fingerprint`) y concluye que el crate está **fresco**: no lo recompila. Evidencia real del run de v0.5.1 (job `Image (amd64)`):

```
#28 366.2  Compiling kesto v0.5.1
#28 366.2  Finished `release` profile [optimized] target(s) in 6m 06s   <-- dummy
#31   0.486  Finished `release` profile [optimized] target(s) in 0.42s   <-- real: nada recompilado
```

La corrección adelanta el mtime de las fuentes (`touch`) **tras** copiar el código y **antes** del build real, de modo que Cargo detecta que el código real es más nuevo que la huella del dummy y **recompila el crate**. El `touch` se coloca dentro del RUN del build real, tocando solo `src/` (nunca `Cargo.toml`/`Cargo.lock`), así que no invalida la capa de dependencias.

## Variables y configuración

Sin variables nuevas. El `touch` no depende de ninguna configuración: es parte del build.

## Cómo verificar

- `just scripts-test` — ejecuta `scripts/release-image-workflow.test.sh`, que incluye la comprobación nº 7.
- `just check` — fmt + clippy + tests.
- **Prueba local:** `podman build -t kesto:hotfix-test .` y arrancar; en el log debe verse la **segunda** compilación de `kesto` con un tiempo real (no ~0.4 s), y el contenedor debe servir `GET /api/v1/health`.
- **Prueba de integración:** la release debe superar el job `smoke` y el `manifest` (publicación de `X.Y.Z` + `latest`).

## Decisiones

- **`touch` frente a `cargo clean -p kesto`:** `cargo clean` requiere conocer el paquete y es más agresivo; `touch` es idiomático, barato y no toca la caché de dependencias.
- **`touch` en el RUN del build real, no en una capa aparte:** así es atómico con el `cargo build` que depende de él y no añade capas intermedias.
- **Guard estático, no un test que construya la imagen:** construir la imagen en el test estructural multiplicaría el tiempo del CI. El guard detecta la regresión a nivel de `Dockerfile`; la prueba real la aportan el `smoke` y el `manifest` del propio pipeline de release.
- **`sccache` no mezcla artefactos:** cachea por hash del código fuente, así que el dummy y el real tienen claves distintas. El fallo era de la huella de Cargo, no de `sccache`.

## Follow-ups

- El tag `v0.5.1` se quedará sin imagen publicada (el `manifest` no llegó a correr); se cubre con la release siguiente.
