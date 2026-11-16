# repository — delta

## MODIFIED Requirements

### Requirement: El repositorio se construye desde la fila, con sus secretos descifrados y sin conocer el cifrado del repositorio de datos

La construcción del repositorio SHALL tomar sus datos de la fila de `repositories` —endpoint, bucket, región, clave de acceso, clave secreta y passphrase—, SHALL recibir la clave secreta y la passphrase **ya descifradas** por la capa de acceso, y NO SHALL descifrar nada por su cuenta ni conocer la clave maestra del proceso. El destino SHALL configurarse con el esquema de almacenamiento que la biblioteca espera y con las opciones que la fila describe.

El destino SHALL configurarse además de modo que una operación contra un destino que no responde falle **rápido** con un error tipado y legible, y NO SHALL reintentar hasta agotar el retroceso exponencial de la biblioteca. El **tope de tiempo** lo impone **Kesto** alrededor de las operaciones que abren o listan el repositorio —no la dependencia, que en la versión fijada no lo admite entre sus opciones ni ofrece punto de inyección—, y el mapa de opciones con el que se construye el backend SHALL llevar un **reintento acotado**, para que el retroceso exponencial no multiplique la espera. El tope SHALL ser lo bastante corto para no hacer esperar al operador ante un destino caído y lo bastante largo para no cortar un destino lento pero vivo.

Un tope agotado NO SHALL ser un desenlace nuevo: es el mismo «el destino no se alcanza» que esta capacidad ya distingue de «la passphrase no abre», y SHALL llegar como el mismo error tipado, sin convertirse en un pánico y sin publicar el motivo crudo de la dependencia ni ningún secreto.

El cifrado en reposo de los tres secretos del §3 es de `db-schema` y está medido: el valor guardado es un sobre y quien lo abre es la capa de acceso. Un módulo de Rustic que además tuviera la clave maestra tendría dos responsabilidades donde hace falta una, y sería el segundo sitio del programa capaz de descifrar credenciales —exactamente lo que la cifra de secretos quiere evitar. La frontera es limpia: la fila entrega el secreto en claro a quien va a usarlo, y nada más.

#### Scenario: El repositorio se construye y abre con la fila
- **GIVEN** una fila de `repositories` con sus tres secretos cifrados en la base
- **WHEN** se construye el repositorio
- **THEN** SHALL abrirse con la clave de acceso y la passphrase descifradas
- **AND** el módulo NO SHALL haber recibido la clave maestra del proceso

#### Scenario: La región y el endpoint de la fila llegan al destino
- **GIVEN** una fila con su `endpoint`, su `bucket` y su `region`
- **WHEN** se construye el destino
- **THEN** SHALL configurarse con esos tres valores
- **AND** NO SHALL tomarse de otra parte

#### Scenario: Una fila cuyo secreto no se puede descifrar falla con un motivo legible
- **GIVEN** una fila cuyo `secret_access_key` fue cifrado con otra clave maestra
- **WHEN** se construye el repositorio
- **THEN** SHALL fallar con un motivo que nombre el repositorio
- **AND** NO SHALL fallar con un pánico

#### Scenario: Un destino que no responde falla rápido con el error del destino inalcanzable
- **GIVEN** una fila que describe un destino donde nadie responde
- **WHEN** se construye el destino y se opera contra él
- **THEN** SHALL fallar en un tiempo **acotado** por el tope, muy por debajo de los 36,9 s medidos del defecto de la biblioteca
- **AND** SHALL llegar como el error tipado de destino inalcanzable
- **AND** NO SHALL confundirse con una passphrase que no abre

### Requirement: Los snapshots se listan desde el repositorio y no desde la base

`GET /api/v1/snapshots?repository_id=<id>` SHALL obtener los snapshots preguntándole al **repositorio**, y NO SHALL deducirlos de las filas de `job_executions`. La lista SHALL estar acotada en número y SHALL informar de cada snapshot su identificador, su fecha, sus rutas, su etiqueta y su resumen de tamaño y número de ficheros.

La lectura de la lista SHALL estar acotada también en **tiempo**: cuando el destino no responda, la ruta SHALL fallar rápido con el error tipado de destino inalcanzable en vez de agotar el retroceso exponencial de la biblioteca, y el error SHALL llegar al panel con su motivo legible para que pueda ofrecer reintentar. Acotar la espera es lo que evita que una vista que depende del S3 quede «cargando» durante minutos: un destino caído es un desenlace, no una espera.

El §5 llama a esta ruta «explorador de snapshots en el almacenamiento S3», y esa frase decide de dónde sale la respuesta: del almacenamiento. La base sabe qué copias **se intentaron**; el repositorio sabe qué copias **están**. Son dos listas distintas y la diferencia es justamente lo que hace útil el explorador: un snapshot que está en el repositorio y no en el histórico es una copia que alguien hizo con otra herramienta, y una ejecución en el histórico sin snapshot en el repositorio es una copia que se dio por buena y no está. Deduplicar la segunda lista de la primera escondería ese caso.

La lectura de la lista es la parte más cara de este change —hay que abrir el repositorio y recorrer su índice—, así que la ruta SHALL acotar la ventana y NO SHALL recorrer el repositorio entero para contestar. No es una limitación de conveniencia sino del formato: el identificador de un snapshot es el nombre de un fichero y no lleva fecha, así que acotar antes de resolver es la única forma de no recorrer el repositorio entero — y acotar antes de resolver implica que el subconjunto devuelto no tiene por qué ser el de los más recientes. Lo que sí se garantiza es el orden de lo que se devuelve, y que quien pregunte sepa que hay más.

#### Scenario: El listado sale del repositorio
- **GIVEN** un repositorio que contiene snapshots creados por Kesto y alguno creado por otra herramienta
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** SHALL listarse los dos
- **AND** NO SHALL listarse solo los que tienen fila en el histórico

#### Scenario: El listado identifica el repositorio del que sale
- **GIVEN** un identificador de repositorio que no existe
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** SHALL contestar `404` con la envoltura

#### Scenario: El listado va acotado
- **GIVEN** un repositorio con más snapshots que la ventana
- **WHEN** se pide el listado
- **THEN** el número devuelto NO SHALL superar la ventana
- **AND** la respuesta SHALL indicar que hay más

#### Scenario: La ventana acota el número y no promete antigüedad
- **GIVEN** un repositorio con más snapshots que la ventana
- **WHEN** se pide el listado
- **THEN** lo devuelto SHALL venir ordenado de más reciente a más antigua
- **AND** el subconjunto devuelto NO SHALL prometerse como el de los más recientes

#### Scenario: Cada snapshot trae lo que pide el panel
- **GIVEN** un repositorio con al menos un snapshot
- **WHEN** se pide el listado
- **THEN** cada elemento SHALL traer su identificador, su fecha, sus rutas, su etiqueta y su resumen

#### Scenario: Un repositorio que no responde acota la espera del listado
- **GIVEN** un repositorio cuyo destino no responde
- **WHEN** se pide `GET /api/v1/snapshots?repository_id=<id>`
- **THEN** la petición SHALL fallar en un tiempo **acotado** por el tope, no tras los 36,9 s de reintentos medidos
- **AND** el error SHALL llegar con su motivo legible
- **AND** NO SHALL dejarse la petición colgada hasta agotar el retroceso exponencial de la biblioteca
