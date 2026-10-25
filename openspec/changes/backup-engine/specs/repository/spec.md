# repository Specification

## Purpose
El trato con la biblioteca de copia: cómo se construye un repositorio a partir de la fila que el panel guardó, cómo se distingue un repositorio que aún no existe de uno que existe y no abre, cómo se archiva el volcado dejando constancia de su procedencia, cómo se aplica la retención y cómo se listan los snapshots. Cubre la traducción de los errores de la biblioteca a algo que el operador pueda leer, y la frontera entre lo que el §3 guarda y lo que la biblioteca sabe hacer. Fuera quedan la extracción de los datos, que es de `extraction`, y el orden de las copias, que es de `engine`.

## ADDED Requirements

### Requirement: El repositorio se construye desde la fila, con sus secretos descifrados y sin conocer el cifrado del repositorio de datos

La construcción del repositorio SHALL tomar sus datos de la fila de `repositories` —endpoint, bucket, región, clave de acceso, clave secreta y passphrase—, SHALL recibir la clave secreta y la passphrase **ya descifradas** por la capa de acceso, y NO SHALL descifrar nada por su cuenta ni conocer la clave maestra del proceso. El destino SHALL configurarse con el esquema de almacenamiento que la biblioteca espera y con las opciones que la fila describe.

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

### Requirement: Conectar e inicializar son dos operaciones distintas y se distinguen sin leer mensajes de error

La puesta en marcha de un repositorio SHALL distinguir tres desenlaces —que exista y abra, que no exista todavía, y que exista pero no abra— y SHALL decidirlos **consultando el destino**, no interpretando el texto de un mensaje de error. Si no hay ningún fichero de configuración en el destino, el repositorio SHALL inicializarse; si lo hay, SHALL abrirse y una passphrase equivocada SHALL reportarse como tal; y si el destino no se puede alcanzar, SHALL reportarse como un fallo de acceso al destino.

No es una preferencia: es que la biblioteca no da otra forma de hacerlo. `open()` señala «no hay fichero de configuración» con el **mismo** tipo de error con el que `init()` señala «ya hay uno», así que distinguir los dos casos por el tipo es imposible y hacerlo por el texto sería atarse a una cadena de una dependencia que puede cambiar en cualquier versión menor. Lo que sí es público es el destino: se puede preguntar por los ficheros de configuración que contiene, y esa respuesta —ninguno, uno, o un error— **es** la distinción. Los tres desenlaces tienen además consecuencias distintas para el operador: «no existe, lo he creado», «existe y he conectado» y «existe y no abre, revisa la passphrase» no se arreglan igual.

#### Scenario: Un destino sin configuración se inicializa
- **GIVEN** un destino alcanzable sin ningún fichero de configuración
- **WHEN** se pone en marcha el repositorio
- **THEN** SHALL inicializarse
- **AND** SHALL quedar con un fichero de configuración en el destino

#### Scenario: Un destino con configuración se abre
- **GIVEN** un destino que ya contiene un repositorio con la passphrase de la fila
- **WHEN** se pone en marcha
- **THEN** SHALL abrirse
- **AND** NO SHALL inicializarse de nuevo
- **AND** NO SHALL fallar por «ya existe»

#### Scenario: Una passphrase equivocada no se confunde con un repositorio inexistente
- **GIVEN** un destino que ya contiene un repositorio y una fila con otra passphrase
- **WHEN** se pone en marcha
- **THEN** SHALL fallar diciendo que la passphrase no abre
- **AND** el destino NO SHALL quedar modificado

#### Scenario: Un destino inalcanzable se distingue de las dos anteriores
- **GIVEN** un endpoint de destino que no responde
- **WHEN** se pone en marcha el repositorio
- **THEN** SHALL fallar diciendo que el destino no se alcanza
- **AND** NO SHALL reportarse como repositorio inexistente ni como passphrase equivocada

### Requirement: El snapshot graba su procedencia y no el argv del proceso

El snapshot que se cree SHALL llevar informado el comando que lo produjo, SHALL llevar las rutas que contiene, y NO SHALL heredar la línea de comandos con la que se lanzó el proceso de Kesto. SHALL llevar también la etiqueta que identifique la tarea y el destino del que salió.

La biblioteca tiene un defecto peligroso aquí y está medido: si el campo del comando no se informa, `SnapshotFile::from_options` rellena el campo con `std::env::args_os()`, es decir con **la línea de comandos de Kesto**. El resultado sería un snapshot que dice haberse producido con los argumentos con los que se arrancó el orquestador —y en un arranque con variables por argumento, con secretos dentro—, guardado de forma permanente en el repositorio y visible en el listado. Es un campo de auditoría que, dejado a su defecto, audita lo que no debe.

#### Scenario: El snapshot graba el comando del volcado
- **GIVEN** una copia de una base de datos que termina bien
- **WHEN** se lee el snapshot
- **THEN** su comando SHALL ser el del volcado
- **AND** NO SHALL contener el nombre ni los argumentos del proceso de Kesto

