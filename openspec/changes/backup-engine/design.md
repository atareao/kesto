# Design: backup-engine

## Contexto

Este change construye las tres piezas que hacen que Kesto copie de verdad: sacar el volcado del contenedor, meterlo en un repositorio Rustic sobre S3 y orquestarlo. El contrato está en `openspec/changes/backup-engine/specs/` —tres capabilities nuevas (`engine`, `extraction`, `repository`) y tres deltas (`api-resources`, `config`, `api-shell`)— y este documento fija las decisiones de forma que un implementador no tenga que volver a deducirlas.

Los hechos de la biblioteca que aparecen aquí **están medidos**, no supuestos: salen de leer el código de `rustic_core` 0.13.0, `rustic_backend` 0.7.0 y `bollard` 0.18.1, y de un *spike* de compilación que llegó a verde e incluye el caso negativo. Donde una medición desmintió una suposición previa, se dice cuál era.

## Estructura de módulos

Los tres módulos del §7 que estaban vacíos se llenan así, y la correspondencia con las capabilities no es uno a uno porque la extracción y el motor comparten el transporte:

| Módulo | Capability que implementa | Contenido |
|---|---|---|
| `src/docker/` | `extraction` (el transporte) | El cliente de la API de Docker: conexión por `DOCKER_HOST`, `exec` con la salida capturada, `inspect_exec` para el código de salida y el `stderr`. No sabe nada de volcados ni de bases. |
| `src/rustic/` | `repository` | El backend S3, la construcción del repositorio, la discriminación de los tres desenlaces, `archive`, la retención en tres operaciones, el listado de snapshots, el gancho de progreso y **el `ReadSource` propio** —el puente. |
| `src/engine/` | `engine` y `extraction` (la lógica) | La cola y el worker por repositorio, los hilos propios, el ciclo de vida de una ejecución, el hub de eventos, el comando de volcado por tipo de target, la elección de la ruta —Docker o sistema de ficheros— y el arranque y la parada. |

`src/scheduler/` **no** se toca: sigue vacío, con el change del planificador.

El `ReadSource` propio vive en `src/rustic/` y no en `src/engine/` porque es un tipo que existe **para** la API de Rustic: implementa un trait suyo, y quien lo consume es `archive()`. Ponerlo en el motor obligaría al motor a importar `rustic_core`, que es justo lo que la separación evita.

## El puente entre el `stream` de Docker y la biblioteca de copia

Es la decisión más delicada del change, y hay tres restricciones que se cruzan.

**El §4.A dibuja algo que la biblioteca no acepta.** El diagrama del SRS pone un búfer en memoria que `rustic_core` consume como `impl AsyncRead`. La API real de `rustic_core` 0.13 no tiene ninguna entrada asíncrona: `Repository::archive()` recibe un `ReadSource`, que es un trait **bloqueante** —`fn open(self) -> RusticResult<Self::Reader>` con `Reader: Read + Send + 'static`—. El puente hay que construirlo.

**El trabajo no puede correr en una tarea de Tokio.** El backend `opendal` de `rustic_backend` crea su **propio** runtime multi-hilo de Tokio y llama a `runtime().enter()` antes de cada operación. Y `archive()` es una función bloqueante que puede tardar minutos. Un `block_on` sobre el runtime que ya está atendiendo peticiones HTTP detiene el hilo que despacha y, con él, el resto de la aplicación. Por eso el trabajo va en un `std::thread` propio, y quien vuelve al runtime para escribir en la base lo hace de forma explícita con un `Handle`.

**El `ReadSource` propio es implementable desde fuera, y está demostrado.** El trait es público, `ReadSourceEntry` tiene sus tres campos `pub`, `Node::new_node` es `pub`, `Node`/`NodeType`/`Metadata` salen por `repofile` y `ReadSourceOpen` tiene *blanket impl* para todo `Read + Send + 'static`. El *spike* implementa el trait, lo pasa a `archive()` y el crate compila en verde.

