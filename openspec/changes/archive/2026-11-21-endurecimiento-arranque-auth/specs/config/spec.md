# config — delta

## Purpose

El contrato de configuración de Kesto: qué variables de entorno se leen y con qué nombre exacto, cuáles tienen valor por defecto, cuáles son obligatorias y cuándo la aplicación debe negarse a arrancar en lugar de degradarse en silencio. Este delta añade la puerta explícita del modo sin autenticación y la única dirección en la que ese modo puede escuchar.

## MODIFIED Requirements

### Requirement: El SRS fija los nombres y la configuración los respeta

`docs/DEFINITION.md` §8 declara `RUST_LOG`, `DATABASE_URL` y `BIND_ADDRESS` en el `compose` de referencia. La aplicación SHALL leerlas con esos nombres exactos, sin prefijo ni alias, de modo que un `compose` copiado de la documentación funcione tal cual.

Las variables que el SRS no declara SHALL llevar prefijo `KESTO_` —o conservar el nombre que ya usa shuul, en el caso de `OIDC_*`— para no colisionar con las de otro servicio del mismo host.

#### Scenario: El compose del SRS se lee sin traducción
- **GIVEN** un entorno con `BIND_ADDRESS=0.0.0.0:9090`, `DATABASE_URL=sqlite:///tmp/x.db`, las tres variables `OIDC_*` y `KESTO_SECRET` —la forma del §8, que declara las tres y sirve en todas las interfaces—
- **WHEN** se carga la configuración
- **THEN** la dirección de escucha efectiva SHALL ser `0.0.0.0:9090`
- **AND** la cadena de base de datos SHALL ser `sqlite:///tmp/x.db`

### Requirement: Las variables que pueden tener valor por defecto lo tienen, y es el del SRS

`RUST_LOG` y `DATABASE_URL` SHALL tener valor por defecto, y SHALL coincidir con el que el §8 declara, para que un `compose` que las omita se comporte como el que las declara.

`BIND_ADDRESS` SHALL tener valor por defecto, y el suyo SHALL ser `127.0.0.1:8080` y **no** el `0.0.0.0:8080` del §8: escuchar en todas las interfaces SHALL ser una decisión explícita del operador —el §8 la toma en su `compose`, que la declara tal cual, y con proveedor de identidad— y NO SHALL ser nunca el resultado de no haber dicho nada.

Una variable **presente pero vacía** NO SHALL confundirse con una ausente: el valor por defecto es para la variable que no está, no para la que está sin valor.

#### Scenario: Sin esas variables, ganan los valores del SRS
- **GIVEN** un entorno con `KESTO_MASTER_KEY`, `KESTO_ALLOW_INSECURE=1` y sin `BIND_ADDRESS` ni `DATABASE_URL`
- **WHEN** se carga la configuración
- **THEN** la dirección de escucha SHALL ser `127.0.0.1:8080`
- **AND** la cadena de base de datos SHALL apuntar a `sqlite:///app/data/kesto.db`

## ADDED Requirements

### Requirement: El modo sin autenticación exige un permiso explícito

La aplicación SHALL negarse a arrancar en modo `Disabled` sin el permiso explícito de `KESTO_ALLOW_INSECURE`, que SHALL valer `1` para permitirlo y `0` para negarlo. Ausente SHALL equivaler a negado, y cualquier otro valor —vacío incluido— SHALL ser un error de formato que nombre la variable.

Sin las tres `OIDC_*` no hay credencial que verificar, así que la ausencia de proveedor NO SHALL ser por sí sola un modo de funcionamiento: un despliegue que pierde las `OIDC_*` —un `.env` que no se carga, un `compose` que interpola a vacío— SHALL negarse a arrancar en vez de quedar abierto en todas las interfaces, con el motor incluido. El permiso es lo que convierte el modo de desarrollo en una decisión y no en un accidente.

Con el modo `Oidc` la variable NO SHALL tener efecto alguno: la capa de sesión se monta igual y el permiso no la relaja.

#### Scenario: Sin proveedor y sin permiso no se arranca
- **GIVEN** un entorno con `KESTO_MASTER_KEY` y sin ninguna variable `OIDC_*` ni `KESTO_ALLOW_INSECURE`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_ALLOW_INSECURE`

#### Scenario: Con el permiso explícito el modo de desarrollo arranca
- **GIVEN** un entorno con `KESTO_MASTER_KEY`, `KESTO_ALLOW_INSECURE=1` y `BIND_ADDRESS=127.0.0.1:8080`, sin ninguna variable `OIDC_*`
- **WHEN** se carga la configuración
- **THEN** el modo SHALL ser `Disabled`
- **AND** la configuración SHALL ser válida

#### Scenario: Un permiso que no es `1` ni `0` es un error de formato
- **GIVEN** un entorno con `KESTO_MASTER_KEY` y `KESTO_ALLOW_INSECURE=true`, sin ninguna variable `OIDC_*`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_ALLOW_INSECURE`

#### Scenario: Con proveedor de identidad el permiso no cambia nada
- **GIVEN** un entorno con las tres variables `OIDC_*`, `KESTO_SECRET` y sin `KESTO_ALLOW_INSECURE`
- **WHEN** se carga la configuración
- **THEN** el modo SHALL ser `Oidc`
- **AND** NO SHALL fallar por falta de permiso

### Requirement: El modo sin autenticación solo escucha en loopback

En modo `Disabled`, la dirección de escucha efectiva SHALL ser de loopback —`127.0.0.0/8` o `::1`— y escuchar en cualquier otra SHALL ser un error de arranque que nombre `BIND_ADDRESS`, aunque el permiso explícito esté dado: sin capa de autenticación, la superficie completa —incluido el motor, que dirige el socket del daemon— NO SHALL quedar alcanzable desde otra interfaz.

En modo `Oidc` NO SHALL haber tal restricción: el §8 declara `0.0.0.0:8080` y es legítimo.

#### Scenario: Sin autenticación y en todas las interfaces no se arranca
- **GIVEN** un entorno con `KESTO_MASTER_KEY`, `KESTO_ALLOW_INSECURE=1` y `BIND_ADDRESS=0.0.0.0:8080`, sin ninguna variable `OIDC_*`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `BIND_ADDRESS`

#### Scenario: Con autenticación, escuchar en todas las interfaces es legítimo
- **GIVEN** un entorno con las tres variables `OIDC_*`, `KESTO_SECRET` y `BIND_ADDRESS=0.0.0.0:8080`
- **WHEN** se carga la configuración
- **THEN** la configuración SHALL ser válida
- **AND** la dirección de escucha efectiva SHALL ser `0.0.0.0:8080`
