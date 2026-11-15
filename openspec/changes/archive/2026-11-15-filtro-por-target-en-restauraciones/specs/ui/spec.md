# ui — delta

## MODIFIED Requirements

### Requirement: El panel muestra el histórico de restauraciones

SHALL existir una pantalla de **Restauraciones**, accesible desde la navegación principal, que liste las ejecuciones de `GET /api/v1/restorations` con su estado, target, snapshot y fechas, con filtro por estado, filtro por target y paginación. El estado SHALL reflejar el desenlace real —no un «lanzada» optimista— y las que están en curso o pendientes SHALL poder refrescarse sin recargar la página.

El filtro por target SHALL ofrecerse como un selector cuyas opciones provienen del catálogo de targets de `GET /api/v1/targets`, y SHALL enviarse a la API como el parámetro `target_id`. El selector SHALL poder limpiarse, y mientras esté activo la pantalla NO SHALL presentar el listado como si fuera completo: el total SHALL corresponder a la consulta acotada. Al cambiar el target SHALL reiniciarse la paginación, de modo que un `offset` heredado NO SHALL producir una página vacía sobre el resultado acotado. El filtro por target y el filtro por estado SHALL poder combinarse, y limpiar el selector SHALL volver a pedir el listado sin `target_id`.

Cuando la opción elegida ya no exista en el catálogo —el target se borró— la pantalla SHALL seguir mostrando el histórico que la API devuelva para ese `target_id`, o su estado vacío, y NO SHALL romperse ni quedarse en carga.

El porqué: la restauración deja de ser efímera; si el registro existe pero el panel no lo muestra, el operador no puede responder «¿se restauró?» ni «¿por qué falló?» sin mirar la base. Y con varias bases, el histórico sin filtro por target no responde a la pregunta que se hace de verdad, que es por una base concreta.

#### Scenario: El histórico lista las restauraciones

- **GIVEN** restauraciones persistidas
- **WHEN** se abre la pantalla de Restauraciones
- **THEN** SHALL listarlas con su estado, target y fechas
- **AND** SHALL poder filtrarse por estado

#### Scenario: El filtro por target acota el listado

- **GIVEN** restauraciones de dos targets distintos
- **WHEN** se elige uno en el selector de target
- **THEN** SHALL pedirse `GET /api/v1/restorations` con su `target_id`
- **AND** la tabla SHALL mostrar solo las de ese target
- **AND** el total SHALL corresponder al resultado acotado

#### Scenario: Cambiar el filtro reinicia la paginación

- **GIVEN** un histórico paginado y una página distinta de la primera
- **WHEN** se cambia el target o el estado
- **THEN** SHALL pedirse el listado desde la primera página

#### Scenario: Limpiar el filtro vuelve al listado completo

- **GIVEN** un listado acotado por target
- **WHEN** se limpia el selector de target
- **THEN** SHALL pedirse el listado sin `target_id`
- **AND** la tabla SHALL volver a mostrar las restauraciones de todos los targets

#### Scenario: Los filtros se combinan

- **GIVEN** un target con restauraciones en estados distintos
- **WHEN** se eligen ese target y el estado `failed`
- **THEN** la petición SHALL llevar ambos parámetros
- **AND** la tabla SHALL mostrar solo las fallidas de ese target

#### Scenario: Una restauración fallida muestra su motivo

- **GIVEN** una restauración en estado `failed` con `error_message`
- **WHEN** se mira en el histórico
- **THEN** SHALL mostrarse su motivo