Y una restricción del propio trait que decide una forma concreta: `ReadSource: Sync + Send` y `entries(&self)` recibe `&self`, mientras que un `std::sync::mpsc::Receiver` **no** es `Sync`. El receptor va en un `Mutex<Option<Receiver<_>>>` y se saca con `.take()` dentro de `entries()` —que es exactamente el truco que usa `ChildStdoutSource` con su `Mutex<Child>`—.

La forma final:

```
exec (bollard)  ──stream asíncrono──►  hilo productor  ──sync_channel acotado──►  LectorDeVolcado (Read)
                                                                                        │
                                                              ReadSourceEntry.open ─────┘
                                                                        │
                                                              archive() bloqueante
```

El canal es `sync_channel` y **acotado**: sin tope, un productor rápido contra un consumidor lento acumula el volcado entero en memoria, y la memoria retenida pasa a ser una función del tamaño de la base. Con el tope, el productor espera.

`size()` devuelve `Ok(None)` para un volcado por `stdout` —medido: el `StdinSource` de rustic hace lo mismo— y `Ok(Some(total))` para un directorio. Es la causa directa de que `percentage` sea nullable.

### El final del volcado y la espera del lector: tres cosas que la medición refutó

La primera versión de este puente declaraba el final con un mensaje por el canal y esperaba con `recv()` sin plazo. Una medición contra un repositorio local —sin S3 y sin contenedores— demostró que las dos cosas estaban mal, y también que el resultado de `archive()` no sirve para decidir si la copia está completa:

- **El final no puede viajar como mensaje.** Medido: un solo `try_send` del mensaje de final se encontró el canal **lleno** justo al terminar el volcado —con un mebibyte entero ya leído—, el final se perdió y el volcado se descartó. Ahora el final es una señal atómica que el productor pone **después** del último byte y que el lector solo da por buena cuando el canal se ha cerrado; un productor que muere —o que se olvida de declararlo— deja el cierre clasificado como «truncado», que es la dirección segura.
- **La espera del lector necesita plazo.** Medido: con el extremo de envío retenido en un campo del túnel mientras el productor moría, el lector se quedó esperando y `archive()` **no volvió nunca** —hubo que matar la ejecución a los 400 s—. El canal se cierra cuando muere el **último** emisor, así que el extremo de envío es un tipo que **no se puede clonar** y que la declaración del final consume; y la espera es `recv_timeout` con un plazo que entra por parámetro, con un error propio que nombra el volcado y el plazo.
- **El `Ok` de `archive()` no dice nada del volcado.** Medido en el peor caso: productor muerto y un padre que ya tenía el nodo del volcado → `archive()` devuelve `Ok` **sin una sola línea de aviso** y el árbol del snapshot es el del padre, de modo que un volcado que no se generó se convierte en una copia válida de los datos anteriores. La única verdad consultable es la señal de la entrega completa, que el motor pregunta por su cuenta cuando el archivado ya volvió.

Y el plazo existe **dos veces** porque son dos esperas distintas: el de la extracción acota el `stream` del `exec` —cuánto puede callarse el proceso— y el del puente acota el **canal** —cuánto puede estar el lector sin recibir nada—. Ninguno cubre al otro: el `timeout` del cliente de la API de Docker se resuelve al llegar las cabeceras y el canal se cierra cuando muere el último emisor.

## La acotación de la extracción: la inactividad, no la duración

Hay una sola cosa que acota el trabajo, y no es la que parece. El `timeout` del cliente de la API de Docker se resuelve **cuando llegan las cabeceras** de la respuesta, así que el cuerpo del `exec` se drena **fuera** de él: el tiempo máximo cubre la conexión y el apretón de manos, no el volcado.

Y la consecuencia de no acotar nada no es un fallo aislado, es un atasco permanente: un volcado colgado deja la lectura esperando indefinidamente, la ejecución se queda en `running`, y con ella queda ocupado el worker secuencial de su repositorio —de modo que todo el trabajo posterior de ese repositorio hace cola detrás para siempre—. Es exactamente la salida que la spec del motor prohíbe.

Por eso lo que se acota es **el tiempo sin recibir salida**, no la duración total. Un volcado de cien gigabytes que avanza tarda lo que tenga que tardar y no puede fallar por tardar; uno que se cuelga falla con un motivo propio que nombra el contenedor y el plazo. El valor del plazo lo decide el motor y lo pasa el transporte, que no elige ninguno por su cuenta.

