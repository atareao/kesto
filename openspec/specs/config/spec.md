# config Specification

## Purpose
El contrato de configuración de Kesto: qué variables de entorno se leen y con qué nombre exacto —las tres del §8 del SRS sin prefijo, el resto con prefijo `KESTO_` y las `OIDC_*` con el nombre que ya usa shuul—, cuáles tienen valor por defecto, cuáles son obligatorias y cuándo la aplicación debe negarse a arrancar en lugar de degradarse en silencio. Incluye cómo se deduce el modo de autenticación de la presencia de las variables `OIDC_*`, la ruta del socket con el que el motor habla con Docker, y la regla de que la carga de la configuración se inyecta para que ninguna prueba tenga que mutar el entorno del proceso.

## Requirements

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

### Requirement: El socket de Docker se configura por `DOCKER_HOST` y tiene un valor por defecto

La aplicación SHALL respetar `DOCKER_HOST` como la variable que decide a qué socket se conecta el motor para hablar con Docker, SHALL usar la ruta por defecto de la plataforma cuando la variable no está informada, y NO SHALL exigirla para arrancar.

El nombre no es una elección de este proyecto: es la variable que la biblioteca de Docker lee por convención, y respetarla permite que un `compose` que ya la define para Kesto funcione sin traducir el nombre. Hace falta además por una razón de este entorno: la máquina de desarrollo no tiene Docker, tiene Podman con una API compatible en otro socket, así que una ruta fija en el binario haría imposible ejercitar el motor sin un Docker de verdad. En producción la variable no se informa y se usa la ruta del §8.

Una variable con valor por defecto no SHALL considerarse obligatoria: informada pero vacía SHALL caer al valor por defecto, como se hizo con las otras rutas del contrato, porque una ruta vacía es una interpolación de `compose` sin variable y no una intención.

#### Scenario: Sin DOCKER_HOST se usa la ruta por defecto
- **GIVEN** un entorno sin `DOCKER_HOST`
- **WHEN** se carga la configuración
- **THEN** el socket SHALL ser la ruta por defecto de la plataforma
- **AND** NO SHALL fallar el arranque por su ausencia

#### Scenario: DOCKER_HOST se respeta
- **GIVEN** un entorno con `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock`
- **WHEN** se carga la configuración
- **THEN** el socket SHALL ser el de esa ruta
- **AND** SHALL usarse para hablar con el motor de contenedores

#### Scenario: DOCKER_HOST vacío cae al valor por defecto
- **GIVEN** un entorno con `DOCKER_HOST=` (vacía)
- **WHEN** se carga la configuración
- **THEN** SHALL valer la ruta por defecto
- **AND** NO SHALL fallar el arranque

### Requirement: La lectura del socket se inyecta como la del resto de la configuración

La lectura de `DOCKER_HOST` SHALL hacerse por el mismo mecanismo inyectado que el resto de las variables, y NO SHALL leerse directamente del entorno dentro del motor. Las pruebas SHALL poder escribir una configuración con un socket distinto del del proceso sin tocar la variable de entorno.

Es la regla que ya rige para las demás variables y la razón por la que existe: una prueba que muta el entorno del proceso deja de ser reproducible en cuanto corre en paralelo con otra, y aquí hay dos pruebas del motor que quieren dos sockets distintos —una que apunta a un motor de contenedores de verdad y otra que apunta a una ruta muerta para comprobar el camino del fallo—. Si el motor leyera la variable por su cuenta, esas dos pruebas no podrían coexistir.

#### Scenario: Una configuración se construye con un socket cualquiera
- **GIVEN** un valor de socket elegido por la prueba
- **WHEN** se construye la configuración con él
- **THEN** el motor SHALL usarlo
- **AND** la variable de entorno del proceso NO SHALL haberse tocado

#### Scenario: Dos configuraciones con sockets distintos conviven
- **GIVEN** dos configuraciones construidas con dos sockets distintos
- **WHEN** las dos se usan a la vez
- **THEN** cada una SHALL hablar con el suyo
- **AND** ninguna SHALL ver el del otro

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

### Requirement: La raíz de las copias de directorio se configura con `KESTO_BACKUP_ROOT`

`KESTO_BACKUP_ROOT` SHALL ser la raíz bajo la que el motor acepta el `host_path` de un target `directory`, y SHALL tener el valor por defecto `/backup_sources`, que es donde el `compose` del §8 monta los directorios a respaldar, para que el despliegue de referencia no tenga que declararla.

Su valor SHALL ser una ruta **absoluta**. Presente pero relativa o vacía SHALL ser un error de arranque que nombra la variable, en la línea de `RUST_LOG` y `BIND_ADDRESS`: un valor presente pero inutilizable no se degrada en silencio. Ausente SHALL tomar el valor por defecto.

La variable lleva el prefijo `KESTO_` que este repositorio reserva para todo lo que no son las tres del §8, `DOCKER_HOST` y las `OIDC_*`.

El porqué: la raíz no se cablea en código porque el e2e y el modo local copian directorios temporales que no viven bajo `/backup_sources`, y porque el operador debe poder moverla sin tocar el binario. Y no se deriva de los montajes del propio contenedor porque en el modo local no hay contenedor de Kesto que inspeccionar.

#### Scenario: Sin la variable gana el defecto

- **GIVEN** un entorno sin `KESTO_BACKUP_ROOT`
- **WHEN** se carga la configuración
- **THEN** la raíz de copias SHALL ser `/backup_sources`
- **AND** la configuración SHALL ser válida

#### Scenario: Una raíz relativa no se acepta

- **GIVEN** un entorno con `KESTO_BACKUP_ROOT=backup_sources`
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_BACKUP_ROOT`

#### Scenario: Una raíz vacía no cae al defecto

- **GIVEN** un entorno con `KESTO_BACKUP_ROOT` presente pero vacía
- **WHEN** se carga la configuración
- **THEN** SHALL fallar en alto
- **AND** el mensaje SHALL nombrar `KESTO_BACKUP_ROOT`

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
