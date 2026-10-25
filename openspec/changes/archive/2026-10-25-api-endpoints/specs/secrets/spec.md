# secrets Specification (delta)

## ADDED Requirements

### Requirement: Un secreto que entra por el cuerpo de una petición viaja envuelto y no se puede publicar formateándolo

Los tipos de la API que reciben un cuerpo con credenciales SHALL declarar `secret_access_key`, `passphrase` y `db_password` como `SecretString`, y NO SHALL declararlos como `String`.

Para que lo anterior sea posible, `SecretString` SHALL implementar `Deserialize` **a mano**, y NO SHALL implementarlo derivándolo. El porqué: `SecretString` guarda su valor en un `Zeroizing<String>`, y el `Deserialize` de `Zeroizing` existe en `zeroize` 1.9.1 pero detrás de su feature `serde` —una dependencia **opcional**, hoy apagada—, así que un `#[derive(Deserialize)]` sobre el tipo del cuerpo no compila tal cual. Encender esa feature resolvería el `derive`, pero es una decisión con motivo, no una imposibilidad: activaría la feature para **todo el grafo** que usa `zeroize` —`crypto-bigint`, `elliptic-curve`, `ecdsa`, `p256`, `p384`, `primeorder`…—, porque las features de Cargo son **aditivas y se unifican por crate**, y dejaría una invariante de seguridad de este crate colgando de una feature de terceros. La implementación manual son unas pocas líneas, pasa por `SecretString::new` —así el envoltorio `Zeroizing` no se puede saltar— y deja la redacción del secreto donde se audita: en este crate.

La otra mitad de la pareja es la que sostiene todo el diseño: `Serialize` NO SHALL implementarse. Al no haberlo, un DTO de respuesta que llevara una credencial **no compila** —la fuga deja de depender de que nadie se equivoque al escribir el tipo de respuesta y pasa a ser imposible de escribir—. Este requisito NO lleva escenario propio para esa mitad a propósito, y el porqué hay que dejarlo escrito: es una propiedad de **compilación**, no de ejecución, y comprobarla exigiría una dependencia de comprobación de compilación (`trybuild`) que este change ha declarado **no** añadir. Lo que sí se comprueba en ejecución es la mitad positiva —que el `Debug` del tipo del cuerpo redacta y que el valor solo sale por el método explícito—, que ya tiene sus escenarios.

El porqué es el mismo que ya fija el requisito «Un secreto en tránsito hacia la base no se filtra al registrarlo» de esta capability para las entidades, aplicado al borde nuevo: con un `String` corriente, un `#[derive(Debug)]` en el tipo del cuerpo —lo primero que se escribe al depurar un manejador que no se comporta— publica la credencial en la línea de registro, y eso no se detecta en una revisión de código. Con el tipo, la redacción la lleva el tipo y el valor solo sale por el método explícito, que es un sitio contable.

La consecuencia práctica SHALL ser que el valor llegue a la capa de acceso **ya envuelto**, sin pasar por una cadena suelta: quien llame no tiene que acordarse de envolver lo que acaba de deserializar, porque lo deserializado ya viene envuelto.

Este requisito NO SHALL confundirse con el del vacío: que un campo ausente conserve el valor almacenado y que uno vacío se rechace con `400` lo fija la capability de los recursos, que es donde vive el contrato del cuerpo.

#### Scenario: El Debug del tipo del cuerpo no publica la credencial
- **GIVEN** un cuerpo deserializado con una credencial de valor distinguible
- **WHEN** se formatea el tipo con `{:?}`
- **THEN** la salida NO SHALL contener el valor
- **AND** SHALL indicar que está redactada

#### Scenario: El valor solo sale por el método explícito
- **GIVEN** el mismo cuerpo deserializado
- **WHEN** se pide la credencial por el método dedicado
- **THEN** SHALL devolver el valor original
- **AND** NO SHALL hacerlo ningún otro camino

#### Scenario: La credencial que entró por la API no aparece en claro en el fichero
- **GIVEN** un alta hecha **a través de la API** con una credencial de valor distinguible y largo
- **WHEN** se busca ese valor literalmente en el contenido del fichero `.db` y en sus ficheros `-wal` y `-shm`
- **THEN** no SHALL encontrarse ninguna aparición
- **AND** la búsqueda SHALL haberse hecho sobre el fichero, no a través de la capa de acceso

### Requirement: Ningún manejador registra el cuerpo de una petición

Los manejadores de la API NO SHALL registrar el **cuerpo** de una petición —ni en crudo, ni como `serde_json::Value`, ni campo a campo por su valor— a ningún nivel de `RUST_LOG`. Lo que SHALL registrarse es **qué** ocurrió, con la identidad del recurso afectado cuando la haya, y nunca el contenido del cuerpo.

El porqué es que la red de tipo del requisito anterior solo cubre el camino **deserializado**: el cuerpo en crudo —los bytes, o el `Value` de `serde_json` antes de mapearlo— no pasa por ningún `SecretString`, y su `Debug` publica las tres credenciales tal cual. Registrar el cuerpo crudo es, además, lo primero que se improvisa al depurar por qué una petición no se acepta, así que la regla tiene que estar escrita antes de que alguien la necesite.

El texto del intérprete cuando un cuerpo se rechaza NO SHALL registrarse tampoco, y el porqué —puede ecoar el valor del campo— lo fija la capability de la forma de la API, que es donde vive esa frontera; aquí solo se declara que el manejador no lo añade por su cuenta.

#### Scenario: El registro de un alta por la API no publica la credencial
- **GIVEN** `RUST_LOG=trace` y un alta hecha a través de la API con credenciales de valores distinguibles
- **WHEN** se revisa la salida del registro del proceso
- **THEN** ninguna línea SHALL contener ninguna de las credenciales
- **AND** SHALL existir la traza de que la petición se atendió, sin el cuerpo

#### Scenario: Un rechazo de validación no pone el cuerpo en el registro
- **GIVEN** `RUST_LOG=trace` y un alta que no pasa la validación, con una credencial de valor distinguible
- **WHEN** se revisa la salida del registro
- **THEN** SHALL quedar la traza del campo que no cumplía
- **AND** la salida NO SHALL contener la credencial
- **AND** NO SHALL contener el cuerpo de la petición
