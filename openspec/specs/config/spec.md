# config Specification

## Purpose
El contrato de configuración de Kesto: qué variables de entorno se leen y con qué nombre exacto —las tres del §8 del SRS sin prefijo, el resto con prefijo `KESTO_` y las `OIDC_*` con el nombre que ya usa shuul—, cuáles tienen valor por defecto, cuáles son obligatorias y cuándo la aplicación debe negarse a arrancar en lugar de degradarse en silencio. Incluye cómo se deduce el modo de autenticación de la presencia de las variables `OIDC_*`, y la regla de que la carga de la configuración se inyecta para que ninguna prueba tenga que mutar el entorno del proceso.

## Requirements

### Requirement: El SRS fija los nombres y la configuración los respeta

`docs/DEFINITION.md` §8 declara `RUST_LOG`, `DATABASE_URL` y `BIND_ADDRESS` en el `compose` de referencia. La aplicación SHALL leerlas con esos nombres exactos, sin prefijo ni alias, de modo que un `compose` copiado de la documentación funcione tal cual.

Las variables que el SRS no declara SHALL llevar prefijo `KESTO_` —o conservar el nombre que ya usa shuul, en el caso de `OIDC_*`— para no colisionar con las de otro servicio del mismo host.

#### Scenario: El compose del SRS se lee sin traducción
- **GIVEN** un entorno con `BIND_ADDRESS=0.0.0.0:9090` y `DATABASE_URL=sqlite:///tmp/x.db`
- **WHEN** se carga la configuración
- **THEN** la dirección de escucha efectiva SHALL ser `0.0.0.0:9090`
- **AND** la cadena de base de datos SHALL ser `sqlite:///tmp/x.db`

### Requirement: Las variables que pueden tener valor por defecto lo tienen, y es el del SRS

`RUST_LOG`, `BIND_ADDRESS` y `DATABASE_URL` SHALL tener valor por defecto, y SHALL coincidir con el que el §8 declara, para que un `compose` que las omita se comporte como el que las declara.

Una variable **presente pero vacía** NO SHALL confundirse con una ausente: el valor por defecto es para la variable que no está, no para la que está sin valor.

#### Scenario: Sin esas variables, ganan los valores del SRS
- **GIVEN** un entorno con `KESTO_MASTER_KEY` y sin `BIND_ADDRESS` ni `DATABASE_URL`
- **WHEN** se carga la configuración
- **THEN** la dirección de escucha SHALL ser `0.0.0.0:8080`
- **AND** la cadena de base de datos SHALL apuntar a `sqlite:///app/data/kesto.db`

### Requirement: Una variable obligatoria ausente falla en alto y nombra solo la variable

Cuando falte una variable obligatoria, la aplicación SHALL negarse a arrancar con un error que nombre la variable ausente, y NO SHALL incluir en el mensaje el valor de ninguna variable, ni siquiera el de las que sí están.

#### Scenario: Sin la clave maestra no se arranca
- **GIVEN** un entorno sin `KESTO_MASTER_KEY`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_MASTER_KEY`

### Requirement: Un valor con formato inválido falla en alto

Un valor presente pero inutilizable SHALL tratarse como un error de arranque, no como un valor válido degradado, y el mensaje SHALL nombrar la variable.

Un valor presente SHALL usarse recortado de los espacios sobrantes. No es cosmética: `EnvFilter` lee `RUST_LOG=" info "` como el nombre de un destino y no como el nivel `INFO`, así que sin recortar el proceso arrancaría ciego con un valor que no está en blanco y que por tanto no delata el error.

#### Scenario: Una dirección de escucha sin puerto no se acepta
- **GIVEN** `BIND_ADDRESS=0.0.0.0`, sin puerto
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `BIND_ADDRESS`

#### Scenario: Un filtro de registro en blanco no deja el proceso mudo
- **GIVEN** `RUST_LOG` presente pero vacía
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `RUST_LOG`

### Requirement: El modo de autenticación se deduce de la presencia de las variables OIDC

La configuración de OIDC SHALL ser opcional en su conjunto, para permitir el desarrollo sin proveedor, y SHALL tener exactamente tres desenlaces: `Oidc` cuando las tres variables están presentes, `Disabled` cuando no está ninguna, y un error de arranque cuando están unas sí y otras no.

`Disabled` es el modo de desarrollo y SHALL anunciarse con un aviso en voz alta al arrancar. Una configuración a medias NO SHALL degradarse a `Disabled`: NO SHALL arrancar.

#### Scenario: Las tres variables presentes encienden la autenticación
- **GIVEN** `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID` y `OIDC_CLIENT_SECRET` presentes
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

#### Scenario: La URL de retorno tiene un valor por defecto de desarrollo
- **GIVEN** las tres variables de OIDC presentes y sin `OIDC_REDIRECT_URL`
- **WHEN** se carga la configuración
- **THEN** la URL de retorno SHALL apuntar al callback local

### Requirement: `KESTO_SECRET` solo es obligatoria con la autenticación encendida

Sin autenticación no hay sesión que firmar, así que `KESTO_SECRET` SHALL ser obligatoria cuando el modo sea `Oidc` y SHALL ser innecesaria cuando sea `Disabled`.

#### Scenario: Con OIDC, falta la clave de firma
- **GIVEN** las tres variables de OIDC presentes y sin `KESTO_SECRET`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_SECRET`

#### Scenario: Sin OIDC, la clave de firma no hace falta
- **GIVEN** un entorno sin ninguna variable `OIDC_*` y sin `KESTO_SECRET`
- **WHEN** se carga la configuración
- **THEN** la configuración SHALL ser válida
- **AND** el modo SHALL ser `Disabled`

### Requirement: La lectura del entorno se inyecta

`Config` SHALL obtener sus valores a través de una lectura inyectada, y la carga (`Config::from_lookup`) NO SHALL consultar el entorno del proceso: en `edition = "2024"`, mutar el entorno es `unsafe`, así que una prueba que lo hiciera sería código inseguro y no reproducible en paralelo. `Config::from_env` SHALL ser el único punto de contacto con el entorno —el pegamento que lee las variables y delega en la carga— y NO SHALL usarse en ninguna prueba.

#### Scenario: Los valores vienen de la lectura inyectada
- **GIVEN** una lectura inyectada que devuelve `BIND_ADDRESS=127.0.0.1:1`
- **WHEN** se carga la configuración desde ella
- **THEN** la dirección de escucha efectiva SHALL ser `127.0.0.1:1`

#### Scenario: Las pruebas no mutan el entorno del proceso
- **WHEN** se ejecuta la suite completa en paralelo
- **THEN** ningún test SHALL haber modificado una variable del entorno
- **AND** ningún test SHALL declarar `unsafe`
