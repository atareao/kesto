//! Los treinta y tres escenarios de `verificacion-e2e`, uno por prueba.
//!
//! Cada escenario monta su propio mundo —motor, servidor S3, cubo, binario de Kesto y
//! repositorio— con [`Mundo::nuevo`] y lo recoge al terminar con [`Mundo::cerrar`]. Nada se
//! comparte entre dos escenarios: ni el almacén, ni la base del binario, ni el histórico.
//!
//! # Lo que se compara es el contenido
//!
//! Ningún escenario se conforma con el código de salida del proceso: lo que se compara es lo
//! que quedó. Las bases se leen con su cliente y se cotejan fila a fila
//! ([`comparar_filas`]), y los directorios, fichero a fichero y byte a byte
//! ([`comparar_arboles`]). El escenario de la diferencia deliberada existe justo para eso:
//! para probar que la comparación **rompe** cuando algo cambia y dice qué cambió.
//!
//! # Los escenarios que no levantan nada
//!
//! Los del CI y los de «falla en voz alta» no hablan con ningún motor: afirman sobre el
//! workflow, sobre el `.justfile` y sobre las funciones puras del arnés. Son los que
//! comprueban que la batería **no se salta nada en silencio**, y por eso no pueden depender
//! de que el entorno esté bien.

use std::path::Path;
use std::time::{Duration, Instant};

use reqwest::{Method, StatusCode};
use serde_json::json;

use super::arnes::{
    ACCESO_S3, BASE, CLAVE_DE_LA_BASE, ESPERA_ENTRE_SONDEOS, Kesto, Motor, Mundo,
    PLAZO_DE_ARRANQUE, PLAZO_DE_INACTIVIDAD, PLAZO_DE_LA_COPIA, PLAZO_DE_LA_RESTAURACION, Receta,
    SECRETO_S3, Salida, comparar_arboles, comparar_filas, copiar_arbol, esperar_arranque,
    esperar_el_s3, imagen_de_alpine, imagen_de_mariadb, imagen_de_postgres, puerto_libre,
    resolver_el_socket, sonda_de_mariadb, sonda_de_postgres,
};

// ── La siembra y la comparación ────────────────────────────────────────────

/// Las filas de la siembra por defecto de las bases.
const FILAS: usize = 8;

/// Las filas de la siembra del escenario del `stdin`: un volcado de cientos de kibibytes, para
/// que el camino S3 → `volcar_a` → `stdin` tenga algo que transportar de verdad.
const FILAS_DEL_VOLUMEN: usize = 2000;

/// El texto determinista de la fila `id`.
///
/// Sin `|`, que es el separador del listado, y sin comillas, que es lo que el SQL de la
/// siembra necesita: el texto entra en la sentencia tal cual.
fn texto_de(id: usize) -> String {
    format!("fila-{id:04}-{}", "kesto".repeat(40))
}

/// El SQL que crea la tabla y siembra `filas` filas conocidas, con `prefijo` delante del texto.
///
/// Vale para los tres motores: `VARCHAR(240)` y `INT PRIMARY KEY` los aceptan MariaDB,
/// PostgreSQL y SQLite. Viaja por la **entrada estándar** del `exec` y no por un argumento:
/// el guion de las dos mil filas del escenario del `stdin` no cabe en un argumento —el límite
/// del kernel para uno suelto ronda los 128 KiB— y el motor contestaría «Argument list too
/// long» antes de ejecutar nada.
///
/// `prefijo` existe para el escenario del volcado global, que siembra **dos** bases con el mismo
/// número de filas: con el mismo texto en las dos, un reparto mal hecho —todo en una sola base—
/// sería indistinguible del bueno.
fn siembra_con(prefijo: &str, filas: usize) -> String {
    let mut sql =
        String::from("CREATE TABLE filas (id INT PRIMARY KEY, texto VARCHAR(240) NOT NULL);\n");

    for id in 1..=filas {
        sql.push_str(&format!(
            "INSERT INTO filas (id, texto) VALUES ({id}, '{prefijo}{}');\n",
            texto_de(id)
        ));
    }

    sql
}

/// El SQL de la siembra por defecto: sin prefijo, la de los escenarios de un solo origen.
fn siembra(filas: usize) -> String {
    siembra_con("", filas)
}

/// El listado que una siembra con `prefijo` tiene que producir: `id|texto`, una fila por línea.
fn listado_con(prefijo: &str, filas: usize) -> String {
    let mut salida = String::new();

    for id in 1..=filas {
        salida.push_str(&format!("{id}|{prefijo}{}\n", texto_de(id)));
    }

    salida
}

/// El listado de la siembra por defecto.
fn listado_esperado(filas: usize) -> String {
    listado_con("", filas)
}

/// Ejecuta un guion dentro de un contenedor y **exige** que terminara bien.
async fn guion(motor: &Motor, contenedor: &str, orden: &str, que: &str) -> Salida {
    let salida = motor
        .ejecutar_guion(contenedor, orden)
        .await
        .unwrap_or_else(|motivo| panic!("{motivo}"));

    salida.exigir_cero(que);

    salida
}

/// Ejecuta una orden entregándole `entrada` por su entrada estándar, y **exige** que terminara
/// bien.
///
/// Los datos van por el `stdin` del `exec` y no por un argumento: es lo que permite sembrar dos
/// mil filas sin que el motor conteste «Argument list too long».
async fn guion_con_entrada(
    motor: &Motor,
    contenedor: &str,
    cmd: &[&str],
    env: &[&str],
    entrada: &str,
    que: &str,
) -> Salida {
    let salida = motor
        .ejecutar_con_entrada(contenedor, cmd, env, entrada.as_bytes())
        .await
        .unwrap_or_else(|motivo| panic!("{motivo}"));

    salida.exigir_cero(que);

    salida
}

/// La única instantánea de un repositorio, exigiendo que sea exactamente una.
async fn la_unica_instantanea(kesto: &Kesto, repositorio: &str) -> String {
    let instantaneas = kesto.instantaneas(repositorio).await;

    assert_eq!(
        instantaneas.len(),
        1,
        "el repositorio tenía que tener una sola instantánea y tiene {}: {instantaneas:?}",
        instantaneas.len()
    );

    instantaneas[0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("la instantánea no trajo `id`: {}", instantaneas[0]))
        .to_owned()
}

/// Da de alta una tarea sobre un target, la copia una vez y devuelve su instantánea.
async fn copiar_una_vez(kesto: &Kesto, target: &str, repositorio: &str, nombre: &str) -> String {
    let tarea = kesto.crear_tarea(target, repositorio, nombre, 7).await;
    kesto
        .copiar_exigiendo_exito(&tarea, PLAZO_DE_LA_COPIA)
        .await;

    la_unica_instantanea(kesto, repositorio).await
}

// ── MariaDB ────────────────────────────────────────────────────────────────

/// Levanta un MariaDB con su base sembrable y espera a que conteste.
async fn mariadb_arrancado(mundo: &mut Mundo, sufijo: &str) -> String {
    mariadb_con_etiqueta(mundo, sufijo, true).await
}

/// Arranca un MariaDB **sin** la etiqueta de fuente de copia.
///
/// Es el contenedor de la prueba negativa del confinamiento: el motor no le crea ningún `exec`,
/// así que una copia de él no puede salir adelante.
async fn mariadb_arrancado_sin_etiqueta(mundo: &mut Mundo, sufijo: &str) -> String {
    mariadb_con_etiqueta(mundo, sufijo, false).await
}

/// Arranca un MariaDB, con o sin la etiqueta que lo declara fuente de copia.
async fn mariadb_con_etiqueta(mundo: &mut Mundo, sufijo: &str, fuente_de_copia: bool) -> String {
    let nombre = mundo.nombre_de_contenedor(sufijo);
    let imagen = imagen_de_mariadb();
    let contrasena = format!("MARIADB_ROOT_PASSWORD={CLAVE_DE_LA_BASE}");
    let base = format!("MARIADB_DATABASE={BASE}");

    mundo
        .contenedor(Receta {
            nombre: &nombre,
            imagen: &imagen,
            cmd: &[],
            env: &[&contrasena, &base],
            puerto: None,
            montaje: None,
            // Se copia de ella y se restaura en ella, así que el motor tiene que poder crear
            // `exec` en su interior: se declara fuente de copia. La prueba negativa la deja sin
            // etiqueta a propósito.
            fuente_de_copia,
        })
        .await;

    let sonda = sonda_de_mariadb();
    let orden = sonda.argv();
    let entorno = sonda.env();
    let argv = orden.as_slice();
    let env = entorno.as_slice();
    let motor = &mundo.motor;
    let contenedor = nombre.as_str();

    // Con supervivencia entre sondeos: el servidor temporal del arranque —que ya tiene la base
    // creada— se apaga antes del definitivo, así que un acierto único no basta.
    esperar_arranque(
        contenedor,
        argv,
        PLAZO_DE_ARRANQUE,
        ESPERA_ENTRE_SONDEOS,
        move || async move {
            match motor.ejecutar(contenedor, argv, env).await {
                Ok(salida) if salida.codigo == 0 => Ok(()),
                Ok(salida) => Err(format!("código {} · {}", salida.codigo, salida.error)),
                Err(causa) => Err(causa),
            }
        },
    )
    .await
    .unwrap_or_else(|motivo| panic!("{motivo}"));

    nombre
}

/// Siembra las filas conocidas en un MariaDB.
async fn sembrar_mariadb(mundo: &Mundo, contenedor: &str, filas: usize) {
    sql_mariadb(mundo, contenedor, &siembra(filas), "la siembra del MariaDB").await;
}

/// Ejecuta SQL contra un MariaDB por su entrada estándar.
///
/// Es la forma de hablar con su base para lo que no cubren los ayudantes de siembra y lectura:
/// el escenario de los valores que parecen un metacomando crea y vacía su propia tabla.
async fn sql_mariadb(mundo: &Mundo, contenedor: &str, sql: &str, que: &str) {
    let clave = format!("-p{CLAVE_DE_LA_BASE}");
    let cmd = ["mariadb", "-uroot", &clave, BASE];

    guion_con_entrada(&mundo.motor, contenedor, &cmd, &[], sql, que).await;
}

/// Lee las filas de un MariaDB en el formato del listado esperado.
async fn leer_mariadb(mundo: &Mundo, contenedor: &str) -> String {
    leer_mariadb_de(mundo, contenedor, "filas").await
}

/// Lee el listado `id|texto` de la tabla `tabla` de un MariaDB.
async fn leer_mariadb_de(mundo: &Mundo, contenedor: &str, tabla: &str) -> String {
    let orden = format!(
        "mariadb -uroot -p{CLAVE_DE_LA_BASE} -N -B {BASE} -e \
         \"SELECT CONCAT(id, '|', texto) FROM {tabla} ORDER BY id\""
    );

    guion(&mundo.motor, contenedor, &orden, "la lectura del MariaDB")
        .await
        .salida
}

