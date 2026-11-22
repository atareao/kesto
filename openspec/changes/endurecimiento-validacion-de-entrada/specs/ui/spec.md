# ui — delta

## Purpose

El panel del §6: sus pantallas, sus formularios y su comportamiento. Este delta acompaña al endurecimiento de la validación de entrada: el panel no debe ofrecer escribir más de lo que el borde acepta.

## ADDED Requirements

### Requirement: Los formularios del panel no ofrecen escribir más de lo que el borde acepta

Cada campo de formulario del panel que corresponda a un campo de texto de la API de `/targets` o `/repositories` SHALL llevar como `maxLength` la **misma** cota que el borde impone, de modo que el formulario no deje escribir un valor que el servidor rechazará con `400`:

- `name` → 120 · `container_name` → 128 · `db_user` → 63 · `db_name` → 63 · `bucket` → 63 · `region` → 64 · `access_key_id` → 128 · `endpoint` → 2048 · `db_path` → 1024 · `host_path` → 4096.

El porqué: es la convención del repositorio —la cota del `400` y el `maxLength` del campo son el mismo número, como documenta `COTA_NOMBRE` en `src/api/texto.rs`—. El panel es el consumidor principal del borde, y un formulario que deja escribir un valor que el servidor rechaza obliga al operador a un ida y vuelta para descubrir el tope.

#### Scenario: Un campo del formulario no deja pasar de su cota
- **GIVEN** el modal de alta de un repositorio con el campo `bucket` pintado
- **WHEN** se inspecciona el campo `bucket`
- **THEN** su `maxLength` SHALL ser 63
- **AND** el campo `name` del mismo formulario SHALL tener `maxLength` 120

#### Scenario: Los campos propios de SQLite llevan su cota
- **GIVEN** el modal de alta de fuente con el tipo `sqlite`
- **WHEN** se inspeccionan sus campos `container_name` y `db_path`
- **THEN** `container_name` SHALL tener `maxLength` 128
- **AND** `db_path` SHALL tener `maxLength` 1024
