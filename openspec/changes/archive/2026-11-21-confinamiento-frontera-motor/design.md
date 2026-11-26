# Design — confinamiento de la frontera «operador → motor»

## D1 · La raíz de las copias de directorio es `KESTO_BACKUP_ROOT`, con defecto `/backup_sources`

Se adopta una variable con el prefijo `KESTO_` que este repositorio reserva para todo lo que no son las tres del §8, `DOCKER_HOST` y las `OIDC_*`.

El **defecto es `/backup_sources`** y no una constante cableada, por tres motivos:

1. Es exactamente donde el `compose` del §8 monta los directorios a respaldar (`/var/www:/backup_sources/var_www:ro`, `/etc/nginx:/backup_sources/nginx_config:ro`), así que el despliegue de referencia funciona sin declarar nada.
2. El **e2e** y el modo local copian directorios temporales que no viven bajo `/backup_sources`; con una constante tendrían que copiar a un directorio del sistema o renunciar al escenario. Con la variable apuntan la raíz a su temporal.
3. El operador puede mover su raíz sin tocar el binario.

Se descarta **derivar la raíz de los montajes del propio contenedor** (leyendo del daemon qué tiene montado Kesto): se adapta sola al despliegue, pero es frágil —en el modo local no hay contenedor de Kesto— y añade una llamada al daemon en el camino de arranque para algo que el operador sabe mejor que el daemon.

El valor SHALL ser **absoluto**. Presente pero relativo o vacío SHALL ser error de arranque que nombra la variable, en la línea de `RUST_LOG=""` y `BIND_ADDRESS=""`: un valor presente pero inutilizable no se degrada en silencio. Ausente SHALL tomar el defecto.

## D2 · La comparación es **por componentes**, no por prefijo de cadena

`/backup_sources_x` **empieza por** `/backup_sources` como cadena y **no** está dentro de él. La comparación se hace con `Path::starts_with`, que compara componentes enteros, y no con `str::starts_with`. Un confinamiento por prefijo de cadena es la forma clásica de dejar pasar justo lo que se quiere cerrar.

## D3 · El borde HTTP comprueba la forma; el motor canonicaliza

Dos comprobaciones con propósitos distintos, y las dos hacen falta:

- **En el alta y la edición** (`alta_desde`, `src/api/targets.rs:265-355`, el único punto que ya comparten `POST` y `PUT`): comprobación **textual** de forma —**absoluta**, **sin `..`** y **bajo la raíz por componentes**—. El rechazo de `..` es necesario porque `Path::starts_with` compara componentes y `..` es uno válido: `/backup_sources/../etc` pasa el filtro por prefijo y alcanza lo que no se declara. NO se exige el segmento vacío —una barra final en un `host_path` de directorio, `/backup_sources/var_www/`, es legítima—, a diferencia de `db_path`, donde sí se exige porque el valor ha de ser el nombre exacto de un fichero. Es el fallo rápido: el operador ve el `400` en el formulario y no en la primera copia. NO SHALL exigir que la ruta exista: al dar de alta un directorio puede no estar montado todavía, y exigir su existencia convertiría el alta en una comprobación del despliegue.
- **En el motor**, antes de leer o escribir: **canonicalización**. Es la que de verdad cierra el agujero, porque resuelve enlaces simbólicos —también los de los directorios **padre**, que es por donde escapa un `host_path` textualmente correcto—. Se aplica en las dos direcciones y también a las **filas anteriores a la regla**: la API no es el único camino al motor (el motor también lee targets creados antes de este change).

La canonicalización de una ruta que puede **no existir** —el destino de una restauración— se resuelve canonicalizando el **anteproyecto existente más profundo** y re-componiendo el resto textualmente: es el patrón que ya usa `normalizar_el_destino` (`src/engine/mod.rs:1460`) para la clave de reserva. La comparación final vuelve a ser por componentes contra la raíz **también canonicalizada**.

## D4 · `db_path`: la frontera es el contenedor, y la ruta ha de ser canónica

**Por qué no una raíz propia.** A diferencia de `host_path`, `db_path` vive en el **sistema de ficheros de otro contenedor**, del que Kesto no conoce la disposición. Inventar una raíz (`KESTO_SQLITE_ROOT`) obligaría a cada despliegue a declararla y dejaría de poder copiarse cualquier base que viviera en otra ruta. La frontera que sí existe y sí es comprobable es **el contenedor**, y ya está restringido por la etiqueta de opt-in (D5). Dentro de un contenedor que el operador ha declarado fuente de copia, lo que queda por cerrar es que la ruta **no escape por un enlace simbólico**.

**Cómo se comprueba.** El motor ejecuta `readlink -f <db_path>` dentro del contenedor **antes** de cualquier otra orden y exige que la salida coincida exactamente con el `db_path` declarado. Si difiere, hay un enlace simbólico o un `..` en la cadena y la ejecución se rechaza sin tocar el fichero. Esto cubre los tres vectores del hallazgo de una vez:

- `..`: `readlink -f` lo resuelve, así que la forma canónica difiere y se rechaza;
- enlace en el **último** componente: se resuelve a su destino, difiere, se rechaza;
- enlace en un directorio **padre** —el caso que un `rm -f` sigue y que borraría un fichero ajeno—: también se resuelve, difiere, se rechaza.

**Consecuencia aceptada.** Una base que viva de verdad bajo un directorio enlazado dejará de copiarse hasta que el operador declare la ruta **canónica**. Es un coste pequeño y explícito, y va documentado: la alternativa —aceptar la ruta declarada sin canonicalizar— deja abierto el vector del padre, que es el peligroso.