/// Vacía de filas el destino: lo deja «limpio» para que la restauración tenga que escribirlo.
async fn limpiar_mariadb(mundo: &Mundo, contenedor: &str) {
    let orden =
        format!("mariadb -uroot -p{CLAVE_DE_LA_BASE} {BASE} -e 'DROP TABLE IF EXISTS filas'");

    guion(&mundo.motor, contenedor, &orden, "la limpieza del MariaDB").await;
}

/// El target de un MariaDB ya arrancado.
async fn target_mariadb(kesto: &Kesto, nombre: &str, contenedor: &str) -> String {
    kesto
        .crear_target(json!({
            "name": nombre,
            "target_type": "mariadb",
            "container_name": contenedor,
            "db_user": "root",
            "db_password": CLAVE_DE_LA_BASE,
            "db_name": BASE,
        }))
        .await
}

// ── PostgreSQL ─────────────────────────────────────────────────────────────

/// Levanta un PostgreSQL con su base sembrable y espera a que acepte conexiones.
async fn postgres_arrancado(mundo: &mut Mundo, sufijo: &str) -> String {
    let nombre = mundo.nombre_de_contenedor(sufijo);
    let imagen = imagen_de_postgres();
    let contrasena = format!("POSTGRES_PASSWORD={CLAVE_DE_LA_BASE}");
    let base = format!("POSTGRES_DB={BASE}");

    mundo
        .contenedor(Receta {
            nombre: &nombre,
            imagen: &imagen,
            cmd: &[],
            env: &[&contrasena, &base],
            puerto: None,
            montaje: None,
            // Se copia de ella y se restaura en ella: se declara fuente de copia.
            fuente_de_copia: true,
        })
        .await;

    let sonda = sonda_de_postgres();
    let orden = sonda.argv();
    let entorno = sonda.env();
    let argv = orden.as_slice();
    let env = entorno.as_slice();
    let motor = &mundo.motor;
    let contenedor = nombre.as_str();

    // El entrypoint oficial de PostgreSQL tiene el mismo servidor temporal que el de MariaDB
    // —crea `POSTGRES_DB` en él y lo para antes del definitivo—, así que exige la misma
    // supervivencia entre sondeos.
    esperar_arranque(
        contenedor,
        argv,
        PLAZO_DE_ARRANQUE,
        ESPERA_ENTRE_SONDEOS,
        move || async move {
            match motor.ejecutar(contenedor, argv, env).await {
                Ok(salida) if salida.codigo == 0 => Ok(()),
                Ok(salida) => Err(format!("código {} · {}", salida.codigo, salida.error)),
                Err(causa) => Err(causa),
            }
        },
    )
    .await
    .unwrap_or_else(|motivo| panic!("{motivo}"));

    // Y se pregunta qué `psql` es: `\restrict` —la segunda capa— existe desde 17.6, y la batería
    // tiene que saber qué cliente está midiendo. Guarda del arranque, no escenario.
    exigir_psql_con_restrict(mundo, &nombre).await;

    nombre
}

/// Siembra las filas conocidas en un PostgreSQL.
async fn sembrar_postgres(mundo: &Mundo, contenedor: &str, filas: usize) {
    sql_postgres(
        mundo,
        contenedor,
        BASE,
        &siembra(filas),
        "la siembra del PostgreSQL",
    )
    .await;
}

/// Ejecuta SQL contra **una base concreta** de un PostgreSQL, por su entrada estándar.
///
/// Es la forma de hablar con una base que no es la del arnés: el escenario del volcado global
/// crea, siembra y borra bases propias, y las tres cosas necesitan su propia conexión. `-X`
/// ignora el `psqlrc` —que un `\set` ajeno cambiaría el resultado— y `ON_ERROR_STOP` hace que un
/// error se note en vez de dejar una base a medias.
async fn sql_postgres(mundo: &Mundo, contenedor: &str, base: &str, sql: &str, que: &str) {
    let clave = format!("PGPASSWORD={CLAVE_DE_LA_BASE}");
    let cmd = [
        "psql",
        "-X",
        "-U",
        "postgres",
        "-d",
        base,
        "-v",
        "ON_ERROR_STOP=1",
    ];

    guion_con_entrada(&mundo.motor, contenedor, &cmd, &[&clave], sql, que).await;
}

/// Crea una base de datos propia en un PostgreSQL, conectando a la de mantenimiento.
async fn crear_base_postgres(mundo: &Mundo, contenedor: &str, base: &str) {
    sql_postgres(
        mundo,
        contenedor,
        "postgres",
        &format!("CREATE DATABASE {base};\n"),
        "la creación de la base",
    )
    .await;
}

/// Borra una base de datos propia de un PostgreSQL, conectando a la de mantenimiento.
///
/// El destino se deja limpio **de verdad**: sin la base, el `CREATE DATABASE` del volcado global
/// tiene que volver a crearla, que es justo lo que mide el reparto por secciones.
async fn borrar_base_postgres(mundo: &Mundo, contenedor: &str, base: &str) {
    sql_postgres(
        mundo,
        contenedor,
        "postgres",
        &format!("DROP DATABASE IF EXISTS {base};\n"),
        "el borrado de la base",
    )
    .await;
}

/// Siembra la tabla `filas` de una base propia de un PostgreSQL con los datos de **esa** base.
async fn sembrar_base_postgres(mundo: &Mundo, contenedor: &str, base: &str, filas: usize) {
    let prefijo = format!("{base}-");

    sql_postgres(
        mundo,
        contenedor,
        base,
        &siembra_con(&prefijo, filas),
        "la siembra de la base",
    )
    .await;
}

/// Lee las filas de un PostgreSQL en el formato del listado esperado.
async fn leer_postgres(mundo: &Mundo, contenedor: &str) -> String {
    leer_postgres_de(mundo, contenedor, BASE, "filas").await
}

/// Lee el listado `id|texto` de la tabla `tabla` de una base concreta de un PostgreSQL.
async fn leer_postgres_de(mundo: &Mundo, contenedor: &str, base: &str, tabla: &str) -> String {
    let orden = format!(
        "PGPASSWORD={CLAVE_DE_LA_BASE} psql -X -A -t -U postgres -d {base} -c \
         \"SELECT id || '|' || texto FROM {tabla} ORDER BY id\""
    );

    guion(
        &mundo.motor,
        contenedor,
        &orden,
        "la lectura del PostgreSQL",
    )
    .await
    .salida
}

/// Vacía de filas el destino de un PostgreSQL.
async fn limpiar_postgres(mundo: &Mundo, contenedor: &str) {
    sql_postgres(
        mundo,
        contenedor,
        BASE,
        "DROP TABLE IF EXISTS filas;\n",
        "la limpieza del PostgreSQL",
    )
    .await;
}

// ── SQLite ─────────────────────────────────────────────────────────────────

/// La ruta del fichero SQLite dentro del contenedor de apoyo.
const BASE_SQLITE: &str = "/datos/base.db";

/// Levanta un contenedor con `sqlite3` instalado y espera a que esté.
///
/// `alpine` no trae el binario, así que se instala al arrancar; la espera sondea el binario
/// —no el contenedor—, que es lo que la extracción necesita de verdad.
async fn alpine_con_sqlite(mundo: &mut Mundo, sufijo: &str) -> String {
    let nombre = mundo.nombre_de_contenedor(sufijo);
    let imagen = imagen_de_alpine();

    mundo
        .contenedor(Receta {
            nombre: &nombre,
            imagen: &imagen,
            cmd: &[
                "sh",
                "-c",
                "apk add --no-cache sqlite >/dev/null && mkdir -p /datos && sleep infinity",
            ],
            env: &[],
            puerto: None,
            montaje: None,
            // Se copia de ella y se restaura en ella: se declara fuente de copia.
            fuente_de_copia: true,
        })
        .await;

    mundo
        .motor
        .esperar_orden(&nombre, &["sqlite3", "-version"], &[], PLAZO_DE_ARRANQUE)
        .await
        .unwrap_or_else(|motivo| panic!("{motivo}"));

    // Que el binario esté no basta: se pregunta por su versión —`-safe` existe desde 3.27— y por
    // `readlink`, que es lo que la restauración necesita para resolver la forma canónica de la
    // ruta antes de retirar el fichero. Son guardas del arranque, no escenarios.
    exigir_sqlite_con_safe(mundo, &nombre).await;
    exigir_readlink(mundo, &nombre).await;

    nombre
}

/// Siembra las filas conocidas en un fichero SQLite.
///
/// `-bail` es lo que hace que un error de la siembra se note: sin él, `sqlite3` sigue con la
/// sentencia siguiente y termina con código cero, y una siembra a medias pasaría por buena.
async fn sembrar_sqlite(mundo: &Mundo, contenedor: &str, filas: usize) {
    guion_con_entrada(
        &mundo.motor,
        contenedor,
        &["sqlite3", "-bail", BASE_SQLITE],
        &[],
        &siembra(filas),
        "la siembra del SQLite",
    )
    .await;
}

/// Lee las filas de un fichero SQLite en el formato del listado esperado.
async fn leer_sqlite(mundo: &Mundo, contenedor: &str) -> String {
    let orden = format!(
        "sqlite3 -noheader -readonly {BASE_SQLITE} \
         \"SELECT id || '|' || texto FROM filas ORDER BY id\""
    );

    guion(&mundo.motor, contenedor, &orden, "la lectura del SQLite")
        .await
        .salida
}

/// Vacía de filas el destino de un SQLite.
async fn limpiar_sqlite(mundo: &Mundo, contenedor: &str) {
    let orden = format!("rm -f {BASE_SQLITE} {BASE_SQLITE}-wal {BASE_SQLITE}-shm");

    guion(&mundo.motor, contenedor, &orden, "la limpieza del SQLite").await;
}

// ── Las guardas de arranque de los contenedores ────────────────────────────

/// La versión mínima de `sqlite3` que la restauración necesita: `-safe` existe desde 3.27.
const SQLITE_MINIMO: (u32, u32) = (3, 27);

/// La versión mínima de `psql` que trae `\restrict`: 17.6.
const PSQL_MINIMO: (u32, u32) = (17, 6);

/// La versión mayor y menor del primer número con forma `X.Y` de una salida de `--version`.
///
/// No vale con tomar el primer token: `sqlite3 --version` la abre (`3.45.1 …`) pero
/// `psql --version` la lleva dentro (`psql (PostgreSQL) 18.6`). Se busca el primer token que
/// empiece por dígito y tenga un punto, que es la versión en las dos formas.
fn version_de(salida: &str) -> Option<(u32, u32)> {
    salida.split_whitespace().find_map(|token| {
        let (mayor, resto) = token.trim_start_matches('(').split_once('.')?;

        Some((mayor.parse().ok()?, resto.split('.').next()?.parse().ok()?))
    })
}