Y el error tiene que ser distinguible de «terminó sin escribir nada», que es el otro desenlace posible de dejar de recibir trozos: los dos son silencio, y confundirlos convierte un colgado en un snapshot vacío dado por bueno.

## La credencial y el registro del proceso

La contraseña de la base viaja en el `env` del `exec` —es lo que fija el §4.A y lo que deja la credencial fuera de la línea de comandos del contenedor—, y eso tiene una consecuencia que **no** se cubre con tipos: la biblioteca de Docker usa el crate `log` y registra a `DEBUG` el **cuerpo serializado** de la petición, con el entorno dentro. El suscriptor de `tracing` instala el puente `log` → `tracing`, así que ese `DEBUG` acaba en el registro de Kesto.

Está medido, y el resultado es que con el nivel más hablador la contraseña se imprime **en claro**: un `Debug` propio que no publique el valor del entorno es necesario —hay uno, y tiene prueba— pero **no suficiente**, porque el valor no pasa por ningún tipo de este crate. La exposición se corta por **filtro** de registro, y por eso la directiva que silencia ese objetivo está declarada donde arranca el proceso y no queda a la memoria de quien lo lance.

La lección general sirve para el resto del change: una credencial puede salir por debajo de tus tipos, en el registro de una dependencia, y la única red que la caza es ejercitar el camino real con el registro hablador puesto.

## La cola y el worker por repositorio

§2 lo exige —«un worker secuencial por repositorio S3 para garantizar el aislamiento de *locks*»— y el motivo está medido: `rustic_core` toma un **bloqueo exclusivo del repositorio**. Dos copias contra el mismo repositorio lanzadas a la vez no corren más: se bloquean entre sí, y el que pierde espera sin que nada lo diga.

Lo que se serializa es el **repositorio**, no la tarea. Dos tareas que escriben en repositorios distintos pueden avanzar a la vez sin tocarse, y hacerlas esperar sería tiempo perdido. La estructura es un mapa `repository_id → (SyncSender, JoinHandle)`: el primero que encola un trabajo para un repositorio crea su worker, y los siguientes le mandan por el canal.

Un worker ocioso se queda **bloqueado en la recepción** del canal, así que no consume CPU: encolar un trabajo lo despierta. Y un fallo de una ejecución se apunta en su fila y devuelve el worker a su cola —nunca se propaga como un pánico— porque en un orquestador de copias lo normal es que algo falle, y un motor que hay que reiniciar cada vez que una base no responde no sirve.

## El ciclo de vida de una ejecución

Cuatro estados, y los fija el `CHECK` del §3, no este change: `pending`, `running`, `success`, `failed`. El motor es el **único** componente que escribe en `job_executions`.

`POST /api/v1/jobs/:id/run` crea la fila en `pending` y encola, y contesta `202` —no `201`: la ejecución no está hecha, está aceptada—. El worker la toma, la pasa a `running`, y al acabar escribe `success` o `failed` con `finished_at` y, si falla, un `error_message` con el motivo.

Dos reglas del `error_message` que son de seguridad y no de estilo: se **trunca** a una longitud acotada —es texto que viene de fuera del proceso, del `stderr` de un `exec` o de un error de biblioteca— y **no lleva credenciales**, porque el campo se muestra en el panel y se guarda en la base, y las dos cosas son formas de publicar un secreto.

`409` y no un segundo `202` cuando la tarea ya tiene una ejecución `pending` o `running`: encolarla otra vez no acelera nada, la pone a esperar a la primera y deja al panel con dos barras para un solo trabajo. Una tarea con `enabled = 0` **sí** se dispara a mano: `enabled` gobierna al planificador, que todavía no existe.

## El hub de eventos

`GET /api/v1/events` es un stream **global** —uno para todos los clientes, no uno por ejecución—, con `execution_progress` como único nombre de evento y los siete campos del §5.

