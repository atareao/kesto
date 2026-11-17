-- Tokens de API (issue #119, fase 1 del change `mcp`): la credencial que permite a una máquina
-- —un agente, un script, el servidor MCP— llamar a la API sin pasar por el navegador.
--
-- La decisión que condiciona la tabla: **solo se guarda el hash** del token, nunca su valor en
-- claro. El valor se entrega una única vez, al crearlo, y quien lo pierda tiene que revocar y
-- emitir otro; por eso no hay ninguna columna con el valor y no hay nada que cifrar en reposo.
--
-- `token_hash` es `UNIQUE` y no un índice aparte: es la clave por la que se verifica un token
-- entrante —quien recibe un `Bearer` hashea lo que le llegó y pregunta por él—, así que el
-- índice único de la restricción es exactamente el que la verificación necesita, y dos filas no
-- pueden compartir hash.
--
-- `scopes` es el vocabulario **cerrado** del permiso y su `CHECK` es la única boca que escribe
-- el texto almacenado: `read`, `run` o `read,run`, en ese orden canónico (la lectura primero).
-- Una lista vacía no cabe —`scopes IN (...)` la rechaza—, así que «sin scopes» no es un estado
-- de la tabla, y un scope inventado desde fuera tampoco entra.
--
-- Las tres fechas anulables lo son porque **no hay dato todavía**, y `NULL` no se confunde con la
-- cadena vacía: `expires_at` es `NULL` para un token que no caduca; `revoked_at`, mientras siga
-- en vigor; y `last_used_at`, hasta que se use. La fila **no se borra** al revocar: se conserva
-- para poder auditar cuándo se emitió y cuándo se revocó.
--
-- `last_used_at` se anota en cada petición atendida con el token; es informativo y no entra en
-- ninguna decisión de permiso.

CREATE TABLE IF NOT EXISTS api_tokens (
    id TEXT PRIMARY KEY NOT NULL,            -- UUIDv4
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,         -- SHA-256 del valor en claro, en base64; nunca el valor
    scopes TEXT NOT NULL CHECK(scopes IN ('read', 'run', 'read,run')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME,                     -- NULL: no caduca
    revoked_at DATETIME,                     -- NULL: en vigor
    last_used_at DATETIME                    -- NULL: no se ha usado todavía
);
