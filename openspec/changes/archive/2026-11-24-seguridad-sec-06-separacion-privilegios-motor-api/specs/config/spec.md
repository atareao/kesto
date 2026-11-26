# config — delta

## Purpose

El contrato de configuración de Kesto. Este delta traslada `DOCKER_HOST` al proceso motor, lo recorta, y añade la ruta del socket IPC y el UID de la API que el motor exige para identificar al par.

## MODIFIED Requirements

### Requirement: El socket de Docker se configura por `DOCKER_HOST` y tiene un valor por defecto

El binario `kesto-motor` SHALL respetar `DOCKER_HOST` como la variable que decide a qué socket se conecta para hablar con Docker, SHALL usar `unix:///var/run/docker.sock` cuando la variable no está informada, SHALL recortar los espacios de su valor y NO SHALL exigirla para arrancar. El binario de la API NO SHALL leer `DOCKER_HOST` ni usarlo para construir el cliente: solo conoce `KESTO_MOTOR_SOCKET`.

El nombre no es una elección de este proyecto: es la variable que la biblioteca de Docker lee por convención, y respetarla permite que un `compose` que ya la define para Kesto funcione sin traducir el nombre. Hace falta además por una razón de este entorno: la máquina de desarrollo no tiene Docker, tiene Podman con una API compatible en otro socket, así que una ruta fija en el binario haría imposible ejercitar el motor sin un Docker de verdad. En producción la variable no se informa y se usa la ruta del §8.

Una variable con valor por defecto NO SHALL considerarse obligatoria: informada pero vacía SHALL caer al valor por defecto, como se hizo con las otras rutas del contrato, porque una ruta vacía es una interpolación de `compose` sin variable y no una intención. Con espacios SHALL caer al mismo valor recortado, por la misma razón.

Un socket configurado que no responde SHALL producir un error en el motor que lo nombra, y NO SHALL impedir el arranque de la API: el fallo SHALL aparecer al ejecutar una copia, en el lado motor, no al arrancar el proceso que sirve HTTP.

#### Scenario: Sin DOCKER_HOST se usa la ruta por defecto
- **GIVEN** un entorno sin `DOCKER_HOST`
- **WHEN** se carga la configuración del motor
- **THEN** el socket SHALL ser `unix:///var/run/docker.sock`
- **AND** NO SHALL fallar el arranque por su ausencia

#### Scenario: DOCKER_HOST se respeta
- **GIVEN** un entorno con `DOCKER_HOST=unix:///run/user/1000/podman/podman.sock`
- **WHEN** se carga la configuración del motor
- **THEN** el socket SHALL ser el de esa ruta
- **AND** SHALL usarse para hablar con el motor de contenedores desde el proceso motor

#### Scenario: DOCKER_HOST vacío cae al valor por defecto
- **GIVEN** un entorno con `DOCKER_HOST=` (vacía)
- **WHEN** se carga la configuración del motor
- **THEN** SHALL valer `unix:///var/run/docker.sock`
- **AND** NO SHALL fallar el arranque

#### Scenario: DOCKER_HOST con espacios se recorta
- **GIVEN** un entorno con `DOCKER_HOST=  unix:///run/user/1000/podman/podman.sock  `
- **WHEN** se carga la configuración del motor
- **THEN** SHALL usarse la ruta sin los espacios
- **AND** NO SHALL fallar el arranque

#### Scenario: La API no lee DOCKER_HOST
- **GIVEN** un entorno con `DOCKER_HOST` apuntando a un socket cualquiera
- **WHEN** arranca el binario de la API
- **THEN** la configuración de la API SHALL NOT contener esa ruta
- **AND** el binario API SHALL NOT enlazar el cliente Docker

#### Scenario: Un socket inexistente no impide arrancar la API
- **GIVEN** un `DOCKER_HOST` configurado en el motor que apunta a una ruta muerta
- **WHEN** arranca la API y se ejecuta una copia
- **THEN** el motor SHALL fallar con un error que nombra el socket
- **AND** la API SHALL haber arrancado y escuchado

## ADDED Requirements

### Requirement: El socket IPC y el UID de la API se configuran por variables separadas

`KESTO_MOTOR_SOCKET` SHALL fijar la ruta del Unix domain socket por el que la API pide operaciones al motor, con valor por defecto `/run/kesto/motor.sock`. La API SHALL usarla para construir su cliente; el motor SHALL usarla para enlazar el socket. Es una ruta IPC local, NO SHALL ser una dirección de red ni el socket del daemon.

`KESTO_API_UID` SHALL ser obligatoria en el binario `kesto-motor`: el motor SHALL compararla con el UID del par vía `SO_PEERCRED` antes de procesar ninguna solicitud y SHALL negarse a arrancar si falta, nombrando la variable en el error. La API SHALL NOT necesitar declararla. Ambas variables SHALL documentarse en `docs/ENTORNO.md` y en la plantilla de entorno.

#### Scenario: El socket IPC usa su defecto
- **GIVEN** un entorno sin `KESTO_MOTOR_SOCKET`
- **WHEN** arrancan la API y el motor
- **THEN** el socket IPC SHALL ser `/run/kesto/motor.sock`
- **AND** ambos procesos SHALL usar esa misma ruta

#### Scenario: El socket IPC se respeta
- **GIVEN** un entorno con `KESTO_MOTOR_SOCKET=/run/kesto/prueba.sock`
- **WHEN** arrancan la API y el motor
- **THEN** el motor SHALL enlazar esa ruta
- **AND** la API SHALL conectarse a esa ruta

#### Scenario: El motor exige el UID de la API
- **GIVEN** un entorno sin `KESTO_API_UID`
- **WHEN** se intenta arrancar `kesto-motor`
- **THEN** SHALL fallar en alto
- **AND** el error SHALL nombrar `KESTO_API_UID`

#### Scenario: La API no necesita el UID de la API
- **GIVEN** un entorno con `KESTO_MOTOR_SOCKET` y sin `KESTO_API_UID`
- **WHEN** arranca solo el binario de la API
- **THEN** SHALL arrancar
- **AND** SHALL conectarse al motor por `KESTO_MOTOR_SOCKET`
