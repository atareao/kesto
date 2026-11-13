# Proposal

## Why

El PR #39 (issue #38) introdujo en el `Dockerfile` una caché de dependencias con el patrón dummy: se compila el crate contra un `src/main.rs` vacío para cachear las dependencias, y después se copia el código real y se vuelve a compilar. Pero **`COPY` de Docker preserva el mtime** del contexto, y el `src/main.rs` real es **más antiguo** que el dummy creado durante el build, así que Cargo considera el crate fresco y **no lo recompila**.

La release **v0.5.1** lo destapó: la imagen se construyó sin errores pero el smoke test falló (el contenedor no arranca: es el `fn main() {}`), y el job `manifest` quedó `skipped`, de modo que no se publicaron `:0.5.1` ni `:latest`. Evidencia del log del job `Image (amd64)`:

```
#28 366.2  Compiling kesto v0.5.1
#28 366.2  Finished `release` profile [optimized] target(s) in 6m 06s
#31   0.486  Finished `release` profile [optimized] target(s) in 0.42s   <-- no recompiló nada
```

Issue de entrada: [#41](https://github.com/atareao/kesto/issues/41).
Plan: `plans/PLAN-012.md`.

## What Changes

- En el `Dockerfile`, tras copiar el código real, se adelanta el mtime de las fuentes (`find src -type f -exec touch {} +`) para que Cargo **recompile** el crate.
- Se añade un **guard estático** en `scripts/release-image-workflow.test.sh` que exige ese `touch`.
- No cambia el contrato externo, ni el pipeline, ni el binario esperado.

## Capabilities

### New Capabilities

Ninguna.

### Modified Capabilities

- `deploy`: la imagen SHALL contener el binario real del crate y NO SHALL publicar un artefacto de la caché de compilación.

## Impact

- **Ficheros:** `Dockerfile`, `scripts/release-image-workflow.test.sh`, `docs/FEATURE-012-*.md`.
- **Registro:** hay que **republicar** la imagen con una release nueva (el `manifest` de `v0.5.1` no llegó a ejecutarse).
