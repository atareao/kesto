# Especificación del scheduler tras la migración de `chrono`/`chrono-tz` a `time`

Change: `openspec/changes/2026-11-25-chrono-migration`
Issue: #185 — Migrar de chrono/chrono-tz a time o jiff (salida de dos excepciones de deny.toml)
Plan: `plans/PLAN-053.md`
Módulo: `src/scheduler`

## ADDED Requirements

### Requirement: Superficie pública del scheduler sin tipos de chrono ni chrono-tz

Una vez completada la migración, ninguna superficie pública del scheduler ni del paquete `kesto` nombra tipos de `chrono` ni de `chrono-tz`. El programador de cron funciona con tipos de `time` únicamente, y la excepción `RUSTSEC-2026-0555` de `deny.toml` se retira. `RUSTSEC-2026-0543` se mantiene, documentada, porque `chrono` sigue llegando de forma inherente por la cadena FTP de `rustic_backend 0.7.0`.

**Contratos públicos de `src/scheduler/validacion.rs`**

```rust
pub fn validar(expresion: &str) -> Result<Agenda, ErrorDeExpresion>;
pub fn normalizar(expresion: &str) -> String;

pub struct Agenda {
    /// Forma de seis campos que entiende el analizador (no la que guardó el operador).
    pub fn normalizada(&self) -> &str;
    /// Próxima ocurrencia estrictamente posterior a `desde`, en UTC.
    pub fn proxima_tras(&self, desde: time::OffsetDateTime) -> Option<time::OffsetDateTime>;
}

pub enum ErrorDeExpresion {
    Vacia;
    NoParseable { expresion: String, motivo: String };
}
```

- `Agenda::proxima_tras` recibe y devuelve `time::OffsetDateTime` (familia `time`, SRS §3). Sin `chrono::DateTime`, `chrono::Utc`, `chrono::FixedOffset` ni `chrono::Timelike` en firmas públicas.
- El analizador de cron usado por `validar` y guardado en `Agenda` es **nativo**, construido sin `chrono`: reemplaza a `croner::Cron` y a la superficie de `tokio_cron_scheduler`. La gramática acepta 5 o 6 campos; las de 5 campos se normalizan anteponiendo `0 ` en vuelo, sin reescribir nunca la fila de `backup_jobs`.

**`src/scheduler/mod.rs` y `planificacion.rs`**

- `Planificador`, `Ajustes`, `PERIODO_DE_RECONCILIACION`, `PLAZO_DE_PARADA`, `ErrorDelPlanificador` sin tipos del reloj externo en su superficie pública.
- Ningún tipo de `tokio_cron_scheduler` (`Job`, `JobScheduler`, `JobToRunAsync`…) aparece fuera del módulo `scheduler`.

**Destino de los módulos huérfanos**

- `src/scheduler/hora.rs`: importa `chrono` y no se declara en `src/scheduler/mod.rs`. Su única razón de ser era el cruce `time` → `chrono` para `croner`; con `croner` eliminado, el módulo **se elimina del disco**.
- `src/scheduler/conversiones.rs`: marcador vacío que documenta que la migración está completa; no menciona `chrono` ni `chrono-tz`. No se declara en `mod.rs`.

**`Cargo.toml` — estado final de `[dependencies]`**

Se eliminan las tres declaraciones directas:

- `chrono = "0.4"`
- `tokio-cron-scheduler = "0.15.1"`
- `croner = "3"`

Se conserva `time = "0.3.55"` (ya declarado para `sqlx` y usado por toda la superficie de fechas). Ninguna otra línea declara `chrono`, `chrono-tz`, `croner` ni `tokio-cron-scheduler`, ni en `[dependencies]` ni en `[dev-dependencies]`.

**`deny.toml` — estado final de `[advisories]`**

Se elimina la entrada de `RUSTSEC-2026-0555` (chrono-tz), que ya no está en el árbol.

Se conserva, con su `reason` documentada, la entrada `{ id = "RUSTSEC-2026-0543", ... }` (chrono): `chrono` ya no es dependencia directa, pero `suppaftp → opendal-service-ftp → opendal → rustic_backend` la arrastra de forma inherente.

