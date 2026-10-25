//! La extracción: por dónde sale la copia y quién la conduce hasta el repositorio.
//!
//! Este módulo responde a dos preguntas y hace una cosa más. La primera pregunta es **por
//! dónde sale la copia** —[`ruta`]—: de un proceso dentro del contenedor de la base, con el
//! comando que le corresponde a su tipo y el alcance que le da `db_name`, o de un directorio
//! del sistema de ficheros, que **no** pasa por Docker. La segunda es **quién conduce ese
//! proceso** —[`volcar`]—: recibe una orden y el productor del puente, y devuelve un
//! [`Resultado`] con el código de salida, los bytes leídos y el motivo que el contenedor
//! escribió por su canal de error. Y lo tercero, que es lo que menos se ve y más importa, es
//! **componer la condición de éxito**, que no se hereda de nadie.
//!
//! # La condición de éxito se compone aquí, en una línea
//!
//! Está medido en `src/rustic`: el archivado de la biblioteca **silencia** el error del
//! lector, sigue y devuelve `Ok` guardando la instantánea **sin el volcado dentro** —y en el
//! peor caso sin una sola línea de aviso—. De un `Ok` del archivado, por tanto, no se deduce
//! que la copia contenga el volcado, y quien mire ese `Ok` aguas abajo dará por buena una
//! copia que existe, ocupa sitio y no se puede restaurar.
//!
//! La forma de componerlo es la que gobierna [`volcar`]: **el productor solo declara el final
//! si el volcado salió bien**. Si el código de salida del `exec` no es `0`, el productor se
//! suelta **sin** llamar a [`terminar`](crate::rustic::ProductorDelVolcado::terminar), y el
//! puente —que falla cerrado— lee ese cierre como un volcado **truncado**. Con eso,
//! [`EntregaDelVolcado::completa`](crate::rustic::EntregaDelVolcado::completa) pasa a ser la
//! **única** señal que el motor necesita: significa «el volcado se entregó entero **y** el
//! proceso terminó bien», las dos cosas a la vez. El código de salida sigue viajando en el
//! [`Resultado`], pero para contarlo y para el motivo —no para decidir—: quien decide es la
//! señal.
//!
//! # El plazo es de inactividad, y hay uno solo para las dos esperas
//!
//! El `timeout` del cliente de la API de Docker **no acota el drenaje** —se resuelve cuando
//! llegan las cabeceras, y el cuerpo lo drena después el bucle del transporte—, así que lo
//! único que acota el trabajo es el silencio del proceso. Ese silencio se mira desde dos
//! sitios: el `stream` del `exec` y el canal del puente. Por eso hay **una** constante para
//! los dos —[`INACTIVIDAD_MAXIMA`]— y no dos: es la misma pregunta, y dos números distintos
//! para la misma pregunta serían dos verdades, con el menor de los dos mandando sin decirlo.
//!
//! # La credencial, en el entorno y fuera del motivo
//!
//! La contraseña de la base viaja en el **entorno** del `exec` —`MYSQL_PWD` para MariaDB y
//! `PGPASSWORD` para PostgreSQL— y **nunca** en su línea de comandos: en un contenedor, el
//! listado de procesos es público, y el `-p<contraseña>` del §4.A dejaba la credencial a la
//! vista donde más gente la mira. Cierra el cabo suelto 4 del SRS.
//!
//! De ahí salen dos consecuencias que este módulo asume. Una: el motivo que se guarda en
//! `job_executions` —que el panel muestra— tiene que salir **sin la credencial**, aunque el
//! contenedor la repita al fallar. Y dos: el valor del entorno no puede acabar en un
//! registro. Lo segundo **no se arregla aquí**: está medido que la biblioteca del motor
//! registra a `DEBUG` el cuerpo serializado de la petición, con el `env` dentro, así que se
//! corta por **filtro** de registro —y de eso se encarga el **arranque declarado**:
//! [`crate::DIRECTIVA_DEL_MOTOR`] lo añade el proceso detrás de lo que pida quien lo lance,
//! de modo que no depende de la receta con la que se arranque—. Ver el `# Seguridad` de
//! [`Orden`] y el de [`Cliente::ejecutar`].

use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::db::entities::{Target, TipoDeTarget};
use crate::docker::{Canal, Cliente, Orden, Trozo};
use crate::rustic::ProductorDelVolcado;
use crate::secret::{REDACTED, SecretString};

/// Capacidad del canal del puente, en **trozos**.
///
/// La unidad no es el byte: el canal retiene hasta esta cantidad de mensajes, y un trozo del
/// `exec` mide como mucho 8 KiB —medido: es el `output_capacity` por defecto de la
/// biblioteca, `bollard-0.18.1/src/exec.rs:222`—, así que la memoria retenida por una copia
/// en vuelo es de **1 MiB** (`128 × 8 KiB`). Es un tope que no da risa y que no estorba:
/// despreciable frente al cifrado en reposo y a la red, y suficiente para que el drenaje del
/// `exec` no se pare a esperar mientras el archivado comprime y deduplica el trozo anterior.
///
/// Más grande no compra nada —el consumidor no necesita más colchón— y más pequeño convierte
/// cada compresión de un trozo de 8 KiB en una parada del proceso del contenedor.
///
/// Es `pub` —y no `pub(crate)`, que es lo que pide el mínimo— por la regla de la casa: lo que
/// no es alcanzable desde la superficie pública del crate se considera código muerto, y quien
/// consume esta constante es la orquestación del motor, que llega **después** de esta mitad.
/// Un `pub(crate)` sin usuario hoy dispararía `dead_code` en la compilación del binario, y
/// declararla `pub` es la forma honesta de decir que forma parte del contrato.
pub const CAPACIDAD_DEL_CANAL: usize = 128;

/// Plazo máximo **sin recibir un solo byte**, para las dos esperas del volcado.
///
/// Es una constante y no dos porque son la misma pregunta —cuánto puede callarse el volcado—
/// vista desde dos sitios: el `stream` del `exec`, cuya inactividad acota el transporte, y el
/// canal del puente, cuya inactividad acota el lector. Dos números distintos para la misma
/// pregunta serían dos verdades, y el menor de los dos sería el que manda sin que nada lo
/// dijera.
///
/// El valor: cinco minutos. Un `mariadb-dump` de verdad tarda horas y **no** puede fallar por
/// tardar —lo que se acota es el silencio, no la duración—, pero tampoco puede estar minutos
/// sin escribir nada: un volcado que avanza escribe de forma continua, y el hueco más largo
/// que se le conoce es el de arranque —abrir la conexión y empezar a recorrer la primera
/// tabla—, que no llega a minutos. Un silencio de cinco minutos no es un volcado lento: es un
/// proceso colgado, y a partir de ahí cada minuto de espera es un minuto con el worker
/// secuencial del repositorio ocupado y todo el trabajo de ese repositorio haciendo cola
/// detrás.
///
/// Es `pub` por la misma razón que [`CAPACIDAD_DEL_CANAL`]: quien la pasa al transporte y al
/// puente es la orquestación del motor, que todavía no está.
pub const INACTIVIDAD_MAXIMA: Duration = Duration::from_secs(300);

/// Longitud máxima del motivo del contenedor, en **caracteres**.
///
/// El motivo es el `stderr` del proceso, y `job_executions.error_message` lo guarda y el
/// panel lo muestra: mil caracteres son más de lo que cabe en una fila de un panel y de sobra
/// para lo que un cliente de volcado dice al fallar —el error, la base y el usuario están en
/// las primeras líneas—, así que lo que venga después es texto que nadie va a leer.
const LIMITE_DEL_MOTIVO: usize = 1024;

/// Bytes que puede ocupar un carácter UTF-8: el margen del tope de acumulación.
///
/// Está aquí y no dentro de la cuenta del tope porque es lo que explica el margen: lo que se
/// acumula son **bytes** y el recorte se hace **por carácter**, así que el tope tiene que
/// dejar sitio a los peores caracteres que existen.
const BYTES_POR_CARACTER: usize = 4;

/// Tope de bytes que se acumulan del canal de error del proceso.
///
/// El `stderr` es el motivo, y el motivo se recorta a [`LIMITE_DEL_MOTIVO`] caracteres: lo que
/// hace falta retener es, como mucho, lo que el recorte va a conservar —mil caracteres, que
/// en el peor caso son `LIMITE_DEL_MOTIVO × BYTES_POR_CARACTER` bytes— más un margen de un
/// carácter, que es lo que cubre el caso en que el tope caiga **en mitad** de un carácter de
/// cuatro bytes. Lo que se tira al llegar aquí es lo que no cabría en el motivo: no se pierde
/// ni un carácter que el recorte hubiera conservado, y a cambio un contenedor que escriba sin
/// parar por su canal de error no infla un vector en el proceso del motor sin tope —el resto
/// del diseño ya está acotado: el canal son [`CAPACIDAD_DEL_CANAL`] trozos de 8 KiB—.
///
/// Que el tope pueda partir un carácter multibyte **no importa**: lo que se acumula son bytes
/// y el recorte por carácter va después, y el margen garantiza que el corte caiga más allá de
/// los mil caracteres que se conservan. Decisión escrita aquí para que nadie «arregle» el
/// tope recortando por dentro.
const TOPE_DEL_CRUDO: usize = LIMITE_DEL_MOTIVO * BYTES_POR_CARACTER + BYTES_POR_CARACTER;

/// Lo que hay que extraer, ya leído de la fila del §3.
///
/// Es la parte de [`Target`] que decide la extracción, y los campos anulables conservan su
/// `Option` porque **`NULL` y la cadena vacía no son lo mismo**: un target de directorio no
/// tiene contenedor y uno de base no tiene ruta de host, y eso el §3 lo dice con `NULL`, no
/// con `''`.
///
/// El [`Debug`] se deriva y es seguro: el único campo sensible es [`SecretString`], que
/// redacta su contenido en `Debug` y en `Display`. Ver la prueba del §7 en
/// [`crate::db::entities::target`], que fija lo mismo para la fila entera.
#[derive(Debug, Clone)]
pub struct Objetivo {
    /// Tipo de fuente, tal y como lo declara el `CHECK` del §3.
    pub tipo: TipoDeTarget,
    /// Contenedor del que extraer el volcado; `None` si es un directorio.
    pub container_name: Option<String>,
    /// Usuario de la base; `None` si es un directorio.
    pub db_user: Option<String>,
    /// Contraseña de la base, **ya descifrada**; `None` si es un directorio.
    pub db_password: Option<SecretString>,
    /// Base concreta del volcado; `None` implica volcado global.
    pub db_name: Option<String>,
    /// Ruta del directorio tal y como la ve este proceso; `None` si es una base.
    pub host_path: Option<String>,
}

impl From<&Target> for Objetivo {
    /// La fila del §3, recortada a lo que la extracción necesita.
    ///
    /// Existe para que nadie copie seis campos a mano: un campo copiado de menos deja la
    /// extracción sin credencial o sin contenedor —y el fallo aparece lejos de aquí—, y uno
    /// copiado **de más** es una extracción del target equivocado, que es peor.
    fn from(fila: &Target) -> Self {
        Self {
            tipo: fila.target_type,
            container_name: fila.container_name.clone(),
            db_user: fila.db_user.clone(),
            db_password: fila.db_password.clone(),
            db_name: fila.db_name.clone(),
            host_path: fila.host_path.clone(),
        }
    }
}

/// Por dónde sale la copia.
///
/// El [`Debug`] se deriva y es seguro, y conviene decir por qué: la variante de contenedor
/// lleva una [`Orden`], cuyo `Debug` publica el comando y los **nombres** de las variables de
/// entorno, nunca sus valores —la credencial va en el valor—. Eso es del transporte y está
/// fijado allí; aquí se hereda.
///
/// No se deriva `Clone`: la orden no lo es —es una orden para un `exec`, y una orden copiada
/// es una credencial copiada— y una ruta se construye una vez por copia, así que copiarla no
/// hace falta para nada.
#[derive(Debug)]
pub enum Ruta {
    /// Un proceso dentro del contenedor: el comando y su entorno.
    PorContenedor {
        /// Nombre del contenedor en el que hay que ejecutar la orden.
        contenedor: String,
        /// La orden, con la credencial —si la hay— en su entorno.
        orden: Orden,
    },
    /// Un directorio del sistema de ficheros, tal como lo ve este proceso.
    ///
    /// Un target de tipo `directory` **no** pasa por Docker: no hay proceso que ejecutar. La
    /// ruta es la de [`Objetivo::host_path`], que el §3 guarda y que el `compose` del §8
    /// monta dentro del contenedor de Kesto; se lee con el origen local de la biblioteca, y
    /// eso se declara como añadido al SRS porque el §4 solo fija los comandos de las dos
    /// bases.
    PorFichero {
        /// Ruta del directorio, en el espacio de ficheros de este proceso.
        ruta: PathBuf,
    },
}

