# db Specification (delta)

## ADDED Requirements

### Requirement: El pool se entrega al estado de la aplicación y es el único de la instancia

El pool abierto y migrado al arrancar SHALL entregarse al estado de la aplicación, que SHALL compartirlo con todos los manejadores; NO SHALL abrirse un segundo pool, y el campo del estado NO SHALL declararse **opcional**.

El aplazamiento fue deliberado y está escrito en el requisito «La base se abre en el DSN de `DATABASE_URL` y el fichero se crea si no existe» de esta capability: `db-schema` dejó el traspaso fuera porque, cuando se escribió, **no había ningún consumidor** —la capa de acceso estaba escrita, probada y auditada, y no la alcanzaba nadie desde el proceso—, y adelantarlo obligaba a elegir entre un campo **no opcional**, que llevaría una base de datos a los **16** sitios que construyen un `AppState` —once en pruebas y cinco fuera, cuatro en `src/state.rs` y uno en `src/main.rs`— y a los **61** puntos que llaman a los siete ayudantes del arnés, que pasan a ser asíncronos porque construir el estado exige una base abierta, y uno **opcional**, que dejaría escrito en el tipo que un Kesto sin base de datos es un estado posible. No lo es: sin base, el panel no arranca y no hay ni una fila que servir.

Aquel texto lo dejó como la **primera tarea del change que estrenara su primer consumidor**. Éste es ese change, y el consumidor son los manejadores del §5, así que la elección ya está tomada por la vía que la propia spec de `db` dejó abierta: el campo es **no opcional** y se lleva la base a las construcciones de estado de las pruebas, que pasan a construirse sobre una base migrada en memoria o en un temporal.

Que sea **el único** pool de la instancia no es una preferencia de estilo: SQLite admite varias conexiones al mismo fichero, así que un segundo pool no falla al abrirse —funciona— y el daño aparece más tarde y de forma intermitente, como una fila que un manejador acaba de escribir y otro no ve. Y hay un motivo de configuración: los pragmas que la base necesita —`foreign_keys` **por conexión**, WAL y el `busy_timeout`— se fijan en las opciones de conexión de **este** pool; un pool abierto por otro camino tendría que acordarse de fijarlos, y el `ON DELETE CASCADE` del §3 dejaría de ocurrir sin que nada lo delatara.

#### Scenario: El estado lleva el pool
- **GIVEN** el estado construido al arrancar la aplicación
- **WHEN** se consulta el estado
- **THEN** SHALL llevar el pool abierto y migrado al arrancar
- **AND** el campo NO SHALL ser opcional

#### Scenario: La escritura y la lectura ven la misma fila
- **GIVEN** el estado con su pool
- **WHEN** se escribe una fila por el estado y después se lee **con SQL directo** sobre el mismo fichero de la base
- **THEN** la fila SHALL estar
- **AND** SHALL tener los valores escritos

#### Scenario: Arrancar no abre una segunda base
- **GIVEN** un `DATABASE_URL` que apunta a un fichero conocido
- **WHEN** la aplicación arranca y el estado queda construido
- **THEN** SHALL existir **un** pool sobre ese fichero
- **AND** NO SHALL haberse abierto un segundo fichero ni un segundo pool sobre el mismo