#### Scenario: El snapshot no filtra los argumentos de arranque
- **GIVEN** un proceso de Kesto arrancado con un argumento que lleva una credencial
- **WHEN** la copia acaba y se lee el snapshot
- **THEN** ese argumento NO SHALL aparecer en el snapshot

#### Scenario: El snapshot lleva sus rutas y su etiqueta
- **GIVEN** una copia terminada
- **WHEN** se lee el snapshot
- **THEN** SHALL listar las rutas copiadas
- **AND** SHALL llevar la etiqueta con la que se identificó la tarea

### Requirement: La retención se aplica en tres operaciones —decidir, retirar y purgar— y solo con los tres `keep_*` que el esquema guarda

Tras un snapshot correcto, la retención SHALL aplicarse con los tres parámetros que el esquema guarda —`keep_daily`, `keep_weekly` y `keep_monthly`— y en **tres** operaciones distinguibles: primero decidir **qué snapshots** se retiran según esas reglas, después retirarlos, y por último reclamar los datos que quedaron huérfanos, calculando el plan de purga **antes** de ejecutarlo. Los parámetros de retención que el §3 no guarda SHALL quedarse con el valor por defecto de la biblioteca, y eso NO SHALL dejarse implícito. Una retención que falle NO SHALL cambiar el resultado de la copia, que ya está hecha y es correcta.

Las tres operaciones son la interfaz de la biblioteca y no un reparto de gusto: los parámetros de retención **no** son parámetros del `prune`. Viven en las reglas de retención, que se evalúan sobre los snapshots para producir la lista de los que hay que retirar; retirarlos es una operación aparte; y reclamar el espacio que ocupaban es el `prune`, que sí se calcula y se **mira** antes de ejecutarlo. Quien los confunda escribe un motor que purga datos que nunca dejaron de estar referenciados, o que retira snapshots sin reclamar nada. El SRS describe el flujo en dos tiempos —«evaluación del árbol de snapshots» y después «eliminación de datos huérfanos»—, que es esta misma secuencia con la retirada de los snapshots implícita en la primera mitad.

Con ninguno de los tres informados la biblioteca rechaza la operación por no tener ninguna regla; aquí no puede pasar, porque las tres columnas del §3 son obligatorias. Y que la retención no tumbe la copia es lo que corresponde: si la copia terminó bien y la purga falla, lo que hay que reportar es que la copia está y la purga no, no que la copia falló.

#### Scenario: Los tres keep_ del esquema deciden qué snapshots se retiran
- **GIVEN** una tarea con `keep_daily = 7`, `keep_weekly = 4` y `keep_monthly = 12` sobre un repositorio con snapshots de varias fechas
- **WHEN** se aplica la retención
- **THEN** los tres valores SHALL llegar a las reglas de retención
- **AND** SHALL producirse la lista de los snapshots que hay que retirar

#### Scenario: La retirada de snapshots y la purga de datos son dos operaciones
- **GIVEN** una retención que decide retirar un snapshot
- **WHEN** la retención se aplica
- **THEN** el snapshot SHALL retirarse en una operación
- **AND** la reclamación de sus datos huérfanos SHALL ser otra
- **AND** la segunda NO SHALL ocurrir antes de la primera

#### Scenario: El plan de purga se calcula antes de tocar nada
- **GIVEN** un repositorio del que se han retirado snapshots
- **WHEN** se reclaman los datos huérfanos
- **THEN** SHALL calcularse un plan antes de borrar nada
- **AND** el borrado SHALL ejecutarse sobre ese plan

#### Scenario: Una retención que falla no invalida la copia
- **GIVEN** una copia que ha terminado bien y una retención que falla
- **WHEN** la ejecución acaba
- **THEN** su estado SHALL ser `success`
- **AND** el fallo de la retención SHALL quedar registrado

### Requirement: Los snapshots se listan desde el repositorio y no desde la base

`GET /api/v1/snapshots?repository_id=<id>` SHALL obtener los snapshots preguntándole al **repositorio**, y NO SHALL deducirlos de las filas de `job_executions`. La lista SHALL estar acotada en número y SHALL informar de cada snapshot su identificador, su fecha, sus rutas, su etiqueta y su resumen de tamaño y número de ficheros.

El §5 llama a esta ruta «explorador de snapshots en el almacenamiento S3», y esa frase decide de dónde sale la respuesta: del almacenamiento. La base sabe qué copias **se intentaron**; el repositorio sabe qué copias **están**. Son dos listas distintas y la diferencia es justamente lo que hace útil el explorador: un snapshot que está en el repositorio y no en el histórico es una copia que alguien hizo con otra herramienta, y una ejecución en el histórico sin snapshot en el repositorio es una copia que se dio por buena y no está. Deduplicar la segunda lista de la primera escondería ese caso.

La lectura de la lista es la parte más cara de este change —hay que abrir el repositorio y recorrer su índice—, así que la ruta SHALL acotar la ventana y NO SHALL recorrer el repositorio entero para contestar.

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

#### Scenario: Cada snapshot trae lo que pide el panel
- **GIVEN** un repositorio con al menos un snapshot
- **WHEN** se pide el listado
- **THEN** cada elemento SHALL traer su identificador, su fecha, sus rutas, su etiqueta y su resumen