/// Elige la ruta y construye el comando.
///
/// No toca el sistema de ficheros ni habla con nadie: decide **por dónde** sale la copia y
/// con **qué** orden, y nada más. Que el contenedor exista o que la ruta esté ahí se averigua
/// al usarla —al ejecutar el `exec` o al archivar el directorio—, no aquí.
///
/// # El comando, y `db_name` decidiendo el alcance
///
/// Es el cabo suelto 3 del SRS. El §4.A fija `mariadb-dump --all-databases` y `pg_dumpall`,
/// que vuelcan el servidor **entero** e ignoran `backup_targets.db_name`, de modo que con
/// ellos la columna era decorativa. El esquema ya había declarado qué significa —`NULL` es
/// global, informado es una sola base—, así que lo que faltaba era hacer los comandos
/// coherentes con ella:
///
/// - `mariadb`: `mariadb-dump --single-transaction --quick --lock-tables=false -u<db_user>` y
///   después `--all-databases` —si `db_name` es nulo— o la base nombrada. `--single-transaction`
///   y `--lock-tables=false` son lo que permite volcar una base en producción sin bloquearla.
/// - `postgresql`: `pg_dumpall -U <db_user>` si `db_name` es nulo, y `pg_dump -U <db_user> -d
///   <db_name>` si no. `pg_dumpall` **no puede** volcar una sola base: para el caso concreto
///   hace falta `pg_dump`, que es otro binario.
///
/// # Errors
///
/// Devuelve [`ErrorDeExtraccion::CampoQueFalta`] si el objetivo no trae un campo que su tipo
/// necesita —`host_path` para un directorio, `container_name` y `db_user` para una base—,
/// [`ErrorDeExtraccion::CampoEnBlanco`] si lo trae informado y no sirve, y
/// [`ErrorDeExtraccion::ValorQuePareceBandera`] si el valor de un campo que acaba en la línea
/// de comandos empieza por `-` —y entonces el cliente de volcado lo leería como una opción y
/// el alcance del volcado cambiaría sin que nada lo note—. Un `db_user` ausente en un target
/// de base es un objetivo incoherente y **no** un comando a medias: sin usuario, el cliente de
/// volcado no sabe con quién conectar.
pub fn ruta(objetivo: &Objetivo) -> Result<Ruta, ErrorDeExtraccion> {
    match objetivo.tipo {
        // Un target de directorio **no pasa por Docker**: no hay ningún proceso que ejecutar,
        // así que `PorContenedor` no se construye por este camino ni por ningún otro. La ruta
        // se interpreta en el espacio de ficheros de este proceso —el `compose` del §8 monta
        // el directorio del host dentro de Kesto—, que es la única lectura que tiene datos
        // detrás: para la otra haría falta un mapeo host→contenedor que nadie ha dado.
        TipoDeTarget::Directory => {
            let ruta = exigido(objetivo, "host_path", objetivo.host_path.as_deref())?;

            Ok(Ruta::PorFichero {
                ruta: PathBuf::from(ruta),
            })
        }

        TipoDeTarget::Mariadb => {
            let (contenedor, usuario, base) = datos_de_una_base(objetivo)?;

            let mut cmd = vec![
                "mariadb-dump".to_owned(),
                // Volcar una base en producción sin bloquearla: una transacción consistente y
                // sin cerrar las tablas mientras el volcado las recorre.
                "--single-transaction".to_owned(),
                "--quick".to_owned(),
                "--lock-tables=false".to_owned(),
                // El usuario va **pegado** a la bandera, que es como lo escribe el §4.A y
                // como lo entiende el cliente. La contraseña, en cambio, no está aquí: viaja
                // por el entorno y solo por el entorno.
                format!("-u{usuario}"),
            ];

            // Y aquí decide `db_name` el alcance: `NULL` es el servidor entero y un nombre es
            // esa sola base. Es la mitad que cierra el cabo suelto 3 del SRS.
            cmd.push(match base {
                Some(base) => base.to_owned(),
                None => "--all-databases".to_owned(),
            });

            Ok(Ruta::PorContenedor {
                contenedor: contenedor.to_owned(),
                orden: Orden {
                    cmd,
                    env: entorno_del_exec(TipoDeTarget::Mariadb, objetivo.db_password.as_ref()),
                },
            })
        }

        TipoDeTarget::Postgresql => {
            let (contenedor, usuario, base) = datos_de_una_base(objetivo)?;

            // `pg_dumpall` no puede volcar una sola base: para el caso concreto hace falta
            // `pg_dump`, que es **otro binario**. Y `-d` no es opcional en `pg_dump`: sin él
            // el volcado se lleva la base por defecto de la conexión, que no es la que el
            // operador nombró.
            let cmd = match base {
                Some(base) => vec![
                    "pg_dump".to_owned(),
                    "-U".to_owned(),
                    usuario.to_owned(),
                    "-d".to_owned(),
                    base.to_owned(),
                ],
                None => vec!["pg_dumpall".to_owned(), "-U".to_owned(), usuario.to_owned()],
            };

            Ok(Ruta::PorContenedor {
                contenedor: contenedor.to_owned(),
                orden: Orden {
                    cmd,
                    env: entorno_del_exec(TipoDeTarget::Postgresql, objetivo.db_password.as_ref()),
                },
            })
        }
    }
}

/// Los tres campos que un target de base necesita, o el error del que falta.
///
/// Se comprueban **antes** de construir nada y en este orden: un objetivo al que le falta el
/// usuario o el contenedor no es un comando a medias, es un objetivo incoherente, y el motivo
/// tiene que decir cuál de los tres falta. El orden es el mismo para las dos bases, así que el
/// error de un objetivo al que le faltan dos campos no depende del tipo.
fn datos_de_una_base(objetivo: &Objetivo) -> Result<(&str, &str, Option<&str>), ErrorDeExtraccion> {
    let contenedor = exigido(
        objetivo,
        "container_name",
        objetivo.container_name.as_deref(),
    )?;
    let usuario = exigido(objetivo, "db_user", objetivo.db_user.as_deref())?;
    let base = campo_opcional(objetivo, "db_name", objetivo.db_name.as_deref())?;

    // Y los dos campos que acaban en la línea de comandos pasan por la guarda de las banderas:
    // el usuario y la base son argumentos de un `argv`, y uno que empiece por `-` no es un
    // nombre sino una opción. El `container_name` **no** pasa por ahí —es un parámetro de la
    // API, no un argumento— y por eso no se le aplica.
    let usuario = sin_ser_bandera(objetivo, "db_user", usuario)?;
    let base = base
        .map(|base| sin_ser_bandera(objetivo, "db_name", base))
        .transpose()?;

    Ok((contenedor, usuario, base))
}

/// Rechaza un valor que la línea de comandos leería como una bandera.
///
/// Es la guarda de los dos campos que acaban en el `cmd` —`db_user` y `db_name`—: no hay
/// intérprete de por medio, así que un `db_name` como `--all-databases` se pega como argumento
/// suelto y **el cliente lo lee como una opción**, de modo que el alcance que decidió la fila
/// del §3 se salta sin que nada lo note. Un nombre de base o de usuario que empiece por `-` no
/// es un nombre: se rechaza con un error propio —no se cita, ni se recorta, ni se escapa—,
/// porque una corrección silenciosa convertiría un error de la fila en un comando distinto al
/// que el operador cree haber pedido.
fn sin_ser_bandera<'a>(
    objetivo: &Objetivo,
    campo: &'static str,
    valor: &'a str,
) -> Result<&'a str, ErrorDeExtraccion> {
    if valor.starts_with('-') {
        return Err(ErrorDeExtraccion::ValorQuePareceBandera {
            tipo: nombre_del_tipo(objetivo.tipo),
            campo,
        });
    }

    Ok(valor)
}

/// Lo que dejó una extracción.
///
/// El [`Debug`] se deriva, y es seguro porque el motivo **ya viene depurado**: se guarda sin
/// la credencial —ver [`motivo_para_guardar`]—, así que publicarlo no publica un secreto. Lo
/// que **no** se puede publicar nunca es el `stderr` crudo, y no hay ningún sitio donde
/// sobreviva.
#[derive(Debug)]
pub struct Resultado {
    /// El código con el que salió el proceso.
    pub codigo: i32,
    /// Los bytes que se leyeron del proceso: es lo que el panel publica como
    /// `bytes_processed`, y **no** son los bytes que llegaron al repositorio.
    pub bytes_leidos: u64,
    /// Lo que el contenedor escribió por su canal de error, ya recortado y ya **sin la
    /// credencial**.
    pub motivo_del_contenedor: String,
}

/// Conduce el `exec`, alimenta el productor con su salida y devuelve lo que dejó.
///
/// El productor se **consume**: solo declara el final si el volcado salió bien. Ver el
/// encabezado del módulo para el porqué —es la composición de la condición de éxito— y aquí
/// solo la forma:
///
/// ```text
/// let resultado = volcar(&cliente, &contenedor, &orden, productor, INACTIVIDAD_MAXIMA).await?;
/// // …
/// if entrega.completa() { /* el volcado se entregó entero Y el proceso terminó bien */ }
/// ```
///
/// # Qué es un éxito y qué no
///
/// Devuelve `Ok` siempre que el `exec` **haya llegado a su término**, con cualquier código de
/// salida: un proceso que sale con `3` es un hecho que hay que poder contar —el código, los
/// bytes leídos y el motivo del contenedor—, y devolverlo como error perdería las tres cosas.
/// Lo que decide si la copia vale es
/// [`EntregaDelVolcado::completa`](crate::rustic::EntregaDelVolcado::completa), y el
/// productor se ha encargado de que signifique las dos cosas a la vez.
///
/// Devuelve `Err` cuando la extracción **no se pudo completar**: el transporte falló, el
/// contenedor no está, o el archivado se rindió y el volcado se quedó sin destino. En los
/// tres casos no hay hechos que devolver —la cuenta de bytes está a medias— y el productor se
/// suelta sin declarar el final, así que el puente lee el cierre como un volcado truncado:
/// falla cerrado.
///
/// Y en los dos últimos desenlaces —el transporte que se rompe y el archivado que se rinde— el
/// error se lleva además lo que el contenedor alcanzó a decir por su canal de error **ya sin
/// la credencial** (ver [`motivo_para_guardar`]): en un fallo a mitad de volcado, esa última
/// frase suele ser la única pista de por qué se quedó a medias, y hasta esta corrección se
/// tiraba con el `return`. El desenlace del contenedor ausente no lleva nada, y no es un
/// olvido: cuando el `exec` no llega a arrancar no hay canal de error del que leer, y en cuanto
/// lo hay el `404` viaja con el motivo en vez de por el camino del contenedor ausente —ver
/// [`ErrorDeExtraccion::del_transporte`]—.
///
/// # El runtime que conduce esto tiene que seguir vivo
///
/// Es la restricción medida del transporte y no se repite aquí: el hilo que lanza el `exec`
/// tiene que drenarlo con **su** runtime vivo, porque si el runtime muere mientras se drena,
/// el drenaje falla con «A Tokio 1.x context was found, but it is being shutdown». Ver el
/// `# Seguridad` de [`Cliente::ejecutar`](crate::docker::Cliente::ejecutar).
///
/// # La cuenta de bytes
///
/// [`Resultado::bytes_leidos`] cuenta lo que llegó por el **canal de salida**, que es el
/// volcado. El canal de error **no** suma: es el motivo de un fallo, y contarlo como datos
/// copiados diría que se leyó algo que no se archivó.
///
/// # Errors
///
/// Devuelve [`ErrorDeExtraccion::ContenedorQueNoEsta`] si el motor contesta que el contenedor
/// no existe, [`ErrorDeExtraccion::Transporte`] para el resto de fallos del transporte y
/// [`ErrorDeExtraccion::ArchivadoSeRindio`] si el productor descubre que ya nadie lee.
pub async fn volcar(
    cliente: &Cliente,
    contenedor: &str,
    orden: &Orden,
    productor: ProductorDelVolcado,
    inactividad_maxima: Duration,
) -> Result<Resultado, ErrorDeExtraccion> {
    let mut bytes_leidos: u64 = 0;
    let mut crudo: Vec<u8> = Vec::new();
    let mut sin_destino: Option<crate::rustic::Error> = None;

    // El cierre va **prestado** al transporte —un `&mut` de un `FnMut` es `FnMut`— y vive en
    // su propio bloque para que el préstamo acabe con la llamada: el productor se consume
    // después.
    let codigo = {
        let mut al_llegar = |trozo: Trozo| match trozo.canal {
            Canal::Salida => {
                bytes_leidos += trozo.bytes.len() as u64;

                // El primer envío que falle significa que el archivado se rindió y ya nadie
                // lee: se recuerda y **no** se vuelve a intentar.
                //
                // Y no se corta el `exec`, porque no se puede: el cierre que el transporte
                // acepta no devuelve nada, así que no hay forma de decirle «basta» desde
                // aquí. El proceso se drena hasta el final y sus bytes se descartan. Lo que
                // importa se cumple igual —el error no se pierde, el productor no declara el
                // final y la extracción acaba en fallo—.
                if sin_destino.is_none()
                    && let Err(causa) = productor.enviar(trozo.bytes)
                {
                    sin_destino = Some(causa);
                }
            }
            // El canal de error **no** es volcado: es el motivo de un fallo, y va al sitio de
            // los motivos. Se acumula tal cual —sin recortar y sin normalizar, que de eso se
            // encarga el final— pero **acotado**: el recorte llega cuando el proceso ya acabó,
            // así que sin tope este vector crecería mientras el contenedor escriba. El tope y
            // su margen están en [`acumular_el_motivo`].
            //
            // Y no se registra nada de lo que llega por aquí: el `Debug` de `Trozo` publica
            // sus bytes —está escrito en `src/docker`—, así que un trozo de este canal en un
            // registro sería el `stderr` del contenedor —con la credencial dentro, si el
            // contenedor la repite— escrito en el registro de Kesto.
            Canal::Error => acumular_el_motivo(&mut crudo, &trozo.bytes),
        };

        cliente
            .ejecutar(contenedor, orden, inactividad_maxima, &mut al_llegar)
            .await
    };

    // Un fallo del transporte no deja hechos que devolver: no hay código de salida y la
    // cuenta de bytes está a medias. El productor se suelta aquí mismo —sin `terminar`, que es
    // lo que hace el `drop` al volver—, así que el puente lee el cierre como un volcado
    // truncado: falla cerrado.
    let codigo = match codigo {
        Ok(codigo) => codigo,
        // Y el error se lleva el motivo acumulado —ya depurado, sin la credencial, y vacío si
        // el contenedor no llegó a decir nada—: es el desenlace en el que más tiene que decir,
        // porque el transporte se rompió con un volcado en marcha.
        Err(causa) => {
            return Err(ErrorDeExtraccion::del_transporte(
                contenedor,
                causa,
                motivo_para_guardar(crudo, orden),
            ));
        }
    };

    // El archivado se rindió a mitad: el volcado no tiene destino y lo que ocurra con el
    // proceso ya no cambia nada de la copia. El `Err` se lleva la causa para que el motivo que
    // acabe en la base diga **por qué** la ejecución no acabó, en vez de un «no se entregó», y
    // el motivo del contenedor por la misma razón que arriba: es la última frase que se
    // escribió antes de que el volcado se quedara sin destino.
    if let Some(causa) = sin_destino {
        return Err(ErrorDeExtraccion::ArchivadoSeRindio {
            contenedor: contenedor.to_owned(),
            causa,
            motivo: motivo_para_guardar(crudo, orden),
        });
    }

    // Y **aquí** está la condición de éxito, en una línea: el productor declara el final solo
    // si el proceso salió con cero. Cualquier otro código suelta el productor sin declararlo,
    // y el puente —que falla cerrado— lee ese cierre como un volcado **truncado**. Por eso
    // `EntregaDelVolcado::completa()` significa las dos cosas a la vez —el volcado se entregó
    // entero **y** el proceso terminó bien— y el motor no tiene que consultar el código por
    // separado para decidir si la copia vale. El código sigue viajando en el resultado, pero
    // para contarlo y para el motivo.
    if codigo == 0 {
        productor.terminar();
    } else {
        // Sin `terminar`. El `drop` va explícito porque el **instante** importa: es lo que
        // cierra el canal sin la señal del final, y con él se desbloquea el lector del puente,
        // que estaba esperando su trozo siguiente o su cierre.
        drop(productor);
    }

    Ok(Resultado {
        codigo,
        bytes_leidos,
        motivo_del_contenedor: motivo_para_guardar(crudo, orden),
    })
}

