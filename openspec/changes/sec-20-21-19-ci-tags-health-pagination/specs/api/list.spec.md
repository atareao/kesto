# Spec Delta — SEC-19: Paginación en listados `targets`, `repositories`, `jobs`, `tokens`

## Contexto

Parte del change `sec-20-21-19-ci-tags-health-pagination` (refs #168).
Hoy los cuatro endpoints devuelven un array plano. A partir de este change devuelven un **wrapper paginado común**.

## Contrato común (POST-change)

### Parámetros de query

| Parámetro | Tipo | Requerido | Defecto | Máximo | Validación |
|---|---|---|---|---|---|
| `limit` | integer | no | `50` | `200` | `1 ≤ limit ≤ 200`, si no → `400` |
| `offset` | integer | no | `0` | — | `offset ≥ 0`, si no → `400` |

### Respuesta exitosa (200)

```json
{
  "items": [ ... ],
  "total": 137,
  "limit": 50,
  "offset": 0
}
```

- `items`: array — elementos de la página actual (mismo schema que antes por recurso).
- `total`: integer — total de filas **reales** en la colección (sin filtrar por ventana).
- `limit`: integer — valor efectivo aplicado (después de sanear/defecto).
- `offset`: integer — valor efectivo aplicado.

### Errores

- `400 Bad Request` si `limit` o `offset` inválidos. Cuerpo:
```json
{ "error": "invalid_query", "field": "limit", "message": "limit must be between 1 and 200" }
```
- `500` para fallos internos (envelope estándar).

## Endpoints afectados

| Endpoint | Recurso (`items` schema) |
|---|---|
| `GET /api/v1/targets` | `Target` (§3/§5 SRS) |
| `GET /api/v1/repositories` | `Repository` (con secretos **cifrados**, nunca en claro) |
| `GET /api/v1/jobs` | `Job` |
| `GET /api/v1/tokens` | `Token` (sin `plaintext`) |

## Escenarios BDD (aplicables a los 4 endpoints)

### Escenario 1 — Happy path: primera página sin parámetros
**Dado** una colección con `N ≥ 1` filas  
**Cuando** el cliente hace `GET /api/v1/<recurso>` sin query  
**Entonces** la respuesta es `200` con wrapper paginado  
**Y** `limit = 50` (defecto)  
**Y** `offset = 0`  
**Y** `items.length = min(50, N)`  
**Y** `total = N`

### Escenario 2 — Happy path: página explícita válida
**Dado** una colección con `N = 137` filas  
**Cuando** el cliente hace `GET /api/v1/<recurso>?limit=20&offset=40`  
**Entonces** `200` con wrapper  
**Y** `limit = 20`  
**Y** `offset = 40`  
**Y** `items.length = 20` (filas 41–60)  
**Y** `total = 137`

### Escenario 3 — Última página parcial
**Dado** `N = 137`, `limit = 50`  
**Cuando** el cliente pide `offset = 100`  
**Entonces** `items.length = 37` (filas 101–137)  
**Y** `total = 137`

### Escenario 4 — Offset más allá del total (página vacía)
**Dado** `N = 137`  
**Cuando** el cliente pide `offset = 200`  
**Entonces** `200` con `items: []`  
**Y** `total = 137`  
**Y** `limit` y `offset` reflejan los valores pedidos

### Escenario 5 — Colección vacía
**Dado** `N = 0`  
**Cuando** el cliente pide cualquier página válida  
**Entonces** `200` con `items: []`, `total: 0`

### Escenario 6 — Error: `limit` por encima del máximo
**Dado** cualquier colección  
**Cuando** el cliente pide `limit = 500`  
**Entonces** `400` con `error.invalid_query`, `field: "limit"`, mensaje que cita el máximo `200`

### Escenario 7 — Error: `limit = 0`
**Cuando** `limit = 0`  
**Entonces** `400` con `field: "limit"`, mensaje "limit must be ≥ 1"

### Escenario 8 — Error: `limit` negativo
**Cuando** `limit = -10`  
**Entonces** `400` con `field: "limit"`

### Escenario 9 — Error: `offset` negativo
**Cuando** `offset = -5`  
**Entonces** `400` con `field: "offset"`, mensaje "offset must be ≥ 0"

### Escenario 10 — Error: `limit` no numérico
**Cuando** `limit = "abc"`  
**Entonces** `400` con `field: "limit"`

### Escenario 11 — Edge case: `offset` no numérico
**Cuando** `offset = "xyz"`  
**Entonces** `400` con `field: "offset"`

### Escenario 12 — Edge case: parámetros extra ignorados
**Cuando** `GET /api/v1/targets?limit=10&foo=bar`  
**Entonces** `200` con paginación correcta (`foo` se ignora sin error)

### Escenario 13 — Consistencia entre endpoints
**Dado** los cuatro endpoints paginados  
**Cuando** se solicitan con los mismos `limit`/`offset`  
**Entonces** la estructura del wrapper es idéntica (claves, orden, tipos)

## Notas de implementación

- Crear un tipo genérico `Paginated<T>` reutilizable en la capa API.
- La validación y saneo de `limit`/`offset` se centraliza en un extractor/guard.
- `total` se obtiene con `COUNT(*)` (o equivalente) **antes** de aplicar `LIMIT/OFFSET`.
- Los secretos de `repositories` y `tokens` siguen **cifrados** en la respuesta (no cambia).
- El panel UI (Ant Design Table) debe consumir `items`, `total` y controlar `pagination` server-side.
- `executions` y `restorations` **ya** están paginados; se alinean al mismo wrapper si su estructura difiere.
- Actualizar `docs/DEFINITION.md` §5 y el doc de feature.