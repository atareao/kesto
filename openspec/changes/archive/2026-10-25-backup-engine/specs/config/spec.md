# config Specification

## Purpose
El contrato de configuración de Kesto: qué variables de entorno se leen y con qué nombre exacto —las tres del §8 del SRS sin prefijo, el resto con prefijo `KESTO_` y las `OIDC_*` con el nombre que ya usa shuul—, cuáles tienen valor por defecto, cuáles son obligatorias y cuándo la aplicación debe negarse a arrancar en lugar de degradarse en silencio. Incluye cómo se deduce el modo de autenticación de la presencia de las variables `OIDC_*`, la ruta del socket con el que el motor habla con Docker, y la regla de que la carga de la configuración se inyecta para que ninguna prueba tenga que mutar el entorno del proceso.

## ADDED Requirements

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
