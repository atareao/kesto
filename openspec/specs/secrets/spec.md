# secrets Specification

## Purpose
El trato de las credenciales: mientras viven en memoria, un tipo que redacta su contenido al formatearse, lo expone solo por un método explícito y lo borra al caer, y el volcado de arranque, que describe la configuración efectiva sin publicar ninguna credencial; y mientras viven en la base, los tres secretos de `repositories` y `backup_targets` cifrados con AES-256-GCM bajo el sobre `v1:<nonce>:<cifrado>`, con nonce aleatorio por valor y una clave derivada de `KESTO_MASTER_KEY` con HKDF-SHA256 y etiqueta constante, más el descifrado que falla en alto en vez de devolver un secreto vacío. Existe porque ese volcado es una funcionalidad deseable —dice de un vistazo con qué está corriendo el servicio— y con un `String` corriente publicaría la clave maestra, la de firma y el secreto del proveedor; y porque un secreto en claro en el fichero de la base lo publica ante cualquiera que lea ese fichero.

## Requirements

### Requirement: `SecretString` no revela su contenido al formatearse

`SecretString` SHALL redactar su contenido en `Display` y en `Debug`, SHALL exponerlo únicamente por un método explícito, y SHALL borrar el contenido de la memoria cuando el valor cae.

#### Scenario: `Debug` no revela el secreto
- **GIVEN** un `SecretString` con el valor `s3cr3t-muy-largo-y-distinguible`
- **WHEN** se formatea con `{:?}`
- **THEN** la salida NO SHALL contener el valor
- **AND** la salida SHALL indicar que está redactado

#### Scenario: `Display` tampoco
- **GIVEN** el mismo `SecretString`
- **WHEN** se formatea con `{}`
- **THEN** la salida NO SHALL contener el valor

#### Scenario: El valor solo sale por el método explícito
- **GIVEN** el mismo `SecretString`
- **WHEN** se pide su valor por el método dedicado
- **THEN** SHALL devolver `s3cr3t-muy-largo-y-distinguible`

### Requirement: El volcado de arranque describe la configuración sin publicar credenciales

El arranque SHALL registrar una sola vez la configuración efectiva, a nivel `info`, y NO SHALL incluir en ella el valor de `KESTO_SECRET`, `KESTO_MASTER_KEY` ni `OIDC_CLIENT_SECRET` —ni el valor, ni truncado, ni su longitud, ni derivado alguno— a ningún nivel de `RUST_LOG`.

El volcado SHALL seguir describiendo el resto de la configuración: es lo que lo hace útil, y es lo que convierte esta restricción en algo comprobable.

#### Scenario: Las tres credenciales aparecen redactadas
- **GIVEN** una configuración con las tres credenciales a valores distinguibles
- **WHEN** se registra el volcado de arranque
- **THEN** ninguna de las tres SHALL aparecer en la salida
- **AND** la salida SHALL mostrar la dirección de escucha y el modo de autenticación efectivos

#### Scenario: El nivel de depuración no las publica tampoco
- **GIVEN** `RUST_LOG=trace`
- **WHEN** la aplicación arranca y carga su configuración
- **THEN** ninguna línea del log SHALL contener el valor de ninguna de las tres credenciales

### Requirement: Una credencial débil se denuncia sin impedir el arranque

Si `KESTO_SECRET` o `KESTO_MASTER_KEY` están presentes y miden menos de 32 caracteres, la aplicación SHALL emitir un aviso que nombre la variable y el mínimo, y SHALL arrancar igualmente. Kesto es el orquestador de las copias de seguridad: negarse a arrancar por una variable floja convertiría un error de configuración en ausencia de copias.

Una credencial **ausente** NO SHALL confundirse con una débil: la ausencia la decide `config`, y en el modo de desarrollo es legítima.

#### Scenario: Una clave corta avisa y arranca
- **GIVEN** `KESTO_MASTER_KEY` de 12 caracteres
- **WHEN** la aplicación arranca
- **THEN** SHALL emitir un aviso que nombre la variable y el mínimo de 32 caracteres
- **AND** el servicio SHALL arrancar y contestar la salud

#### Scenario: Una clave larga no avisa
- **GIVEN** `KESTO_MASTER_KEY` y `KESTO_SECRET` de 64 caracteres cada una
- **WHEN** la aplicación arranca
- **THEN** NO SHALL emitir ningún aviso sobre credenciales

### Requirement: Un secreto se guarda cifrado y no se puede leer del fichero

Los secretos que viven en la base de datos —`repositories.secret_access_key`, `repositories.passphrase` y `backup_targets.db_password`— SHALL escribirse **cifrados**, con un AEAD AES-256-GCM, y el valor almacenado SHALL llevar un **prefijo de versión** con esta forma:

```
v1:<nonce en base64>:<texto cifrado en base64>
```

