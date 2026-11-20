# arnes-de-pruebas Specification

## Purpose
El arnés de pruebas de Kesto es el conjunto de ayudantes que crean, aíslan y recogen lo que una prueba necesita del sistema —un directorio temporal, una base SQLite migrada, una clave—. Su trabajo es que cada prueba corra sobre recursos propios y que, al terminar la batería, no quede nada suyo acumulándose fuera de su sitio: ni un directorio que enturbie la siguiente corrida, ni un recurso del sistema —un inodo— agotado por no haberse recogido.

## Requirements

### Requirement: Los directorios temporales de prueba no se acumulan en /tmp

El arnés SHALL crear los directorios temporales de cada prueba **bajo una raíz única por ejecución**, en el directorio temporal del sistema con el nombre `kesto-prueba-<pid>/` (el pid del proceso de pruebas), y NO SHALL crear entradas `kesto-prueba-*` sueltas en el primer nivel del directorio temporal. Cada prueba SHALL seguir teniendo su **propio subdirectorio** bajo esa raíz, de modo que dos pruebas no compartan directorio.

Al arrancar, el arnés SHALL barrer las raíces de ejecuciones anteriores cuyo proceso **ya no esté vivo**, preguntando por `/proc/<pid>` —el mismo criterio que el arnés e2e usa con la etiqueta `kesto-e2e-pid`— y NO SHALL barrer la raíz de un proceso vivo. El barrido SHALL correr **una sola vez por proceso**.

La limpieza NO SHALL depender de que se ejecute un `Drop`: un directorio retenido por el arnés y la muerte abrupta de un proceso —un test que panica, una suite que se mata— son caminos por los que un `Drop` no pasa, y son la causa de los restos. El barrido al arrancar acota lo que puede quedar a, como mucho, la raíz de la ejecución en curso.

El barrido SHALL acotarse **estrictamente** a las entradas `kesto-prueba-*` cuyo pid no esté vivo; NO SHALL borrar ninguna otra entrada del directorio temporal.

#### Scenario: Una ejecución limpia no deja restos acumulados
- **GIVEN** una batería que corre y termina sin incidencias
- **WHEN** termina el proceso de pruebas
- **THEN** las pruebas SHALL haber corrido cada una en su subdirectorio bajo la raíz de la ejecución
- **AND** NO SHALL haber quedado entradas `kesto-prueba-*` sueltas de esa ejecución por debajo de la raíz

#### Scenario: Los restos de un proceso muerto se recogen al arrancar
- **GIVEN** una raíz `kesto-prueba-<pid>/` de una ejecución anterior cuyo proceso ya no está vivo
- **WHEN** arranca una ejecución nueva de la batería
- **THEN** el arnés SHALL borrar esa raíz antes de empezar
- **AND** el número de entradas `kesto-prueba-*` de primer nivel NO SHALL crecer entre ejecuciones consecutivas

#### Scenario: La raíz de un proceso vivo no se toca
- **GIVEN** dos ejecuciones de la batería simultáneas, cada una con su raíz `kesto-prueba-<pid>/`
- **WHEN** una de ellas barre los restos
- **THEN** SHALL conservar la raíz cuyo proceso está vivo
- **AND** SHALL borrar solo la raíz cuyo proceso ya no lo está

#### Scenario: Una prueba conserva su propio directorio
- **GIVEN** dos pruebas que piden cada una un directorio temporal
- **WHEN** cada una escribe en el suyo
- **THEN** cada una SHALL escribir en un subdirectorio distinto bajo la raíz de su ejecución
- **AND** ninguna SHALL ver los datos de la otra

#### Scenario: El barrido no toca nada que no sea suyo
- **GIVEN** un directorio temporal con entradas ajenas y con raíces `kesto-prueba-*`
- **WHEN** el arnés barre los restos
- **THEN** NO SHALL borrar ninguna entrada que no sea una raíz `kesto-prueba-*` de un proceso muerto

### Requirement: El arnés no agota los inodos en silencio

El arnés NO SHALL dejar que la batería agote los inodos del directorio temporal del sistema de forma acumulativa y **silenciosa**. Cuando el número de inodos libres esté por debajo de un umbral, el arnés SHALL dejar constancia de la situación —nombrando que quedan pocos inodos en el directorio temporal— y NO SHALL fallar por ello: un aviso ha de informar, no convertir una máquina con poco `/tmp` pero sana en un falso fallo.

El arnés SHALL ofrecer una forma **documentada** de limpiar los restos a mano: una receta `just` que borre los restos `kesto-prueba-*`, descrita en `docs/COMANDOS.md`.

#### Scenario: La suite avisa cuando quedan pocos inodos
- **GIVEN** un directorio temporal con pocos inodos libres
- **WHEN** arranca la batería
- **THEN** SHALL dejar constancia de que quedan pocos inodos
- **AND** NO SHALL fallar por ello

#### Scenario: Limpiar los restos está documentado
- **GIVEN** un desarrollador con restos `kesto-prueba-*` en su directorio temporal
- **WHEN** consulta `docs/COMANDOS.md`
- **THEN** SHALL encontrar una receta `just` para borrarlos
- **AND** su descripción SHALL decir qué borra

### Requirement: El alta de un repositorio tolera un destino aún no listo bajo carga

El arnés SHALL reintentar de forma **acotada** el alta de un repositorio ante una respuesta `502` del servidor, y NO SHALL reintentar ante ningún otro código ni tras un `2xx`. El reintento SHALL usar un número de intentos pequeño y una espera con backoff entre intentos, reenviando la misma petición cada vez.

Reintentar el alta SHALL ser seguro porque un `502` deja **cero** filas escritas: la fila del repositorio se escribe después de abrir el destino, así que un `502` no deja estado parcial que un segundo intento pudiera duplicar.

Agotados los intentos con `502`, el arnés SHALL fallar en voz alta nombrando el código de la última respuesta y su cuerpo, en vez de devolver un valor vacío.

#### Scenario: Un destino recién arrancado se reintenta hasta que responde
- **GIVEN** un arnés cuyo destino responde `502` en el primer intento y `2xx` en el siguiente
- **WHEN** el arnés da de alta un repositorio
- **THEN** SHALL reenviar la petición tras el `502`
- **AND** SHALL devolver los datos del `2xx`

#### Scenario: Un `2xx` no se reintenta
- **GIVEN** un arnés cuyo destino responde `2xx` al primer intento
- **WHEN** el arnés da de alta un repositorio
- **THEN** SHALL hacer una sola petición
- **AND** SHALL devolver sus datos

#### Scenario: Un `502` persistente falla en voz alta
- **GIVEN** un arnés cuyo destino responde `502` a todos los intentos
- **WHEN** el arnés agota los intentos
- **THEN** SHALL fallar nombrando el código de la última respuesta y su cuerpo
- **AND** NO SHALL devolver un valor vacío