/// Exige que el `sqlite3` del contenedor sea lo bastante nuevo para `-safe`.
///
/// El arnés no lo supone: lo **pregunta**. `-safe` es la opción con la que la restauración ignora
/// los metacomandos del `.dump` (`.shell`, `.system`…), y existe desde SQLite 3.27; un `sqlite3`
/// más viejo aceptaría el `exec` y aplicaría el volcado **sin** la guarda, así que la falta de
/// herramienta tiene que hacer fallar el escenario, no colarse por buena.
async fn exigir_sqlite_con_safe(mundo: &Mundo, contenedor: &str) {
    let salida = guion(
        &mundo.motor,
        contenedor,
        "sqlite3 --version",
        "la versión del sqlite3",
    )
    .await;
    let version = salida.salida.trim();

    let Some((mayor, menor)) = version_de(version) else {
        panic!("no se pudo leer la versión de `sqlite3 --version`: {version:?}");
    };

    assert!(
        (mayor, menor) >= SQLITE_MINIMO,
        "el `sqlite3` del contenedor es `{version}` y la restauración necesita \
         {SQLITE_MINIMO:?} o más para `-safe`: sin esa opción el volcado se aplicaría sin la \
         guarda que ignora sus metacomandos"
    );
}

/// Exige que el contenedor traiga `readlink`.
///
/// Lo usa la comprobación de forma canónica de la ruta del SQLite antes de retirar el fichero:
/// sin él, la restauración no podría resolver un enlace en un directorio padre y el borrado
/// previo a la reimportación apuntaría a un fichero que no es el declarado.
async fn exigir_readlink(mundo: &Mundo, contenedor: &str) {
    guion(
        &mundo.motor,
        contenedor,
        "command -v readlink",
        "la comprobación de que existe readlink",
    )
    .await;
}

/// Exige que el `psql` del contenedor soporte `\restrict`.
///
/// `\restrict` es la **segunda** capa del endurecimiento —la que rechaza cualquier metacomando
/// que el troceador dejara escapar— y existe desde `psql` 17.6. Es una **precondición
/// obligatoria**: la restauración **falla** con un `psql` que no lo honre (ver el escenario
/// [`un_psql_sin_restrict_falla_la_restauracion_sin_restaurar_nada`]), así que un contenedor que
/// no lo traiga mediría otra cosa —una restauración que ni siquiera se ejecuta—. La guarda se
/// conserva en el arranque de la batería para que eso se note **aquí**, al preparar el mundo, y
/// no como un fallo confuso más adelante.
async fn exigir_psql_con_restrict(mundo: &Mundo, contenedor: &str) {
    let salida = guion(
        &mundo.motor,
        contenedor,
        "psql --version",
        "la versión del psql",
    )
    .await;
    let version = salida.salida.trim();

    let Some((mayor, menor)) = version_de(version) else {
        panic!("no se pudo leer la versión de `psql --version`: {version:?}");
    };

    assert!(
        (mayor, menor) >= PSQL_MINIMO,
        "el `psql` del contenedor es `{version}` y la segunda capa del endurecimiento \
         (`\\restrict`) necesita {PSQL_MINIMO:?} o más: con un `psql` viejo la batería estaría \
         midiendo otra cosa"
    );
}

/// La imagen de PostgreSQL cuyo `psql` es **17.5**: justo por debajo del mínimo de `\restrict`.
///
/// Es la opción preferida del escenario negativo: mide el borde —una rama 17 anterior al parche de
/// 2025-08-14— sin alejarse de la rama que la batería usa en el resto de escenarios.
const IMAGEN_POSTGRES_SIN_RESTRICT: &str = "docker.io/library/postgres:17.5-alpine";

/// La imagen de respaldo: su `psql` es **16.9**, por debajo del mínimo 16.10 de su rama.
///
/// Se usa cuando la de 17.5 no se puede traer —una etiqueta antigua que un espejo puede no
/// conservar—: sigue midiendo lo mismo, un cliente sin `\restrict`.
const IMAGEN_POSTGRES_SIN_RESTRICT_DE_RESPALDO: &str = "docker.io/library/postgres:16.9-alpine";

/// La imagen de PostgreSQL sin `\restrict` que la batería consigue traer.
///
/// Es la **guarda de disponibilidad** del escenario negativo: primero la de 17.5, después la de
/// respaldo, y si ninguna se puede traer el escenario **falla nombrando las dos** —no puede medir
/// sin un cliente que no honre `\restrict`, y darlo por bueno sería fingir el escenario—.
async fn imagen_de_postgres_sin_restrict(mundo: &Mundo) -> String {
    for candidata in [
        IMAGEN_POSTGRES_SIN_RESTRICT,
        IMAGEN_POSTGRES_SIN_RESTRICT_DE_RESPALDO,
    ] {
        if mundo.motor.asegurar_imagen(candidata).await.is_ok() {
            return candidata.to_owned();
        }
    }

    panic!(
        "el escenario del `psql` sin `\\restrict` necesita una imagen de PostgreSQL anterior al \
         ciclo de parches de 2025-08-14 (`{IMAGEN_POSTGRES_SIN_RESTRICT}` o, en su defecto, \
         `{IMAGEN_POSTGRES_SIN_RESTRICT_DE_RESPALDO}`) y ninguna se pudo traer"
    );
}

/// Levanta un PostgreSQL de una imagen concreta **sin** exigir `\restrict`.
///
/// Es el arranque de [`postgres_arrancado`] menos su guarda [`exigir_psql_con_restrict`]: este
/// escenario mide justo lo contrario —que un `psql` sin `\restrict` **falla** la restauración—,
/// así que exigir la guarda sería exigir lo que se quiere negar. El resto de escenarios la
/// conservan.
async fn postgres_sin_restrict_arrancado(mundo: &mut Mundo, sufijo: &str, imagen: &str) -> String {
    let nombre = mundo.nombre_de_contenedor(sufijo);
    let contrasena = format!("POSTGRES_PASSWORD={CLAVE_DE_LA_BASE}");
    let base = format!("POSTGRES_DB={BASE}");

    mundo
        .contenedor(Receta {
            nombre: &nombre,
            imagen,
            cmd: &[],
            env: &[&contrasena, &base],
            puerto: None,
            montaje: None,
            // Se copia de ella y se restaura en ella: se declara fuente de copia.
            fuente_de_copia: true,
        })
        .await;

    let sonda = sonda_de_postgres();
    mundo
        .motor
        .esperar_orden(&nombre, &sonda.argv(), &sonda.env(), PLAZO_DE_ARRANQUE)
        .await
        .unwrap_or_else(|motivo| panic!("{motivo}"));

    nombre
}

/// Si existe una base en un PostgreSQL: es lo que mide «no se restauró nada».
async fn existe_la_base_postgres(mundo: &Mundo, contenedor: &str, base: &str) -> bool {
    let orden = format!(
        "PGPASSWORD={CLAVE_DE_LA_BASE} psql -X -A -t -U postgres -d postgres -c \
         \"SELECT 1 FROM pg_database WHERE datname = '{base}'\""
    );

    !guion(
        &mundo.motor,
        contenedor,
        &orden,
        "la comprobación de que la base existe",
    )
    .await
    .salida
    .trim()
    .is_empty()
}

// ── Directorio ─────────────────────────────────────────────────────────────

/// Siembra un árbol conocido: ficheros de texto, uno binario y dos niveles de anidamiento.
fn sembrar_directorio(raiz: &Path) {
    std::fs::create_dir_all(raiz.join("sub/anidado"))
        .unwrap_or_else(|causa| panic!("no se pudo crear `{}`: {causa}", raiz.display()));

    std::fs::write(raiz.join("a.txt"), "contenido de a\n")
        .unwrap_or_else(|causa| panic!("no se pudo escribir `a.txt`: {causa}"));
    std::fs::write(raiz.join("sub/b.bin"), (0u8..=255).collect::<Vec<u8>>())
        .unwrap_or_else(|causa| panic!("no se pudo escribir `sub/b.bin`: {causa}"));
    std::fs::write(raiz.join("sub/anidado/c.txt"), "contenido de c\n")
        .unwrap_or_else(|causa| panic!("no se pudo escribir `sub/anidado/c.txt`: {causa}"));
}

/// Nombre del fichero suelto que sobra en el destino de la sobrescritura.
const FICHERO_OBSOLETO: &str = "obsoleto.txt";
/// Nombre del directorio **no vacío** que sobra en el destino de la sobrescritura.
const DIRECTORIO_OBSOLETO: &str = "sobrante";

/// Deja el directorio con contenido previo: el «destino con contenido» de la sobrescritura.
///
/// Siembra los **dos** tipos de resto que la poda tiene que retirar —un fichero suelto en la raíz y
/// un directorio **no vacío**—, porque no se retiran por el mismo camino: el fichero, por el
/// borrado de ficheros; el directorio, por el recursivo. Un destino solo con un fichero dejaría el
/// segundo sin medir.
fn sembrar_previo(raiz: &Path) {
    if raiz.exists() {
        std::fs::remove_dir_all(raiz)
            .unwrap_or_else(|causa| panic!("no se pudo vaciar `{}`: {causa}", raiz.display()));
    }

    let dentro = raiz.join(DIRECTORIO_OBSOLETO).join("anidado");
    std::fs::create_dir_all(&dentro)
        .unwrap_or_else(|causa| panic!("no se pudo crear `{}`: {causa}", dentro.display()));
    std::fs::write(raiz.join(FICHERO_OBSOLETO), "contenido anterior\n")
        .unwrap_or_else(|causa| panic!("no se pudo escribir `{FICHERO_OBSOLETO}`: {causa}"));
    std::fs::write(dentro.join("resto.bin"), "contenido anterior\n").unwrap_or_else(|causa| {
        panic!("no se pudo escribir dentro de `{DIRECTORIO_OBSOLETO}`: {causa}")
    });
}

/// El target de un directorio del host.
async fn target_directorio(kesto: &Kesto, nombre: &str, ruta: &Path) -> String {
    kesto
        .crear_target(json!({
            "name": nombre,
            "target_type": "directory",
            "host_path": ruta
                .to_str()
                .unwrap_or_else(|| panic!("la ruta `{}` no es UTF-8", ruta.display())),
        }))
        .await
}

// ══ Requisito: una base se da por arrancada cuando contesta por su base ════

