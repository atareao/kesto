# config — delta

## Purpose

El contrato de configuración de Kesto: qué variables de entorno se leen y con qué nombre exacto, cuáles tienen valor por defecto, cuáles son obligatorias y cuándo la aplicación debe negarse a arrancar en lugar de degradarse en silencio. Este delta hace obligatoria y `https` la URL de retorno de OIDC y añade el permiso explícito que relaja la validación del `endpoint` de un repositorio.

## REMOVED Requirements

### Requirement: El modo de autenticación se deduce de la presencia de las variables OIDC

**Reason**: el retorno de OIDC deja de tener valor por defecto y su ausencia pasa a ser un error de arranque, así que el escenario que fijaba el defecto de desarrollo deja de ser válido. Un bloque MODIFIED no puede perder escenarios, así que el requisito se sustituye por su versión endurecida.

**Migration**: el despliegue que hoy no declare `OIDC_REDIRECT_URL` tiene que declararla por `https`; sin las tres `OIDC_*` no hay nada que migrar, porque la variable se sigue ignorando.

## ADDED Requirements

### Requirement: El modo de autenticación se deduce de la presencia de las variables OIDC, y con ellas el retorno es obligatorio y `https`

La configuración de OIDC SHALL ser opcional en su conjunto, para permitir el desarrollo sin proveedor, y SHALL tener exactamente tres desenlaces: `Oidc` cuando las tres variables están presentes, `Disabled` cuando no está ninguna, y un error de arranque cuando están unas sí y otras no.

`Disabled` es el modo de desarrollo y SHALL anunciarse con un aviso en voz alta al arrancar. Una configuración a medias NO SHALL degradarse a `Disabled`: NO SHALL arrancar.

`OIDC_REDIRECT_URL` NO SHALL tener valor por defecto y NO SHALL decidir el modo. Con las tres variables presentes SHALL ser obligatoria y su esquema SHALL ser `https`: ausente, o presente con cualquier otro esquema, SHALL ser un error de arranque que nombra la variable, y la aplicación NO SHALL arrancar. Sin ninguna de las tres SHALL ignorarse en silencio, como hasta ahora: no hay modo OIDC al que aplicarle la regla.

El motivo: la URL de retorno es la dirección a la que el proveedor devuelve la sesión, y por ella viaja el código de autorización; un retorno por `http` expone ese código y no tiene uso legítimo en un despliegue. Y el retorno **solo** se usa en modo OIDC, así que un valor por defecto no puede ser válido nunca: es mejor no tenerlo y decir qué falta que tenerlo y decir que no sirve.

#### Scenario: Las tres variables presentes encienden la autenticación
- **GIVEN** `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` y `OIDC_REDIRECT_URL` por `https`
- **WHEN** se carga la configuración
- **THEN** el modo SHALL ser `Oidc`
- **AND** el emisor SHALL ser el de `OIDC_ISSUER_URL`

#### Scenario: Ninguna variable deja la autenticación apagada y lo anuncia
- **GIVEN** un entorno sin ninguna variable `OIDC_*`
- **WHEN** se carga la configuración
- **THEN** el modo SHALL ser `Disabled`
- **AND** el arranque SHALL emitir un aviso que declare el modo de desarrollo

#### Scenario: Una configuración a medias es un error, no un modo desarrollo
- **GIVEN** `OIDC_ISSUER_URL` y `OIDC_CLIENT_ID` presentes pero sin `OIDC_CLIENT_SECRET`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `OIDC_CLIENT_SECRET`
- **AND** NO SHALL arrancar con la autenticación apagada

#### Scenario: La URL de retorno no tiene valor por defecto
- **GIVEN** las tres variables de OIDC presentes y sin `OIDC_REDIRECT_URL`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `OIDC_REDIRECT_URL`
- **AND** NO SHALL aplicarse ningún valor por defecto

#### Scenario: Un retorno por `http` no arranca
- **GIVEN** las tres variables `OIDC_*` presentes y `OIDC_REDIRECT_URL=http://localhost:8080/api/v1/auth/callback`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `OIDC_REDIRECT_URL`

#### Scenario: Un retorno por `https` arranca
- **GIVEN** las tres variables `OIDC_*` presentes y `OIDC_REDIRECT_URL=https://panel.example.test/api/v1/auth/callback`
- **WHEN** se carga la configuración
- **THEN** el modo SHALL ser `Oidc`
- **AND** la URL de retorno SHALL ser la declarada

#### Scenario: Sin modo OIDC la variable se ignora
- **GIVEN** un entorno con `OIDC_REDIRECT_URL` por `http` y sin ninguna de las tres `OIDC_*`
- **WHEN** se carga la configuración
- **THEN** el modo SHALL ser `Disabled`
- **AND** NO SHALL fallar por la URL de retorno

### Requirement: El `endpoint` de un repositorio se valida salvo permiso explícito

`KESTO_ALLOW_INSECURE_ENDPOINT` SHALL gobernar si un `endpoint` de repositorio puede ser inseguro: `1` SHALL conceder el permiso, `0` o ausente SHALL negarlo, y cualquier otro valor —vacío incluido— SHALL ser un error de arranque que nombra la variable. El defecto SHALL ser el permiso negado. La variable lleva el prefijo `KESTO_` que este repositorio reserva para todo lo que no son las tres del §8, `DOCKER_HOST` y las `OIDC_*`.

Con el permiso concedido, el borde HTTP SHALL aceptar un `endpoint` por `http` y uno que apunte al loopback —o a un nombre `localhost`—; NO SHALL aceptar, ni con el permiso, un `endpoint` que apunte a una dirección de enlace local, a la de metadatos, a la no especificada o que lleve credenciales embebidas. Las reglas completas del `endpoint` están en la capability `api-resources`.

El motivo de una sola variable para las dos rebajas: `http` y loopback son el mismo caso —«el destino está en mi propia máquina y en claro»—, que es lo que hacen el desarrollo local y la batería. Lo que la variable NO SHALL rebajar nunca es el enlace local, la dirección de metadatos ni la no especificada: no hay laboratorio que necesite alcanzar el servicio de metadatos.

#### Scenario: El permiso se concede con `1`
- **GIVEN** un entorno con `KESTO_ALLOW_INSECURE_ENDPOINT=1`
- **WHEN** se carga la configuración
- **THEN** la configuración SHALL ser válida
- **AND** el permiso SHALL estar concedido

#### Scenario: Sin la variable el permiso está negado
- **GIVEN** un entorno sin `KESTO_ALLOW_INSECURE_ENDPOINT`
- **WHEN** se carga la configuración
- **THEN** el permiso SHALL estar negado
- **AND** la configuración SHALL ser válida

#### Scenario: Un valor que no es `1` ni `0` no arranca
- **GIVEN** un entorno con `KESTO_ALLOW_INSECURE_ENDPOINT=si`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_ALLOW_INSECURE_ENDPOINT`

#### Scenario: Una variable vacía no vale como permiso
- **GIVEN** un entorno con `KESTO_ALLOW_INSECURE_ENDPOINT` presente pero vacía
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_ALLOW_INSECURE_ENDPOINT`
