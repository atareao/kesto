# secrets Specification (delta)

## ADDED Requirements

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
