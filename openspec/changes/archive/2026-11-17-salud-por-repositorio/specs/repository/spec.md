# repository — delta

## ADDED Requirements

### Requirement: La comprobación de salud es una lectura y no crea ni inicializa el repositorio

La comprobación de salud de un repositorio SHALL consultar el destino **sin inicializarlo**: NO SHALL crear un repositorio donde no lo hay. SHALL distinguir los desenlaces por los **hechos del destino** —no hay configuración, hay configuración y abre, hay configuración y no abre— y por el resultado del listado, y SHALL traducirlos a un estado legible: que responde y lista, que no hay repositorio todavía, que la passphrase no abre, que el destino no se alcanza, o que abre pero el listado falla. Cuando el destino responde y abre, SHALL además **listar instantáneas**, para que un destino alcanzable pero sin permiso de lectura no se dé por sano.

La comprobación SHALL respetar el **tope de tiempo** que esta capacidad ya impone alrededor de las operaciones que abren o listan el repositorio: un tope agotado SHALL llegar como «el destino no se alcanza», el mismo desenlace tipado que ya existe, NO SHALL convertirse en un pánico y NO SHALL publicar el motivo crudo de la dependencia ni ningún secreto. El **motivo** asociado al estado SHALL ser un texto acotado y legible para el operador, sin credenciales.

Esta capacidad NO SHALL decir «sano» sobre la base de un listado que no ocurrió: cada desenlace se decide por el hecho medido, nunca por el texto de un mensaje de error.

#### Scenario: Un destino que responde, abre y lista está sano
- **GIVEN** un destino alcanzable que contiene un repositorio con la passphrase de la fila
- **WHEN** se comprueba su salud
- **THEN** el estado SHALL ser «sano»
- **AND** SHALL haberse listado sus instantáneas

#### Scenario: Un destino sin configuración no se inicializa al comprobarlo
- **GIVEN** un destino alcanzable sin fichero de configuración
- **WHEN** se comprueba su salud
- **THEN** el estado SHALL ser «sin repositorio»
- **AND** el destino NO SHALL quedar inicializado
- **AND** NO SHALL crearse ningún fichero de configuración

#### Scenario: Una passphrase que no abre se distingue del destino inalcanzable
- **GIVEN** un destino que contiene un repositorio y una passphrase equivocada en la fila
- **WHEN** se comprueba su salud
- **THEN** el estado SHALL ser «passphrase»
- **AND** NO SHALL confundirse con «no se alcanza»

#### Scenario: Un destino que no responde falla acotado y sin secretos
- **GIVEN** una fila que describe un destino donde nadie responde
- **WHEN** se comprueba su salud
- **THEN** el estado SHALL ser «no se alcanza» en un tiempo acotado por el tope
- **AND** el motivo NO SHALL contener credenciales ni el texto crudo de la dependencia

#### Scenario: Un destino que abre pero no lista no se da por sano
- **GIVEN** un destino que abre pero cuyo listado de instantáneas falla
- **WHEN** se comprueba su salud
- **THEN** el estado SHALL distinguir el fallo de listado
- **AND** NO SHALL declararse «sano»
