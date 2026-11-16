# api-resources — delta

## REMOVED Requirements

### Requirement: La tarea lleva un nombre legible

**Reason**: el requisito fijaba el nombre como **opcional y no único**, con el blanco guardado a `null` y `name` anulable en las respuestas. Ese contrato es el que deja a cada pantalla sin nada que pintar y obliga a inventarse un sustituto cuando el nombre falta —el hueco de #87, el `label` de #89—, y no puede convivir con un nombre obligatorio y único: un `null` en la columna y un `409` por duplicado dicen cosas distintas del mismo campo.

**Migration**: el contrato nuevo vive en el requisito «El nombre de la tarea es obligatorio y único», que reproduce las reglas que siguen valiendo —el recorte y el tope de 120 caracteres— y sustituye las que se retiran: el blanco ya no se guarda como `null` sino que contesta `400` nombrando `name`, y el listado publica `name` como cadena, nunca `null`. Ninguna respuesta de la API cambia de forma salvo eso: `name` sigue siendo una cadena, deja de ser anulable.

## ADDED Requirements

### Requirement: El nombre de la tarea es obligatorio y único

`POST /api/v1/jobs` y `PUT /api/v1/jobs/:id` SHALL **exigir** `name`, y la misma regla SHALL valer para las dos rutas —son una sola puerta de escritura, `alta_desde`—. Un cuerpo que no traiga el campo, que lo traiga a `null`, o que lo traiga vacío o en blanco **tras recortar**, SHALL contestar `400` con la envoltura y un mensaje que **nombre `name`** y diga que **falta el nombre**; si supera **120** caracteres SHALL contestar `400` con la envoltura, nombrando `name` y **sin repetir el valor**. El nombre SHALL guardarse **recortado**: los espacios de los extremos no son parte del nombre, y el tope se mide sobre lo recortado.

Un `name` que ya tenga **otra** tarea SHALL contestar **`409`** con la envoltura y un mensaje que **nombre `name`**, diga que ese nombre ya está en uso y **no repita el valor**. El mensaje del `409` SHALL ser **distinto** del de la falta de nombre: los dos rechazan la misma entrada, pero mandan a hacer cosas distintas —uno a escribir un nombre, el otro a cambiarlo—, y un panel que no los distinga obliga al operador a adivinar cuál de las dos cosas arreglar. Editar una tarea **conservando su propio nombre** NO SHALL considerarse un duplicado: una tarea no choca consigo misma.

La unicidad SHALL estar **en la base** —`UNIQUE(name)`, tal como lo declara `db`—, y el `409` SHALL derivarse de la violación de esa restricción, NO de una lectura previa: comprobar y luego escribir deja la ventana abierta a que dos altas simultáneas pasen las dos la comprobación y una escriba de todos modos. La comparación SHALL ser la de la base, sensible a mayúsculas, de modo que el `409` no prometa más ni menos de lo que la restricción garantiza.

`GET /api/v1/jobs` y `GET /api/v1/jobs/:id` SHALL publicar `name` como **cadena no vacía**, nunca `null`: la columna es `NOT NULL` y las tareas anteriores a la migración llevan el nombre derivado que ella escribió. El nombre NO SHALL omitirse de la vista.

El porqué: el nombre es la **identidad legible** de la tarea —lo que distingue dos tareas sobre el mismo target, donde antes el Histórico usaba el nombre del target como proxy y las confundía—. Anulable, cada pantalla tiene que inventarse un sustituto cuando no lo hay, y esos sustitutos se contradicen entre sí: una deja el hueco en blanco y la otra cae al identificador, que es el de la etiqueta del snapshot. Obligatorio y único, la misma cadena vale en Tareas, en el Histórico y en Snapshots, y el panel no tiene que inventar nada. Que la unicidad viva en la base es lo que la convierte en una garantía y no en una convención; y el `409` es el código que describe el choque, porque la petición **está bien** y contradice lo que ya hay —ni `400`, que diría que el cuerpo está mal escrito, ni `422`, que diría que la operación no se sabe hacer—.

#### Scenario: Un alta sin name contesta 400 nombrando name

- **GIVEN** un cuerpo de alta con target, repositorio, cron y retención válidos, y sin el campo `name`
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name`
- **AND** NO SHALL escribirse ninguna fila

#### Scenario: Un nombre en blanco contesta 400 nombrando name

- **GIVEN** un alta o una edición de tarea con `name` a una cadena de solo espacios
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name`
- **AND** NO SHALL guardarse la fila con el nombre en blanco

#### Scenario: El alta guarda el nombre recortado

- **GIVEN** un alta de tarea con `name` con espacios a ambos lados de `copia nocturna`
- **WHEN** se guarda y se lee la tarea
- **THEN** su `name` SHALL ser `copia nocturna`, sin los espacios de los extremos
- **AND** SHALL conservarse el resto de campos de la tarea

#### Scenario: Un nombre de más de 120 caracteres contesta 400 nombrando name

- **GIVEN** un alta o una edición de tarea con un `name` de 121 caracteres
- **WHEN** se envía
- **THEN** SHALL contestar `400` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name` y NO SHALL repetir el valor

#### Scenario: Un nombre duplicado en el alta contesta 409

- **GIVEN** una tarea ya dada de alta con el nombre `copia nocturna`
- **WHEN** se da de alta otra tarea con ese mismo `name`
- **THEN** SHALL contestar `409` con la envoltura
- **AND** el mensaje SHALL nombrar el campo `name` y decir que ya está en uso
- **AND** NO SHALL repetir el valor
- **AND** NO SHALL escribirse la segunda fila

#### Scenario: Un nombre duplicado en la edición contesta 409

- **GIVEN** dos tareas, `copia nocturna` y `copia diaria`
- **WHEN** se edita la segunda con el nombre de la primera
- **THEN** SHALL contestar `409` con la envoltura
- **AND** la segunda tarea SHALL conservar su nombre anterior

#### Scenario: Editar una tarea conservando su propio nombre no choca

- **GIVEN** una tarea con nombre `copia nocturna`
- **WHEN** se edita —por ejemplo, para cambiar su cron— enviando ese mismo `name`
- **THEN** SHALL contestar `200`
- **AND** su `name` SHALL seguir siendo `copia nocturna`

#### Scenario: Los dos rechazos del nombre se distinguen entre sí

- **GIVEN** un alta sin `name` y un alta con un `name` ya en uso
- **WHEN** se envían las dos
- **THEN** la primera SHALL contestar `400` y la segunda `409`
- **AND** los dos mensajes SHALL ser distintos
- **AND** SHALL poder decidirse con ellos si hay que escribir un nombre o cambiarlo

#### Scenario: La unicidad la garantiza la base

- **GIVEN** una base con las migraciones aplicadas y una tarea con nombre
- **WHEN** se inserta con SQL directo otra fila con ese mismo `name`
- **THEN** la inserción SHALL fallar
- **AND** el `409` del borde HTTP NO SHALL depender de una comprobación previa en Rust

#### Scenario: El listado de tareas publica el nombre de cada tarea

- **GIVEN** tres tareas dadas de alta —las anteriores a la migración incluidas—
- **WHEN** se pide `GET /api/v1/jobs`
- **THEN** cada fila SHALL traer su `name`
- **AND** ninguna SHALL traerlo a `null` ni omitirlo