El nonce SHALL ser **aleatorio por valor**, nunca fijo ni derivado de la clave: un nonce repetido con la misma clave rompe el AEAD —revela que dos secretos son iguales y permite ataques con texto claro conocido—, así que repetirlo NO SHALL ocurrir ni siquiera entre dos valores idénticos. Como consecuencia, guardar dos veces el mismo secreto SHALL producir dos textos cifrados **distintos**.

El prefijo de versión SHALL permitir que un esquema de cifrado posterior conviva con éste sin migrar las filas ya escritas: la capa de descifrado elige por el prefijo, y por eso el prefijo forma parte del valor y no de la configuración.

La columna SHALL seguir siendo `TEXT` y el texto en claro NO SHALL aparecer en el fichero `.db`, en ninguno de sus ficheros asociados —`-wal` y `-shm`— ni en ninguna copia de seguridad del mismo. La comprobación SHALL hacerse **leyendo el fichero en crudo**, no consultando la capa de acceso: lo que se está afirmando es una propiedad del fichero, y una consulta que pasa por la capa que descifra no la prueba.

#### Scenario: El valor guardado lleva el prefijo de versión
- **GIVEN** un alta de repositorio con su `secret_access_key`
- **WHEN** se lee la columna en crudo desde la base
- **THEN** el valor SHALL empezar por `v1:`
- **AND** SHALL tener tres partes separadas por `:`

#### Scenario: El secreto no aparece en claro en el fichero
- **GIVEN** una base con un repositorio y un target dados de alta, con secretos de valores distinguibles y largos
- **WHEN** se busca cada uno de esos valores literalmente en el contenido del fichero `.db` y en sus ficheros `-wal` y `-shm`
- **THEN** ninguna de las apariciones SHALL encontrarse
- **AND** la búsqueda SHALL haberse hecho sobre el fichero, no a través de la capa de acceso

#### Scenario: Dos cifrados del mismo secreto son distintos
- **GIVEN** dos altas con el mismo valor de secreto
- **WHEN** se comparan los valores almacenados en las dos filas
- **THEN** los dos textos cifrados NO SHALL ser iguales
- **AND** los dos nonces NO SHALL ser iguales

#### Scenario: La lectura por la capa de acceso devuelve el valor original
- **GIVEN** un secreto dado de alta con un valor conocido
- **WHEN** se lee la fila por la capa de acceso
- **THEN** el secreto SHALL ser el valor original
- **AND** el prefijo de versión NO SHALL aparecer en el valor devuelto

### Requirement: El descifrado falla en alto y no devuelve un secreto vacío

Descifrar un valor con un **prefijo desconocido**, con un **nonce de longitud equivocada**, con un **texto cifrado manipulado** —que el AEAD no autentica— o con un **valor en claro** SHALL ser un error tipado. En ninguno de esos casos SHALL devolverse un secreto vacío, un secreto truncado ni un valor parcial.

Es el requisito que evita el fallo silencioso, y por eso cada caso tiene su escenario. Un descifrado que devuelve la cadena vacía convierte un fichero corrupto, una clave rotada o una fila manipulada en un repositorio que se configura **sin credenciales** y falla más tarde, en el motor, con un error que habla de S3 y no de la base. El AEAD ya autentica el texto cifrado: la regla es no tragarse ese veredicto.

El error SHALL conservar la causa para el registro del servidor y NO SHALL incluir el valor cifrado ni el nonce en el mensaje que sale del proceso.

#### Scenario: Un prefijo desconocido no se descifra
- **GIVEN** un valor almacenado que empieza por `v2:` o por un prefijo inventado
- **WHEN** se lee la fila por la capa de acceso
- **THEN** SHALL devolverse un error
- **AND** NO SHALL devolverse un secreto vacío ni parcial

#### Scenario: Un nonce de longitud equivocada no se descifra
- **GIVEN** un valor `v1:` cuyo nonce decodifica a una longitud distinta de la del algoritmo
- **WHEN** se lee la fila por la capa de acceso
- **THEN** SHALL devolverse un error
- **AND** NO SHALL devolverse un secreto vacío

#### Scenario: Un texto cifrado manipulado no se descifra
- **GIVEN** un valor `v1:` en regla al que se le ha cambiado un byte del texto cifrado
- **WHEN** se lee la fila por la capa de acceso
- **THEN** SHALL devolverse un error, porque el AEAD no autentica
- **AND** NO SHALL devolverse un secreto parcial

#### Scenario: Un valor en claro no se descifra
- **GIVEN** una fila cuyo secreto se ha escrito en claro, sin el sobre de cifrado
- **WHEN** se lee la fila por la capa de acceso
- **THEN** SHALL devolverse un error
- **AND** NO SHALL devolverse ese valor en claro como si fuera un secreto legítimo

#### Scenario: El error no publica el valor cifrado
- **GIVEN** un fallo de descifrado
- **WHEN** se formatea el error
- **THEN** el mensaje NO SHALL contener el valor cifrado ni el nonce
- **AND** NO SHALL contener el valor en claro

### Requirement: La clave de cifrado se deriva de `KESTO_MASTER_KEY` y nunca acompaña al dato