/// Escenario: la espera de PostgreSQL no se conforma con que el servidor escuche.
///
/// `pg_isready` contesta que sí durante la fase temporal del arranque de la imagen oficial
/// —un servidor escuchando con `POSTGRES_DB` todavía sin crear—, y quien siembre entonces se
/// encuentra con un `database "pruebas" does not exist`. La sonda tiene que **consultar** la
/// base sembrable, que es lo único que falla hasta que la base está.
#[test]
fn la_sonda_de_postgres_consulta_la_base_sembrable() {
    let sonda = sonda_de_postgres();

    assert!(
        !sonda
            .cmd
            .iter()
            .any(|argumento| argumento.contains("pg_isready")),
        "`pg_isready` no distingue el servidor temporal del definitivo: {}",
        sonda.cmd.join(" ")
    );
    assert!(
        sonda.cmd.iter().any(|argumento| argumento == "SELECT 1"),
        "la sonda tiene que ser una consulta de verdad: {}",
        sonda.cmd.join(" ")
    );
    assert!(
        sonda.cmd.iter().any(|argumento| argumento == BASE),
        "la consulta tiene que ir contra `{BASE}` y no contra la base por defecto: {}",
        sonda.cmd.join(" ")
    );
    assert!(
        sonda
            .entorno
            .iter()
            .any(|variable| variable == &format!("PGPASSWORD={CLAVE_DE_LA_BASE}")),
        "sin `PGPASSWORD` la consulta no puede autenticarse: {}",
        sonda.entorno.join(" ")
    );
}

/// Escenario: la espera de MariaDB no se conforma con que su cliente conteste.
///
/// El cliente que no nombra base —`mariadb -uroot -e 'SELECT 1'`— contesta que sí contra el
/// servidor temporal del arranque, antes de que `MARIADB_DATABASE` exista, y la siembra
/// entraría antes de tiempo igual que en PostgreSQL.
#[test]
fn la_sonda_de_mariadb_consulta_la_base_sembrable() {
    let sonda = sonda_de_mariadb();

    assert!(
        sonda.cmd.iter().any(|argumento| argumento == BASE),
        "la consulta tiene que ir contra `{BASE}`: la que no nombra base contesta que sí \
         antes de que la base exista: {}",
        sonda.cmd.join(" ")
    );
    assert!(
        sonda.cmd.iter().any(|argumento| argumento == "SELECT 1"),
        "la sonda tiene que ser una consulta de verdad: {}",
        sonda.cmd.join(" ")
    );
}

// ══ Requisito: la espera de arranque no se conforma con el servidor temporal ═

/// Escenario: las sondas que nombran la base exigen la supervivencia del definitivo.
///
/// Cubre el escenario «PostgreSQL exige la misma supervivencia que MariaDB» del requisito, y de
/// paso lo afirma también para MariaDB: itera las dos sondas, porque las dos nombran la base.
///
/// Los dos entrypoints oficiales levantan un **servidor temporal** para inicializarse —que ya
/// tiene la base creada, porque la crea él— y lo apagan antes del definitivo (`--skip-networking`
/// con `MARIADB_DATABASE` en MariaDB; `docker_temp_server_start` con `listen_addresses=''`,
/// `docker_setup_db` y `docker_temp_server_stop` en PostgreSQL). La sonda de cada motor **nombra
/// la base**, y eso la protege del temporal antes de que la base exista, pero no de su tramo
/// final: por eso ninguno de los dos se da por arrancado con el **primer** acierto, sino que el
/// acierto tiene que **sobrevivir** a la espera entre sondeos. Aquí se afirma con la sonda de
/// cada motor y un sondeo simulado que acierta una vez y falla después.
#[tokio::test]
async fn las_sondas_que_nombran_la_base_exigen_la_supervivencia_del_definitivo() {
    const PLAZO_DE_SONDEO: Duration = Duration::from_millis(200);
    const ESPERA_DE_SONDEO: Duration = Duration::from_millis(20);

    for sonda in [sonda_de_mariadb(), sonda_de_postgres()] {
        let mut sondeos = 0;

        let desenlace = esperar_arranque(
            "la base",
            &sonda.argv(),
            PLAZO_DE_SONDEO,
            ESPERA_DE_SONDEO,
            || {
                sondeos += 1;
                let primero = sondeos == 1;

                async move {
                    if primero {
                        Ok(())
                    } else {
                        Err("el socket no existe".to_owned())
                    }
                }
            },
        )
        .await;

        assert!(
            desenlace.is_err(),
            "el primer acierto de la sonda de `{}` no basta: {desenlace:?}",
            sonda.cmd.join(" ")
        );
        assert!(
            sondeos >= 2,
            "la sonda de `{}` tiene que sondearse al menos dos veces",
            sonda.cmd.join(" ")
        );
    }
}

// ══ Requisito: batería de punta a punta por tipo de origen ═════════════════

/// Escenario: copia y restauración de MariaDB conserva las filas.
#[tokio::test]
async fn copia_y_restauracion_de_mariadb_conserva_las_filas() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS).await;

    let target = target_mariadb(&mundo.kesto, "e2e-mariadb", &contenedor).await;
    let instantanea =
        copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-mariadb").await;

    // El destino se deja limpio: sin tablas, la restauración tiene que escribirlas.
    limpiar_mariadb(&mundo, &contenedor).await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let obtenido = leer_mariadb(&mundo, &contenedor).await;

    if let Err(diferencia) = comparar_filas(&listado_esperado(FILAS), &obtenido) {
        panic!("las filas restauradas no coinciden con las sembradas: {diferencia}");
    }

    mundo.cerrar().await;
}

/// Escenario: copia y restauración de PostgreSQL conserva las filas.
#[tokio::test]
async fn copia_y_restauracion_de_postgresql_conserva_las_filas() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = postgres_arrancado(&mut mundo, "postgres").await;
    sembrar_postgres(&mundo, &contenedor, FILAS).await;

    let target = mundo
        .kesto
        .crear_target(json!({
            "name": "e2e-postgres",
            "target_type": "postgresql",
            "container_name": contenedor,
            "db_user": "postgres",
            "db_password": CLAVE_DE_LA_BASE,
            "db_name": BASE,
        }))
        .await;
    let instantanea =
        copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-postgres").await;

    limpiar_postgres(&mundo, &contenedor).await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let obtenido = leer_postgres(&mundo, &contenedor).await;

    if let Err(diferencia) = comparar_filas(&listado_esperado(FILAS), &obtenido) {
        panic!("las tablas y filas restauradas no coinciden con las sembradas: {diferencia}");
    }

    mundo.cerrar().await;
}

/// Escenario: copia y restauración de SQLite conserva la base.
#[tokio::test]
async fn copia_y_restauracion_de_sqlite_conserva_la_base() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = alpine_con_sqlite(&mut mundo, "sqlite").await;
    sembrar_sqlite(&mundo, &contenedor, FILAS).await;

    let target = mundo
        .kesto
        .crear_target(json!({
            "name": "e2e-sqlite",
            "target_type": "sqlite",
            "container_name": contenedor,
            "db_path": BASE_SQLITE,
        }))
        .await;
    let instantanea = copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-sqlite").await;

    limpiar_sqlite(&mundo, &contenedor).await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let obtenido = leer_sqlite(&mundo, &contenedor).await;

    if let Err(diferencia) = comparar_filas(&listado_esperado(FILAS), &obtenido) {
        panic!("la base restaurada no contiene exactamente las filas sembradas: {diferencia}");
    }

    mundo.cerrar().await;
}

/// Escenario: copia y restauración de directorio conserva el árbol.
#[tokio::test]
async fn copia_y_restauracion_de_directorio_conserva_el_arbol() {
    let mundo = Mundo::nuevo().await;

    let origen = mundo.trabajo("directorio", "origen");
    sembrar_directorio(&origen);

    // El original se guarda aparte: lo que se compara es el árbol de antes de la copia, no el
    // que quede después de la restauración —que es, justamente, lo que se pone a prueba—.
    let esperado = mundo.trabajo("directorio", "esperado");
    copiar_arbol(&origen, &esperado).unwrap_or_else(|motivo| panic!("{motivo}"));

    let target = target_directorio(&mundo.kesto, "e2e-directorio", &origen).await;
    let instantanea =
        copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-directorio").await;

    sembrar_previo(&origen);

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    if let Err(diferencia) = comparar_arboles(&esperado, &origen) {
        panic!("la jerarquía y el contenido no coinciden con el original: {diferencia}");
    }

    mundo.cerrar().await;
}

/// Escenario: una diferencia de datos rompe la prueba.
///
/// No basta con que la comparación funcione cuando todo va bien: tiene que **romper** cuando
/// algo cambia, y decir qué. Se altera una sola fila de las restauradas y se exige que la
/// comparación falle nombrando esa fila.
#[tokio::test]
async fn una_diferencia_de_datos_rompe_la_prueba() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS).await;

    let target = target_mariadb(&mundo.kesto, "e2e-diferencia", &contenedor).await;
    let instantanea =
        copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-diferencia").await;

    limpiar_mariadb(&mundo, &contenedor).await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let esperado = listado_esperado(FILAS);
    let intacto = leer_mariadb(&mundo, &contenedor).await;

    if let Err(diferencia) = comparar_filas(&esperado, &intacto) {
        panic!("un ciclo que terminó bien tenía que coincidir y no coincidió: {diferencia}");
    }

    // Una sola fila distinta, y la comparación tiene que romper nombrándola.
    let orden = format!(
        "mariadb -uroot -p{CLAVE_DE_LA_BASE} {BASE} -e \
         \"UPDATE filas SET texto = 'alterada a mano' WHERE id = 3\""
    );
    guion(
        &mundo.motor,
        &contenedor,
        &orden,
        "la alteración de una fila",
    )
    .await;

    let alterado = leer_mariadb(&mundo, &contenedor).await;
    let diferencia = comparar_filas(&esperado, &alterado).expect_err(
        "una fila distinta tenía que romper la comparación, y la comparación dijo que todo \
         coincidía",
    );

    assert!(
        diferencia.contains("la fila 3"),
        "la diferencia tenía que nombrar la fila que cambió: {diferencia}"
    );

    mundo.cerrar().await;
}

// ══ Requisito: almacén S3 real en la batería ═══════════════════════════════

/// Escenario: el repositorio se crea contra S3 real.
#[tokio::test]
async fn el_repositorio_se_crea_contra_s3_real() {
    let mundo = Mundo::nuevo().await;

    let origen = mundo.trabajo("directorio", "origen");
    sembrar_directorio(&origen);

    let target = target_directorio(&mundo.kesto, "e2e-s3", &origen).await;
    copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-s3").await;

    let instantaneas = mundo.kesto.instantaneas(&mundo.repositorio).await;

    assert_eq!(
        instantaneas.len(),
        1,
        "la instantánea tenía que estar en el almacén y listarse: {instantaneas:?}"
    );
    assert!(
        instantaneas[0]["bytes"].as_u64().unwrap_or(0) > 0,
        "la instantánea tenía que listarse con su tamaño: {}",
        instantaneas[0]
    );
    assert!(
        instantaneas[0]["files"].as_u64().unwrap_or(0) >= 3,
        "la instantánea tenía que declarar los ficheros que procesó: {}",
        instantaneas[0]
    );

    // Y los bytes están **en el servidor S3**, no solo en su índice: el contenedor del almacén
    // tiene ficheros en su volumen de datos.
    let ficheros = guion(
        &mundo.motor,
        &mundo.s3,
        "find /data -type f | wc -l",
        "la cuenta de ficheros del servidor S3",
    )
    .await;

    let ficheros: u64 = ficheros.salida.trim().parse().unwrap_or(0);
    assert!(
        ficheros > 0,
        "el servidor S3 de la batería no tiene ningún fichero en su volumen de datos: la \
         instantánea se listó pero los bytes no llegaron al almacén"
    );

    mundo.cerrar().await;
}

