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

La parada cambia de forma. Hoy espera a las peticiones en curso; con el motor tiene además que dejar de aceptar trabajo, esperar la ejecución en vuelo de forma **acotada** y unir los hilos. Acotada importa porque una copia puede durar minutos: esperarla sin límite convierte un `docker stop` en una espera que el orquestador resuelve con un `SIGKILL`, que es justo lo que la parada ordenada existía para evitar. Lo que no termine en esa espera se registra como tal y el proceso acaba igualmente.

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