La clave de cifrado SHALL derivarse de `KESTO_MASTER_KEY` con **HKDF-SHA256**, con una etiqueta `info` **fija y versionada** —`kesto/secrets/v1`— y NO SHALL almacenarse junto al dato, ni en la base, ni en una columna, ni en una variable global.

La sal SHALL ser la del propio HKDF y la etiqueta `info` SHALL ser **constante en el código** y no configurable: cambiarla cambia todas las claves derivadas, y una etiqueta que se puede tocar por variable de entorno es una forma de perder los secretos sin haber rotado nada. La etiqueta lleva `v1` para que un cambio de esquema de cifrado estrene etiqueta nueva, en paralelo al prefijo del valor.

`KESTO_MASTER_KEY` NO SHALL guardarse en la base en ninguna forma, y su valor NO SHALL aparecer en ningún registro —ni entero, ni truncado, ni su longitud, ni derivado alguno—, como ya fija el requisito «El volcado de arranque describe la configuración sin publicar credenciales» de esta capability.

**Rotar `KESTO_MASTER_KEY` deja ilegibles los secretos ya guardados**, y eso es un límite **aceptado** —un non-goal del change—, no un olvido: no hay clave de datos envuelta por una clave maestra rotable, porque eso sería un segundo sistema de claves que nadie ha pedido. Queda escrito para que quien rote sepa que la clave anterior hace falta para leer lo que ya está guardado, y que la rotación es una operación con plan, no un cambio de variable.

#### Scenario: La clave no se guarda en la base
- **GIVEN** una base con secretos cifrados
- **WHEN** se busca el valor de `KESTO_MASTER_KEY` en el contenido de la base
- **THEN** NO SHALL aparecer
- **AND** NO SHALL existir ninguna columna que lo contenga

#### Scenario: La etiqueta de derivación es fija
- **GIVEN** dos ejecuciones del proceso con la misma `KESTO_MASTER_KEY`
- **WHEN** se deriva la clave de cifrado en cada una
- **THEN** las dos claves derivadas SHALL ser iguales
- **AND** SHALL depender solo de la clave maestra y de la etiqueta constante

#### Scenario: Cambiar la clave maestra deja los secretos ilegibles
- **GIVEN** una base con secretos cifrados con una `KESTO_MASTER_KEY`
- **WHEN** la aplicación arranca con otra `KESTO_MASTER_KEY` y se lee una de esas filas
- **THEN** SHALL devolverse un error de descifrado
- **AND** NO SHALL devolverse un secreto vacío

#### Scenario: La clave maestra no aparece en el registro al cifrar ni al descifrar
- **GIVEN** `RUST_LOG=trace` y una operación de alta y otra de lectura con secreto
- **WHEN** se revisa la salida del registro
- **THEN** ninguna línea SHALL contener el valor de `KESTO_MASTER_KEY` ni el de la clave derivada

### Requirement: Un secreto en tránsito hacia la base no se filtra al registrarlo

El tipo que envuelve un secreto mientras viaja entre la configuración, la capa de acceso y la base NO SHALL revelar su contenido al formatearse con `Display` ni con `Debug`, y SHALL exponerlo únicamente por un método explícito. Es el mismo trato que ya fija el requisito «`SecretString` no revela su contenido al formatearse» de esta capability, y aquí se declara cómo se aplica a los secretos que van y vienen de la base: los tres campos cifrados de `repositories` y `backup_targets` SHALL circular como `SecretString`, no como `String`.

El cumplimiento NO SHALL depender de un cuidado manual en cada llamada de registro: si el secreto es un `String` corriente, basta un `#[derive(Debug)]` en la entidad para publicarlo, y eso no se revisa en una revisión de código. Con el tipo, la redacción la lleva el tipo.

Como consecuencia, el `Debug` de las entidades que contienen secretos NO SHALL publicarlos: o los campos son `SecretString`, o la entidad no deriva `Debug`. Y el valor descifrado SHALL salir de la capa de acceso ya envuelto, no como una cadena suelta que quien llame tenga que acordarse de envolver.

#### Scenario: El `Debug` de la entidad no publica el secreto
- **GIVEN** un repositorio leído de la base, con su `secret_access_key` y su `passphrase`
- **WHEN** se formatea la entidad con `{:?}`
- **THEN** la salida NO SHALL contener ninguno de los dos valores
- **AND** SHALL indicar que están redactados

#### Scenario: El secreto solo sale por el método explícito
- **GIVEN** un target leído de la base con su `db_password`
- **WHEN** se pide el valor por el método dedicado
- **THEN** SHALL devolver el valor original
- **AND** NO SHALL hacerlo ningún otro camino

#### Scenario: El registro de una lectura no publica el secreto
- **GIVEN** `RUST_LOG=trace`
- **WHEN** se da de alta un repositorio y un target, y después se leen y se actualizan
- **THEN** ninguna línea del registro SHALL contener el valor de ninguno de los tres secretos
- **AND** NO SHALL contener tampoco su forma cifrada