/// Escenario: el S3 está aislado entre ejecuciones.
///
/// La batería se ejecuta **dos veces** en el mismo proceso —dos mundos completos, cada uno con
/// su servidor S3, su binario y su cubo— y la segunda no ve nada de la primera.
#[tokio::test]
async fn el_s3_esta_aislado_entre_ejecuciones() {
    // Primera ejecución: deja una instantánea en su almacén.
    let primera = Mundo::nuevo().await;

    let origen = primera.trabajo("directorio", "origen");
    sembrar_directorio(&origen);

    let target = target_directorio(&primera.kesto, "e2e-aislamiento", &origen).await;
    copiar_una_vez(
        &primera.kesto,
        &target,
        &primera.repositorio,
        "e2e-aislamiento",
    )
    .await;

    let suya = la_unica_instantanea(&primera.kesto, &primera.repositorio).await;

    // Segunda ejecución: otro cubo, otro servidor, otro binario.
    let segunda = Mundo::nuevo().await;
    let ajenas = segunda.kesto.instantaneas(&segunda.repositorio).await;

    assert!(
        ajenas.is_empty(),
        "la segunda ejecución veía instantáneas de la primera: {ajenas:?}"
    );
    assert!(
        !ajenas.iter().any(|ajena| ajena["id"] == suya.as_str()),
        "la instantánea `{suya}` de la primera ejecución aparecía en el listado de la segunda"
    );

    // Y la primera sigue teniendo la suya: la segunda no la pisó.
    assert_eq!(
        primera.kesto.instantaneas(&primera.repositorio).await.len(),
        1,
        "la segunda ejecución se llevó por delante el almacén de la primera"
    );

    primera.cerrar().await;
    segunda.cerrar().await;
}

/// Escenario: una credencial incorrecta no se publica en el motivo.
#[tokio::test]
async fn una_credencial_incorrecta_no_se_publica_en_el_motivo() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS).await;

    let target = target_mariadb(&mundo.kesto, "e2e-credencial", &contenedor).await;
    let tarea = mundo
        .kesto
        .crear_tarea(&target, &mundo.repositorio, "e2e-credencial", 7)
        .await;

    // La credencial se estropea **después** del alta: el alta abre el repositorio y con una
    // credencial falsa no habría llegado a existir la fila.
    let falsa = format!("clave-de-acceso-incorrecta-{}", ACCESO_S3);
    mundo
        .kesto
        .estropear_la_credencial(&mundo.repositorio, &mundo.endpoint, &mundo.cubo, &falsa)
        .await;

    let fila = mundo.kesto.copiar(&tarea, PLAZO_DE_LA_COPIA).await;

    assert_eq!(
        fila["status"], "failed",
        "la copia con una credencial incorrecta tenía que fallar: {fila}"
    );

    let motivo = fila["error_message"].as_str().unwrap_or_default();

    assert!(
        !motivo.is_empty(),
        "la ejecución falló sin decir por qué: {fila}"
    );
    assert!(
        !motivo.contains(&falsa),
        "el motivo publica la clave de acceso: {motivo}"
    );
    assert!(
        !motivo.contains(SECRETO_S3),
        "el motivo publica el secreto del almacén: {motivo}"
    );

    mundo.cerrar().await;
}

// ══ Requisito: ejecución en CI con motor de contenedores ═══════════════════

/// El texto de un job del workflow, desde su nombre hasta el job siguiente.
#[track_caller]
fn job_del_workflow(workflow: &str, nombre: &str) -> String {
    let mut dentro = false;
    let mut lineas = Vec::new();

    for linea in workflow.lines() {
        let es_cabecera =
            linea.starts_with("  ") && !linea.starts_with("   ") && linea.trim_end().ends_with(':');

        if es_cabecera {
            dentro = linea.trim_end() == format!("  {nombre}:");
            continue;
        }

        if dentro {
            lineas.push(linea);
        }
    }

    assert!(
        !lineas.is_empty(),
        "el workflow no tiene un job `{nombre}`: {workflow}"
    );

    lineas.join("\n")
}

/// El workflow del CI, leído del repositorio.
fn workflow() -> &'static str {
    include_str!("../../.github/workflows/ci.yml")
}

/// Escenario: el job `e2e` ejecuta la batería.
#[tokio::test]
async fn el_job_e2e_ejecuta_la_bateria() {
    let job = job_del_workflow(workflow(), "e2e");

    assert!(
        job.contains("--features e2e") && job.contains("--test e2e"),
        "el job `e2e` no ejecuta la batería: sin `--features e2e --test e2e` el target está \
         detrás de una feature y no se compila ni se ejecuta.\n--- job `e2e`\n{job}"
    );
    assert!(
        job.contains("--ignored"),
        "el job `e2e` no ejecuta los tests existentes marcados como omitidos, que son la otra \
         mitad de lo que el requisito pide.\n--- job `e2e`\n{job}"
    );
}

/// Escenario: un fallo de la batería deja el job en rojo.
///
/// El desenlace de un `run:` que termina en un tubo es el del **último** comando del tubo, así
/// que `cargo test … | tee registro` sale con el código del `tee` —cero— y un fallo de la
/// batería dejaría el job en verde. El job tiene que cortar eso con `pipefail`, y esta prueba
/// lo exige sobre cada línea que ejecute la batería.
#[tokio::test]
async fn un_fallo_de_la_bateria_deja_el_job_en_rojo() {
    let job = job_del_workflow(workflow(), "e2e");
    let tiene_pipefail = job.contains("pipefail");

    for linea in job.lines() {
        let linea = linea.trim();

        if !linea.contains("cargo test") || !linea.contains('|') {
            continue;
        }

        assert!(
            tiene_pipefail,
            "el job `e2e` ejecuta `{linea}`, que lleva su desenlace a un tubo: sin \
             `set -o pipefail` el código que ve el job es el del último comando —cero— y un \
             fallo de la batería dejaría el job en verde.\n--- job `e2e`\n{job}"
        );
    }
}

/// Escenario: el job `backend` no depende de contenedores.
#[tokio::test]
async fn el_job_backend_no_depende_de_contenedores() {
    let job = job_del_workflow(workflow(), "backend");

    assert!(
        job.contains("cargo test --locked"),
        "el job `backend` tiene que seguir ejecutando `cargo test --locked`.\n--- job \
         `backend`\n{job}"
    );
    assert!(
        !job.contains("docker") && !job.contains("MARIA_IMAGE") && !job.contains("S3_IMAGE"),
        "el job `backend` no puede requerir motor de contenedores ni servidor S3.\n--- job \
         `backend`\n{job}"
    );
    assert!(
        !job.lines()
            .any(|linea| linea.trim_start().starts_with("needs:") && linea.contains("e2e")),
        "el job `backend` no puede depender del job de contenedores: tiene que correr sin motor \
         de contenedores disponible.\n--- job `backend`\n{job}"
    );
}

// ══ Requisito: ejecución local reproducible ════════════════════════════════

/// El cuerpo de una receta del `.justfile`, desde su cabecera hasta la receta siguiente.
#[track_caller]
fn receta_del_justfile(justfile: &str, nombre: &str) -> String {
    let mut dentro = false;
    let mut cuerpo = Vec::new();

    for linea in justfile.lines() {
        let es_cabecera = !linea.is_empty()
            && !linea.starts_with([' ', '\t', '#'])
            && linea.trim_end().ends_with(':');

        if es_cabecera {
            let suyo = linea.trim_end().trim_end_matches(':');
            dentro = suyo.split([' ', '\t']).next().unwrap_or_default() == nombre;
            continue;
        }

        if dentro && !linea.trim().is_empty() {
            cuerpo.push(linea);
        }
    }

    assert!(
        !cuerpo.is_empty(),
        "el `.justfile` no tiene la receta `{nombre}`: {justfile}"
    );

    cuerpo.join("\n")
}

/// Escenario: receta local única.
#[tokio::test]
async fn la_receta_local_es_unica() {
    let receta = receta_del_justfile(include_str!("../../.justfile"), "e2e");

    assert!(
        receta.contains("DOCKER_HOST") && receta.contains("podman.sock"),
        "la receta `e2e` tiene que apuntar el motor al socket de podman por defecto y dejar que \
         `DOCKER_HOST` lo cambie.\n--- receta `e2e`\n{receta}"
    );
    assert!(
        receta.contains("--features e2e") && receta.contains("--test e2e"),
        "la receta `e2e` tiene que ejecutar la batería.\n--- receta `e2e`\n{receta}"
    );
    assert!(
        receta.contains("--test-threads=1"),
        "la receta `e2e` tiene que ejecutar la batería en serie.\n--- receta `e2e`\n{receta}"
    );
}

/// Escenario: sin socket de motor no se finge éxito.
///
/// Se afirma sobre la resolución del socket —pura, sin entorno de por medio— y sobre la
/// ausencia de escenarios omitidos en la batería: un escenario marcado como omitido es
/// exactamente lo que este requisito prohíbe.
#[tokio::test]
async fn sin_socket_de_motor_no_se_finge_exito() {
    let sin_variable = resolver_el_socket(None).expect_err("sin `DOCKER_HOST` no hay motor");
    assert!(
        sin_variable.contains("DOCKER_HOST") && sin_variable.contains("podman.sock"),
        "el motivo tiene que nombrar la variable y el socket esperado: {sin_variable}"
    );

    let vacia = resolver_el_socket(Some("   ")).expect_err("una variable vacía no es un motor");
    assert!(
        vacia.contains("DOCKER_HOST"),
        "el motivo tiene que nombrar la variable que falta: {vacia}"
    );

    let remota = resolver_el_socket(Some("tcp://127.0.0.1:2375"))
        .expect_err("la batería habla con el motor por su socket unix");
    assert!(
        remota.contains("tcp://127.0.0.1:2375"),
        "el motivo tiene que decir qué valor no sirve: {remota}"
    );

    // El atributo se arma **en tiempo de ejecución** y no se escribe literal: el fichero que
    // se está leyendo es este mismo, así que el atributo escrito en la comprobación haría que
    // la comprobación se encontrara a sí misma y fallara siempre.
    let omision = format!("#[{}]", "ignore");

    for (nombre, fuente) in [
        ("escenarios.rs", include_str!("escenarios.rs")),
        ("arnes.rs", include_str!("arnes.rs")),
        ("main.rs", include_str!("main.rs")),
    ] {
        assert!(
            !fuente.contains(&omision),
            "la batería no puede marcar ningún escenario de `{nombre}` como omitido: lo que \
             falta tiene que hacer fallar, no saltarse"
        );
    }
}

