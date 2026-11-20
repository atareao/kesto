# api-resources — delta

## Purpose

Las rutas de recursos sirven las cuatro entidades de la base con tipos propios de la API: cada listado pagina con ventana acotada y devuelve su total, y el total y la página salen de la misma instantánea de datos.

## MODIFIED Requirements

### Requirement: El histórico de restauraciones se lista con filtros y paginación

SHALL existir `GET /api/v1/restorations` que devuelva las ejecuciones de restauración persistidas, ordenadas por fecha de comienzo descendente, con paginación acotada y los filtros `status` y `target_id`, y SHALL seguir la envoltura de `api-shell`. Cada fila SHALL publicar `id`, `snapshot_id`, `target_id`, `target_name`, `target_type`, `status`, `error_message`, `started_at` y `finished_at`, con los identificadores como texto en su forma canónica con guiones y las fechas como cadenas RFC3339. `target_name` y `target_type` SHALL resolverse desde el target referenciado por `target_id`, y SHALL publicarse como `null` solo si `target_id` es `null`.

El **total** de la respuesta SHALL salir de la **misma transacción** que la página: el `COUNT` y la consulta de la página SHALL ejecutarse dentro de un mismo `begin()`/`commit()`, como ya hace el listado de ejecuciones. Sin esa transacción, entre las dos consultas podría entrar una escritura del motor y el `total` diría un número que la página no sostiene. Las dos son lecturas, así que la transacción NO SHALL bloquear a nadie mientras dura.

La ruta SHALL colgar de la mitad protegida del router, como el resto de `/api/v1/*`, y NO SHALL añadirse a la lista blanca de rutas públicas.

El porqué: la ejecución de restauración deja de ser efímera; sin un listado, el registro existiría pero nadie podría verlo, y el panel no tendría de dónde pintar el histórico.

#### Scenario: El listado devuelve las restauraciones

- **GIVEN** dos restauraciones persistidas
- **WHEN** se pide `GET /api/v1/restorations`
- **THEN** SHALL devolver las dos, la más reciente primero
- **AND** cada fila SHALL publicar su `status` y sus fechas

#### Scenario: El filtro por estado acota el listado

- **GIVEN** restauraciones en estados distintos
- **WHEN** se pide `GET /api/v1/restorations?status=failed`
- **THEN** SHALL devolver solo las fallidas

#### Scenario: El listado pagina de forma acotada

- **GIVEN** más restauraciones que el tope de una página
- **WHEN** se pide `GET /api/v1/restorations`
- **THEN** SHALL devolver una página acotada con el total
- **AND** la siguiente página SHALL traer las restantes

#### Scenario: El listado exige sesión cuando el modo la exige

- **GIVEN** el modo de autenticación habilitado
- **WHEN** se pide `GET /api/v1/restorations` sin sesión
- **THEN** SHALL responder como el resto de rutas protegidas

#### Scenario: El total y la página salen de la misma transacción

- **GIVEN** el histórico de restauraciones de una entidad de acceso
- **WHEN** se pide el listado con ventana
- **THEN** el `COUNT` y la consulta de la página SHALL ir dentro de un mismo `begin()`/`commit()`
- **AND** el `total` NO SHALL poder contar un conjunto distinto del que acota la página
