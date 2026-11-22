# api-resources — delta

## Purpose

El contrato HTTP de los recursos de Kesto: rutas, tipos y códigos. Este delta añade la validación del `endpoint` de un repositorio en el alta y en la edición, que es donde el operador lo escribe.

## ADDED Requirements

### Requirement: El alta y la edición de un repositorio validan el `endpoint`

`POST /api/v1/repositories` y `PUT /api/v1/repositories/{id}` SHALL validar el campo `endpoint` **antes** de escribir la fila y antes de hablar con el destino. Cuando la validación falle, la petición SHALL contestar `400` nombrando el campo `endpoint` y NO SHALL escribirse ni modificarse ninguna fila.

El `endpoint` SHALL ser una URL absoluta con esquema y host. SHALL aceptarse el esquema `https`; el esquema `http` SHALL rechazarse salvo que la configuración conceda el permiso explícito —ver la capability `config`—.

Sobre el host, y siempre que sea una dirección literal:

- SHALL rechazarse **siempre**, con permiso o sin él, la dirección de enlace local (`169.254.0.0/16`, `fe80::/10`) —que incluye la de metadatos de la nube, `169.254.169.254`—, la de metadatos IPv6 de AWS (`fd00:ec2::254`) y la no especificada (`0.0.0.0`, `::`);
- SHALL rechazarse **salvo permiso explícito** el loopback (`127.0.0.0/8`, `::1`) y un nombre `localhost` o terminado en `.localhost`;
- SHALL aceptarse el rango privado (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`) y las direcciones públicas.

Una dirección IPv6 que lleve **embebida** una IPv4 —la forma IPv4-mapeada `::ffff:a.b.c.d` (RFC 4291) y el prefijo NAT64 bien conocido `64:ff9b::/96` (RFC 6052)— SHALL juzgarse por esa IPv4, porque es la dirección que de verdad se alcanza: `https://[::ffff:127.0.0.1]` es loopback y `https://[::ffff:169.254.169.254]` es la dirección de metadatos.

Un `endpoint` que lleve credenciales embebidas (`https://usuario:clave@host`) SHALL rechazarse siempre: acabarían escritas en la fila y en los registros.

El mensaje del `400` SHALL nombrar el campo y NO SHALL repetir el valor recibido.

El porqué del rango privado permitido: el destino natural de Kesto es un almacén S3 autoalojado —MinIO, SeaweedFS— en la red interna, y rechazarlo dejaría el producto inservible para su uso más común. Lo que se cierra es el **loopback** del propio proceso y la **dirección de metadatos**, donde viven las credenciales de instancia de la máquina.

El porqué del borde y no del motor: la edición NO SHALL conectar con el destino, así que una comprobación dentro del motor no la alcanzaría y una fila podría quedar guardada con un `endpoint` inadmisible. El alta SHALL conservar su comprobación de conectividad: la validación del `endpoint` es una puerta previa, no un sustituto, y NO SHALL uniformarse la respuesta del alta según el destino responda o no.

La validación NO SHALL resolver el nombre del host: un `endpoint` con nombre se acepta por su forma, así que un nombre que resuelva por DNS a una dirección rechazada NO SHALL detectarse en esta versión.

#### Scenario: Un endpoint por `http` se rechaza sin permiso
- **GIVEN** un alta cuyo `endpoint` es `http://almacen.example.test:9000` y sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un endpoint a loopback se rechaza sin permiso
- **GIVEN** un alta cuyo `endpoint` es `http://127.0.0.1:9000` y sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: La dirección de metadatos se rechaza incluso con permiso
- **GIVEN** un alta cuyo `endpoint` es `https://169.254.169.254` y el permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: Una IPv4 embebida en una IPv6 se juzga por la IPv4
- **GIVEN** un alta cuyo `endpoint` es `https://[::ffff:127.0.0.1]:9000` y sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** lo mismo SHALL cumplirse para la forma NAT64 `https://[64:ff9b::7f00:1]:9000`
- **AND** con el permiso declarado, `https://[::ffff:127.0.0.1]:9000` SHALL admitirse como se admite `127.0.0.1`

#### Scenario: Con permiso, un endpoint local y en claro se acepta
- **GIVEN** un alta cuyo `endpoint` es `http://127.0.0.1:9000` y el permiso declarado
- **WHEN** se atiende la petición
- **THEN** la petición SHALL pasar la validación del `endpoint`
- **AND** su desenlace SHALL depender de la conectividad, como en cualquier alta

#### Scenario: El rango privado se acepta por `https`
- **GIVEN** un alta cuyo `endpoint` es `https://192.168.1.10:9000`
- **WHEN** se atiende la petición
- **THEN** la petición SHALL pasar la validación del `endpoint`

#### Scenario: Un esquema que no es `http` ni `https` no vale
- **GIVEN** un alta cuyo `endpoint` es `ftp://almacen.example.test`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: Un valor que no es una URL absoluta no vale
- **GIVEN** un alta cuyo `endpoint` es `almacen.example.test:9000`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`

#### Scenario: Un endpoint con credenciales embebidas no vale
- **GIVEN** un alta cuyo `endpoint` es `https://usuario:clave@almacen.example.test`
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** el cuerpo NO SHALL contener la credencial

#### Scenario: La edición aplica la misma regla
- **GIVEN** un repositorio existente y un `PUT` que le pone un `endpoint` por `http` sin permiso declarado
- **WHEN** se atiende la petición
- **THEN** SHALL contestar `400` nombrando el campo `endpoint`
- **AND** la fila NO SHALL cambiar

#### Scenario: El alta conserva su comprobación de conectividad
- **GIVEN** un alta con un `endpoint` válido contra un destino que no responde
- **WHEN** se atiende la petición
- **THEN** SHALL contestar el error de destino inalcanzable con la envoltura
- **AND** NO SHALL quedar fila en la base
