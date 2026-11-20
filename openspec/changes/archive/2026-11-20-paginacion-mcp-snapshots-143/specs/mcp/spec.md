# mcp — delta

## Purpose

El servidor MCP de Kesto es un cliente de la API: expone el estado y las acciones del orquestador al agente sin tocar nada por su cuenta. Sus herramientas de lectura devuelven el cuerpo de la API tal cual, y sus filtros los reenvía a la API, que es quien valida.

## ADDED Requirements

### Requirement: El listado de instantáneas se pagina por `limit` y `offset`

La herramienta de lectura `list_snapshots` SHALL aceptar `limit` y `offset` **opcionales** en su entrada, además del `repository_id` obligatorio, y SHALL reenviarlos a `GET /api/v1/snapshots` tal cual cuando el agente los informe.

Cuando el agente NO informe `limit`, la herramienta NO SHALL mandar un `limit` propio: SHALL dejar que aplique el defecto de la API, para que el defecto y el tope del MCP sean **los mismos** que los de la API y no haya un valor duplicado que mantener. El tope y el rango de `limit` los valida la API, que contesta `400` a un valor fuera de rango; la herramienta NO SHALL recortarlo ni inventarse un tope propio.

La descripción de la herramienta SHALL decir que la respuesta trae `total` y `truncated`, y que un listado truncado se completa pidiendo el resto con `offset`, de modo que el agente sepa cuándo paginar.

#### Scenario: Sin `limit`, se aplica el defecto de la API
- **GIVEN** un agente que invoca `list_snapshots` solo con el identificador del repositorio
- **WHEN** la herramienta llama a la API
- **THEN** NO SHALL mandar un `limit` propio
- **AND** la API SHALL aplicar su defecto

#### Scenario: `limit` y `offset` se reenvían
- **GIVEN** un agente que invoca `list_snapshots` con `limit` y `offset`
- **WHEN** la herramienta llama a la API
- **THEN** SHALL reenviar ambos valores tal cual
- **AND** NO SHALL recortar el `limit` aunque supere el tope de la API

#### Scenario: Un listado truncado se puede completar
- **GIVEN** un repositorio con más instantáneas que las devueltas en una página
- **WHEN** el agente invoca `list_snapshots`
- **THEN** SHALL recibir `total` y `truncated` en la respuesta
- **AND** la descripción de la herramienta SHALL explicar que el resto se pide con `offset`