El hub es un `tokio::sync::broadcast` con capacidad acotada, y la elección de `broadcast` sobre un canal por cliente no es casual: resuelve las dos cosas que pide el requisito sin código nuestro. Tiene un tope, así que no crece; y **descarta** lo que no cabe en vez de bloquear, así que un cliente que se queda atrás pierde eventos y no frena al motor. Un cliente que se pierde eventos no se queda a ciegas: `GET /api/v1/executions` sigue contando la verdad.

`broadcast::Receiver::recv()` devuelve `RecvError::Lagged(n)` cuando el cliente se ha perdido `n` mensajes, y eso no es un error que haya que cortar: se registra y se sigue leyendo.

## La construcción del repositorio y los tres desenlaces

El repositorio se construye desde la fila, con el `secret_access_key` y la `passphrase` **ya descifrados** por la capa de acceso. El módulo de Rustic **no** recibe la clave maestra: tenerla sería un segundo sitio del programa capaz de descifrar credenciales, que es exactamente lo que el cifrado en reposo quiere evitar.

Poner en marcha un repositorio tiene **tres** desenlaces —existe y abre, no existe todavía, existe pero no abre— y distinguirlos es obligatorio porque para el operador no se arreglan igual: «no existe, lo he creado», «existe y he conectado» y «existe y no abre, revisa la passphrase».

Y no se pueden distinguir por el error: `open()` señala «no hay fichero de configuración» con el **mismo** `ErrorKind::Configuration` con el que `init()` señala «ya hay uno». Por tipo es imposible; por texto sería atarse a una cadena de una dependencia que puede cambiar en cualquier versión menor.

La vía que sí funciona es preguntarle **al destino**. `RepositoryBackends::repository()` devuelve el `Arc<dyn WriteBackend>`, así que se conserva una copia antes de pasársela a `RepositoryBackends::new`, y sobre esa copia se llama a `list(FileType::Config)`: vacío ⇒ inicializar, con ficheros ⇒ abrir, error ⇒ el destino no se alcanza. Sin leer un solo mensaje de error.

Encadenado de tipos, que tiene su propia trampa: `init`/`open` viven en `impl Repository<()>` y devuelven `Repository<OpenStatus>`; `list`, `get_snapshots` y `prune` viven en `impl<S: Open>`; y `archive` en `impl<S: IndexedIds>`. Un `to_indexed()` en medio da `Repository<IndexedFullStatus>`, que sirve para las tres cosas.

## Lo que la copia no puede dar por bueno: el emparejamiento y el error silenciado

Dos mediciones contra un repositorio local —sin S3 y sin motor de contenedores— destaparon los dos fallos más graves de este change, y los dos son de la clase que no se ve hasta que se intenta restaurar.

**El primero: la copia de hoy con los datos de ayer.** El nodo que el puente fabrica para el volcado lleva tamaño cero y no lleva fechas ni inodo, porque no puede llevarlos —la longitud de un volcado por `stdout` no se conoce hasta que termina—. El emparejamiento de la biblioteca compara exactamente esos campos, así que dos copias del mismo destino comparan igual, la segunda **empareja**, la biblioteca **descarta el lector sin leerlo** y el snapshot nuevo apunta al contenido del anterior. Medido restaurando: la segunda copia devolvió los bytes de la primera, y su resumen declaraba «un fichero sin modificar» con cero bytes. La mitigación está medida y es desactivar el emparejamiento con el padre; y la propia biblioteca ya lo hace en su ruta de entrada estándar, así que quien llama al archivado directamente tiene que hacerlo por su cuenta. Perder la deduplicación no es una pérdida real: un origen de flujo no puede probar que no ha cambiado sin leerlo.

**El segundo: un resultado correcto que no dice nada.** El archivado **silencia** el error del lector con un aviso, se lo traga y devuelve un resultado correcto **guardando el snapshot igualmente**, sin el volcado dentro: la entrada que falló no llega al árbol, así que el fichero desaparece. Y en el peor caso medido —el productor murió y el snapshot anterior sí tenía el nodo del volcado— no hay **ni una línea de aviso**: el archivado devuelve un resultado correcto y el árbol del snapshot nuevo es idéntico al del anterior. Un volcado que no se generó acaba siendo una copia válida de los datos de ayer, en silencio.

