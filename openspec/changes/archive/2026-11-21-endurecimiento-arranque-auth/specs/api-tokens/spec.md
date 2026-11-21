# api-tokens — delta

## Purpose

La gestión de los tokens de API de Kesto: cómo se entregan, cómo se guardan y quién puede gestionarlos. Este delta cierra la única ruta que no puede existir cuando no hay nada que verificar: la que emite credenciales.

## MODIFIED Requirements

### Requirement: En modo `disabled` no se exige nada

Con el modo `Disabled` —sin ninguna variable `OIDC_*`, con el permiso explícito y un bind de loopback— NO SHALL exigirse token ni cookie: la superficie de `/api/v1` sigue abierta. La capa de tokens SHALL existir pero NO SHALL ponerse delante cuando no hay proveedor de identidad. Este modo es para desarrollo y no para un despliegue expuesto.

La **gestión** de tokens es la excepción: sus rutas NO SHALL montarse en este modo, y SHALL contestar `404` con la misma envoltura que cualquier ruta desconocida, exactamente como `/api/v1/auth/*`. Sin proveedor de identidad no hay sesión que exigir, así que una ruta de gestión solo puede quedarse abierta —y emitir credenciales sin credencial alguna— o no existir. Un token emitido así sobrevive al cambio de modo: sigue sirviendo cuando la autenticación se enciende, porque su hash está en la base y no depende de quién lo pidió.

#### Scenario: En modo disabled una ruta protegida no pide token
- **GIVEN** una configuración en modo `Disabled`
- **WHEN** se pide `GET /api/v1/repositories` sin cookie y sin `Authorization`
- **THEN** SHALL contestar `200`
- **AND** NO SHALL contestar `401`

#### Scenario: En modo disabled un Bearer inválido no bloquea
- **GIVEN** una configuración en modo `Disabled`
- **WHEN** se pide `GET /api/v1/repositories` con `Authorization: Bearer basura`
- **THEN** SHALL contestar `200`
- **AND** NO SHALL contestar `401`

#### Scenario: En modo disabled la gestión de tokens no está montada
- **GIVEN** una configuración en modo `Disabled`
- **WHEN** se pide `GET /api/v1/tokens`, o se hace `POST /api/v1/tokens`, o se borra con `DELETE /api/v1/tokens/{id}`
- **THEN** SHALL contestar `404` con la envoltura de error
- **AND** NO SHALL emitirse ningún token
