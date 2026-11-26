# Migración de chrono/chrono-tz a `time`/`jiff` (#185) — Implementation Plan

> Issue: #185 · Plan: `plans/PLAN-053.md` · Estado: **todas las tareas PENDIENTES**.

## Objetivo

Eliminar `chrono` y `chrono-tz` del stack del scheduler (y del árbol de dependencias en lo posible), sustituyéndolos por una única librería de fecha/hora, y retirar las excepciones de auditoría que esos crates arrastran.

## Arquitectura

Cambio **full-stack del scheduler**, no una simple conversión de tipos: se audita el uso real en `src/scheduler/{validacion,mod,planificacion}.rs`, se elige **una** librería destino (`time` o `jiff`), se reemplazan `DateTime`/`Duration`/`TimeZone` y sus llamadas asociadas, se depuran `Cargo.toml` (dependencies y dev-dependencies) y `deny.toml`, y se documenta el uso residual de `chrono` que llega por la cadena FTP (`suppaftp → opendal → rustic_backend`), que **no** se puede eliminar sin tocar esa cadena.

## Tareas

### Tarea 1: Auditar usos de chrono/chrono-tz en el scheduler

**Archivos:**
- Leer: `src/scheduler/validacion.rs`
- Leer: `src/scheduler/mod.rs`
- Leer: `src/scheduler/planificacion.rs`

- [ ] **Paso 1:** Localizar cada uso de `chrono` y `chrono-tz` en los tres módulos.
      ```bash
      rg -n 'chrono|DateTime|Duration|TimeZone|Tz|Utc|Local|FixedOffset' src/scheduler/
      ```
- [ ] **Paso 2:** Clasificar cada uso por tipo (`DateTime<Tz>`, `DateTime<Utc>`, `Duration`, `TimeZone`/`Tz`, parsing/formatting) y anotar el archivo:línea en una tabla de inventario.
- [ ] **Paso 3:** Registrar qué operaciones dependen de zona horaria (DST, offsets fijos) para saber si la librería destino debe soportar tz o basta con offset.

### Tarea 2: Elegir librería destino (`time` vs `jiff`)

**Archivos:**
- Modificar: `Cargo.toml` (solo tras la decisión, en Tarea 4)

- [ ] **Paso 1:** Confirmar disponibilidad en el árbol: `time` ya está presente vía `sqlx`; `jiff` ya está en el árbol vía `rustic_core`.
      ```bash
      cargo tree -i time
      cargo tree -i jiff
      ```
- [ ] **Paso 2:** Decidir según el inventario de la Tarea 1:
      - **`time`** si el uso es mayoritariamente `OffsetDateTime`/`Duration` con offset fijo y no se requiere base de datos de zonas IANA completas (ya compilado por `sqlx`, coste marginal ~0).
      - **`jiff`** si se necesitan zonas horarias con reglas DST reales (`tz::TimeZone`), ya presente vía `rustic_core`.
- [ ] **Paso 3:** Documentar la justificación de la elección en `docs/FEATURE-053-chrono-migration.md` (Tarea 7).

### Tarea 3: Reemplazar tipos chrono por la librería elegida

**Archivos:**
- Modificar: `src/scheduler/validacion.rs`
- Modificar: `src/scheduler/mod.rs`
- Modificar: `src/scheduler/planificacion.rs`

- [ ] **Paso 1:** Sustituir `DateTime<Tz>`/`DateTime<Utc>` por el tipo equivalente de la librería destino (`OffsetDateTime`/`Zoned` según Tarea 2).
- [ ] **Paso 2:** Sustituir `chrono::Duration` por `time::Duration` o `jiff::SignedDuration`/`Span` según corresponda.
- [ ] **Paso 3:** Sustituir `TimeZone`/`Tz`/`FixedOffset` por el mecanismo de zona/offset de la librería elegida; ajustar parsing y formatting (`strftime` → `format_description!`/`-`/`jiff` fmt).
- [ ] **Paso 4:** Ajustar llamadas colindantes (comparaciones, aritmética, serialización) para que compilen contra los nuevos tipos.

### Tarea 4: Depurar dependencias en Cargo.toml

**Archivos:**
- Modificar: `Cargo.toml`

- [ ] **Paso 1:** Eliminar `chrono` y `chrono-tz` de `[dependencies]`.
- [ ] **Paso 2:** Eliminar `chrono`/`chrono-tz` de `[dev-dependencies]` si aparecen.
- [ ] **Paso 3:** Añadir la librería elegida en Tarea 2 (si no estaba ya declarada de forma directa) y verificar que no se introduce un duplicado con la transitiva de `sqlx`/`rustic_core`.
      ```bash
      cargo tree -d
      ```

### Tarea 5: Retirar excepciones de deny.toml y documentar chrono residual

**Archivos:**
- Modificar: `deny.toml`

- [ ] **Paso 1:** Eliminar las dos excepciones `RUSTSEC-2026-0543` y `RUSTSEC-2026-0555` del bloque de advisories permitidas.
- [ ] **Paso 2:** Documentar el uso residual de `chrono` que **permanece** por la cadena FTP `suppaftp → opendal → rustic_backend` (no eliminable sin tocar esa cadena; queda justificado por transitividad).
      ```bash
      cargo tree -i chrono
      cargo deny --workspace check
      ```
- [ ] **Paso 3:** Confirmar que `just deny` pasa sin las excepciones retiradas.

### Tarea 6: Verificación full-stack del scheduler

**Archivos:**
- (sin cambios; verificación)

- [ ] **Paso 1:** Type check y build del workspace.
      ```bash
      cargo check --workspace
      ```
- [ ] **Paso 2:** Suite completa del CI local.
      ```bash
      just check
      ```
- [ ] **Paso 3:** Puerta de dependencias y spec.
      ```bash
      just check-all
      ```

### Tarea 7: Documentación

**Archivos:**
- Modificar: `docs/ENTORNO.md`
- Crear: `docs/FEATURE-053-chrono-migration.md`
- Modificar: `docs/ESTADO.md`

- [ ] **Paso 1:** Crear `docs/FEATURE-053-chrono-migration.md` con: qué era, ficheros implicados, librería elegida y justificación (Tarea 2), inventario de reemplazos, y el `chrono` residual por la cadena FTP.
- [ ] **Paso 2:** Actualizar `docs/ENTORNO.md` reflejando la librería de fecha/hora del stack y la retirada de dependencias.
- [ ] **Paso 3:** Actualizar `docs/ESTADO.md` marcando la migración como construida/verificada.