De ahí la regla que gobierna el motor: **la condición de éxito no se hereda del archivado**. Se compone de tres cosas que el motor sí puede observar —el volcado se entregó entero, el proceso de extracción terminó con éxito y el snapshot tiene el volcado dentro— y ninguna de las tres es «el archivado no dio error».

Y una tercera medición, de operación: **reutilizar el repositorio ya abierto entre dos copias desactiva la deduplicación por completo**, porque el índice en memoria no contiene el árbol que la copia anterior acaba de escribir —la biblioteca avisa de que no encuentra el árbol del padre—. Es decir: un repositorio de larga vida no ahorra trabajo, lo esconde. El motor abre el repositorio por ejecución.

## La retención, en tres operaciones

Los tres `keep_*` del esquema **no** son parámetros del `prune`. Son reglas de retención, y `Repository` no tiene método `forget`. Confundir las dos cosas escribe un motor que purga datos que nunca dejaron de estar referenciados.

El flujo medido:

1. **Decidir**: los snapshots del repositorio —`list::<SnapshotId>()` más `get_snapshots`— entran en `KeepOptions` con los tres setters que salen de las columnas del §3, y `ForgetGroups::from_grouped_snapshots_with_retention` produce los grupos; `.into_forget_ids()` da la lista de identificadores a retirar.
2. **Retirar**: `Repository::delete_snapshots(&[SnapshotId])`.
3. **Purgar**: `PruneOptions::default()` → `prune_plan()` → `prune()`. El plan se calcula **antes** de ejecutarlo, y por eso se puede mirar.

`KeepOptions::apply` devuelve `ErrorKind::InvalidInput` si no hay ninguna regla; aquí no puede pasar, porque las tres columnas del §3 son `NOT NULL`. Las otras ocho familias de `keep_*` de la biblioteca se quedan con su valor por defecto porque el §3 no les da columna, y eso queda escrito en vez de implícito.

Una retención que falla **no** cambia el resultado de la copia: si la copia terminó bien y la purga falla, lo que hay que reportar es que la copia está y la purga no.

## El listado de snapshots

`GET /api/v1/snapshots?repository_id=<id>` obtiene la lista **del repositorio**, no de `job_executions`. La base sabe qué copias se intentaron; el repositorio sabe qué copias están, y la diferencia es lo que hace útil al explorador: un snapshot en el repositorio que no está en el histórico es una copia hecha con otra herramienta; una ejecución en el histórico sin snapshot es una copia que se dio por buena y no está.

Aquí está la segunda trampa que el *spike* cazó: `Repository::list::<SnapshotFile>()` **no compila**, porque `RepoId` exige `Copy` y `SnapshotFile` no lo es. La vía es `list::<SnapshotId>()` → identificador a **hex completo** → `get_snapshots(&[String]) -> Vec<SnapshotFile>`. Y con una trampa dentro de la trampa: el `Display` de `SnapshotId` devuelve **solo los 8 primeros caracteres hex**, es decir un prefijo ambiguo, así que hay que pasar el hex completo.

La ventana se acota a **200**, como el listado de ejecuciones, porque es la lectura más cara del change: hay que abrir el repositorio y recorrer su índice.

## El estado, el arranque y la parada

El motor entra en el `AppState` como campo **no opcional**, igual que el estado de base: un Kesto sin motor no es un estado válido para un panel cuyas tres rutas nuevas existen para dispararlo y seguirlo.

El orden del arranque es el mismo que estrenó `api-endpoints` y por la misma razón: **base abierta y migrada → motor montado → listener**. Un motor arrancado contra una base sin migrar escribe en tablas que no existen.

Un socket de Docker inalcanzable **no** impide arrancar, y es deliberado: el panel tiene que poder levantarse para enseñar que el socket no responde. Un proceso que se niega a arrancar por un socket caído deja al operador sin la herramienta con la que diagnosticarlo. El fallo aparece al ejecutar una copia.