// ══ Requisito: la batería falla en voz alta ════════════════════════════════

/// Escenario: falta el motor de contenedores.
#[tokio::test]
async fn falta_el_motor_de_contenedores() {
    let inventado = format!("unix:///tmp/kesto-e2e-no-existe-{}.sock", puerto_libre());

    let Err(motivo) = Motor::conectar_en(&inventado).await else {
        panic!("un socket que no existe no puede ser un motor, y `{inventado}` lo fue");
    };

    assert!(
        motivo.contains(&inventado),
        "el motivo tiene que nombrar el socket que no responde: {motivo}"
    );
}

/// Escenario: falta el servidor S3.
#[tokio::test]
async fn falta_el_servidor_s3() {
    let endpoint = format!("http://127.0.0.1:{}", puerto_libre());

    let motivo = esperar_el_s3(&endpoint, Duration::from_secs(2))
        .await
        .expect_err("donde no hay servidor S3 no puede haber almacén");

    assert!(
        motivo.contains(&endpoint),
        "el motivo tiene que nombrar el endpoint que no llegó: {motivo}"
    );
    assert!(
        motivo.contains("no degrada"),
        "el motivo tiene que decir que la batería no degrada a un almacén local: {motivo}"
    );
}

// ══ Requisito: restauración hacia contenedor de punta a punta ══════════════

/// Escenario: el snapshot viaja de S3 al `stdin` del contenedor.
///
/// El volcado es grande a propósito: lo que se comprueba es que llega **entero** al `stdin`
/// del proceso de restauración y que el destino acaba con todos sus datos.
#[tokio::test]
async fn el_snapshot_viaja_de_s3_al_stdin_del_contenedor() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS_DEL_VOLUMEN).await;

    let target = target_mariadb(&mundo.kesto, "e2e-stdin", &contenedor).await;
    let instantanea = copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-stdin").await;

    let instantaneas = mundo.kesto.instantaneas(&mundo.repositorio).await;
    assert!(
        instantaneas[0]["bytes"].as_u64().unwrap_or(0) > 100_000,
        "el volcado tenía que ser grande para que el camino del `stdin` signifique algo: {}",
        instantaneas[0]
    );

    limpiar_mariadb(&mundo, &contenedor).await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let obtenido = leer_mariadb(&mundo, &contenedor).await;

    if let Err(diferencia) = comparar_filas(&listado_esperado(FILAS_DEL_VOLUMEN), &obtenido) {
        panic!("el volcado no llegó entero al destino: {diferencia}");
    }

    mundo.cerrar().await;
}

// ══ Requisito: el volcado es contenido no confiable ════════════════════════

/// Escenario: la restauración global de PostgreSQL reparte cada base en la suya.
///
/// Cubre las tareas 4.1 y 4.2b: un target `postgresql` **sin** `db_name` se extrae con
/// `pg_dumpall` —un volcado global, con las dos bases dentro— y se restaura. `pg_dumpall` separa
/// las bases con metacomandos `\connect`, y el único modo de `psql` que rechaza metacomandos
/// —`\restrict`— **bloquea `\connect`**: anteponerlo a un volcado global **desvía los datos a la
/// base equivocada** —las filas acaban todas en `postgres`—. Por eso la restauración trocea el
/// volcado por los `\connect` de primer nivel y restaura **base por base**. Este escenario mide
/// que cada base acaba con **sus** datos y no con los de otra: es lo que antes fallaba.
#[tokio::test]
async fn copia_y_restauracion_global_de_postgresql_reparte_cada_base_en_la_suya() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = postgres_arrancado(&mut mundo, "global").await;

    // Dos bases propias, cada una con datos **identificables**: el texto lleva delante el nombre
    // de la base, así que un dato de una no puede pasar por el de la otra.
    for base in ["almacen_a", "almacen_b"] {
        crear_base_postgres(&mundo, &contenedor, base).await;
        sembrar_base_postgres(&mundo, &contenedor, base, FILAS).await;
    }

    // El target **sin** `db_name`: es lo que hace que la extracción use `pg_dumpall`.
    let target = mundo
        .kesto
        .crear_target(json!({
            "name": "e2e-postgres-global",
            "target_type": "postgresql",
            "container_name": contenedor,
            "db_user": "postgres",
            "db_password": CLAVE_DE_LA_BASE,
        }))
        .await;
    let instantanea = copiar_una_vez(
        &mundo.kesto,
        &target,
        &mundo.repositorio,
        "e2e-postgres-global",
    )
    .await;

    // El destino se deja limpio **de verdad**: sin las bases, el `CREATE DATABASE` del volcado
    // tiene que volver a crearlas, que es justo lo que prueba que el reparto por secciones
    // funciona.
    for base in ["almacen_a", "almacen_b"] {
        borrar_base_postgres(&mundo, &contenedor, base).await;
    }

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    for base in ["almacen_a", "almacen_b"] {
        let obtenido = leer_postgres_de(&mundo, &contenedor, base, "filas").await;

        if let Err(diferencia) = comparar_filas(&listado_con(&format!("{base}-"), FILAS), &obtenido)
        {
            panic!("la base `{base}` no quedó con sus datos: {diferencia}");
        }

        // Y, explícitamente, **no** con los de la otra: el fallo que se arregló metía todo en una
        // sola base, así que la comprobación tiene que ser capaz de ver el dato ajeno.
        let otra = if base == "almacen_a" {
            "almacen_b"
        } else {
            "almacen_a"
        };
        assert!(
            !obtenido.contains(otra),
            "la base `{base}` recibió los datos de `{otra}`: {obtenido}"
        );
    }

    mundo.cerrar().await;
}

/// Escenario: un `psql` que no honra `\restrict` falla la restauración sin restaurar nada.
///
/// Es la **precondición obligatoria** del reparto por secciones: con un `psql` anterior al ciclo de
/// parches que trajo `\restrict` —17.6, o el mínimo de su rama— la segunda capa no existe, y un
/// volcado hostil conseguiría ejecutar metacomandos como `root` en el contenedor destino. Por eso
/// la restauración **falla en voz alta** —nombrando el contenedor, la versión leída y el mínimo—
/// en vez de degradarse, y **no** restaura nada: la base que se vació antes sigue sin existir.
///
/// La imagen es una por debajo del umbral (ver [`imagen_de_postgres_sin_restrict`]); traerla es la
/// guarda previa del escenario. Y no se usa [`postgres_arrancado`] porque éste exige justo lo que
/// aquí se quiere negar.
#[tokio::test]
#[ignore = "requiere un motor de contenedores y descargar una imagen antigua de PostgreSQL"]
async fn un_psql_sin_restrict_falla_la_restauracion_sin_restaurar_nada() {
    let mut mundo = Mundo::nuevo().await;

    let imagen = imagen_de_postgres_sin_restrict(&mundo).await;
    let contenedor = postgres_sin_restrict_arrancado(&mut mundo, "sin-restrict", &imagen).await;

    // Una base propia sembrada y un target **sin** `db_name`: la extracción es el volcado global.
    crear_base_postgres(&mundo, &contenedor, "almacen").await;
    sembrar_base_postgres(&mundo, &contenedor, "almacen", FILAS).await;

    let target = mundo
        .kesto
        .crear_target(json!({
            "name": "e2e-postgres-sin-restrict",
            "target_type": "postgresql",
            "container_name": contenedor,
            "db_user": "postgres",
            "db_password": CLAVE_DE_LA_BASE,
        }))
        .await;
    let instantanea = copiar_una_vez(
        &mundo.kesto,
        &target,
        &mundo.repositorio,
        "e2e-postgres-sin-restrict",
    )
    .await;

    // El destino se vacía: la restauración tiene que **no** reponerlo.
    borrar_base_postgres(&mundo, &contenedor, "almacen").await;

    let (estado, valor) = mundo
        .kesto
        .restaurar(&instantanea, &mundo.repositorio, true)
        .await;
    assert!(
        estado.is_success(),
        "la restauración tenía que lanzarse y contestó {estado} con {valor}"
    );

    let ejecucion = valor["data"]["execution_id"]
        .as_str()
        .unwrap_or_else(|| panic!("la restauración no trajo `execution_id`: {valor}"));
    let fila = mundo
        .kesto
        .esperar_la_restauracion(ejecucion, PLAZO_DE_LA_RESTAURACION)
        .await;

    assert_eq!(
        fila["status"], "failed",
        "un `psql` sin `\\restrict` tenía que fallar la restauración: {fila}"
    );

    let motivo = fila["error_message"].as_str().unwrap_or_default();
    assert!(
        motivo.contains("17.6") && motivo.contains("restrict"),
        "el motivo no nombra el mínimo de `\\restrict`: {motivo}"
    );

    // Y **no** se restauró nada: la base que se vació sigue sin existir.
    assert!(
        !existe_la_base_postgres(&mundo, &contenedor, "almacen").await,
        "la restauración falló pero repuso la base `almacen`: la precondición no cortó antes de \
         ejecutar nada"
    );

    mundo.cerrar().await;
}

/// Los valores que **parecen** un metacomando de cliente y que el ciclo tiene que conservar.
///
/// Son datos, no órdenes: viven en una tabla y tienen que volver a ella byte a byte. Cubren los
/// tres metacomandos que el descarte tiene que reconocer —`\!`, `\.shell` y `\connect`— y uno con
/// comilla y barras invertidas, que es donde un escapado descuidado perdería la fila.
const METACOMANDOS: &[&str] = &[
    r"\! touch /tmp/kesto-pwned",
    r".shell touch /tmp/kesto-pwned",
    r"\connect otra_base",
    r"comillas ' y \\",
];

/// Escapa un valor para un literal SQL de comillas simples.
///
/// Escapa la barra invertida **y** la comilla; en PostgreSQL el literal se abre además con el
/// prefijo `E`, de modo que el valor se lee igual con `standard_conforming_strings` en cualquier
/// estado y no depende del modo del servidor.
fn escapar_sql(valor: &str) -> String {
    valor.replace('\\', "\\\\").replace('\'', "\\'")
}

/// El SQL que siembra los valores de [`METACOMANDOS`] en la tabla `frases`.
///
/// `prefijo_e` decide cómo se abre el literal: `E'…'` en PostgreSQL —donde el prefijo `E` fija que
/// la barra invertida escape— y `'…'` en MariaDB, cuyo modo por defecto ya la trata como escape.
fn siembra_de_metacomandos(prefijo_e: bool) -> String {
    let mut sql = String::from("CREATE TABLE frases (id INT PRIMARY KEY, texto TEXT NOT NULL);\n");

    for (indice, valor) in METACOMANDOS.iter().enumerate() {
        let escapado = escapar_sql(valor);
        let literal = if prefijo_e {
            format!("E'{escapado}'")
        } else {
            format!("'{escapado}'")
        };

        sql.push_str(&format!(
            "INSERT INTO frases (id, texto) VALUES ({}, {literal});\n",
            indice + 1
        ));
    }

    sql
}