/// El error de la extracción: lo que puede fallar antes de tener un volcado que copiar.
///
/// Se llama [`ErrorDeExtraccion`] y **no** `Error` a propósito, aunque `src/rustic` llame así
/// al suyo: este módulo va a reexportarlo desde [`crate::engine`] junto a los tipos de la
/// orquestación —y el motor tendrá que nombrar a la vez su error, el del transporte y el de
/// la biblioteca—, así que un `Error` a secas obligaría a alias en cada import y dejaría el
/// nombre más corto para el que menos lo merece. Los tres errores de esta casa se llaman
/// distinto y se leen sin ambigüedad.
///
/// Cada variante conserva lo que hace falta para diagnosticar, y los motivos que vienen de
/// fuera —el mensaje del motor— se conservan tal cual, porque son del motor.
#[derive(Debug, Error)]
pub enum ErrorDeExtraccion {
    /// El objetivo no trae un campo que su tipo necesita.
    #[error(
        "a un objetivo de tipo `{tipo}` le falta `{campo}`: sin él no hay extracción que valga"
    )]
    CampoQueFalta {
        /// Tipo del objetivo, con el texto que guarda el `CHECK` del §3.
        tipo: &'static str,
        /// El campo sin informar.
        campo: &'static str,
    },

    /// El objetivo trae el campo, pero no sirve para nada.
    ///
    /// Informado y en blanco **no** es lo mismo que ausente: un `container_name` con espacios
    /// llegaría al motor y el fallo aparecería como «ese contenedor no existe», que manda a
    /// buscar el problema en el motor en vez de en la fila que se editó mal. Se distingue para
    /// que el motivo diga cuál de las dos cosas pasa.
    #[error(
        "el `{campo}` de un objetivo de tipo `{tipo}` está informado y en blanco: no nombra nada"
    )]
    CampoEnBlanco {
        /// Tipo del objetivo, con el texto que guarda el `CHECK` del §3.
        tipo: &'static str,
        /// El campo informado y vacío.
        campo: &'static str,
    },

    /// El objetivo trae un valor que la línea de comandos leería como una bandera.
    ///
    /// Los campos que acaban en la línea de comandos —`db_user` y `db_name`— los escribe un
    /// operador autenticado, pero eso no los hace inofensivos: no hay intérprete de por medio,
    /// así que un valor como `--all-databases` se pega como argumento suelto y **el cliente lo
    /// lee como una opción**, y el alcance que había decidido la fila del §3 se salta sin que
    /// nada lo note. No es una inyección de shell —no hay shell—, es una entrada que se
    /// interpreta como control, y se rechaza en vez de citarse: no hay forma de escribir un
    /// nombre de base o de usuario que empiece por `-` que sea legítima, y una cita silenciosa
    /// convertiría un error de la fila en un comando distinto al que el operador cree.
    ///
    /// El valor **no** se publica —ni aquí ni en el motivo—: lo que se nombra es el campo, que
    /// es lo que hace falta para encontrar la fila que se editó mal. El valor es una cadena
    /// que empieza por `-`, y publicarla sería meter un trozo de línea de comandos en el
    /// registro y en `job_executions.error_message`; `CampoEnBlanco` toma la misma decisión y
    /// por la misma razón.
    #[error(
        "el `{campo}` de un objetivo de tipo `{tipo}` empieza por `-`: la línea de comandos lo \
         leería como una bandera y el alcance del volcado cambiaría sin que nada lo note"
    )]
    ValorQuePareceBandera {
        /// Tipo del objetivo, con el texto que guarda el `CHECK` del §3.
        tipo: &'static str,
        /// El campo cuyo valor parece una bandera.
        campo: &'static str,
    },

    /// El contenedor del objetivo no está en el motor de contenedores.
    ///
    /// El motivo del motor se conserva —hoy dice «no such container», y es su texto—, y el
    /// nombre del contenedor se pone aquí porque es lo que el operador tiene que poder
    /// reconocer en la fila del §3.
    #[error("el contenedor `{contenedor}` no está: {motivo}")]
    ContenedorQueNoEsta {
        /// Contenedor al que se refería la extracción.
        contenedor: String,
        /// Mensaje del motor, tal cual.
        motivo: String,
    },

    /// El transporte con la API de Docker no llegó a completar la extracción.
    ///
    /// Cubre los otros desenlaces del transporte: el socket que no está, el motor que no
    /// contesta, y la orden que deja de producir salida. Los distingue el transporte, y aquí
    /// se conservan: el diagnóstico de cada uno se hace en un sitio distinto.
    ///
    /// Y lleva además lo que el contenedor alcanzó a escribir por su canal de error **ya
    /// depurado** —la salida de [`motivo_para_guardar`], nunca el `stderr` crudo, así que
    /// aquí no hay credencial que publicar—: en un fallo a mitad de volcado, la última frase
    /// del contenedor suele ser la única pista de por qué se quedó a medias. Vacío cuando no
    /// dijo nada, que es el caso del socket que se cae antes de arrancar el `exec`.
    ///
    /// No se deriva `#[from]` desde [`crate::docker::Error`] a propósito: el `?` construiría
    /// esta variante con el motivo vacío **sin que nadie se entere**, que es exactamente la
    /// pérdida que esta corrección viene a cerrar. Se construye por
    /// [`ErrorDeExtraccion::del_transporte`], que sí recibe el motivo.
    #[error("{causa}{}", lo_que_dijo_el_contenedor(.motivo))]
    Transporte {
        /// La causa del transporte, tal y como la informó la biblioteca.
        #[source]
        causa: crate::docker::Error,
        /// Lo que el contenedor dijo por su canal de error, sin la credencial; vacío si no
        /// llegó a decir nada.
        motivo: String,
    },

    /// El archivado dejó de leer y el volcado se quedó sin destino.
    ///
    /// No es un fallo del contenedor ni del proceso: es que el otro extremo del puente se
    /// rindió. La extracción lo detecta en cuanto un envío falla, y esta variante lleva la
    /// causa para que el motivo que se guarde diga **por qué** la ejecución no acabó —y el
    /// motivo del contenedor, depurado igual que en [`ErrorDeExtraccion::Transporte`], porque
    /// su última frase es la pista de qué estaba pasando cuando el volcado se quedó sin
    /// destino—.
    #[error(
        "la extracción de `{contenedor}` se ha quedado sin destino: el archivado del volcado dejó de leer ({causa}){}",
        lo_que_dijo_el_contenedor(.motivo)
    )]
    ArchivadoSeRindio {
        /// Contenedor del que se estaba extrayendo.
        contenedor: String,
        /// La causa, tal y como la informó el puente.
        #[source]
        causa: crate::rustic::Error,
        /// Lo que el contenedor dijo por su canal de error, sin la credencial; vacío si no
        /// llegó a decir nada.
        motivo: String,
    },
}

/// Lo que un error añade con lo que dijo el contenedor, si dijo algo.
///
/// Es la mitad de la decoración de [`ErrorDeExtraccion::Transporte`] y de
/// [`ErrorDeExtraccion::ArchivadoSeRindio`], y vive aquí para que las dos variantes lo digan
/// igual: vacío cuando no hay nada que decir —un motivo que no existe no puede dejar un «el
/// contenedor dijo:» colgado en el mensaje, que se leería como que sí dijo—, y con el motivo
/// detrás cuando lo hay.
///
/// Lo que entra aquí ya viene de [`motivo_para_guardar`]: sin la credencial y recortado. Es la
/// única forma de que un error pueda llevar lo que dijo el contenedor sin publicar un secreto.
fn lo_que_dijo_el_contenedor(motivo: &str) -> String {
    if motivo.trim().is_empty() {
        return String::new();
    }

    format!("; el contenedor dijo: {motivo}")
}

impl ErrorDeExtraccion {
    /// Traduce un fallo del transporte, reconociendo el caso del contenedor que no está.
    ///
    /// El transporte ya distingue un `404` del motor de un problema del socket, y aquí se
    /// conserva esa distinción en el vocabulario de la extracción porque los dos desenlaces
    /// no se arreglan igual: uno es un `container_name` que no corresponde a ningún
    /// contenedor en marcha —se corrige la fila del §3— y el otro es un motor al que no se
    /// llega —se corrige el despliegue—.
    ///
    /// El `motivo` es lo que el contenedor dijo por su canal de error, ya depurado. Y decide el
    /// desenlace del `404`: cuando el `exec` no llegó a arrancar —el `create_exec` contesta que
    /// el contenedor no está, y entonces el búfer del canal de error está vacío por
    /// construcción— el desenlace es «el contenedor no está». Un `404` que llegue **con** algo
    /// escrito por el canal de error es otro caso —el contenedor desapareció a mitad del
    /// drenaje, o al preguntarle por el código— y ahí lo que el contenedor dijo tiene el mismo
    /// valor que en cualquier otro fallo del transporte, así que va con él: el `404` sigue
    /// dicho dentro de la causa, que es lo que el diagnóstico necesita.
    fn del_transporte(contenedor: &str, causa: crate::docker::Error, motivo: String) -> Self {
        match causa {
            // El 404 es el motor diciendo que ese contenedor no está. Se reconoce aquí para
            // que el operador lea «el contenedor `x` no está» y no un código de estado: el
            // diagnóstico es mirar la fila del §3, no el motor.
            crate::docker::Error::MotorContesta {
                status_code: 404,
                message,
                ..
            } if motivo.is_empty() => Self::ContenedorQueNoEsta {
                contenedor: contenedor.to_owned(),
                motivo: message,
            },

            // Todo lo demás se conserva tal cual, con la variante del transporte: es donde
            // están el socket que no existe, el motor que no contesta y la orden que dejó de
            // producir salida, y cada uno se diagnostica en un sitio distinto. Con lo que el
            // contenedor alcanzó a decir, que es la mitad que faltaba para diagnosticarlo.
            causa => Self::Transporte { causa, motivo },
        }
    }
}

/// El texto con el que se nombra un tipo de target: el mismo que guarda el `CHECK` del §3.
///
/// El motivo se lee en el panel y nombra el tipo como lo nombra la base —`mariadb`, no
/// `Mariadb`—, que es como el operador lo ve al mirar la columna.
fn nombre_del_tipo(tipo: TipoDeTarget) -> &'static str {
    match tipo {
        TipoDeTarget::Mariadb => "mariadb",
        TipoDeTarget::Postgresql => "postgresql",
        TipoDeTarget::Directory => "directory",
    }
}