La parada cambia de forma. Hoy espera a las peticiones en curso; con el motor tiene además que dejar de aceptar trabajo, **cerrar el reparto de eventos**, esperar la ejecución en vuelo de forma **acotada** y unir los hilos. Acotada importa porque una copia puede durar minutos: esperarla sin límite convierte un `docker stop` en una espera que el orquestador resuelve con un `SIGKILL`, que es justo lo que la parada ordenada existía para evitar. Lo que no termine en esa espera se registra como tal y el proceso acaba igualmente.

Y el reparto se cierra **antes** de esperar a las conexiones, que es lo que no se deduce del resto: el cuerpo de un `text/event-stream` no tiene final propio —su única salida es el cierre del canal— y el cierre del servidor no resuelve hasta que todas las conexiones lo hacen, así que sin cerrar el reparto el proceso se queda vivo hasta que el período de gracia del orquestador lo mata con un `SIGKILL`, con el hilo de trabajo a mitad de archivar. Medido con una espera de parada de 5 s: sin cliente el proceso muere en 2 s; con un cliente conectado seguía vivo pasados 20 s. Y hay un límite que conviene tener escrito: un cliente que **deje de leer** puede retrasar la parada, porque vaciar su búfer de escritura no está en manos del motor; lo que sí lo está —cerrar el canal— es lo que se hace, así que un cliente **que lea** deja de recibir y su conexión termina.

## Las rutas nuevas y la excepción del SSE

Las tres rutas cuelgan de la mitad protegida, y la lista blanca no crece. `GET /api/v1/events` es la **única** respuesta de la API que no viaja con la envoltura JSON: un `text/event-stream` con la envoltura dentro de un `data:` no es un `text/event-stream`.

La excepción **no** alcanza a los errores: una petición al stream sin sesión en modo OIDC contesta `401` **con** la envoltura, porque ahí sí es una respuesta HTTP normal. La envoltura se cae cuando el stream ya está abierto, no antes.

En el router eso se consigue declarando la ruta fuera de las capas que envuelven, pero **recibiendo** las dos que sí tienen que llegarle: la de sesión, que es lo que la protege, y la de traza, que es lo que permite ver en el registro que alguien se conectó. El `fallback` de ruta desconocida no cambia.

## Decisiones cerradas en la puerta de aprobación