/// Comprueba que la orden de shell señuelo **no** se ejecutó en el contenedor.
///
/// Los valores de [`METACOMANDOS`] incluyen `touch /tmp/kesto-pwned`: si el descarte de
/// metacomandos hubiera tratado el **dato** como orden, el fichero estaría ahí. No existe ni antes
/// ni después; lo que fija el test es que el valor que *parecía* una orden viaja como dato.
async fn comprobar_que_no_se_ejecuto_la_orden(mundo: &Mundo, contenedor: &str) {
    guion(
        &mundo.motor,
        contenedor,
        "test ! -e /tmp/kesto-pwned",
        "la comprobación de que la orden señuelo no se ejecutó",
    )
    .await;
}

/// Escenario: los valores que parecen un metacomando sobreviven al ciclo (PostgreSQL).
///
/// Fija que el descarte de metacomandos **no se come contenido**: los valores viajan dentro de un
/// `COPY` del volcado y tienen que volver íntegros. Si el troceador descartara por subcadena en
/// vez de por estado léxico, este test perdería filas —o el `\connect` de un dato abriría una
/// sección falsa—.
#[tokio::test]
async fn los_valores_que_parecen_un_metacomando_sobreviven_al_ciclo_en_postgresql() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = postgres_arrancado(&mut mundo, "postgres").await;
    sql_postgres(
        &mundo,
        &contenedor,
        BASE,
        &siembra_de_metacomandos(true),
        "la siembra de las frases",
    )
    .await;

    let esperado = leer_postgres_de(&mundo, &contenedor, BASE, "frases").await;

    // La siembra dejó de verdad los valores que se querían: si el escapado fallara, el ciclo
    // conservaría un valor equivocado y el escenario pasaría por bueno.
    for valor in METACOMANDOS {
        assert!(
            esperado.contains(valor),
            "la siembra no dejó el valor `{valor}`: {esperado}"
        );
    }

    let target = mundo
        .kesto
        .crear_target(json!({
            "name": "e2e-metacomandos-postgres",
            "target_type": "postgresql",
            "container_name": contenedor,
            "db_user": "postgres",
            "db_password": CLAVE_DE_LA_BASE,
            "db_name": BASE,
        }))
        .await;
    let instantanea = copiar_una_vez(
        &mundo.kesto,
        &target,
        &mundo.repositorio,
        "e2e-metacomandos-postgres",
    )
    .await;

    sql_postgres(
        &mundo,
        &contenedor,
        BASE,
        "DROP TABLE IF EXISTS frases;\n",
        "la limpieza del destino",
    )
    .await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let obtenido = leer_postgres_de(&mundo, &contenedor, BASE, "frases").await;

    if let Err(diferencia) = comparar_filas(&esperado, &obtenido) {
        panic!("los valores que parecen un metacomando no sobrevivieron: {diferencia}");
    }

    comprobar_que_no_se_ejecuto_la_orden(&mundo, &contenedor).await;

    mundo.cerrar().await;
}

/// Escenario: los valores que parecen un metacomando sobreviven al ciclo (MariaDB).
///
/// La misma garantía que en PostgreSQL —el descarte de metacomandos no se come datos— para el
/// cliente de MariaDB, que ignora sus metacomandos con `--binary-mode`: los valores viajan dentro
/// de los `INSERT` del volcado y tienen que volver íntegros.
#[tokio::test]
async fn los_valores_que_parecen_un_metacomando_sobreviven_al_ciclo_en_mariadb() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sql_mariadb(
        &mundo,
        &contenedor,
        &siembra_de_metacomandos(false),
        "la siembra de las frases",
    )
    .await;

    let esperado = leer_mariadb_de(&mundo, &contenedor, "frases").await;

    for valor in METACOMANDOS {
        assert!(
            esperado.contains(valor),
            "la siembra no dejó el valor `{valor}`: {esperado}"
        );
    }

    let target = target_mariadb(&mundo.kesto, "e2e-metacomandos-maria", &contenedor).await;
    let instantanea = copiar_una_vez(
        &mundo.kesto,
        &target,
        &mundo.repositorio,
        "e2e-metacomandos-maria",
    )
    .await;

    sql_mariadb(
        &mundo,
        &contenedor,
        "DROP TABLE IF EXISTS frases;\n",
        "la limpieza del destino",
    )
    .await;

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    let obtenido = leer_mariadb_de(&mundo, &contenedor, "frases").await;

    if let Err(diferencia) = comparar_filas(&esperado, &obtenido) {
        panic!("los valores que parecen un metacomando no sobrevivieron: {diferencia}");
    }

    comprobar_que_no_se_ejecuto_la_orden(&mundo, &contenedor).await;

    mundo.cerrar().await;
}

/// Escenario: un contenedor que no drena su `stdin` no cuelga la restauración.
///
/// Cuesta el plazo de inactividad del motor —cinco minutos— y por eso es el escenario más caro
/// de la batería: el contenedor de destino es un `alpine` con un falso `mariadb` que se duerme
/// sin leer nada, y la restauración tiene que terminar en error acotado en vez de esperar para
/// siempre.
#[tokio::test]
async fn un_contenedor_que_no_drena_su_stdin_no_cuelga_la_restauracion() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS).await;

    let target = target_mariadb(&mundo.kesto, "e2e-sin-drenaje", &contenedor).await;
    let instantanea =
        copiar_una_vez(&mundo.kesto, &target, &mundo.repositorio, "e2e-sin-drenaje").await;

    // El contenedor que no coopera: un `mariadb` que se duerme sin leer su entrada estándar.
    let sordo = mundo.nombre_de_contenedor("sordo");
    let imagen = imagen_de_alpine();
    mundo
        .contenedor(Receta {
            nombre: &sordo,
            imagen: &imagen,
            cmd: &[
                "sh",
                "-c",
                "printf '#!/bin/sh\\nsleep 600\\n' > /usr/local/bin/mariadb && \
                 chmod +x /usr/local/bin/mariadb && sleep infinity",
            ],
            env: &[],
            puerto: None,
            montaje: None,
            // Es el destino de la restauración, así que el motor tiene que poder crear un
            // `exec` en él: se declara fuente de copia.
            fuente_de_copia: true,
        })
        .await;
    mundo
        .motor
        .esperar_orden(
            &sordo,
            &["test", "-x", "/usr/local/bin/mariadb"],
            &[],
            PLAZO_DE_ARRANQUE,
        )
        .await
        .unwrap_or_else(|motivo| panic!("{motivo}"));

    // El destino lo resuelve el vínculo de la instantánea, así que el cambio de contenedor se
    // hace sobre el target y no sobre la petición de restauración.
    mundo
        .kesto
        .datos(
            "el cambio de contenedor del target",
            Method::PUT,
            &format!("/api/v1/targets/{target}"),
            Some(&json!({
                "name": "e2e-sin-drenaje",
                "target_type": "mariadb",
                "container_name": sordo,
                "db_user": "root",
                "db_name": BASE,
            })),
        )
        .await;

    let inicio = Instant::now();
    let (estado, valor) = mundo
        .kesto
        .restaurar(&instantanea, &mundo.repositorio, true)
        .await;

    assert!(
        estado.is_success(),
        "la restauración tenía que lanzarse y contestó {estado} con {valor}"
    );

    let ejecucion = valor["data"]["execution_id"]
        .as_str()
        .unwrap_or_else(|| panic!("la restauración no trajo `execution_id`: {valor}"))
        .to_owned();

    let fila = mundo
        .kesto
        .esperar_la_restauracion(&ejecucion, PLAZO_DE_LA_RESTAURACION)
        .await;

    assert_eq!(
        fila["status"], "failed",
        "la restauración sobre un contenedor que no drena su `stdin` tenía que terminar en \
         error acotado: {fila}"
    );

    let motivo = fila["error_message"].as_str().unwrap_or_default();
    assert!(
        !motivo.is_empty(),
        "la restauración falló sin decir por qué: {fila}"
    );

    // El acotado: el motor no puede quedarse esperando a un `stdin` que nadie lee, y el plazo
    // que gobierna eso es su plazo de inactividad. Si no lo hubiera aplicado, esta prueba no
    // habría llegado a leer nada —`esperar_la_restauracion` habría entrado en pánico por
    // plazo—, así que lo que se deja dicho es cuánto tardó en decidir.
    let tardanza = inicio.elapsed();
    assert!(
        tardanza < PLAZO_DE_LA_RESTAURACION,
        "la restauración tardó {tardanza:?} en rendirse, más que el propio plazo de espera de \
         la prueba ({PLAZO_DE_LA_RESTAURACION:?}): el motor tenía que haber cortado por \
         inactividad ({PLAZO_DE_INACTIVIDAD:?})"
    );

    mundo.cerrar().await;
}

// ══ Requisito: sobrescritura de un destino con contenido ═══════════════════

/// Escenario: sobrescribir un directorio existente.
///
/// El destino queda con contenido previo que la copia no tiene —un fichero suelto y un directorio
/// **no vacío**—, y con la confirmación de sobrescritura la poda tiene que retirar los dos: no
/// basta con que el árbol restaurado coincida con el original, tiene que **no quedar nada** de lo
/// previo.
#[tokio::test]
async fn sobrescribir_un_directorio_existente() {
    let mundo = Mundo::nuevo().await;

    let origen = mundo.trabajo("directorio", "origen");
    sembrar_directorio(&origen);

    let esperado = mundo.trabajo("directorio", "esperado");
    copiar_arbol(&origen, &esperado).unwrap_or_else(|motivo| panic!("{motivo}"));

    let target = target_directorio(&mundo.kesto, "e2e-sobrescritura", &origen).await;
    let instantanea = copiar_una_vez(
        &mundo.kesto,
        &target,
        &mundo.repositorio,
        "e2e-sobrescritura",
    )
    .await;

    // El destino queda con contenido previo que la copia no tiene.
    sembrar_previo(&origen);

    mundo
        .kesto
        .restaurar_exigiendo_exito(
            &instantanea,
            &mundo.repositorio,
            true,
            PLAZO_DE_LA_RESTAURACION,
        )
        .await;

    assert!(
        !origen.join(FICHERO_OBSOLETO).exists(),
        "el contenido previo del destino tenía que sustituirse por el del snapshot y \
         `{FICHERO_OBSOLETO}` sigue ahí"
    );
    assert!(
        !origen.join(DIRECTORIO_OBSOLETO).exists(),
        "el árbol previo del destino tenía que sustituirse por el del snapshot y el directorio \
         `{DIRECTORIO_OBSOLETO}` —no vacío— sigue ahí"
    );

    if let Err(diferencia) = comparar_arboles(&esperado, &origen) {
        panic!("el contenido restaurado no sustituyó al previo: {diferencia}");
    }

    mundo.cerrar().await;
}