/// Un campo que el tipo exige: o su valor, o el error que dice cuál falta.
///
/// El valor se devuelve **tal cual se informó**: se rechaza el que está en blanco —no puede
/// nombrar un contenedor ni una ruta—, pero no se recorta por dentro. Recortar una ruta o un
/// nombre de base sería cambiar en silencio lo que el operador escribió, y esta casa no
/// corrige en silencio.
fn exigido<'a>(
    objetivo: &Objetivo,
    campo: &'static str,
    valor: Option<&'a str>,
) -> Result<&'a str, ErrorDeExtraccion> {
    let tipo = nombre_del_tipo(objetivo.tipo);

    match valor {
        None => Err(ErrorDeExtraccion::CampoQueFalta { tipo, campo }),

        // El que solo tiene espacios no nombra nada: un contenedor `" "` no es un contenedor y
        // una ruta `"   "` no es una ruta. Se distingue del ausente para que el motivo mande a
        // la fila del §3 —el campo está informado— y no a un «no existe» del motor, que es
        // donde el operador buscaría el problema si los dos desenlaces dijeran lo mismo.
        Some(valor) if valor.trim().is_empty() => {
            Err(ErrorDeExtraccion::CampoEnBlanco { tipo, campo })
        }

        // Y se devuelve **tal cual se informó**: recortar por dentro una ruta o un nombre de
        // base sería cambiar en silencio lo que el operador escribió.
        Some(valor) => Ok(valor),
    }
}

/// Un campo que el §3 declara anulable: `None` es legítimo y significa lo que dice el §3.
///
/// `db_name` es el caso: `NULL` significa volcado global y no es un hueco que rellenar. Lo
/// que sí se rechaza es un `Some` en blanco, que no es `NULL` ni una base: sería un volcado
/// sin alcance declarado.
fn campo_opcional<'a>(
    objetivo: &Objetivo,
    campo: &'static str,
    valor: Option<&'a str>,
) -> Result<Option<&'a str>, ErrorDeExtraccion> {
    match valor {
        None => Ok(None),

        Some(valor) if valor.trim().is_empty() => Err(ErrorDeExtraccion::CampoEnBlanco {
            tipo: nombre_del_tipo(objetivo.tipo),
            campo,
        }),

        Some(valor) => Ok(Some(valor)),
    }
}

/// El entorno del `exec`: la contraseña, **nunca** en la línea de comandos.
///
/// La variable es la del cliente que va a volcar —`MYSQL_PWD` para MariaDB y `PGPASSWORD`
/// para PostgreSQL—, y las dos las entienden sus clientes de toda la vida. Sin contraseña no
/// se añade variable ninguna.
///
/// Y una contraseña **vacía** tampoco: `MYSQL_PWD=` no significa «sin contraseña», significa
/// «con contraseña vacía», que no es lo mismo y haría fallar la conexión por un camino que no
/// dice nada de esto. Como la capa de datos no deja guardar un secreto vacío, la guarda es la
/// mitad simétrica de aquélla.
///
/// El valor sale de [`SecretString::expose`], que es el único camino por el que el contenido
/// del tipo sale —y por eso es un método con nombre: cada sitio que lo usa se ve—. Lo que se
/// construye es el `CLAVE=valor` que el `exec` necesita, y esa cadena vive en una [`Orden`],
/// cuyo `Debug` publica el nombre de la variable y nunca su valor.
fn entorno_del_exec(tipo: TipoDeTarget, contrasena: Option<&SecretString>) -> Vec<String> {
    // Sin contraseña no se añade **nada**: el `env` es aditivo sobre el del contenedor, así
    // que una variable de más es una variable de más.
    let Some(contrasena) = contrasena else {
        return Vec::new();
    };

    // `expose` es el único camino por el que el valor sale del `SecretString`, y está aquí y
    // en ningún otro sitio de este fichero.
    let valor = contrasena.expose();

    // Una contraseña vacía tampoco entra: `MYSQL_PWD=` no significa «sin contraseña»,
    // significa «con contraseña vacía», que no es lo mismo y haría fallar la conexión por un
    // camino que no dice nada de esto.
    if valor.is_empty() {
        return Vec::new();
    }

    let variable = match tipo {
        TipoDeTarget::Mariadb => "MYSQL_PWD",
        TipoDeTarget::Postgresql => "PGPASSWORD",

        // Un directorio no pasa por Docker: no hay `exec` al que darle un entorno. No se
        // puede alcanzar desde `ruta` —el directorio sale por el sistema de ficheros— y se
        // contesta con el entorno vacío en vez de con un pánico: un motor no se cae por un
        // camino que no debía recorrerse.
        TipoDeTarget::Directory => return Vec::new(),
    };

    // Lo que se construye aquí es la credencial en claro dentro de una cadena, y es
    // inevitable: es lo que el `exec` necesita. Esa cadena vive en una `Orden`, cuyo `Debug`
    // publica el nombre de la variable y nunca su valor; y la exposición que no se puede
    // cortar por tipo —la petición que serializa la biblioteca a `DEBUG`— se corta por filtro
    // de registro en el arranque.
    vec![format!("{variable}={valor}")]
}

/// Acumula lo que el proceso escribió por su canal de error, hasta [`TOPE_DEL_CRUDO`].
///
/// Acotado y no «todo lo que llegue»: el motivo se recorta al final —cuando el proceso ya
/// acabó—, así que retener más de lo que el recorte va a conservar no compra nada y sí deja a
/// un contenedor que escriba sin parar inflando memoria en el proceso del motor. Lo que se
/// tira aquí es exactamente lo que no cabría en el motivo, y el margen de [`TOPE_DEL_CRUDO`]
/// cubre que el corte caiga en mitad de un carácter: el recorte por carácter va después y el
/// margen garantiza que ese corte quede más allá de los caracteres que se conservan.
fn acumular_el_motivo(crudo: &mut Vec<u8>, bytes: &[u8]) {
    let libres = TOPE_DEL_CRUDO.saturating_sub(crudo.len());
    let hasta = libres.min(bytes.len());

    crudo.extend_from_slice(&bytes[..hasta]);
}

/// El motivo del contenedor listo para guardarse: **sin la credencial** y **recortado**.
///
/// El orden de las dos cosas no es indiferente, y por eso están en la misma función: primero
/// se quita la credencial y **después** se recorta. Al revés, un motivo cuya contraseña
/// estuviera justo en el borde del límite se cortaría **por dentro de la credencial**, el
/// valor buscado ya no aparecería entero y se guardarían sus primeros caracteres —que es
/// publicar media credencial creyendo haberla quitado—. Con este orden, lo único que puede
/// quedar cortado a medias es la marca que la sustituye, y la marca no dice nada de nadie.
///
/// El búfer crudo entra **por valor** y no sale de aquí: es lo único que lleva el `stderr` sin
/// depurar, y consumirlo en esta función deja claro que no sobrevive a ella. Lo que sale es un
/// [`String`] que ya no lleva la credencial.
///
/// Y de dónde viene: lo acumula [`acumular_el_motivo`], que lo deja acotado a
/// [`TOPE_DEL_CRUDO`]. Lo que se tiró allí es lo que no cabría en el motivo, así que esta
/// función nunca ve el `stderr` entero sino su principio —que es justo lo que el recorte
/// conservaría— y el llamante no tiene que acordarse de nada.
fn motivo_para_guardar(crudo: Vec<u8>, orden: &Orden) -> String {
    // El `stderr` de un contenedor no tiene por qué ser UTF-8 válido —una base puede
    // escribirlo en la codificación de su idioma—, así que lo que no lo sea se sustituye en
    // vez de rechazarse: perder el motivo entero por un byte raro dejaría al operador sin el
    // único texto que dice qué pasó.
    let texto = String::from_utf8_lossy(&crudo);

    // Primero la credencial, después el recorte. Ver el encabezado de la función.
    recortar(&quitar_los_valores_del_entorno(&texto, orden))
}

/// El texto **sin** los valores que la orden entregó por su entorno.
///
/// Se quitan **todos** los valores del entorno de la orden, no solo los de las dos variables
/// que hoy llevan una contraseña. La razón es que la orden es lo único que [`volcar`] tiene a
/// mano —no recibe el [`Objetivo`]— y su entorno es exactamente el conjunto de lo que se le
/// dio al proceso: el día que una orden lleve otra credencial —un fichero de credenciales, un
/// testigo— la red es la misma sin que nadie tenga que acordarse de ampliarla.
///
/// Un valor **vacío** no se quita: sustituir la cadena vacía insertaría la marca entre todos
/// los caracteres del motivo y lo dejaría ilegible. Y como una contraseña vacía no llega a
/// entrar en el entorno (ver [`entorno_del_exec`]), las dos mitades van juntas.
///
/// Lo que **no** se hace con el valor: copiarlo. La comparación y la sustitución trabajan
/// sobre el `&str` que ya vive en la orden, sin pasar por ninguna estructura nueva que un
/// `Debug` pueda publicar y sin construir una segunda copia del secreto.
///
/// # El orden: de mayor a menor longitud
///
/// La sustitución va del valor **más largo** al más corto, y no en el orden del entorno. La
/// razón es concreta: dos valores en los que uno es **prefijo** del otro —`Zq7-clave` y
/// `Zq7-clave-de-base`— dejan la punta del largo en el texto si primero se sustituye el corto,
/// porque después la sustitución del largo ya no encuentra su cadena entera y lo que sobrevive
/// es su cola. Del más largo al más corto, el largo se va antes de que nadie le corte nada y
/// lo que queda del corto son sus apariciones de verdad.
///
/// Hoy no es alcanzable —una orden lleva cero o una variable—, pero el contrato de esta red es
/// el día en que una orden lleve otra credencial, y una red que no cubre ese caso es peor que
/// no tenerla: se creería cubierto. Dos valores de la **misma** longitud no pueden ser prefijo
/// uno del otro, así que el orden entre iguales da igual.
fn quitar_los_valores_del_entorno(texto: &str, orden: &Orden) -> String {
    let mut limpio = texto.to_owned();

    let mut valores: Vec<&str> = valores_del_entorno(&orden.env)
        // El valor vacío se salta: `replace("")` insertaría la marca **entre todos** los
        // caracteres del motivo y lo dejaría ilegible. Y como una contraseña vacía no llega a
        // entrar en el entorno, las dos mitades van juntas.
        .filter(|valor| !valor.is_empty())
        .collect();
    valores.sort_by_key(|valor| std::cmp::Reverse(valor.len()));

    for valor in valores {
        // Y el valor se usa **prestado**, desde el `&str` que ya vive en la orden: no se
        // construye ninguna copia del secreto que pueda acabar en un `Debug` o en un registro,
        // y la única cadena nueva es la que sale con la marca dentro.
        limpio = limpio.replace(valor, REDACTED);
    }

    limpio
}

/// Los **valores** del entorno de una orden, sin sus nombres.
///
/// Una variable sin `=` no tiene valor que quitar y se descarta: no es una variable de
/// entorno, es una cadena que alguien puso donde no iba.
fn valores_del_entorno(env: &[String]) -> impl Iterator<Item = &str> {
    env.iter()
        .filter_map(|variable| variable.split_once('=').map(|(_nombre, valor)| valor))
}

