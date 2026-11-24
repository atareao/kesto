# Proposal — SEC-19 / SEC-20 / SEC-21

## Título

Cierre de hallazgos SEC-19, SEC-20 y SEC-21: paginación de listados, acciones de CI fijadas a SHA y `/health` sin fuga de metadatos.

## Issue

Refs #168 — campaña de seguridad #156 (agrupa SEC-12..SEC-22).

## Contexto

Tres hallazgos de severidad baja/nit detectados en la campaña de seguridad:

| ID | Severidad | Hallazgo |
|---|---|---|
| SEC-19 | BAJO | Los endpoints de listado `targets`, `repositories`, `jobs` y `tokens` devuelven la colección completa, sin ventana. Solo `executions` y `restorations` están paginados. Un catálogo grande degrada la respuesta y facilita el sondeo masivo. |
| SEC-20 | NIT | Las acciones de GitHub Actions en `.github/workflows/*` están referenciadas por tag mutable (`@v4`, `@main`, etc.), lo que permite un ataque de supply-chain por retag. |
| SEC-21 | NIT | `GET /health` es público y revela `version` y `auth_mode`, información útil para fingerprinting y para elegir un vector de ataque acorde al modo de autenticación. |

## Objetivos

1. **SEC-19:** Añadir paginación (`limit`/`offset`) a los cuatro listados con ventana por defecto server-side, encapsulada en un wrapper paginado común, y límites saneados (defecto y máximo). Mantener compatibilidad del contrato existente en lo posible.
2. **SEC-20:** Fijar cada `uses:` de los workflows a un SHA de commit completo (40 hex), conservando el tag en un comentario para legibilidad y auditoría.
3. **SEC-21:** Reducir la respuesta pública de `/health` a un estado mínimo (`status`), retirando `version` y `auth_mode` del payload público.

## Alcance

**Dentro:**
- Capa API: parámetros de query y respuesta de los listados de `targets`, `repositories`, `jobs`, `tokens` (§5 del SRS).
- Respuesta pública de `GET /api/v1/health`.
- Workflows de `.github/workflows/*`: pin de `uses:` a SHA.

**Fuera:**
- Cambios en la persistencia (el recorte se hace en la consulta o en el mapeo de la capa API).
- Rediseño del panel UI más allá del ajuste necesario para consumir el wrapper paginado.
- Rotación o gestión de credenciales y cualquier otro hallazgo de la campaña (SEC-12..SEC-18, SEC-22).

## Impacto

- **Contrato de API:** los cuatro listados pasan a devolver un objeto paginado (`items`, `total`, `limit`, `offset`). Consumidores del panel y cualquier cliente externo deben adaptarse. Se documenta como cambio de contrato en los specs deltas.
- **Observabilidad / healthchecks:** los probes que leían `version` o `auth_mode` desde `/health` deben dejar de depender de ellos. Los que solo comprueban `status`/código HTTP no se ven afectados.
- **CI:** el pin a SHA no cambia el comportamiento funcional; solo endurece la cadena de suministro. Requiere proceso de actualización manual/acotada de los SHAs.
- **Documentación:** `docs/DEFINITION.md` §5 (endpoints) y los docs de feature deben reflejar el nuevo contrato.

## Riesgos

| Riesgo | Mitigación |
|---|---|
| Romper consumidores que esperan un array plano en los listados | Documentar el cambio, actualizar el panel en el mismo change y declarar el wrapper como contrato explícito en el spec delta. |
| Probes de infraestructura que dependen de `version`/`auth_mode` en `/health` | Anunciar la retirada, mantener `status` estable y verificar que los healthchecks de compose solo miran estado/código. |
| Pins a SHA que quedan obsoletos o sin parche de seguridad | Conservar el tag en comentario, documentar el procedimiento de actualización y (opcional futuro) automatizar con Dependabot/Renovate. |
| Valores de `limit` abusivos (DoS por consulta enorme) | Imponer máximo server-side y saneado de valores inválidos con error `400` explícito. |
| Inconsistencia entre listados paginados y no paginados | Wrapper paginado único y reutilizado por los cuatro endpoints. |

## Criterios de aceptación (alto nivel)

- Los cuatro listados aceptan `limit`/`offset`, aplican defecto y máximo, y devuelven el wrapper paginado.
- Todos los `uses:` de los workflows apuntan a un SHA de 40 hex con el tag en comentario.
- `GET /api/v1/health` público no incluye `version` ni `auth_mode`.
- Specs deltas con escenarios BDD (happy path, errores, edge cases) y tareas TDD completadas; `just check` verde.