/// Escenario: sin confirmación no se toca el destino.
///
/// El destino es un target de base de datos —que es donde la confirmación es obligatoria— y se
/// deja con contenido **distinto** del snapshot: si la restauración se lanzara, lo pisaría.
#[tokio::test]
async fn sin_confirmacion_no_se_toca_el_destino() {
    let mut mundo = Mundo::nuevo().await;

    let contenedor = mariadb_arrancado(&mut mundo, "maria").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS).await;

    let target = target_mariadb(&mundo.kesto, "e2e-confirmacion", &contenedor).await;
    let instantanea = copiar_una_vez(
        &mundo.kesto,
        &target,
        &mundo.repositorio,
        "e2e-confirmacion",
    )
    .await;

    let poda =
        format!("mariadb -uroot -p{CLAVE_DE_LA_BASE} {BASE} -e 'DELETE FROM filas WHERE id > 1'");
    guion(&mundo.motor, &contenedor, &poda, "la poda del destino").await;
    let antes = leer_mariadb(&mundo, &contenedor).await;

    let (estado, valor) = mundo
        .kesto
        .restaurar(&instantanea, &mundo.repositorio, false)
        .await;

    assert_eq!(
        estado,
        StatusCode::PRECONDITION_REQUIRED,
        "restaurar sin la confirmación tenía que rechazarse con un 428 y contestó {estado} con \
         {valor}"
    );

    let mensaje = valor["message"].as_str().unwrap_or_default();
    assert!(
        mensaje.contains("sobrescribir"),
        "el rechazo tenía que nombrar la confirmación que falta: {mensaje}"
    );

    let despues = leer_mariadb(&mundo, &contenedor).await;
    assert_eq!(
        despues, antes,
        "el contenido previo del destino tenía que quedar intacto"
    );

    mundo.cerrar().await;
}

// ══ Requisito: retención y purga contra S3 real ════════════════════════════

/// Escenario: la retención deja solo lo que conserva.
#[tokio::test]
async fn la_retencion_deja_solo_lo_que_conserva() {
    let mundo = Mundo::nuevo().await;

    let origen = mundo.trabajo("directorio", "origen");
    sembrar_directorio(&origen);

    let target = target_directorio(&mundo.kesto, "e2e-retencion", &origen).await;

    // `keep_daily = 1` y nada más: tres copias del mismo día tienen que dejar una sola.
    let tarea = mundo
        .kesto
        .crear_tarea(&target, &mundo.repositorio, "e2e-retencion", 1)
        .await;

    let mut ultima = String::new();
    for _ in 0..3 {
        let fila = mundo
            .kesto
            .copiar_exigiendo_exito(&tarea, PLAZO_DE_LA_COPIA)
            .await;

        ultima = fila["snapshot_id"].as_str().unwrap_or_default().to_owned();
    }

    let instantaneas = mundo.kesto.instantaneas(&mundo.repositorio).await;

    assert_eq!(
        instantaneas.len(),
        1,
        "con `keep_daily = 1` sobre tres copias del mismo día tenía que quedar una sola \
         instantánea y quedan {}: {instantaneas:?}",
        instantaneas.len()
    );
    assert_eq!(
        instantaneas[0]["id"],
        ultima.as_str(),
        "la instantánea que sobrevive tiene que ser la última, no otra: {instantaneas:?}"
    );

    mundo.cerrar().await;
}

/// Escenario: una purga fallida no deja el repositorio a medias.
///
/// El fallo se provoca parando el servidor S3 después de una copia correcta: la copia
/// siguiente no puede hablar con el almacén, y lo que se comprueba es que la ejecución lo dice
/// y que el repositorio sigue siendo legible.
#[tokio::test]
async fn una_purga_fallida_no_deja_el_repositorio_a_medias() {
    let mundo = Mundo::nuevo().await;

    let origen = mundo.trabajo("directorio", "origen");
    sembrar_directorio(&origen);

    let target = target_directorio(&mundo.kesto, "e2e-purga", &origen).await;
    let tarea = mundo
        .kesto
        .crear_tarea(&target, &mundo.repositorio, "e2e-purga", 7)
        .await;

    mundo
        .kesto
        .copiar_exigiendo_exito(&tarea, PLAZO_DE_LA_COPIA)
        .await;

    // El almacén se cae a mitad de camino: el contenedor sigue ahí —y su fila en la base
    // también—, pero su puerto ya no contesta.
    mundo
        .motor
        .parar(&mundo.s3)
        .await
        .unwrap_or_else(|motivo| panic!("{motivo}"));

    let fila = mundo.kesto.copiar(&tarea, PLAZO_DE_LA_COPIA).await;

    assert_eq!(
        fila["status"], "failed",
        "la copia contra un almacén caído tenía que fallar: {fila}"
    );

    let motivo = fila["error_message"].as_str().unwrap_or_default();
    assert!(
        !motivo.is_empty(),
        "la ejecución falló sin decir por qué: {fila}"
    );
    assert!(
        motivo.contains(&mundo.endpoint),
        "el motivo tenía que nombrar el almacén que no responde (`{}`): {motivo}",
        mundo.endpoint
    );

    let (estado, valor) = mundo.kesto.repositorio(&mundo.repositorio).await;

    assert!(
        estado.is_success(),
        "el repositorio tenía que seguir siendo legible y contestó {estado} con {valor}"
    );

    mundo.cerrar().await;
}

// ══ Requisito: la frontera del operador al motor se verifica ═══════════════

/// Escenario: un directorio fuera de la raíz de copias no se copia.
///
/// El alta solo comprueba la **forma** del `host_path` —textual y sin tocar el disco—, así que
/// una ruta que empieza por la raíz y la abandona por un enlace intermedio pasa el borde HTTP.
/// El motor canonicaliza antes de leer y ve que el enlace la lleva fuera: la copia acaba en
/// `failed`, el motivo nombra el campo y no llega a escribirse ninguna instantánea. Es la mitad
/// que solo el motor puede cerrar, y la que el escenario mide de punta a punta.
#[tokio::test]
async fn un_directorio_fuera_de_la_raiz_de_copias_no_se_copia() {
    let mundo = Mundo::nuevo().await;

    // El directorio que se copiaría está **fuera** de la raíz de copias de la batería: cuelga del
    // temporal del sistema, no de `mundo.raiz`, que es la raíz que el binario declara como
    // `KESTO_BACKUP_ROOT`. Tiene que existir: la canonicalización resuelve el enlace hasta él.
    let fuera = std::env::temp_dir().join(mundo.nombre_de_contenedor("fuera-de-la-raiz"));
    sembrar_directorio(&fuera);

    // El `host_path` declarado es, textualmente, una ruta **bajo** la raíz —el alta la acepta—,
    // y un enlace la lleva fuera de ella. Se sustituye el directorio que crea `trabajo` por el
    // enlace: lo que se declara es la ruta, no su destino.
    let enlace = mundo.trabajo("directorio", "enlace-fuera");
    std::fs::remove_dir(&enlace)
        .unwrap_or_else(|causa| panic!("no se pudo preparar `{}`: {causa}", enlace.display()));
    std::os::unix::fs::symlink(&fuera, &enlace).unwrap_or_else(|causa| {
        panic!("no se pudo crear el enlace `{}`: {causa}", enlace.display())
    });

    let target = target_directorio(&mundo.kesto, "e2e-fuera-de-la-raiz", &enlace).await;
    let tarea = mundo
        .kesto
        .crear_tarea(&target, &mundo.repositorio, "e2e-fuera-de-la-raiz", 7)
        .await;

    let fila = mundo.kesto.copiar(&tarea, PLAZO_DE_LA_COPIA).await;

    assert_eq!(
        fila["status"], "failed",
        "una copia cuyo `host_path` escapa de la raíz tenía que fallar: {fila}"
    );

    let motivo = fila["error_message"].as_str().unwrap_or_default();
    assert!(
        motivo.contains("host_path"),
        "el motivo tenía que nombrar el campo que quedó fuera de la raíz: {motivo}"
    );

    let instantaneas = mundo.kesto.instantaneas(&mundo.repositorio).await;
    assert!(
        instantaneas.is_empty(),
        "un `host_path` fuera de la raíz no puede dejar instantánea y dejó {instantaneas:?}"
    );

    let _ = std::fs::remove_dir_all(&fuera);
    mundo.cerrar().await;
}

/// Escenario: un contenedor que no se declara fuente de copia no se copia.
///
/// El motor inspecciona el contenedor antes de crear ningún `exec` y exige la etiqueta
/// `io.kesto.backup-source=true`. Sin ella la copia acaba en `failed`, el motivo nombra el
/// contenedor y no llega a escribirse ninguna instantánea: la frontera es **fail-closed** y la
/// declaración **viaja con el contenedor** —no es una lista de configuración que alguien pudiera
/// dejar vacía—. Es la mitad que solo el motor puede cerrar: el borde HTTP no sabe si un
/// contenedor se declaró fuente de copia.
#[tokio::test]
async fn un_contenedor_sin_la_etiqueta_de_fuente_no_se_copia() {
    let mut mundo = Mundo::nuevo().await;

    // El contenedor existe y tiene datos, pero **no** lleva la etiqueta: el motor no le crearía
    // ningún `exec`. La siembra va por el arnés, que habla con el motor directamente.
    let contenedor = mariadb_arrancado_sin_etiqueta(&mut mundo, "sin-etiqueta").await;
    sembrar_mariadb(&mundo, &contenedor, FILAS).await;

    let target = target_mariadb(&mundo.kesto, "e2e-sin-etiqueta", &contenedor).await;
    let tarea = mundo
        .kesto
        .crear_tarea(&target, &mundo.repositorio, "e2e-sin-etiqueta", 7)
        .await;

    let fila = mundo.kesto.copiar(&tarea, PLAZO_DE_LA_COPIA).await;

    assert_eq!(
        fila["status"], "failed",
        "una copia de un contenedor sin la etiqueta de fuente tenía que fallar: {fila}"
    );

    let motivo = fila["error_message"].as_str().unwrap_or_default();
    assert!(
        motivo.contains(&contenedor),
        "el motivo tenía que nombrar el contenedor que no se declaró fuente de copia: {motivo}"
    );

    let instantaneas = mundo.kesto.instantaneas(&mundo.repositorio).await;
    assert!(
        instantaneas.is_empty(),
        "un contenedor sin la etiqueta no puede dejar instantánea y dejó {instantaneas:?}"
    );

    mundo.cerrar().await;
}
