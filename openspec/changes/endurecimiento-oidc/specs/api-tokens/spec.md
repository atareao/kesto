## MODIFIED Requirements

### Requirement: El token se entrega una sola vez y solo se guarda su hash

`POST /api/v1/tokens` SHALL crear un token con un nombre obligatorio y un conjunto de scopes, y SHALL devolver el valor en claro **una única vez** en esa respuesta. La base SHALL almacenar únicamente su hash —nunca el valor en claro, ni en columnas, ni en logs, ni en la respuesta de un listado—, de modo que un token perdido no se pueda recuperar y haya que revocar y emitir otro.

El valor SHALL llevar un prefijo reconocible y suficiente entropía para no ser adivinable. El hash SHALL ser determinista para poder verificar un token entrante sin guardar el original.

El hash SHALL ser HMAC-SHA256 con un secreto de servidor derivado de `KESTO_MASTER_KEY` con HKDF y la etiqueta `kesto/api-tokens/v1` (etiqueta nueva, distinta de `kesto/secrets/v1`), y el resultado SHALL codificarse en base64 como hoy. NO SHALL ser un SHA-256 desnudo. La verificación SHALL seguir siendo una búsqueda por `token_hash`, sin migración ni columna nueva: el hash es determinista.

Rotar `KESTO_MASTER_KEY` SHALL invalidar los tokens de API existentes, del mismo modo que ya invalida los secretos cifrados en reposo.

#### Scenario: Al crear un token se devuelve el valor en claro una sola vez
- **GIVEN** una sesión OIDC válida y un cuerpo con `name` y `scopes: ["read"]`
- **WHEN** se pide `POST /api/v1/tokens`
- **THEN** SHALL contestar `201` con el token en claro en el cuerpo
- **AND** la base SHALL contener solo su hash, nunca el valor en claro

#### Scenario: El listado no devuelve el token
- **GIVEN** un token ya creado
- **WHEN** se pide `GET /api/v1/tokens`
- **THEN** cada fila SHALL traer su identificador, nombre, scopes, fechas y estado
- **AND** NO SHALL aparecer el valor en claro ni el hash del token

#### Scenario: Un token caducado no vale
- **GIVEN** un token con `expires_at` en el pasado
- **WHEN** se usa en `Authorization: Bearer`
- **THEN** la petición SHALL contestar `401` con la envoltura

#### Scenario: Con otro secreto de servidor el hash cambia
- **GIVEN** el mismo token en claro y dos valores distintos de `KESTO_MASTER_KEY`
- **WHEN** se calcula el hash con cada uno
- **THEN** los dos hashes SHALL ser distintos
- **AND** los dos SHALL ser deterministas respecto de su propio secreto

#### Scenario: Sin el secreto de servidor no se reproduce el hash
- **GIVEN** un token en claro y el hash que la aplicación guardó para él
- **WHEN** se intenta reproducir ese hash sin la clave derivada de `KESTO_MASTER_KEY` (etiqueta `kesto/api-tokens/v1`)
- **THEN** NO SHALL poder reproducirse

#### Scenario: La verificación sigue siendo una búsqueda por `token_hash`
- **GIVEN** un token en claro recién emitido
- **WHEN** se usa en `Authorization: Bearer`
- **THEN** la aplicación SHALL calcular su hash HMAC-SHA256 y buscarlo con `WHERE token_hash = ?1`
- **AND** SHALL encontrar la fila y aceptar la petición