1. **Alcance: el motor sin el planificador.** `docker` + `rustic` + `engine` y los tres endpoints. `src/scheduler` queda fuera.
2. **Las credenciales del volcado viajan por el entorno del `exec`** (`MYSQL_PWD` / `PGPASSWORD`), nunca en el `cmd`. Cierra el cabo suelto 4 del SRS.
3. **`GET /api/v1/snapshots?repository_id=<id>` entra en este change**, porque es Rustic quien los lista y Rustic llega aquí.
4. **`POST /api/v1/repositories` conecta y, si el repositorio no existe, lo inicializa**; si el destino no se alcanza o la passphrase no abre, **no deja fila**. `PUT` **no** conecta ni inicializa: solo el alta habla con el destino.
5. **`host_path` es la ruta tal como la ve Kesto dentro de su propio contenedor**, no la del host. Montar el directorio es trabajo del `compose` del §8, que ya monta `/backup_sources/...` en solo lectura.
6. **Un target `directory` no pasa por Docker**: se lee con el `LocalSource` de `rustic_core` desde el `host_path`. Se declara como añadido al SRS porque el §4 solo fija los comandos de las dos bases. Consecuencia útil: un directorio se copia aunque el socket esté caído.
7. **`percentage` es nulo cuando el total no se conoce** y numérico cuando sí. Medido: `StdinSource::size()` → `Ok(None)`; `LocalSource::size()` → `Ok(Some(total))`. Inventar un total sería pintar una cifra inventada con la misma autoridad que una de verdad.
8. **`status` en inglés y `current_action` en español.** Los valores de `status` los fija el `CHECK` del §3 y no son texto de pantalla; `current_action` sí lo es. **Contradice el ejemplo literal del §5**, que trae `current_action` en inglés; se elige el criterio «enum = inglés, pantalla = español» a sabiendas.
9. **`bytes_processed` y `bytes_uploaded` no cuentan los bytes que han llegado a S3.** El primero son los bytes leídos del `exec`; el segundo, los que el archivador ha procesado según su gancho de progreso. Se declara así en vez de dejar creer lo contrario.
10. **La ventana de `GET /snapshots` es 200**, como el listado de ejecuciones, y la respuesta indica que hay más.
11. **Solo tres de las once familias `keep_*` tienen columna** (`keep_daily`, `keep_weekly`, `keep_monthly`); el resto se queda con el valor por defecto de la biblioteca. No se añaden columnas al §3 en este change.
12. **El CI no tiene Docker.** Las pruebas van por capas para que el subconjunto que corre en CI no necesite un motor de contenedores, y lo que quede sin cubrir automáticamente se declara por escrito en `docs/FEATURE-005-backup-engine.md` en vez de fingirlo.
13. **La extracción se acota por inactividad, no por duración.** El `timeout` de la biblioteca no cubre el drenaje y sin acotar nada un volcado colgado deja ocupado el worker de su repositorio para siempre. El plazo lo decide el motor; el transporte solo lo aplica. Un volcado lento que avanza no falla nunca.
14. **La credencial de la base se protege por filtro de registro, además de por tipos.** La biblioteca registra el cuerpo de la petición a `DEBUG` con el `env` dentro, así que el arranque declara la directiva que lo corta. El `Debug` propio de la orden que lleva el entorno es necesario y no suficiente, y así queda escrito en el código.
15. **El puente declara el final con una señal y su lectura tiene plazo.** Medido: el mensaje de final se pierde con el canal lleno, y un extremo de envío retenido deja `archive()` sin volver nunca. El extremo del productor es un tipo que no se puede clonar, declarar el final lo consume, y tanto la capacidad del canal como el plazo de inactividad los elige el motor y entran por parámetro.
16. **Un volcado de flujo no empareja con el snapshot anterior.** Medido: con el emparejamiento activo, la segunda copia del mismo destino reutiliza el contenido de la primera y el snapshot de hoy contiene los datos de ayer. Se desactiva el emparejamiento con el padre, que es lo que la propia biblioteca hace para su origen de entrada estándar. La deduplicación con la copia anterior no se pierde de verdad: un origen de flujo no puede probar que no ha cambiado sin leerlo.
17. **El `Ok` del archivado no es la condición de éxito.** Medido: el archivado silencia el error del lector, devuelve un resultado correcto y guarda el snapshot sin el volcado dentro —y en el peor caso sin avisar siquiera—. El éxito lo compone el motor: volcado entregado entero, proceso de extracción con éxito y snapshot con el volcado dentro. Y el repositorio se abre **por ejecución**, porque reutilizarlo desactiva la deduplicación en silencio.

## Trampas medidas de la biblioteca

Se dejan aquí porque cada una costaría una tarde a quien no las sepa:

| Trampa | Consecuencia |
|---|---|
| `list::<SnapshotFile>()` no compila (`RepoId` exige `Copy`) | Hay que listar por `SnapshotId` y resolver con `get_snapshots` |
| `Display` de `SnapshotId` da solo los 8 primeros hex | Prefijo ambiguo: hay que pasar el hex completo |
| `SnapshotFile::from_options` cae a `std::env::args_os()` si `command` es `None` | El snapshot grabaría la línea de comandos de Kesto, con secretos si los hubiera |
| `open()` e `init()` comparten `ErrorKind::Configuration` | Los tres desenlaces se deciden preguntando al destino, no por el error |
| `snap.paths.set_paths` es `pub(crate)` | Las rutas se pasan en `backup_paths` de `archive()`, que ya lo hace |
| `keep_*` no son de `prune` sino de `KeepOptions` | La retención tiene tres operaciones, no dos |
| `PruneOptions::get_plan` está deprecado | La vía es `PrunePlan::from_prune_options` o `prune_plan()` |
| `rustic_backend` pide `rustic_core = "0"` | Unifica en una sola copia; no duplica el árbol |
| `rustic_core` re-exporta `jiff` | No hace falta añadirlo al manifiesto |
| El `stream` de `bollard` es `Pin<Box<dyn Stream>>` | Hay que drenarlo con `StreamExt` y un runtime: `futures-util` pasa de transitiva a directa |
| `bollard` registra a `DEBUG` el cuerpo serializado de la petición | Lleva el `env` del `exec` dentro, así que la contraseña de la base sale impresa con el registro hablador. Se corta por **filtro** (`bollard=warn`), no por un `Debug` nuestro |
| El `timeout` de `bollard` se resuelve al llegar las cabeceras | No acota el drenaje: hay que acotar la **inactividad** entre trozos, o un volcado colgado bloquea el worker de su repositorio para siempre |
| El prefijo de versión de API no llega al cable | `Uri::parse` compone la ruta y el `join` posterior se lleva el prefijo por delante: hoy fijar la versión es inerte, y el día que un parche lo restaure la salida es negociar la versión |
| El motor de contenedores agrupa los trozos por canal | El entrelazado fino entre `stdout` y `stderr` **no** es observable. El orden dentro de cada canal sí es contrato; el de entre canales, no |
| El nodo de un volcado de flujo empareja con el snapshot anterior | El segundo archivado del mismo destino **no lee el volcado**: el snapshot nuevo apunta al contenido del viejo. Hay que desactivar el emparejamiento con el padre |
| El archivado silencia el error del lector y devuelve un resultado correcto | Guarda el snapshot **sin el volcado dentro**, y en el peor caso sin una sola línea de aviso: la condición de éxito la tiene que componer el motor |
| Reutilizar el repositorio abierto entre copias | El índice en memoria no ve el árbol que la copia anterior escribió: se pierde la deduplicación por completo. El motor abre el repositorio por ejecución |
| `archive()` descarta con un `warn!` el error del lector y guarda el snapshot igualmente | Un `Ok` de `archive()` **no** significa que el volcado esté completo, y en el peor caso ni siquiera hay aviso: lo dice la señal de la entrega del puente, no el resultado |
| Un `sync_channel` se cierra cuando muere el **último** emisor | Un extremo de envío retenido sin usarse deja al lector esperando para siempre: es la razón del plazo en el puente y de que el extremo no se pueda clonar |
| Un `try_send` del mensaje de final se pierde con el canal lleno | El final no puede viajar como mensaje: va en una señal atómica que el productor pone después de empujar el último byte |

## Alternativas descartadas

- **`docker cp` o un volumen compartido** para sacar el volcado. Escribiría en disco lo que el §4.A pide no escribir, y en un contenedor el disco puede ser un `tmpfs` pequeño.
- **Un canal sin tope** en el puente. Convierte la memoria retenida en una función del tamaño de la base.
- **Un worker global o un `tokio::spawn` por ejecución.** El worker global serializa repositorios distintos sin motivo; el `spawn` por ejecución bloquea el runtime —`archive()` es bloqueante y el backend de opendal entra en su propio runtime— y además rompe el aislamiento de locks del §2.
- **Distinguir los tres desenlaces por el texto del error.** Ataría este change a una cadena de una dependencia que puede cambiar en cualquier versión menor.
- **Un canal por cliente en el hub.** Crecería sin tope y ataría el motor a que alguien lea.
- **`mariadb-dump --all-databases` / `pg_dumpall` como los publica el §4.A.** Ignoran `db_name`, que es el cabo suelto 3 del SRS. Se corrigen a `mariadb-dump <db>` y `pg_dump -d <db>`, que es lo que hace falta para volcar una sola base.

## Riesgos abiertos

- **El `Cargo.lock`.** Es el primer change con dependencias pesadas y el CI corre `cargo test --locked`. El árbol se resuelve una vez con red y se commitea; sin él el CI se cae en el `preflight`.
- **La negociación de versión de la API de Docker.** `AGENTS.md` avisa de que con Podman puede atascarse; si se atasca, se fija explícitamente en vez de dejar que la deduzca.
- **Los `keep_*` que el esquema no guarda.** Se quedan con el valor por defecto de la biblioteca y eso se registra; si el SRS crece, hay que añadir columnas antes de configurarlos.
- **La cobertura en CI.** Sin motor de contenedores, la parte que necesita un contenedor de verdad no se prueba automáticamente. Queda declarada por escrito, con lo que sí se prueba por capas.