`cargo deny --workspace --locked check` debe salir en verde con esa única excepción documentada.

**Precondición para retirar las excepciones**

La ruta `suppaftp → opendal-service-ftp → opendal → rustic_backend` es inherente a `rustic_backend 0.7.0`: fija `services-ftp` en su dependencia `opendal` y las features de Cargo son aditivas, así que desactivar su feature `opendal` rompe `rustic_backend::opendal`. Por tanto `chrono` no puede eliminarse del grafo desde Kesto y el criterio de ARCHIVE se ajusta: se retira `RUSTSEC-2026-0555` (chrono-tz, ya ausente) y se mantiene `RUSTSEC-2026-0543` con una `reason` que documente la cadena FTP y que `chrono` ya no es dependencia directa. No se exige una rama sin `chrono` para archivar.

#### Scenario: Parseo de expresión cron válida

- **GIVEN** el scheduler con una expresión cron de cinco campos, `"0 3 * * *"`.
- **WHEN** se invoca `validar("0 3 * * *")`.
- **THEN** devuelve `Ok(agenda)` donde `agenda.normalizada() == "0 0 3 * * *"` (se antepone `0 ` solo a las de exactamente cinco campos) y `agenda.proxima_tras(datetime!(2023-11-14 22:13:20 UTC)) == Some(datetime!(2023-11-15 03:00:00 UTC))`.
- **AND** con la forma de seis campos `"0 0 3 * * *"`, que no se reescribe y devuelve la misma próxima ejecución que la de cinco campos.

#### Scenario: Expresión inválida produce error

- **GIVEN** una expresión ilegible, `"a b c d e"`, y una cadena vacía `""`.
- **WHEN** se invoca `validar("a b c d e")` y `validar("")`.
- **THEN** la primera devuelve `Err(ErrorDeExpresion::NoParseable { expresion, motivo })` con `expresion == "a b c d e"` y `motivo` no vacío; el `Display` del error nombra la expresión original e incluye el motivo del analizador. La vacía devuelve `Err(ErrorDeExpresion::Vacia)` cuyo motivo indica que la expresión estaba vacía.

#### Scenario: Cálculo de la próxima ocurrencia respetando zona/offset y DST

- **GIVEN** una agenda para `"0 3 * * *"` y un instante base con offset explícito, `2023-07-15 23:30:00+02:00` (zona con DST activo).
- **WHEN** se invoca `agenda.proxima_tras(desde)`.
- **THEN** devuelve `Some(2023-07-16 03:00:00+00:00)`: el instante se normaliza al absoluto de `time` (UTC) antes de calcular, el resultado lleva el offset de `time` y nunca un tipo de `chrono`; el cambio de horario de la zona de entrada no distorsiona el instante calculado. Con `desde = datetime!(2023-11-14 22:13:20 UTC)` se obtiene `Some(datetime!(2023-11-15 03:00:00 UTC))`, determinista con cualquier `TZ` del proceso.

#### Scenario: cargo deny en verde con solo la excepción 0543 documentada

- **GIVEN** `Cargo.toml` sin declaraciones de `chrono`, `chrono-tz`, `croner` ni `tokio-cron-scheduler`, y `deny.toml` sin la excepción `RUSTSEC-2026-0555` pero con `RUSTSEC-2026-0543` justificada por la cadena FTP.
- **WHEN** se ejecuta `cargo deny --workspace --locked check`.
- **THEN** código de salida 0, sin fallos de la familia advisories, con SOLO `RUSTSEC-2026-0543` documentada.

#### Scenario: cargo tree -i chrono solo devuelve la ruta FTP

- **GIVEN** el workspace re-resuelto con `Cargo.lock` sin `chrono-tz` en ninguna rama y con `chrono` únicamente vía el transporte FTP inherente a `rustic_backend 0.7.0`.
- **WHEN** se ejecuta `cargo tree -i chrono` y `cargo tree -i chrono-tz`.
- **THEN** `cargo tree -i chrono` solo devuelve la ruta `suppaftp → opendal-service-ftp → opendal → rustic_backend` (sin dependencia directa desde `kesto`), y `cargo tree -i chrono-tz` devuelve salida vacía.