/// Corta el motivo a [`LIMITE_DEL_MOTIVO`] caracteres, y lo dice cuando lo corta.
///
/// **Por carácter y no por byte**: el `stderr` de un contenedor lleva acentos y, con acentos,
/// caracteres de más de un byte. Un corte por byte parte el carácter por la mitad —en Rust ni
/// siquiera compila, porque un `&str` no puede quedar desalineado— y lo que quedaría no es un
/// texto más corto: es un texto roto.
///
/// El `…` final no es adorno: es la única señal de que el motivo está incompleto. Sin él, un
/// error cortado se leería como el error entero y el operador buscaría una frase que no está
/// en ninguna parte.
///
/// Y el borde, escrito para que no lo «arregle» nadie: un motivo que mida **exactamente** el
/// límite no se toca —cabe—, y uno que mida uno más se corta a [`LIMITE_DEL_MOTIVO`] caracteres
/// contando el `…`, así que pierde un carácter respecto a lo que traía. El resultado mide el
/// límite en los dos casos, que es lo que el campo de la base puede guardar.
fn recortar(motivo: &str) -> String {
    if motivo.chars().count() <= LIMITE_DEL_MOTIVO {
        return motivo.to_owned();
    }

    // Un carácter del límite se reserva para el `…`, así que el resultado mide exactamente el
    // límite. Y el corte es por carácter —`take` avanza por caracteres—, así que ningún
    // carácter de más de un byte puede quedar partido: en Rust un `&str` no puede quedar
    // desalineado, así que la alternativa ni siquiera compila; el riesgo real sería la de
    // tratar los bytes a mano, y ésa es la que esta línea descarta.
    let mut recortado: String = motivo.chars().take(LIMITE_DEL_MOTIVO - 1).collect();
    recortado.push('…');

    recortado
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{
        CAPACIDAD_DEL_CANAL, ErrorDeExtraccion, INACTIVIDAD_MAXIMA, LIMITE_DEL_MOTIVO, Objetivo,
        Resultado, Ruta, TOPE_DEL_CRUDO, acumular_el_motivo, motivo_para_guardar, nombre_del_tipo,
        recortar, ruta, volcar,
    };
    use crate::db::entities::{Target, TipoDeTarget};
    use crate::docker::{Cliente, Orden};
    use crate::rustic::{ProductorDelVolcado, TunelDelVolcado, tunel_del_volcado};
    use crate::secret::{REDACTED, SecretString};
    use crate::test_support::{
        ContenedorDePrueba, TIMEOUT_DEL_MOTOR, lector_del_volcado, socket_del_motor,
    };

    /// Contraseña distinguible: si aparece en una salida, la salida es el fallo.
    ///
    /// Empieza por caracteres que no aparecen en ningún otro texto de estas pruebas, porque
    /// una de ellas busca los **prefijos** de la contraseña y un prefijo que coincidiera con
    /// el texto de al lado daría un falso positivo.
    const CONTRASENA: &str = "Zq7-clave-de-base-distinguible";

    /// Plazo de inactividad del estancamiento: tiene que vencer, así que es corto.
    ///
    /// Dos segundos y no los 500 ms que usa el transporte: aquí la prueba necesita que el
    /// contenedor alcance a escribir su motivo por el canal de error **antes** de callarse, y
    /// lo que mide es el estancamiento, no la carrera de arranque del `exec`.
    const PLAZO_ESTANCADO: Duration = Duration::from_secs(2);

    // ── Ayudantes ────────────────────────────────────────────────────────────

    /// Un `SecretString` con el valor dado.
    fn secreto(valor: &str) -> SecretString {
        SecretString::new(valor.to_owned())
    }

    /// La línea de comandos como la escribiría un `argv`, para poder compararla entera.
    ///
    /// Comparar el `cmd` **completo** y no solo la presencia de una bandera es deliberado: lo
    /// que la spec fija es el comando, con su orden, y una bandera de más o de menos que no
    /// se nombre en la prueba pasaría desapercibida con una aserción de pertenencia.
    fn argv(argumentos: &[&str]) -> Vec<String> {
        argumentos
            .iter()
            .map(|argumento| (*argumento).to_owned())
            .collect()
    }

    /// Un objetivo de MariaDB con todo informado, y el alcance que diga `db_name`.
    fn objetivo_mariadb(db_name: Option<&str>) -> Objetivo {
        Objetivo {
            tipo: TipoDeTarget::Mariadb,
            container_name: Some("contenedor-de-la-base".to_owned()),
            db_user: Some("operador".to_owned()),
            db_password: Some(secreto(CONTRASENA)),
            db_name: db_name.map(ToOwned::to_owned),
            host_path: None,
        }
    }

    /// Un objetivo de PostgreSQL con todo informado, y el alcance que diga `db_name`.
    fn objetivo_postgresql(db_name: Option<&str>) -> Objetivo {
        Objetivo {
            tipo: TipoDeTarget::Postgresql,
            container_name: Some("contenedor-de-la-base".to_owned()),
            db_user: Some("operador".to_owned()),
            db_password: Some(secreto(CONTRASENA)),
            db_name: db_name.map(ToOwned::to_owned),
            host_path: None,
        }
    }

    /// Un objetivo de directorio: sin contenedor, sin credenciales y con su ruta.
    fn objetivo_directorio(host_path: Option<&str>) -> Objetivo {
        Objetivo {
            tipo: TipoDeTarget::Directory,
            container_name: None,
            db_user: None,
            db_password: None,
            db_name: None,
            host_path: host_path.map(ToOwned::to_owned),
        }
    }

    /// La ruta de un objetivo, exigiendo que sea **por contenedor**.
    ///
    /// El `panic` del otro desenlace es la mitad de la prueba en todos los casos de base: un
    /// objetivo de base que saliera por el sistema de ficheros no tendría comando con el que
    /// medir nada, y el fallo tiene que decir eso y no «no hay comando».
    fn por_contenedor(objetivo: &Objetivo) -> (String, Orden) {
        match ruta(objetivo).expect("esta ruta tiene que construirse") {
            Ruta::PorContenedor { contenedor, orden } => (contenedor, orden),
            Ruta::PorFichero { ruta } => panic!(
                "un objetivo de tipo `{:?}` no puede salir por el sistema de ficheros: {ruta:?}",
                objetivo.tipo
            ),
        }
    }

    // ── Sin motor de contenedores: esto corre en el CI ───────────────────────

    /// MariaDB sin `db_name` vuelca **todo el servidor**.
    ///
    /// Cubre el primer escenario del requisito del comando: lleva `--all-databases` y **no**
    /// nombra ninguna base —el comando entero está fijado, así que no hay ningún argumento
    /// suelto que pueda ser un nombre de base—.
    #[test]
    fn el_comando_de_mariadb_sin_base_nombra_todo_el_servidor() {
        let (contenedor, orden) = por_contenedor(&objetivo_mariadb(None));

        assert_eq!(contenedor, "contenedor-de-la-base");
        assert_eq!(
            orden.cmd,
            argv(&[
                "mariadb-dump",
                "--single-transaction",
                "--quick",
                "--lock-tables=false",
                "-uoperador",
                "--all-databases",
            ]),
            "el comando de un volcado global no es el del §4.A: {:?}",
            orden.cmd
        );
    }

    /// MariaDB con `db_name` vuelca **esa** base y no el servidor entero.
    ///
    /// Es la mitad que cierra el cabo suelto 3: con `--all-databases` la columna `db_name`
    /// del §3 era decorativa.
    #[test]
    fn el_comando_de_mariadb_con_base_nombra_esa_base_y_solo_esa() {
        let (_contenedor, orden) = por_contenedor(&objetivo_mariadb(Some("tienda")));

        assert_eq!(
            orden.cmd,
            argv(&[
                "mariadb-dump",
                "--single-transaction",
                "--quick",
                "--lock-tables=false",
                "-uoperador",
                "tienda",
            ]),
            "el comando no nombra la base pedida: {:?}",
            orden.cmd
        );
        assert!(
            !orden
                .cmd
                .iter()
                .any(|argumento| argumento == "--all-databases"),
            "un volcado de una sola base no puede llevarse el servidor entero: {:?}",
            orden.cmd
        );
    }

    /// PostgreSQL sin `db_name` usa `pg_dumpall`.
    #[test]
    fn el_comando_de_postgresql_sin_base_usa_pg_dumpall() {
        let (_contenedor, orden) = por_contenedor(&objetivo_postgresql(None));

        assert_eq!(
            orden.cmd,
            argv(&["pg_dumpall", "-U", "operador"]),
            "el comando de un volcado global no es `pg_dumpall`: {:?}",
            orden.cmd
        );
    }

    /// PostgreSQL con `db_name` usa `pg_dump -d` y **no** `pg_dumpall`.
    ///
    /// `pg_dumpall` no puede volcar una sola base: para el caso concreto hace falta otro
    /// binario, y es lo que fija esta prueba.
    #[test]
    fn el_comando_de_postgresql_con_base_usa_pg_dump() {
        let (_contenedor, orden) = por_contenedor(&objetivo_postgresql(Some("almacen")));

        assert_eq!(
            orden.cmd,
            argv(&["pg_dump", "-U", "operador", "-d", "almacen"]),
            "el comando no volca la base pedida: {:?}",
            orden.cmd
        );
        assert!(
            !orden.cmd.iter().any(|argumento| argumento == "pg_dumpall"),
            "un volcado de una sola base no puede llevarse el servidor entero: {:?}",
            orden.cmd
        );
    }

    /// **La contraseña no está en el `cmd` y sí en la variable de entorno de su tipo.**
    ///
    /// Es la prueba que cierra el cabo suelto 4 del SRS: en un contenedor, la línea de
    /// comandos es pública para cualquiera que mire el listado de procesos, y el
    /// `-p<contraseña>` del §4.A dejaba la credencial justo ahí.
    #[test]
    fn la_contrasena_viaja_en_el_entorno_y_nunca_en_la_linea_de_comandos() {
        for (objetivo, variable, usuario_en_el_cmd) in [
            // El usuario se escribe como lo escribe cada cliente: pegado a la bandera en
            // MariaDB y como argumento suelto en PostgreSQL.
            (objetivo_mariadb(None), "MYSQL_PWD", "-uoperador"),
            (objetivo_postgresql(None), "PGPASSWORD", "operador"),
        ] {
            let (_contenedor, orden) = por_contenedor(&objetivo);

            assert!(
                !orden
                    .cmd
                    .iter()
                    .any(|argumento| argumento.contains(CONTRASENA)),
                "la contraseña está en la línea de comandos: {:?}",
                orden.cmd
            );
            assert!(
                orden
                    .cmd
                    .iter()
                    .any(|argumento| argumento == usuario_en_el_cmd),
                "el comando tiene que llevar el usuario: {:?}",
                orden.cmd
            );
            assert_eq!(
                orden.env,
                vec![format!("{variable}={CONTRASENA}")],
                "la contraseña no viaja en `{variable}`: {:?}",
                orden.env
            );

            // Y el `Debug` de la orden —y el de la ruta que la lleva dentro— tampoco la
            // publica: es lo que hace que un registro de depuración no sea una filtración.
            let ruta = ruta(&objetivo).expect("la ruta ya se construyó una vez");
            assert!(
                !format!("{ruta:?}").contains(CONTRASENA),
                "el Debug de la ruta publica la contraseña: {ruta:?}"
            );
        }
    }

    /// Un target **sin** contraseña no inventa una variable de entorno.
    ///
    /// Cubre las dos formas de «sin contraseña»: la columna `NULL` —que es el `None`— y el
    /// secreto vacío, que no es lo mismo que `NULL` pero que tampoco puede convertirse en
    /// `MYSQL_PWD=` —eso sería «contraseña vacía», y el cliente fallaría por un camino que no
    /// dice nada de esto—.
    #[test]
    fn un_target_sin_contrasena_no_inventa_la_variable() {
        for contrasena in [None, Some(secreto(""))] {
            let objetivo = Objetivo {
                db_password: contrasena,
                ..objetivo_mariadb(None)
            };

            let (_contenedor, orden) = por_contenedor(&objetivo);

            assert!(
                orden.env.is_empty(),
                "un target sin contraseña no puede llevar entorno: {:?}",
                orden.env
            );
            assert_eq!(
                orden.cmd,
                argv(&[
                    "mariadb-dump",
                    "--single-transaction",
                    "--quick",
                    "--lock-tables=false",
                    "-uoperador",
                    "--all-databases",
                ]),
                "sin contraseña el comando tiene que ser el mismo: {:?}",
                orden.cmd
            );
        }
    }

    /// Un target de base **sin usuario** no se puede extraer, y el motivo nombra el campo.
    ///
    /// Un `db_user` ausente no es una configuración floja: es un comando que no se puede
    /// construir —el cliente de volcado no sabría con quién conectar— y devolver una orden a
    /// medias daría un fallo del contenedor que no diría nada de la fila del §3.
    #[test]
    fn un_target_de_base_sin_usuario_no_se_puede_extraer() {
        for tipo in [TipoDeTarget::Mariadb, TipoDeTarget::Postgresql] {
            let objetivo = Objetivo {
                db_user: None,
                ..objetivo_mariadb(None)
            };
            let objetivo = Objetivo { tipo, ..objetivo };

            let error = ruta(&objetivo).expect_err("sin usuario no hay comando que construir");

            assert!(
                matches!(
                    &error,
                    ErrorDeExtraccion::CampoQueFalta {
                        campo: "db_user",
                        tipo: cual,
                    } if *cual == nombre_del_tipo(tipo)
                ),
                "el fallo no nombra el campo que falta: {error:?}"
            );
            assert!(
                error.to_string().contains("db_user"),
                "el motivo no nombra el campo: {error}"
            );
        }
    }

    /// Un target de base **sin contenedor** no se puede extraer, y el motivo lo dice.
    #[test]
    fn un_target_de_base_sin_contenedor_no_se_puede_extraer() {
        let objetivo = Objetivo {
            container_name: None,
            ..objetivo_mariadb(None)
        };

        let error = ruta(&objetivo).expect_err("sin contenedor no hay dónde ejecutar la orden");

        assert!(
            matches!(
                &error,
                ErrorDeExtraccion::CampoQueFalta {
                    campo: "container_name",
                    ..
                }
            ),
            "el fallo no nombra el contenedor que falta: {error:?}"
        );
        assert!(
            error.to_string().contains("container_name"),
            "el motivo no nombra el campo: {error}"
        );
    }

    /// Un campo informado **en blanco** no sirve, y el motivo lo distingue de «falta».
    ///
    /// No es lo mismo `NULL` que `' '`: el primero no está y el segundo llegaría al motor
    /// —o al sistema de ficheros— como un nombre que no nombra nada, y el fallo aparecería
    /// lejos de la fila que se editó mal.
    #[test]
    fn un_campo_informado_pero_en_blanco_no_sirve() {
        let caso = |objetivo: Objetivo, campo: &'static str| {
            let error = ruta(&objetivo).expect_err("un campo en blanco no puede dar una ruta");

            assert!(
                matches!(
                    &error,
                    ErrorDeExtraccion::CampoEnBlanco { campo: cual, .. } if *cual == campo
                ),
                "un campo en blanco tiene que rechazarse nombrando el campo `{campo}`: {error:?}"
            );
        };

        caso(
            Objetivo {
                container_name: Some("   ".to_owned()),
                ..objetivo_mariadb(None)
            },
            "container_name",
        );
        caso(
            Objetivo {
                db_user: Some(String::new()),
                ..objetivo_postgresql(None)
            },
            "db_user",
        );
        caso(
            Objetivo {
                db_name: Some("  ".to_owned()),
                ..objetivo_mariadb(None)
            },
            "db_name",
        );
        caso(objetivo_directorio(Some("")), "host_path");
    }

    /// Un `db_name` **nulo** es legítimo y significa volcado global: no es un campo que falte.
    ///
    /// Es la mitad simétrica de la prueba de arriba, y la que impide «arreglar» los comandos
    /// exigiendo siempre una base: el §3 dice que `NULL` significa el servidor entero.
    #[test]
    fn un_db_name_nulo_no_es_un_campo_que_falte() {
        por_contenedor(&objetivo_mariadb(None));
        por_contenedor(&objetivo_postgresql(None));
    }

    /// Un valor que **parece una bandera** se rechaza, y el motivo nombra el campo.
    ///
    /// Es la corrección de la auditoría: no hay intérprete de por medio, así que un `db_name`
    /// como `--all-databases` se pega como argumento suelto y el cliente lo lee como una
    /// opción —el alcance que `db_name` venía a decidir se salta sin que nada lo note—. Se
    /// rechaza con un error propio, y no se cita ni se escapa: una corrección silenciosa
    /// convertiría un error de la fila en un comando distinto al que el operador cree pedir.
    #[test]
    fn un_valor_que_parece_bandera_se_rechaza() {
        let caso = |objetivo: Objetivo, campo: &'static str, valor: &str| {
            let error =
                ruta(&objetivo).expect_err("un valor que parece una bandera no da un comando");
            let mensaje = error.to_string();

            assert!(
                matches!(
                    &error,
                    ErrorDeExtraccion::ValorQuePareceBandera { campo: cual, .. } if *cual == campo
                ),
                "el fallo tiene que nombrar el campo `{campo}`: {error:?}"
            );
            assert!(
                mensaje.contains(campo),
                "el motivo no nombra el campo `{campo}`: {mensaje}"
            );
            // Y no publica el valor: lo que hace falta para encontrar la fila mal editada es el
            // campo, no un trozo de línea de comandos en el registro y en la base.
            assert!(
                !mensaje.contains(valor),
                "el motivo publica el valor que se rechazó: {mensaje}"
            );
        };

        caso(
            Objetivo {
                db_name: Some("--all-databases".to_owned()),
                ..objetivo_mariadb(None)
            },
            "db_name",
            "--all-databases",
        );
        caso(
            Objetivo {
                db_name: Some("-d".to_owned()),
                ..objetivo_postgresql(None)
            },
            "db_name",
            "-d",
        );
        caso(
            Objetivo {
                db_user: Some("--user".to_owned()),
                ..objetivo_mariadb(None)
            },
            "db_user",
            "--user",
        );
        caso(
            Objetivo {
                db_user: Some("-uperador".to_owned()),
                ..objetivo_postgresql(None)
            },
            "db_user",
            "-uperador",
        );
    }

    /// El `container_name` **no** pasa por la guarda de las banderas: no es un argumento.
    ///
    /// La guarda es de los campos que acaban en la línea de comandos, y el contenedor no acaba
    /// ahí: es un parámetro de la API de Docker. Rechazarlo sería inventar una restricción que
    /// el motor no tiene, y el diagnóstico de un contenedor mal escrito ya está en el motor.
    #[test]
    fn el_nombre_del_contenedor_no_pasa_por_la_guarda_de_las_banderas() {
        let objetivo = Objetivo {
            container_name: Some("-contenedor".to_owned()),
            ..objetivo_mariadb(None)
        };

        let (contenedor, _) = por_contenedor(&objetivo);

        assert_eq!(contenedor, "-contenedor");
    }

    /// Un target de `directory` sale por el sistema de ficheros y **nunca** por un contenedor.
    ///
    /// La ruta es la de `host_path`, que es la que el proceso ve —el `compose` del §8 es
    /// quien monta el directorio del host—, y no hay ningún `exec` en el camino. Cubre el
    /// requisito «un target de directorio no pasa por Docker».
    #[test]
    fn un_target_de_directorio_se_lee_del_sistema_de_ficheros() {
        let ruta_esperada = PathBuf::from("/backup_sources/var_www");

        match ruta(&objetivo_directorio(Some("/backup_sources/var_www")))
            .expect("un directorio con su ruta da una ruta")
        {
            Ruta::PorFichero { ruta } => assert_eq!(ruta, ruta_esperada),
            Ruta::PorContenedor { contenedor, orden } => panic!(
                "un directorio no pasa por Docker, y salió por el contenedor `{contenedor}` con \
                 la orden {orden:?}"
            ),
        }
    }

    /// Un directorio **sin** ruta no se puede copiar, y el motivo nombra el campo.
    #[test]
    fn un_target_de_directorio_sin_ruta_no_se_puede_copiar() {
        let error =
            ruta(&objetivo_directorio(None)).expect_err("sin ruta no hay directorio que copiar");

        assert!(
            matches!(
                &error,
                ErrorDeExtraccion::CampoQueFalta {
                    campo: "host_path",
                    ..
                }
            ),
            "el fallo no nombra la ruta que falta: {error:?}"
        );
    }

    /// **El motivo del contenedor va en el error**, y el `404` que llega con motivo no se lee
    /// como «el contenedor no está».
    ///
    /// Es la decisión de la corrección, fijada sin motor: un fallo del transporte con algo
    /// escrito por el canal de error lleva ese algo —depurado, que es lo que entra aquí—, y un
    /// `404` **sin** nada escrito sigue siendo el contenedor ausente del `create_exec`, que es
    /// el único camino en el que el búfer puede estar vacío por construcción.
    #[test]
    fn el_motivo_del_contenedor_viaja_en_el_error_del_transporte() {
        let no_esta = || crate::docker::Error::MotorContesta {
            contenedor: "contenedor-de-la-base".to_owned(),
            status_code: 404,
            message: "no such container".to_owned(),
        };

        // Sin nada que el contenedor haya dicho: el desenlace de siempre.
        assert!(
            matches!(
                ErrorDeExtraccion::del_transporte(
                    "contenedor-de-la-base",
                    no_esta(),
                    String::new()
                ),
                ErrorDeExtraccion::ContenedorQueNoEsta { .. }
            ),
            "un 404 sin nada escrito sigue siendo el contenedor ausente"
        );

        // Con algo dicho: el motivo no se tira, y el 404 sigue dicho dentro de la causa.
        let error = ErrorDeExtraccion::del_transporte(
            "contenedor-de-la-base",
            no_esta(),
            format!("; el contenedor dijo: no puedo con {REDACTED}"),
        );
        let mensaje = error.to_string();

        assert!(
            matches!(&error, ErrorDeExtraccion::Transporte { motivo, .. } if motivo.contains(REDACTED)),
            "el motivo del contenedor se tiró por el camino: {error:?}"
        );
        assert!(
            mensaje.contains("el contenedor dijo"),
            "el motivo no llega al mensaje, que es lo que se guarda: {mensaje}"
        );
        assert!(
            mensaje.contains("404"),
            "el 404 se perdió al no reconocerse como contenedor ausente: {mensaje}"
        );
    }

    /// Un motivo **vacío** no deja un «el contenedor dijo:» colgado en el mensaje.
    ///
    /// Un motivo que no existe se leería como que el contenedor sí dijo algo, y mandaría a
    /// buscar una frase que no está en ninguna parte.
    #[test]
    fn un_motivo_vacio_no_deja_la_frase_del_contenedor_colgando() {
        let causa = crate::docker::Error::ExtraccionEstancada {
            contenedor: "contenedor-de-la-base".to_owned(),
            inactividad: INACTIVIDAD_MAXIMA,
        };

        let error =
            ErrorDeExtraccion::del_transporte("contenedor-de-la-base", causa, String::new());
        let mensaje = error.to_string();

        assert!(
            !mensaje.contains("el contenedor dijo"),
            "el mensaje dice que el contenedor habló sin que lo hiciera: {mensaje}"
        );
        assert!(
            !mensaje.ends_with(' '),
            "el mensaje deja un hueco donde iría el motivo: {mensaje:?}"
        );
    }

    /// El pegamento con la fila del §3 copia **cada campo en su sitio**.
    ///
    /// Se comprueba campo a campo y no con una igualdad de la estructura entera porque lo que
    /// hay que cazar es el cruce: `db_user` y `db_password` son los dos de tipo parecido, y
    /// una copia con los campos intercambiados daría una extracción sin credencial con un
    /// síntoma que no diría nada de aquí.
    #[test]
    fn el_objetivo_sale_de_la_fila_del_s3() {
        let fila = Target {
            id: uuid::Uuid::new_v4(),
            name: "target-de-prueba".to_owned(),
            target_type: TipoDeTarget::Postgresql,
            container_name: Some("contenedor-de-la-base".to_owned()),
            db_user: Some("operador".to_owned()),
            db_password: Some(secreto(CONTRASENA)),
            db_name: Some("almacen".to_owned()),
            host_path: Some("/backup_sources/almacen".to_owned()),
            created_at: time::OffsetDateTime::now_utc(),
        };

        let objetivo = Objetivo::from(&fila);

        assert_eq!(objetivo.tipo, TipoDeTarget::Postgresql);
        assert_eq!(
            objetivo.container_name.as_deref(),
            Some("contenedor-de-la-base")
        );
        assert_eq!(objetivo.db_user.as_deref(), Some("operador"));
        assert_eq!(
            objetivo.db_password.as_ref().map(SecretString::expose),
            Some(CONTRASENA)
        );
        assert_eq!(objetivo.db_name.as_deref(), Some("almacen"));
        assert_eq!(
            objetivo.host_path.as_deref(),
            Some("/backup_sources/almacen")
        );
    }

    /// El motivo se recorta **por carácter**, y lo dice.
    ///
    /// El motivo lleva caracteres de más de un byte —los acentos de un mensaje de cliente de
    /// base— justo en el borde del límite: un corte por byte partiría el carácter y lo que
    /// quedaría no sería un texto más corto sino un texto roto. El `…` final es lo que impide
    /// que un motivo cortado se lea como el motivo entero.
    #[test]
    fn el_motivo_se_recorta_por_caracter_y_lo_dice() {
        // De dos bytes (`á`), de tres (`€`) y de cuatro (`🦀`): el borde del límite tiene que
        // caer dentro de uno de ellos para que un corte por byte se note.
        let mut motivo = "á".repeat(LIMITE_DEL_MOTIVO - 1);
        motivo.push('€');
        motivo.push_str(&"🦀".repeat(50));
        motivo.push_str(&"á".repeat(LIMITE_DEL_MOTIVO));

        let recortado = recortar(&motivo);

        assert_eq!(
            recortado.chars().count(),
            LIMITE_DEL_MOTIVO,
            "el motivo recortado no mide el límite: {} caracteres",
            recortado.chars().count()
        );
        assert!(
            recortado.ends_with('…'),
            "un motivo cortado tiene que decirlo: …{}",
            &recortado[recortado.len().saturating_sub(20)..]
        );
        assert!(
            recortado
                .chars()
                .all(|caracter| caracter != char::REPLACEMENT_CHARACTER),
            "el recorte partió un carácter por la mitad"
        );
    }

    /// Un motivo que cabe **no se toca**: ni se recorta ni se le añade nada.
    #[test]
    fn un_motivo_corto_no_se_toca() {
        let motivo = "mariadb-dump: Got error: 1045: Access denied for user 'operador'";

        assert_eq!(recortar(motivo), motivo);
    }

    /// **El límite del motivo no tiene un `off-by-one`**: el que lo mide entero cabe, y el que
    /// mide uno más se corta.
    ///
    /// Es la frontera exacta, y por eso se fijan los dos lados: el `…` se lleva un carácter del
    /// límite, así que el corte conserva `LIMITE_DEL_MOTIVO - 1` y el resultado mide el límite
    /// —ni uno más, que no cabría en el campo, ni uno menos, que tiraría texto que cabe—. Sin
    /// las dos aserciones, un `take` de más o de menos no se notaría: la prueba del borde no es
    /// la del recorte.
    #[test]
    fn el_limite_del_motivo_no_tiene_un_off_by_one() {
        let justo = "a".repeat(LIMITE_DEL_MOTIVO);

        assert_eq!(
            recortar(&justo),
            justo,
            "un motivo que mide exactamente el límite cabe entero: no se recorta ni se le añade \
             nada"
        );

        let recortado = recortar(&"a".repeat(LIMITE_DEL_MOTIVO + 1));

        assert_eq!(
            recortado,
            format!("{}…", "a".repeat(LIMITE_DEL_MOTIVO - 1)),
            "el motivo que mide uno más no se corta donde debe: conserva el límite menos el \
             sitio del `…`"
        );
        assert_eq!(
            recortado.chars().count(),
            LIMITE_DEL_MOTIVO,
            "el motivo recortado tiene que medir exactamente el límite"
        );
    }

    /// **La punta del valor más largo no sobrevive** cuando dos valores del entorno se solapan.
    ///
    /// Es la corrección de la auditoría, y el fallo era de orden: sustituyendo en el orden del
    /// entorno, `Zq7-clave` parte `Zq7-clave-de-base` y su cola queda en el motivo
    /// —`<redactado>-de-base`—, que es publicar la mitad larga de una credencial creyendo
    /// haberla quitado. Del más largo al más corto, el largo se va entero.
    #[test]
    fn dos_valores_solapados_no_dejan_la_punta_del_mas_largo() {
        let orden = Orden {
            cmd: argv(&["mariadb-dump"]),
            // El corto va **primero**, que es el orden del entorno que destapaba el fallo.
            env: argv(&["OTRA=Zq7-clave", "MYSQL_PWD=Zq7-clave-de-base"]),
        };

        let motivo =
            motivo_para_guardar("no puedo con Zq7-clave-de-base".as_bytes().to_vec(), &orden);

        assert_eq!(
            motivo,
            format!("no puedo con {REDACTED}"),
            "el motivo no quedó redactado entero: {motivo}"
        );
        assert!(
            !motivo.contains("de-base"),
            "el motivo guarda la cola del valor más largo: {motivo}"
        );
    }

    /// El motivo se acumula **acotado**, y el tope no parte nada que el recorte conserve.
    ///
    /// Sin tope, un contenedor que escriba sin parar por su canal de error deja a este vector
    /// creciendo en el proceso del motor hasta que el proceso acabe: el resto del diseño está
    /// acotado —el canal son [`CAPACIDAD_DEL_CANAL`] trozos de 8 KiB— y esto no lo estaba. Lo
    /// que se tira en el tope es lo que no cabría en el motivo, y el margen cubre el caso en
    /// que el corte caiga **en mitad** de un carácter, que es lo que la última aserción fija.
    #[test]
    fn el_motivo_se_acumula_acotado() {
        let trozo: Vec<u8> = "x".repeat(1024).into_bytes();
        let mut crudo = Vec::new();

        for _ in 0..64 {
            acumular_el_motivo(&mut crudo, &trozo);
        }

        assert!(
            crudo.len() <= TOPE_DEL_CRUDO,
            "el búfer del motivo pasó del tope: {} bytes",
            crudo.len()
        );
        assert_eq!(
            crudo.len(),
            TOPE_DEL_CRUDO,
            "el búfer no llegó al tope, así que 64 trozos de 1 KiB no bastan para llenarlo"
        );
        assert!(
            crudo.iter().all(|byte| *byte == b'x'),
            "el búfer no guarda el principio de lo que escribió el contenedor"
        );

        // Y una vez lleno no vuelve a crecer: lo que se tira es lo que no cabría en el motivo.
        acumular_el_motivo(&mut crudo, b"cola que no cabe en el motivo");
        assert_eq!(
            crudo.len(),
            TOPE_DEL_CRUDO,
            "el búfer creció después de llenarse"
        );

        // El caso del carácter partido, que es el que justifica el margen: con un carácter de
        // tres bytes, el corte del tope cae **dentro** de uno. El búfer lo lleva —son bytes—,
        // pero el recorte, que va por carácter, no llega a verlo: lo que se conserva son mil
        // caracteres buenos.
        let mut crudo = Vec::new();
        acumular_el_motivo(&mut crudo, &"€".repeat(TOPE_DEL_CRUDO).into_bytes());

        let texto = String::from_utf8_lossy(&crudo);
        assert!(
            texto.contains(char::REPLACEMENT_CHARACTER),
            "el corte del tope no partió ningún carácter, así que esta prueba no está midiendo \
             lo que dice"
        );

        let motivo = recortar(&texto);
        assert_eq!(
            motivo.chars().count(),
            LIMITE_DEL_MOTIVO,
            "el motivo recortado no mide el límite"
        );
        assert!(
            !motivo.contains(char::REPLACEMENT_CHARACTER),
            "el tope partió un carácter dentro de los que el recorte conserva: el margen de \
             `TOPE_DEL_CRUDO` no está haciendo su trabajo"
        );
    }

    /// El motivo **no** lleva la credencial: la marca sustituye a todas sus apariciones.
    ///
    /// Es el caso que el contrato exige cubrir —el contenedor que repite la contraseña en su
    /// mensaje de error—, y el campo donde acabaría se muestra en el panel y se guarda en la
    /// base: las dos son formas de publicar un secreto.
    #[test]
    fn el_motivo_no_lleva_la_credencial() {
        let orden = Orden {
            cmd: argv(&["mariadb-dump", "-uoperador", "--all-databases"]),
            env: vec![format!("MYSQL_PWD={CONTRASENA}")],
        };
        let crudo = format!(
            "mariadb-dump: Got error: 1045: Access denied for user 'operador'@'localhost' \
             (using password: YES) con la clave {CONTRASENA}, y otra vez {CONTRASENA}"
        );

        let motivo = motivo_para_guardar(crudo.into_bytes(), &orden);

        assert!(
            !motivo.contains(CONTRASENA),
            "el motivo publica la contraseña: {motivo}"
        );
        assert!(
            motivo.contains(REDACTED),
            "el motivo no dice que haya algo redactado: {motivo}"
        );
        assert_eq!(
            motivo.matches(REDACTED).count(),
            2,
            "no se sustituyeron todas las apariciones: {motivo}"
        );
    }

    /// **El recorte no deja los primeros caracteres de la credencial.**
    ///
    /// Es la prueba del **orden** de las dos operaciones, y por eso la contraseña está justo
    /// en el borde del límite: recortar antes de quitar el valor cortaría la credencial por
    /// dentro, la cadena buscada ya no aparecería entera y se guardarían sus primeros
    /// caracteres —publicar media credencial creyendo haberla quitado—. Con el orden bueno,
    /// lo único que puede quedar a medias es la marca que la sustituye.
    #[test]
    fn el_recorte_no_deja_los_primeros_caracteres_de_la_credencial() {
        let orden = Orden {
            cmd: argv(&["mariadb-dump"]),
            env: vec![format!("MYSQL_PWD={CONTRASENA}")],
        };

        // El relleno lleva la contraseña a caballo del límite: sus primeros caracteres caen
        // dentro y el resto fuera.
        let relleno = "x".repeat(LIMITE_DEL_MOTIVO - CONTRASENA.len() / 2);
        let crudo = format!("{relleno}{CONTRASENA}{}", "y".repeat(100));

        let motivo = motivo_para_guardar(crudo.into_bytes(), &orden);

        for longitud in 1..=CONTRASENA.len() {
            let prefijo = &CONTRASENA[..longitud];
            assert!(
                !motivo.contains(prefijo),
                "el motivo guarda los primeros {longitud} caracteres de la contraseña: {motivo}"
            );
        }
    }

    /// Un valor de entorno **vacío** no se convierte en una máscara ilegible.
    ///
    /// Sustituir la cadena vacía insertaría la marca entre todos los caracteres del motivo, y
    /// el operador se encontraría un error que no se puede leer. Como una contraseña vacía
    /// tampoco llega al entorno, la guarda va con la otra mitad.
    #[test]
    fn un_valor_de_entorno_vacio_no_destruye_el_motivo() {
        let orden = Orden {
            cmd: argv(&["mariadb-dump"]),
            env: argv(&["MYSQL_PWD=", "SIN_IGUAL"]),
        };

        let motivo = motivo_para_guardar(b"el motivo de siempre".to_vec(), &orden);

        assert_eq!(
            motivo, "el motivo de siempre",
            "un valor vacío —o una variable sin valor— destrozó el motivo"
        );
    }

    /// El motivo de un `stderr` que no es UTF-8 se conserva como texto **válido**.
    ///
    /// El `stderr` de un contenedor no tiene por qué ser UTF-8 —una base puede escribirlo en
    /// la codificación de su idioma—, y perder el motivo entero por un byte raro dejaría al
    /// operador sin el único texto que dice qué pasó.
    #[test]
    fn un_motivo_que_no_es_utf8_llega_igual_como_texto() {
        let orden = Orden {
            cmd: argv(&["mariadb-dump"]),
            env: Vec::new(),
        };

        let motivo = motivo_para_guardar(b"fallo: \xF1and\xFA".to_vec(), &orden);

        assert!(
            motivo.starts_with("fallo:"),
            "el motivo se perdió por un byte que no era UTF-8: {motivo}"
        );
    }

    // ── Con motor de contenedores: `cargo test -- --ignored` ─────────────────

    /// Una orden de shell: el contenedor de prueba trae `sh` de busybox.
    fn orden_de_shell(guion: &str) -> Orden {
        Orden {
            cmd: argv(&["sh", "-c", guion]),
            env: Vec::new(),
        }
    }

    /// Un túnel de prueba, con la capacidad y el plazo **de producción**.
    ///
    /// Los dos números son los que se despliegan, no unos de prueba: lo que estas pruebas
    /// fijan es que la acotación no estorba a una extracción que avanza, y para eso el valor
    /// que interesa es el de verdad.
    fn tunel_de_prueba(nombre: &str) -> TunelDelVolcado {
        tunel_del_volcado(nombre, CAPACIDAD_DEL_CANAL, INACTIVIDAD_MAXIMA)
            .expect("el nombre del volcado de prueba es válido")
    }

    /// Conduce una extracción **como lo hace el motor**: en un hilo propio con su runtime.
    ///
    /// No es una comodidad, es la forma medida del transporte: el `exec` y su drenaje tienen
    /// que compartir el runtime del hilo que los lanza, porque si ese runtime muere mientras
    /// el otro hilo drena, el drenaje falla con «A Tokio 1.x context was found, but it is
    /// being shutdown». Por eso la prueba **no** usa el runtime de `#[tokio::test]` para
    /// conducir el `exec`: usa el mismo camino que usará el motor.
    fn volcar_en_su_hilo(
        contenedor: &str,
        orden: Orden,
        productor: ProductorDelVolcado,
    ) -> std::thread::JoinHandle<Result<Resultado, ErrorDeExtraccion>> {
        volcar_en_su_hilo_con_plazo(contenedor, orden, productor, INACTIVIDAD_MAXIMA)
    }

    /// La misma conducción, con el plazo que se le pida.
    ///
    /// Existe por la prueba del estancamiento, que es la única que necesita un plazo **corto**:
    /// todas las demás fijan que el plazo de producción no estorba a una extracción que avanza,
    /// y ahí el número que interesa es el de verdad. El ayudante es el mismo y el camino es el
    /// mismo: lo único que cambia es el número.
    fn volcar_en_su_hilo_con_plazo(
        contenedor: &str,
        orden: Orden,
        productor: ProductorDelVolcado,
        inactividad_maxima: Duration,
    ) -> std::thread::JoinHandle<Result<Resultado, ErrorDeExtraccion>> {
        let socket = socket_del_motor().expect("las pruebas del motor necesitan un socket");
        let contenedor = contenedor.to_owned();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("el runtime del hilo debe poder construirse");

            runtime.block_on(async move {
                let cliente =
                    Cliente::conectar(&socket, TIMEOUT_DEL_MOTOR).expect("el socket existe");

                volcar(&cliente, &contenedor, &orden, productor, inactividad_maxima).await
            })
        })
    }

    /// **La ruta entera de punta a punta**: el `exec`, sus bytes, su motivo y su señal.
    ///
    /// Un contenedor de verdad escribe el volcado por `stdout` y un motivo por `stderr` y sale
    /// con cero: lo que llega al lector tiene que ser **exactamente** el volcado —ni el motivo
    /// ni un byte de más—, la cuenta de bytes tiene que cuadrar con lo escrito, el motivo
    /// tiene que ser el que escribió el contenedor, y la entrega tiene que quedar completa.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn la_extraccion_entrega_el_volcado_entero_y_su_motivo() {
        const VOLCADO: &[u8] = b"volcado";
        const MOTIVO: &str = "motivo";

        let contenedor =
            ContenedorDePrueba::nuevo("la_extraccion_entrega_el_volcado_entero_y_su_motivo").await;
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
        } = tunel_de_prueba("volcado-de-tienda.sql");

        let orden = orden_de_shell("printf volcado; printf motivo >&2; exit 0");
        let hilo = volcar_en_su_hilo(contenedor.nombre(), orden, productor);

        let mut lector = lector_del_volcado(&origen);
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("el volcado se declaró terminado, así que la lectura no puede fallar");

        let resultado = hilo
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect("una extracción que sale con cero no es un fallo de la extracción");

        assert_eq!(resultado.codigo, 0, "el guion sale con cero");
        assert_eq!(
            leido,
            VOLCADO,
            "al lector no le llegó exactamente el volcado, sino {:?}",
            String::from_utf8_lossy(&leido)
        );
        assert_eq!(
            resultado.bytes_leidos,
            VOLCADO.len() as u64,
            "la cuenta de bytes no cuadra con lo que el comando escribió"
        );
        assert_eq!(
            resultado.motivo_del_contenedor, MOTIVO,
            "el motivo del contenedor no se recogió tal cual"
        );
        assert!(
            entrega.completa(),
            "el volcado se entregó entero y el proceso salió con cero, y la señal sigue en \
             falso: el motor daría por fallida una copia buena"
        );

        contenedor.retirar().await;
    }

    /// **Un proceso que sale con código distinto de cero no declara el final.**
    ///
    /// Es la prueba que ata la condición de éxito al éxito del proceso, y la que sostiene todo
    /// el diseño: el `Ok` del archivado no dice nada del volcado, así que la señal tiene que
    /// significar las dos cosas a la vez. Aquí el proceso sale con `3` y escribe un motivo: el
    /// resultado los trae, y la entrega queda **en falso** aunque los bytes hayan llegado al
    /// canal —el lector, que es quien marca la señal, se encuentra el cierre sin el final
    /// declarado y devuelve un error en vez de un fin de fichero—.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn un_proceso_que_falla_no_declara_el_final() {
        let contenedor =
            ContenedorDePrueba::nuevo("un_proceso_que_falla_no_declara_el_final").await;
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
        } = tunel_de_prueba("volcado-que-falla.sql");

        let orden = orden_de_shell("printf algo; printf fallo-del-volcado >&2; exit 3");
        let hilo = volcar_en_su_hilo(contenedor.nombre(), orden, productor);

        let mut lector = lector_del_volcado(&origen);
        let mut leido = Vec::new();
        let lectura = lector.read_to_end(&mut leido);

        let resultado = hilo
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect("un proceso que sale con 3 llegó a su término: hay hechos que contar");

        assert_eq!(
            resultado.codigo, 3,
            "el código de salida no es el del proceso"
        );
        assert_eq!(
            resultado.motivo_del_contenedor, "fallo-del-volcado",
            "el motivo que escribió el contenedor se perdió"
        );
        assert_eq!(
            leido, b"algo",
            "los bytes que el proceso escribió antes de fallar no llegaron"
        );
        assert!(
            lectura.is_err(),
            "el lector devolvió un fin de fichero limpio sobre un volcado truncado: la \
             biblioteca lo leería como «el volcado se acabó» y daría la copia por buena"
        );
        assert!(
            !entrega.completa(),
            "el proceso falló y la señal dice que el volcado se entregó entero"
        );

        contenedor.retirar().await;
    }

    /// Un volcado grande llega **entero** y la cuenta de bytes cuadra con lo escrito.
    ///
    /// Es la prueba que cubre el caso del §4.A —un volcado de verdad no es pequeño— sin
    /// necesitar una base: cuatro mebibytes que el lector tiene que recibir byte a byte,
    /// mientras la cuenta crece en el otro extremo. Un contador que sumara los bytes que no
    /// salieron, o que se perdiera trozos, no daría este número.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn un_volcado_grande_llega_entero_y_contado() {
        /// Bytes que escribe el comando: cuatro mebibytes.
        const BYTES_ESCRITOS: usize = 4 * 1024 * 1024;

        let contenedor =
            ContenedorDePrueba::nuevo("un_volcado_grande_llega_entero_y_contado").await;
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
        } = tunel_de_prueba("volcado-grande.sql");

        let orden = orden_de_shell(&format!("yes a | head -c {BYTES_ESCRITOS}"));
        let hilo = volcar_en_su_hilo(contenedor.nombre(), orden, productor);

        let mut lector = lector_del_volcado(&origen);
        let mut leido = Vec::new();
        lector
            .read_to_end(&mut leido)
            .expect("el volcado se declaró terminado, así que la lectura no puede fallar");

        let resultado = hilo
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect("un `stdout` grande no es un fallo de la extracción");

        assert_eq!(resultado.codigo, 0, "el comando sale con cero");
        assert_eq!(
            resultado.bytes_leidos, BYTES_ESCRITOS as u64,
            "la cuenta de bytes leídos no cuadra con lo que el comando escribió"
        );
        assert_eq!(
            leido.len(),
            BYTES_ESCRITOS,
            "los bytes que llegaron al lector no son los que se escribieron"
        );
        assert!(
            entrega.completa(),
            "el volcado llegó entero y la señal de la entrega sigue en falso"
        );

        contenedor.retirar().await;
    }

    /// Un contenedor que **repite la contraseña** al fallar no la publica en el motivo.
    ///
    /// Es el caso que el contrato exige cubrir, extremo a extremo y con un contenedor de
    /// verdad: la credencial viaja por el entorno del `exec` y el proceso la escribe en su
    /// canal de error, que es el campo que se guarda en la base y se muestra en el panel.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn el_motivo_de_un_contenedor_que_repite_la_contrasena_no_la_publica() {
        let contenedor = ContenedorDePrueba::nuevo(
            "el_motivo_de_un_contenedor_que_repite_la_contrasena_no_la_publica",
        )
        .await;
        let TunelDelVolcado {
            origen,
            productor,
            entrega: _,
        } = tunel_de_prueba("volcado-sin-credencial.sql");

        let orden = Orden {
            env: vec![format!("MYSQL_PWD={CONTRASENA}")],
            ..orden_de_shell("printf 'no puedo con %s' \"$MYSQL_PWD\" >&2; exit 1")
        };
        let hilo = volcar_en_su_hilo(contenedor.nombre(), orden, productor);

        let mut lector = lector_del_volcado(&origen);
        let mut leido = Vec::new();
        let _ = lector.read_to_end(&mut leido);

        let resultado = hilo
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect("el proceso llegó a su término: hay hechos que contar");

        assert_eq!(resultado.codigo, 1, "el guion sale con uno");
        assert!(
            !resultado.motivo_del_contenedor.contains(CONTRASENA),
            "el motivo que se guardaría publica la contraseña de la base: {}",
            resultado.motivo_del_contenedor
        );
        assert!(
            resultado.motivo_del_contenedor.contains(REDACTED),
            "el motivo no dice que haya algo redactado: {}",
            resultado.motivo_del_contenedor
        );

        contenedor.retirar().await;
    }

    /// Un contenedor que no existe se reconoce **por su nombre**, no como un fallo de volcado.
    ///
    /// Cubre el escenario del contrato: el operador tiene que poder distinguir «la fila del §3
    /// nombra un contenedor que no está» de «la base rechazó las credenciales», porque los dos
    /// fallos se arreglan en sitios distintos.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn un_contenedor_que_no_esta_no_se_confunde_con_un_fallo_de_volcado() {
        const AUSENTE: &str = "kesto-prueba-contenedor-que-no-existe-extraccion";

        let TunelDelVolcado {
            origen: _origen,
            productor,
            entrega: _,
        } = tunel_de_prueba("volcado-sin-contenedor.sql");

        let error = volcar_en_su_hilo(AUSENTE, orden_de_shell("echo nunca-llega"), productor)
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect_err("un contenedor que no existe no puede ejecutar nada");

        assert!(
            matches!(
                &error,
                ErrorDeExtraccion::ContenedorQueNoEsta { contenedor, .. } if contenedor == AUSENTE
            ),
            "el contenedor ausente no se reconoció como tal: {error:?}"
        );
        assert!(
            error.to_string().contains(AUSENTE),
            "el motivo no nombra el contenedor: {error}"
        );
    }

    /// **Un fallo sin una gota en la salida no es una base vacía.**
    ///
    /// Es el caso «no hay cliente de volcado en la imagen», que es justo el que hay que
    /// distinguir de una base sin datos: los dos dejan el `stdout` en cero bytes, y lo único
    /// que los separa es el código de salida y el motivo. El contenedor de prueba es un
    /// `alpine`, que no trae `mariadb-dump`, así que el comando de verdad de un volcado global
    /// sale con un código distinto de cero, no escribe nada por su salida y sí deja un motivo
    /// por su canal de error. La señal de la entrega tiene que quedarse **en falso**: darla por
    /// buena sería dar por buena una copia vacía, que existe, ocupa sitio y no se puede
    /// restaurar.
    ///
    /// La prueba que ya había escribe bytes antes de fallar; ésta no escribe ninguno, que es la
    /// forma en que un fallo de extracción se parece a una base vacía y la razón de que el
    /// código de salida y el canal de error sean lo único que los distingue.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn un_proceso_que_falla_sin_escribir_en_la_salida_no_es_una_base_vacia() {
        let contenedor = ContenedorDePrueba::nuevo(
            "un_proceso_que_falla_sin_escribir_en_la_salida_no_es_una_base_vacia",
        )
        .await;
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
        } = tunel_de_prueba("volcado-sin-cliente.sql");

        // El comando de un volcado global de MariaDB, con la forma que le da `ruta`: en `alpine`
        // ese binario no está, que es exactamente lo que pasa cuando la imagen de la base no
        // trae el cliente de volcado.
        let orden = Orden {
            cmd: argv(&["mariadb-dump", "--single-transaction", "--all-databases"]),
            env: Vec::new(),
        };
        let hilo = volcar_en_su_hilo(contenedor.nombre(), orden, productor);

        let mut lector = lector_del_volcado(&origen);
        let mut leido = Vec::new();
        let lectura = lector.read_to_end(&mut leido);

        let resultado = hilo
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect("el proceso llegó a su término: hay hechos que contar");

        assert_ne!(
            resultado.codigo, 0,
            "el binario no está en la imagen, así que el proceso no puede salir con cero"
        );
        assert_eq!(
            resultado.bytes_leidos, 0,
            "un binario ausente no escribe nada por su salida: la cuenta de bytes sería el \
             volcado y aquí no hay ninguno"
        );
        assert!(
            leido.is_empty(),
            "al lector no puede llegarle nada: {} bytes",
            leido.len()
        );
        assert!(
            !resultado.motivo_del_contenedor.is_empty(),
            "el motivo del contenedor se perdió, y es la única mitad que distingue este fallo de \
             una base vacía"
        );
        assert!(
            lectura.is_err(),
            "el lector devolvió un fin de fichero limpio sobre un volcado que nunca se escribió: \
             la biblioteca lo leería como «el volcado se acabó» y daría la copia por buena"
        );
        assert!(
            !entrega.completa(),
            "una extracción que falló sin escribir nada no puede darse por entregada"
        );

        contenedor.retirar().await;
    }

    /// **Una orden que se calla acaba en estancamiento, y la entrega queda en falso.**
    ///
    /// Es el otro camino que el contrato exige distinguir de una extracción que terminó sin
    /// escribir nada: la orden deja de producir salida durante más que el plazo, `volcar` lo
    /// propaga con el plazo que se le dio, y el lector del puente se encuentra el cierre
    /// **sin** la señal del final, así que el volcado no queda entregado. Sin acotar la
    /// inactividad, esto no sería un fallo con nombre: sería una ejecución en `running` para
    /// siempre, con el worker de su repositorio ocupado y todo su trabajo posterior haciendo
    /// cola detrás.
    ///
    /// Y es además la prueba, extremo a extremo, de lo que el error **lleva**: el guion escribe
    /// la credencial por su canal de error antes de callarse, así que el motivo del contenedor
    /// viaja en el error —la mitad que cerró la auditoría, porque antes se tiraba con el
    /// `return`— y viaja **sin la credencial**, que es la mitad que no se puede romper.
    #[tokio::test]
    #[ignore = "requiere un motor de contenedores: se ejecuta con cargo test -- --ignored"]
    async fn una_orden_que_se_calla_acaba_en_estancamiento_y_sin_entrega() {
        let contenedor = ContenedorDePrueba::nuevo(
            "una_orden_que_se_calla_acaba_en_estancamiento_y_sin_entrega",
        )
        .await;
        let TunelDelVolcado {
            origen,
            productor,
            entrega,
        } = tunel_de_prueba("volcado-estancado.sql");

        let orden = Orden {
            env: vec![format!("MYSQL_PWD={CONTRASENA}")],
            ..orden_de_shell("printf 'no puedo con %s' \"$MYSQL_PWD\" >&2; sleep 600")
        };
        let hilo =
            volcar_en_su_hilo_con_plazo(contenedor.nombre(), orden, productor, PLAZO_ESTANCADO);

        let mut lector = lector_del_volcado(&origen);
        let mut leido = Vec::new();
        let lectura = lector.read_to_end(&mut leido);

        let error = hilo
            .join()
            .expect("el hilo que conduce el `exec` no puede entrar en pánico")
            .expect_err("una orden que se calla más que el plazo no puede darse por buena");

        assert!(
            matches!(
                &error,
                ErrorDeExtraccion::Transporte {
                    causa: crate::docker::Error::ExtraccionEstancada {
                        contenedor: cual,
                        inactividad,
                    },
                    ..
                } if cual == contenedor.nombre() && *inactividad == PLAZO_ESTANCADO
            ),
            "el vencimiento no llegó como estancamiento con el plazo que se le dio: {error:?}"
        );

        // Lo que el contenedor dijo antes de callarse viaja en el error, y sin la credencial.
        let mensaje = error.to_string();
        assert!(
            mensaje.contains("no puedo con"),
            "el error se dejó lo que el contenedor alcanzó a decir por su canal de error: \
             {mensaje}"
        );
        assert!(
            !mensaje.contains(CONTRASENA),
            "el error publica la credencial de la base: {mensaje}"
        );

        assert!(
            leido.is_empty(),
            "el guion no escribe nada por su salida: {} bytes",
            leido.len()
        );
        assert!(
            lectura.is_err(),
            "el lector devolvió un fin de fichero limpio sobre un volcado que no se declaró \
             terminado"
        );
        assert!(
            !entrega.completa(),
            "el volcado no se declaró terminado y la señal dice que se entregó entero"
        );

        contenedor.retirar().await;
    }
}
