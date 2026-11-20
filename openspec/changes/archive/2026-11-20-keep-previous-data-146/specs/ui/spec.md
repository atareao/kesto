# ui — delta

## Purpose

El panel sirve las pantallas del orquestador contra la API: lista, filtra, pagina y muestra el avance. Los listados paginados piden cada página al servidor y conservan la anterior mientras llega la siguiente.

## ADDED Requirements

### Requirement: Los listados paginados conservan la página anterior mientras llega la siguiente

Los tres hooks de listado paginado —`useEjecuciones` (Histórico), `useSnapshots` (Snapshots) y `useRestauraciones` (Restauraciones)— SHALL conservar los datos de la página anterior mientras se carga la nueva, de modo que la tabla NO SHALL desmontarse ni mostrar el estado «cargando» al cambiar de página.

Para ello SHALL usar `placeholderData: keepPreviousData` de React Query: mientras llega la página nueva, `data` SHALL seguir siendo la página anterior, `isPlaceholderData` SHALL ser `true` e `isPending` SHALL quedarse en `false`. La clave de la consulta (con su `offset`) NO SHALL cambiar: cada página sigue siendo una entrada de caché propia y se pide al servidor.

El `total` que alimenta el paginador SHALL seguir leyéndose de la respuesta: durante la transición es el de la página anterior, coherente con las filas que todavía se ven.

#### Scenario: Al cambiar de página no se vuelve a «cargando»
- **GIVEN** el Histórico con una página ya cargada
- **WHEN** se pasa a la página siguiente y su petición todavía no ha llegado
- **THEN** la tabla SHALL seguir mostrando la página anterior
- **AND** `isPending` SHALL ser `false` y `isPlaceholderData` SHALL ser `true`
- **AND** NO SHALL mostrarse el estado «cargando»

#### Scenario: Snapshots conserva la página anterior
- **GIVEN** Snapshots con una página ya cargada de un repositorio
- **WHEN** se pasa a la página siguiente y su petición todavía no ha llegado
- **THEN** la tabla SHALL seguir mostrando la página anterior
- **AND** `isPlaceholderData` SHALL ser `true`

#### Scenario: Restauraciones conserva la página anterior
- **GIVEN** Restauraciones con una página ya cargada
- **WHEN** se pasa a la página siguiente y su petición todavía no ha llegado
- **THEN** la tabla SHALL seguir mostrando la página anterior
- **AND** `isPlaceholderData` SHALL ser `true`