**Alternativa descartada: sondear con `sh -c`.** El repositorio construye los `argv` del `exec` **sin shell** a propósito (la credencial viaja por el entorno y no por la línea de órdenes). Introducir un `sh -c 'test "$(readlink -f "$1")" = "$1"'` añadiría una dependencia de shell que hoy no existe. En su lugar se captura el `stdout` de `readlink -f` y la comparación la hace Kesto.

**Requisito del contenedor.** El contenedor del target `sqlite` SHALL disponer de `readlink` —busybox o coreutils lo traen— además de `sqlite3`. Su ausencia se lee como el fallo de la ejecución, con el error del contenedor, igual que hoy con `sqlite3`.

## D5 · `container_name`: etiqueta de opt-in, no lista de configuración

Se exige que el contenedor lleve la etiqueta **`io.kesto.backup-source=true`**, comprobada con `inspect_container` antes de crear el `exec`.

Frente a una lista por configuración (`KESTO_ALLOWED_CONTAINERS`):

- es **fail-closed sin configuración**: no hay una lista que se olvide de poblar, ni un valor por defecto permisivo;
- la declaración **viaja con el contenedor**: quien crea el contenedor dice si es fuente de copia, y no hay dos sitios que mantener en sincronía;
- `kesto-app` queda denegado **por sí solo**, porque nadie lo etiqueta: no hace falta un nombre cableado que se rompa en cuanto alguien renombre el contenedor;
- el coste es simétrico y se acepta: para copiar un contenedor hay que **recrearlo** con la etiqueta. Es un cambio incompatible y va documentado.

**Por qué en el cliente y no en el motor.** Todos los `exec` de bollard pasan por `Cliente::ejecutar` y `Cliente::ejecutar_con_entrada` (`src/docker/cliente.rs:143`, `:332`). Poner la comprobación en el **único punto** que crea `exec` hace imposible que un camino nuevo del motor —hoy extracción y restauración, mañana otro— se salte la lista. Ponerla en cada camino invitaría a olvidarla en el siguiente. Es la misma razón por la que la extracción y la restauración comparten la guarda de las banderas.

Se descarta la variante «lista **o** etiqueta»: dos mecanismos para lo mismo son dos cosas que explicar, dos que probar y una que siempre se olvida.

## D6 · Dónde vive cada comprobación

- `src/engine/frontera.rs` — módulo nuevo con las funciones **puras** de alcance (forma de `host_path` y `db_path`, comparación por componentes, composición de la ruta canónica con su anteproyecto existente). Sin Docker: se prueba sin motor de contenedores. Lo usan la API (forma) y el motor (canonicalización), de modo que la regla se escribe **una vez**.
- `src/api/targets.rs` — llama a la forma en `alta_desde`, el punto que ya comparten `POST` y `PUT`.
- `src/engine/extraccion.rs` / `src/engine/restauracion.rs` — canonicalizan antes de leer o escribir, y la restauración acota el `rm -f`.
- `src/docker/cliente.rs` — la etiqueta de opt-in, antes de crear el `exec`.
- `src/config.rs` — `KESTO_BACKUP_ROOT` y su validación de forma.

El módulo `frontera` es el único sitio donde se decide qué es una ruta aceptable, y no se duplica por dirección: a diferencia de las guardas de banderas —que se duplican a propósito porque sus mensajes hablan de campos distintos—, aquí la regla es la misma para extracción y restauración y un solo sitio evita que divergan.

## D7 · Coste de compatibilidad, asumido y documentado

Los tres cambios rompen despliegues existentes y se documentan como tales en `docs/ENTORNO.md`:

| Qué deja de funcionar | Reparación |
|---|---|
| Target `directory` con `host_path` fuera de la raíz | Mover el directorio bajo `KESTO_BACKUP_ROOT` (por defecto `/backup_sources`) o declarar la raíz donde está |
| Target de base con el contenedor sin etiqueta | Recrear el contenedor con `--label io.kesto.backup-source=true` |
| Target `sqlite` con `db_path` no canónico | Declarar la ruta canónica (sin enlaces ni `..` en la cadena) |

No se migran filas: el motor rechaza la fila al usarla, con un mensaje que nombra el campo y el contenedor, y la copia deja de hacerse en vez de hacerse mal. Las filas de la tabla se quedan como están; es la **ejecución** la que se niega.

## D8 · Lo que este change NO hace

- **Separación de privilegio del motor** (SEC-06, segunda mitad): ejecutar los `exec` con un usuario sin privilegios o desde un daemon aparte es un cambio de despliegue con su propio diseño y no cabe aquí. Se abre issue de seguimiento.
- **`CHECK` en el esquema**: la coherencia de campos ya se exige en el borde HTTP y añadir restricciones obligaría a una migración de reconstrucción para filas que el motor ya rechaza.
- **Confiscar el socket del daemon**: `compose.yaml` monta `/var/run/docker.sock` en solo lectura y así se queda; el aislamiento efectivo de un socket montado es un problema de despliegue, no de código.
- **La ventana TOCTOU entre la comprobación y el uso**: entre `canonicalize` / `readlink -f` y la lectura o el `rm -f` hay una ventana en la que la ruta podría cambiar. Se declara como **límite aceptado**: no es explotable en el modelo de amenaza de este change —el operador controla el contenedor y el host, el `host_path` se lee de un montaje `:ro` y el `rm -f` corre dentro de un contenedor que el atacante ya controla—, así que cerrarla no aportaría garantía alguna y no se aborda aquí.
