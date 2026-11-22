//! El arnés de la batería de punta a punta del issue #86.
//!
//! Monta las cuatro piezas que la batería necesita —un motor de contenedores, un servidor
//! S3 de verdad, el **binario** de Kesto y habla HTTP con él— y las ofrece como el mundo
//! de una prueba. No hay ningún doble: el almacén es un servidor S3 real, las bases son las
//! imágenes oficiales y el que copia y restaura es el binario que se despliega, no una
//! función extraída que podría ejercitarse sin estar cableada.
//!
//! # Por qué el binario y no la biblioteca
//!
//! El crate es **solo binario** (`src/main.rs`, sin `[lib]`) y su arnés interno es
//! `#[cfg(test)]`, así que desde `tests/` no hay nada que importar. Es la misma decisión que
//! [`tests/arranque.rs`](../arranque/index.html) tomó antes: se lanza el proceso de verdad,
//! con su entorno, y se habla con él por HTTP.
//!
//! # Por qué bollard y no el CLI
//!
//! En desarrollo el motor es Podman y en el CI es Docker; el CLI se llama distinto en cada
//! sitio y arrastraría al arnés a elegir binario según la máquina. La API es la misma en los
//! dos —Podman la implementa compatible— y el proyecto ya habla por ella: el arnés usa el
//! mismo `bollard` que el motor, contra el socket que diga `DOCKER_HOST`.
//!
//! # Falla en voz alta
//!
//! Ningún escenario se salta: si falta `DOCKER_HOST`, si el socket no responde, si el
//! servidor S3 no llega a estar disponible o si una imagen no se puede conseguir, el
//! escenario **falla** diciendo qué falta. Las funciones puras de este módulo
//! ([`resolver_el_socket`], [`esperar_el_s3`], [`comparar_arboles`]) devuelven el motivo en un
//! `Result` para que haya pruebas que lo afirmen sin fingir que el entorno está bien.

use std::collections::HashMap;
use std::fs::File;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogOutput, RemoveContainerOptions,
    StartContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::image::CreateImageOptions;
// Los dos modelos de la publicación de puertos viven en `models` —el reexport de la
// biblioteca— y no en `container`: allí el nombre está **privado**, así que la ruta corta no
// compila. Es la misma trampa que documenta `src/docker/cliente.rs` con la inspección del
// `exec`.
use bollard::models::{ContainerSummary, HostConfig, PortBinding};
use futures_util::StreamExt;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

// ── Lo que la batería necesita del entorno ─────────────────────────────────

/// Variable con el socket del motor de contenedores.
pub const ENV_DOCKER_HOST: &str = "DOCKER_HOST";

/// Socket de Podman en esta máquina de desarrollo, el que usa la receta `just e2e`.
///
/// Se nombra para que los motivos de fallo puedan decir **cuál** se esperaba: «falta
/// `DOCKER_HOST`» sin decir qué poner no arregla nada.
pub const SOCKET_DE_PODMAN: &str = "unix:///run/user/1000/podman/podman.sock";

/// Clave maestra con la que arrancan los binarios de la batería.
///
/// Es obligatoria siempre —sin ella no hay cifrado en reposo— y aquí es un valor de relleno:
/// la base de datos de cada prueba vive en un directorio temporal que se borra al terminar.
pub const CLAVE_MAESTRA: &str = "clave-maestra-de-la-bateria-e2e-000000000000000000000000";

/// Credencial del servidor S3 de la batería. No es un secreto: es la de un servidor que se
/// levanta y se tira en cada prueba.
pub const ACCESO_S3: &str = "kesto-e2e-access";
/// Secreto del servidor S3 de la batería.
pub const SECRETO_S3: &str = "kesto-e2e-secret-000000000000000000000";
/// Frase de paso del repositorio Rustic. Tampoco es un secreto.
pub const FRASE_DE_PASO: &str = "frase-de-paso-de-la-bateria-e2e-000000000000000000";
/// Región con la que se declaran los repositorios. El servidor no la comprueba, pero el
/// almacén la firma.
pub const REGION: &str = "us-east-1";

/// Etiqueta con la que cada contenedor de la batería recuerda **qué proceso lo creó**.
///
/// Su valor es el identificador del proceso —el de la ejecución de `cargo test` que corre la
/// batería—, y existe para que el barrido de restos pueda distinguir una ejecución **muerta** de
/// una **viva**: el nombre del contenedor lleva la marca del escenario, que se sortea en cada
/// mundo, así que el nombre no dice nada de la ejecución. Ver [`Motor::recoger_los_restos`].
pub const ETIQUETA_DEL_CREADOR: &str = "kesto-e2e-pid";

/// Etiqueta con la que un contenedor de la batería se declara **fuente de copia**.
///
/// Es el **mismo** valor que `crate::docker::ETIQUETA_DE_FUENTE_DE_COPIA`: el motor solo crea
/// `exec` en contenedores que la llevan con el valor [`VALOR_DE_FUENTE_DE_COPIA`], así que los
/// contenedores de los que Kesto copia o en los que restaura tienen que declararla. Se repite
/// aquí porque la batería es un crate aparte y no puede importar el binario; al cambiar el valor
/// en producción hay que cambiarlo también aquí.
pub const ETIQUETA_DE_FUENTE_DE_COPIA: &str = "io.kesto.backup-source";

/// Valor de la etiqueta que declara un contenedor fuente de copia.
///
/// El **mismo** que `crate::docker::VALOR_DE_FUENTE_DE_COPIA`: la comparación del motor es exacta.
pub const VALOR_DE_FUENTE_DE_COPIA: &str = "true";

/// Imagen por defecto del servidor S3 de la batería.
///
/// Fijada por **versión** —nunca `latest`—, como el resto de las piezas, para que la batería no
/// cambie bajo los pies. Es el **mismo** valor que `S3_IMAGE` del job `e2e` de
/// `.github/workflows/ci.yml`: al actualizar la versión hay que cambiarla en los dos sitios y
/// comprobar que la etiqueta se descarga de verdad (`podman pull`).
pub const IMAGEN_S3: &str = "docker.io/chrislusf/seaweedfs:4.53";
/// Imagen por defecto de una base MariaDB de origen.
pub const IMAGEN_MARIADB: &str = "docker.io/library/mariadb:11";
/// Imagen por defecto de una base PostgreSQL de origen.
pub const IMAGEN_POSTGRES: &str = "docker.io/library/postgres:18-alpine";
/// Imagen por defecto de un contenedor de apoyo: ficheros SQLite y contenedores que no
/// cooperan.
pub const IMAGEN_ALPINE: &str = "docker.io/library/alpine:3.23";

/// La imagen de una pieza de la batería: la que diga el entorno, o la de por defecto.
///
/// El job `e2e` del CI declara `MARIA_IMAGE`, `POSTGRES_IMAGE`, `ALPINE_IMAGE` y `S3_IMAGE`
/// —las fija por versión y las pre-descarga—, y la batería las respeta: sin esto, el job
/// descargaría unas imágenes y la batería usaría otras, y el «fijadas por versión» del
/// requisito viviría en un fichero que nadie lee. En local no hay ninguna declarada y se
/// usan las de por defecto, que son las mismas versiones.
pub fn imagen(variable: &str, por_defecto: &str) -> String {
    std::env::var(variable)
        .ok()
        .filter(|valor| !valor.trim().is_empty())
        .unwrap_or_else(|| por_defecto.to_owned())
}

/// La imagen del servidor S3 de la batería, con la variable del CI por delante.
pub fn imagen_del_s3() -> String {
    imagen("S3_IMAGE", IMAGEN_S3)
}

/// La imagen de una base MariaDB de origen, con la variable del CI por delante.
pub fn imagen_de_mariadb() -> String {
    imagen("MARIA_IMAGE", IMAGEN_MARIADB)
}

/// La imagen de una base PostgreSQL de origen, con la variable del CI por delante.
pub fn imagen_de_postgres() -> String {
    imagen("POSTGRES_IMAGE", IMAGEN_POSTGRES)
}

/// La imagen de un contenedor de apoyo, con la variable del CI por delante.
pub fn imagen_de_alpine() -> String {
    imagen("ALPINE_IMAGE", IMAGEN_ALPINE)
}

/// Contraseña de `root` en las bases de la batería.
pub const CLAVE_DE_LA_BASE: &str = "clave-de-la-base";
/// Nombre de la base sembrada en los orígenes de base de datos.
pub const BASE: &str = "pruebas";

/// Plazo para que una imagen esté disponible y un contenedor arranque.
pub const PLAZO_DE_ARRANQUE: Duration = Duration::from_secs(180);
/// Plazo de una orden suelta dentro de un contenedor.
pub const PLAZO_DE_LA_ORDEN: Duration = Duration::from_secs(180);
/// Plazo para que el servidor S3 de la batería conteste.
pub const PLAZO_DEL_S3: Duration = Duration::from_secs(120);
/// Plazo de una copia (extracción, subida y retención).
pub const PLAZO_DE_LA_COPIA: Duration = Duration::from_secs(300);
/// Plazo de una restauración.
///
/// Es **más largo** que [`PLAZO_DE_INACTIVIDAD`] a propósito: el escenario del contenedor que
/// no drena su `stdin` no termina hasta que vence el plazo de inactividad del motor, y el
/// que espera tiene que aguantar un poco más que el que decide.
pub const PLAZO_DE_LA_RESTAURACION: Duration = Duration::from_secs(420);
/// El plazo de inactividad del motor, escrito aquí solo para que el escenario del cuelgue
/// pueda nombrarlo en su motivo.
pub const PLAZO_DE_INACTIVIDAD: Duration = Duration::from_secs(300);

// ── Las sondas de arranque de las bases ────────────────────────────────────

/// La sonda que da por arrancada una base: la orden y el entorno que la acompañan.
///
/// Es una función pura con forma de par de listas, para que las pruebas de las sondas puedan
/// afirmarla sin levantar nada. Ojo: el «la sonda nombra la base» que introdujo el #170 **no**
/// está en ningún spec —vive en las pruebas de `escenarios.rs` y en el `CHANGELOG`—, así que
/// estas pruebas no citan ningún escenario de spec que las respalde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sonda {
    /// El comando y sus argumentos.
    pub cmd: Vec<String>,
    /// Las variables de entorno que el proceso necesita para contestar.
    pub entorno: Vec<String>,
}

impl Sonda {
    /// El comando en la forma que pide [`Motor::ejecutar`].
    pub fn argv(&self) -> Vec<&str> {
        self.cmd.iter().map(String::as_str).collect()
    }

    /// El entorno en la forma que pide [`Motor::ejecutar`].
    pub fn env(&self) -> Vec<&str> {
        self.entorno.iter().map(String::as_str).collect()
    }
}

/// La sonda de un PostgreSQL: una consulta de verdad contra [`BASE`].
///
/// `pg_isready` **no vale**: contesta que el servidor está listo durante la fase temporal del
/// arranque de la imagen oficial —un servidor escuchando mientras `POSTGRES_DB` todavía no
/// existe—, así que quien esperase con él sembraría antes de tiempo y se llevaría un
/// `database "pruebas" does not exist`. La consulta contra [`BASE`] falla hasta que la base
/// está y acepta consultas.
pub fn sonda_de_postgres() -> Sonda {
    Sonda {
        cmd: vec![
            "psql".to_owned(),
            "-U".to_owned(),
            "postgres".to_owned(),
            "-d".to_owned(),
            BASE.to_owned(),
            "-c".to_owned(),
            "SELECT 1".to_owned(),
        ],
        entorno: vec![format!("PGPASSWORD={CLAVE_DE_LA_BASE}")],
    }
}

/// La sonda de un MariaDB: una consulta de verdad contra [`BASE`].
///
/// El cliente que no nombra base contesta que sí contra el servidor temporal del arranque,
/// antes de que `MARIADB_DATABASE` exista: la base va en la orden, igual que en la siembra.
pub fn sonda_de_mariadb() -> Sonda {
    let clave = format!("-p{CLAVE_DE_LA_BASE}");

    Sonda {
        cmd: vec![
            "mariadb".to_owned(),
            "-uroot".to_owned(),
            clave,
            BASE.to_owned(),
            "-e".to_owned(),
            "SELECT 1".to_owned(),
        ],
        entorno: Vec::new(),
    }
}

// ── El socket del motor ────────────────────────────────────────────────────

/// Resuelve el socket del motor a partir de lo que traiga el entorno.
///
/// Es una función pura —recibe el valor, no lee el entorno— para que el escenario «sin socket
/// de motor no se finge éxito» pueda afirmarla con un valor inventado.
///
/// # Errors
///
/// Devuelve el motivo cuando la variable falta, viene vacía o no nombra un socket local. El
/// motivo **nombra la variable y el socket esperado**, que es lo que el requisito pide.
pub fn resolver_el_socket(valor: Option<&str>) -> Result<String, String> {
    let Some(valor) = valor.map(str::trim).filter(|valor| !valor.is_empty()) else {
        return Err(format!(
            "la batería e2e necesita un motor de contenedores y no hay `{ENV_DOCKER_HOST}` \
             en el entorno: exporta `{ENV_DOCKER_HOST}={SOCKET_DE_PODMAN}` (o el socket del \
             motor que uses) y vuelve a lanzarla"
        ));
    };

    if !valor.starts_with("unix://") {
        return Err(format!(
            "`{ENV_DOCKER_HOST}={valor}` no es un socket local: la batería se conecta al \
             motor por su socket unix y espera algo como `{SOCKET_DE_PODMAN}`"
        ));
    }

    Ok(valor.to_owned())
}

/// Un puerto libre de `127.0.0.1`.
///
/// Se reserva y se suelta: lo que se le pide al sistema es un número que no esté ocupado
/// ahora mismo, y quien lo va a ocupar es un contenedor que se publica ahí mismo. Evita fijar
/// puertos —con dos pruebas seguidas, o con el laboratorio de al lado, un puerto fijo choca—.
pub fn puerto_libre() -> u16 {
    let reserva = TcpListener::bind("127.0.0.1:0").expect("la reserva de un puerto no falla");

    reserva
        .local_addr()
        .expect("el puerto reservado se conoce")
        .port()
}

// ── El motor de contenedores ───────────────────────────────────────────────

/// Lo que un contenedor de la batería necesita para existir.
pub struct Receta<'a> {
    /// Nombre del contenedor: único por batería, para que dos ejecuciones no se pisen.
    pub nombre: &'a str,
    /// Imagen de la que se crea.
    pub imagen: &'a str,
    /// Orden con la que arranca.
    pub cmd: &'a [&'a str],
    /// Entorno del contenedor.
    pub env: &'a [&'a str],
    /// Puerto a publicar en `127.0.0.1`: `(el del contenedor, el del host)`.
    pub puerto: Option<(u16, u16)>,
    /// Fichero del host montado dentro: `(ruta del host, ruta del contenedor)`.
    pub montaje: Option<(&'a str, &'a str)>,
    /// Si el contenedor se declara **fuente de copia** para el motor.
    ///
    /// El motor solo crea `exec` en contenedores que llevan la etiqueta
    /// `io.kesto.backup-source=true`, así que los contenedores de los que Kesto copia o en los
    /// que restaura tienen que declararlo. Los que solo existen para la prueba —el servidor S3,
    /// por ejemplo— no: nacen sin la etiqueta y el motor no les crearía ningún `exec`.
    pub fuente_de_copia: bool,
}

/// Lo que un proceso dejó al terminar dentro de un contenedor.
pub struct Salida {
    /// Código de salida.
    pub codigo: i64,
    /// Lo que escribió por su salida estándar.
    pub salida: String,
    /// Lo que escribió por su error estándar: es el motivo cuando algo va mal.
    pub error: String,
}

impl Salida {
    /// Exige que el proceso terminara bien, y si no lo hizo, cuenta por qué.
    #[track_caller]
    pub fn exigir_cero(&self, que: &str) {
        assert!(
            self.codigo == 0,
            "{que} tenía que terminar con código 0 y terminó con {}.\n--- su salida\n{}\n--- \
             su error\n{}",
            self.codigo,
            self.salida,
            self.error
        );
    }
}

/// El motor de contenedores de la batería: el cliente, el socket y lo que ha creado.
pub struct Motor {
    docker: Docker,
    socket: String,
    creados: Vec<String>,
}

impl Motor {
    /// Se conecta al motor y **falla en voz alta** si no está.
    ///
    /// Deja el terreno limpio: los contenedores que dejara una ejecución anterior **ya muerta**
    /// —una prueba que entró en pánico antes de recoger— se borran antes de empezar, así que dos
    /// ejecuciones seguidas de la batería no comparten nada. Los de una ejecución que esté viva a la
    /// vez no se tocan: ver [`Motor::recoger_los_restos`].
    ///
    /// # Errors
    ///
    /// Si falta `DOCKER_HOST` o el socket no contesta, con el motivo por delante.
    pub async fn conectar() -> Result<Self, String> {
        let valor = std::env::var(ENV_DOCKER_HOST).ok();
        let socket = resolver_el_socket(valor.as_deref())?;

        Self::conectar_en(&socket).await
    }

    /// Se conecta a **este** socket, sin mirar el entorno.
    ///
    /// Está separada de [`Motor::conectar`] para que el escenario «un socket que no responde»
    /// pueda afirmarlo con una ruta inventada sin tocar la variable del proceso.
    ///
    /// # Errors
    ///
    /// Si el cliente no se puede preparar o el motor no contesta, con el socket nombrado.
    pub async fn conectar_en(socket: &str) -> Result<Self, String> {
        let docker = Docker::connect_with_socket(socket, 600, bollard::API_DEFAULT_VERSION)
            .map_err(|causa| {
                format!("no se pudo preparar el cliente del motor en `{socket}`: {causa}")
            })?;

        docker.ping().await.map_err(|causa| {
            format!("el motor de contenedores de `{ENV_DOCKER_HOST}={socket}` no contesta: {causa}")
        })?;

        let motor = Self {
            docker,
            socket: socket.to_owned(),
            creados: Vec::new(),
        };
        motor.recoger_los_restos().await;

        Ok(motor)
    }

    /// El socket con el que habla.
    pub fn socket(&self) -> &str {
        &self.socket
    }

    /// Borra los contenedores que dejara una ejecución anterior de la batería **ya muerta**.
    ///
    /// # Por qué no se barre por el prefijo del nombre
    ///
    /// El nombre de un contenedor de la batería es `kesto-e2e-<marca>-<sufijo>`, y la marca la
    /// sortea cada [`Mundo::nuevo`]: identifica al **escenario**, no a la ejecución. Un barrido por
    /// el prefijo `kesto-e2e-` no distingue por tanto los restos de una ejecución que murió de los
    /// contenedores de otra que esté corriendo a la vez —en otra terminal, o en el CI mientras
    /// alguien mira en local—, y borrarlos es matar el trabajo de otro. Pasó, y por eso esto cambió.
    ///
    /// # Cómo se distingue una ejecución muerta
    ///
    /// Cada contenedor se crea con la etiqueta [`ETIQUETA_DEL_CREADOR`], que lleva el identificador
    /// del proceso que lo creó —ver [`Motor::crear`]—. El barrido borra los que la llevan y cuyo
    /// proceso **ya no existe**: los de una ejecución viva se quedan donde están, y los que no
    /// llevan la etiqueta tampoco se tocan, porque de esos no se sabe quién los creó y lo que no se
    /// sabe no se borra.
    ///
    /// Lo que queda fuera, y hay que saberlo: los restos de una ejecución **viva** de este mismo
    /// proceso —un escenario que entró en pánico a medias— no se recogen aquí, porque su proceso es
    /// el que está preguntando. Se recogen cuando ese proceso muera y la batería vuelva a correr.
    ///
    /// El barrido corre **una sola vez por proceso**, y eso no es una optimización: el escenario del
    /// aislamiento entre ejecuciones monta **dos mundos vivos a la vez**, y con una pasada al
    /// principio basta para lo que se busca —restos de ejecuciones que ya no están—.
    async fn recoger_los_restos(&self) {
        static BARRIDO: std::sync::OnceLock<()> = std::sync::OnceLock::new();

        if BARRIDO.get().is_some() {
            return;
        }

        let opciones = ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        };
        let Ok(contenedores) = self.docker.list_containers(Some(opciones)).await else {
            return;
        };

        for contenedor in contenedores {
            // Las dos condiciones, por nombre y por etiqueta: la primera acota a lo que es de la
            // batería y la segunda dice quién lo creó. Sin la segunda no se puede distinguir una
            // ejecución muerta de una viva, y sin la primera el barrido dependería de una etiqueta
            // que cualquiera puede poner.
            let nuestro = contenedor
                .names
                .as_ref()
                .is_some_and(|nombres| nombres.iter().any(|nombre| es_de_la_bateria(nombre)));

            let Some(creador) = creador(&contenedor) else {
                continue;
            };

            if !nuestro || proceso_vivo(creador) {
                continue;
            }

            for nombre in contenedor.names.unwrap_or_default() {
                let _ = self.borrar(nombre.trim_start_matches('/')).await;
            }
        }

        let _ = BARRIDO.set(());
    }

    /// Se asegura de que la imagen **está** y, si no lo está, la trae.
    ///
    /// La batería no da por hecho que el CI haya pre-descargado nada: si la imagen no está, la
    /// descarga, y si tampoco se puede descargar, **falla nombrando la imagen** —que es lo que
    /// el requisito de «falla en voz alta» pide—.
    ///
    /// # Errors
    ///
    /// Con el nombre de la imagen cuando no se puede conseguir.
    pub async fn asegurar_imagen(&self, imagen: &str) -> Result<(), String> {
        if self.docker.inspect_image(imagen).await.is_ok() {
            return Ok(());
        }

        let opciones = CreateImageOptions::<String> {
            from_image: imagen.to_owned(),
            ..Default::default()
        };

        let mut traida = self.docker.create_image(Some(opciones), None, None);
        while let Some(paso) = traida.next().await {
            paso.map_err(|causa| {
                format!("no se pudo traer la imagen `{imagen}` que la batería necesita: {causa}")
            })?;
        }

        self.docker.inspect_image(imagen).await.map_err(|causa| {
            format!("la imagen `{imagen}` no está disponible después de intentar traerla: {causa}")
        })?;

        Ok(())
    }

    /// Crea y arranca un contenedor, y se lo apunta para recogerlo al terminar.
    ///
    /// # Errors
    ///
    /// Si la imagen no se puede conseguir, si el contenedor no se crea o si no arranca.
    pub async fn crear(&mut self, receta: Receta<'_>) -> Result<String, String> {
        self.asegurar_imagen(receta.imagen).await?;

        let puertos = receta.puerto.map(|(dentro, fuera)| {
            let mut mapa = HashMap::new();
            mapa.insert(
                format!("{dentro}/tcp"),
                Some(vec![PortBinding {
                    host_ip: Some("127.0.0.1".to_owned()),
                    host_port: Some(fuera.to_string()),
                }]),
            );
            mapa
        });

        let montajes = receta
            .montaje
            .map(|(del_host, del_contenedor)| vec![format!("{del_host}:{del_contenedor}:ro,z")]);

        // La etiqueta que deja dicho **qué proceso** creó el contenedor, que es lo único que
        // permite después distinguir los restos de una ejecución muerta de los contenedores de
        // otra que esté viva: ver [`Motor::recoger_los_restos`].
        let mut etiquetas = HashMap::from([(
            ETIQUETA_DEL_CREADOR.to_owned(),
            std::process::id().to_string(),
        )]);

        // Y, si la receta lo pide, la etiqueta que declara el contenedor **fuente de copia**: es
        // la que el motor exige para crear un `exec` en él, así que sin ella Kesto no podría
        // copiar ni restaurar. Se añade **sin pisar** la del creador: son dos etiquetas distintas
        // y las dos hacen falta.
        if receta.fuente_de_copia {
            etiquetas.insert(
                ETIQUETA_DE_FUENTE_DE_COPIA.to_owned(),
                VALOR_DE_FUENTE_DE_COPIA.to_owned(),
            );
        }

        let configuracion = Config {
            image: Some(receta.imagen.to_owned()),
            // Un `cmd` vacío se deja **sin declarar**, y no como lista vacía: declararlo vacío
            // pisaría el `CMD` de la imagen —el `mariadbd` de MariaDB, el `postgres` de
            // PostgreSQL— y el contenedor arrancaría sin nada que ejecutar.
            cmd: (!receta.cmd.is_empty())
                .then(|| receta.cmd.iter().map(|parte| (*parte).to_owned()).collect()),
            env: Some(receta.env.iter().map(|par| (*par).to_owned()).collect()),
            labels: Some(etiquetas),
            host_config: Some(HostConfig {
                port_bindings: puertos,
                binds: montajes,
                ..Default::default()
            }),
            ..Default::default()
        };

        let opciones = CreateContainerOptions::<String> {
            name: receta.nombre.to_owned(),
            platform: None,
        };

        let creado = self
            .docker
            .create_container(Some(opciones), configuracion)
            .await
            .map_err(|causa| {
                format!(
                    "no se pudo crear el contenedor `{}` con la imagen `{}`: {causa}",
                    receta.nombre, receta.imagen
                )
            })?;

        self.docker
            .start_container(&creado.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|causa| {
                format!(
                    "no se pudo arrancar el contenedor `{}`: {causa}",
                    receta.nombre
                )
            })?;

        self.creados.push(receta.nombre.to_owned());

        Ok(receta.nombre.to_owned())
    }

    /// Ejecuta una orden dentro de un contenedor y devuelve su salida y su desenlace.
    ///
    /// # Errors
    ///
    /// Si el contenedor no está, si la orden no se puede lanzar o si no termina dentro del
    /// plazo de una orden.
    pub async fn ejecutar(
        &self,
        contenedor: &str,
        cmd: &[&str],
        env: &[&str],
    ) -> Result<Salida, String> {
        self.ejecutar_con(contenedor, cmd, env, None).await
    }

    /// Ejecuta una orden y le entrega `entrada` por su **entrada estándar**.
    ///
    /// Es la forma de mandar datos que no caben en un argumento: el límite del kernel para un
    /// argumento suelto ronda los 128 KiB, y un guion de siembra de dos mil filas lo pasa de
    /// largo —el motor contesta «Argument list too long» **antes** de ejecutar nada, así que el
    /// fallo no es del producto sino del arnés—. Con la entrada por el `stdin` del `exec`, los
    /// datos viajan por su propio canal y el `argv` se queda con lo que es: el comando.
    ///
    /// # Errors
    ///
    /// Las mismas que [`Motor::ejecutar`], más el corte de la escritura.
    pub async fn ejecutar_con_entrada(
        &self,
        contenedor: &str,
        cmd: &[&str],
        env: &[&str],
        entrada: &[u8],
    ) -> Result<Salida, String> {
        self.ejecutar_con(contenedor, cmd, env, Some(entrada)).await
    }

    /// El `exec` de verdad: con o sin entrada estándar.
    ///
    /// # Errors
    ///
    /// Si el contenedor no está, si la orden no se puede lanzar o si no termina dentro del
    /// plazo de una orden.
    async fn ejecutar_con(
        &self,
        contenedor: &str,
        cmd: &[&str],
        env: &[&str],
        entrada: Option<&[u8]>,
    ) -> Result<Salida, String> {
        let opciones = CreateExecOptions {
            attach_stdin: Some(entrada.is_some()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            tty: Some(false),
            env: Some(env.to_vec()),
            cmd: Some(cmd.to_vec()),
            ..Default::default()
        };

        let creado = self
            .docker
            .create_exec(contenedor, opciones)
            .await
            .map_err(|causa| format!("no se pudo preparar `{cmd:?}` en `{contenedor}`: {causa}"))?;

        let iniciado = self
            .docker
            .start_exec(
                &creado.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: None,
                }),
            )
            .await
            .map_err(|causa| format!("no se pudo lanzar `{cmd:?}` en `{contenedor}`: {causa}"))?;

        let StartExecResults::Attached {
            mut output, input, ..
        } = iniciado
        else {
            return Err(format!(
                "`{cmd:?}` en `{contenedor}` no dejó leer su salida: el motor la soltó"
            ));
        };

        // La entrada se escribe **a la vez** que se drena la salida, y no antes: un proceso que
        // escribe su salida mientras todavía no ha leído toda su entrada se queda esperando a
        // que alguien vacíe el tubo, y con las dos cosas en serie los dos se esperarían.
        let escritura = async move {
            let Some(entrada) = entrada else {
                return Ok::<(), String>(());
            };

            let mut input = input;

            input.write_all(entrada).await.map_err(|causa| {
                format!("se cortó la escritura de la entrada estándar: {causa}")
            })?;

            // Cerrar la entrada es lo que le dice al proceso que ya está todo: sin esto, un
            // cliente que lee hasta el final del flujo se queda esperando para siempre.
            let _ = input.shutdown().await;

            Ok(())
        };

        let drenaje = async {
            let mut salida = String::new();
            let mut error = String::new();

            while let Some(trozo) = output.next().await {
                let trozo = trozo.map_err(|causa| {
                    format!("se cortó la salida de `{cmd:?}` en `{contenedor}`: {causa}")
                })?;

                match trozo {
                    LogOutput::StdOut { message } => {
                        salida.push_str(&String::from_utf8_lossy(&message));
                    }
                    LogOutput::StdErr { message } => {
                        error.push_str(&String::from_utf8_lossy(&message));
                    }
                    _ => {}
                }
            }

            Ok::<(String, String), String>((salida, error))
        };

        let (escritura, drenaje) = tokio::time::timeout(PLAZO_DE_LA_ORDEN, async {
            tokio::join!(escritura, drenaje)
        })
        .await
        .map_err(|_| {
            format!(
                "`{cmd:?}` en `{contenedor}` no terminó en {PLAZO_DE_LA_ORDEN:?}: el motor \
                 dejó de entregar su salida"
            )
        })?;

        escritura?;
        let (salida, error) = drenaje?;

        let estado = self
            .docker
            .inspect_exec(&creado.id)
            .await
            .map_err(|causa| format!("no se pudo leer el desenlace de `{cmd:?}`: {causa}"))?;

        Ok(Salida {
            codigo: estado.exit_code.unwrap_or(-1),
            salida,
            error,
        })
    }

    /// Ejecuta un guion de shell dentro de un contenedor.
    ///
    /// Sirve para lo que no necesita datos por la entrada estándar: crear un cubo, sondear un
    /// servicio, escribir un fichero pequeño. Lo que sí necesita datos —una siembra grande—
    /// va por [`Motor::ejecutar_con_entrada`].
    ///
    /// # Errors
    ///
    /// Las mismas que [`Motor::ejecutar`].
    pub async fn ejecutar_guion(&self, contenedor: &str, guion: &str) -> Result<Salida, String> {
        self.ejecutar(contenedor, &["sh", "-c", guion], &[]).await
    }

    /// Insiste con una orden —y su entorno— hasta que termina bien, y lo deja dicho si no llega
    /// a pasar.
    ///
    /// Es la espera de arranque: lo que se le pide es una orden que **falle** mientras el
    /// servicio no esté de verdad listo —una consulta contra su base, no un «¿estás?»—, porque
    /// un servicio a medio arrancar puede contestar que sí. El entorno es lo que la orden
    /// necesita para autenticarse, como el `PGPASSWORD` de [`sonda_de_postgres`].
    ///
    /// # Errors
    ///
    /// Con el último motivo que dio el contenedor cuando vence el plazo.
    pub async fn esperar_orden(
        &self,
        contenedor: &str,
        cmd: &[&str],
        env: &[&str],
        plazo: Duration,
    ) -> Result<(), String> {
        let inicio = Instant::now();

        loop {
            let ultimo = match self.ejecutar(contenedor, cmd, env).await {
                Ok(salida) if salida.codigo == 0 => return Ok(()),
                Ok(salida) => format!("código {} · {}", salida.codigo, salida.error),
                Err(causa) => causa,
            };

            if inicio.elapsed() >= plazo {
                return Err(format!(
                    "`{contenedor}` no respondió a `{cmd:?}` en {plazo:?}: {ultimo}"
                ));
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Para un contenedor sin borrarlo.
    ///
    /// Es la forma de provocar un fallo del almacén a mitad de camino: el contenedor sigue
    /// ahí —y su fila en la base también— pero su puerto ya no contesta.
    ///
    /// # Errors
    ///
    /// Si el motor no puede pararlo.
    pub async fn parar(&self, contenedor: &str) -> Result<(), String> {
        self.docker
            .stop_container(contenedor, None)
            .await
            .map_err(|causa| format!("no se pudo parar el contenedor `{contenedor}`: {causa}"))
    }

    /// Borra un contenedor.
    ///
    /// # Errors
    ///
    /// Si el motor no puede borrarlo.
    pub async fn borrar(&self, contenedor: &str) -> Result<(), String> {
        self.docker
            .remove_container(
                contenedor,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|causa| format!("no se pudo borrar el contenedor `{contenedor}`: {causa}"))
    }

    /// Recoge lo que esta ejecución creó.
    ///
    /// Cada escenario lo llama al terminar. Si uno entra en pánico antes, lo que dejó lo
    /// recoge la siguiente ejecución de la batería —[`Motor::conectar`] barre el terreno, y
    /// solo lo que dejó una ejecución que ya no está—.
    pub async fn recoger(&mut self) {
        for contenedor in std::mem::take(&mut self.creados) {
            let _ = self.borrar(&contenedor).await;
        }
    }
}

/// ¿Es el nombre de un contenedor de la batería?
///
/// El nombre lo pone el arnés al crearlo —`kesto-e2e-<marca>-<sufijo>`—, así que el prefijo es
/// suyo y de nadie más. Se compara con la barra de delante quitada, que es como lo devuelve el
/// motor al listar.
fn es_de_la_bateria(nombre: &str) -> bool {
    nombre.trim_start_matches('/').starts_with("kesto-e2e-")
}

/// El proceso que creó el contenedor, según su etiqueta.
///
/// Devuelve `None` cuando el contenedor no la lleva o cuando lo que lleva no es un identificador
/// de proceso: en los dos casos no se sabe quién lo creó, y quien pregunta tiene que tratarlo como
/// lo que es —un contenedor del que no se sabe nada— y no borrarlo.
fn creador(contenedor: &ContainerSummary) -> Option<u32> {
    contenedor
        .labels
        .as_ref()?
        .get(ETIQUETA_DEL_CREADOR)?
        .parse()
        .ok()
}

/// ¿Sigue vivo el proceso `pid`?
///
/// Se pregunta por `/proc`, que es la forma de saberlo sin abrir un `kill` ni depender de `libc`.
/// El proceso que se busca es el que corre la batería, y ese es siempre **local**: el socket del
/// motor puede apuntar a otro equipo, pero quien crea los contenedores es este, y su `/proc` es el
/// que se mira. Un `pid` que ya no está es el de una ejecución que murió, y sus restos son lo que
/// el barrido recoge.
fn proceso_vivo(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

// ── La espera de arranque de un motor ──────────────────────────────────────

/// La espera entre los dos sondeos que confirman un arranque: **2 s**.
///
/// # De dónde sale el número
///
/// Es la **ventana del servidor temporal** medida sobre las imágenes de la batería —lo que tarda
/// el entrypoint desde que le crea la base al temporal hasta que lo apaga—, 5 corridas por motor
/// en máquina ociosa:
///
/// | Motor | Ventanas medidas (ms) | Máximo |
/// |---|---|---|
/// | MariaDB (`mariadb:11`) | 31 · 26 · 25 · 28 · 25 | **31** |
/// | PostgreSQL (`postgres:18-alpine`) | 134 · 137 · 133 · 148 · 131 | **148** |
///
/// Los dos segundos son **~13× el máximo medido**: no es el valor de la ventana, es una holgura
/// deliberada. La suite levanta contenedores en paralelo y la máquina puede estar cargada, y lo
/// que se gana con el margen —no confundir el temporal con el definitivo— vale más que la espera.
///
/// El coste se paga **una vez por arranque de contenedor**: la espera va entre los dos sondeos de
/// la confirmación, no antes de cada sondeo, así que no escala con el número de intentos.
pub const ESPERA_ENTRE_SONDEOS: Duration = Duration::from_secs(2);

/// Espera a que un motor esté arrancado **de verdad**: la sonda tiene que acertar **dos veces**,
/// separadas por `espera`, y solo entonces se da el arranque por bueno.
///
/// # Por qué un acierto no basta
///
/// Las imágenes oficiales de MariaDB y de PostgreSQL levantan un **servidor temporal** para
/// inicializarse —que **ya tiene la base creada**, porque la crea él— y lo apagan antes de
/// arrancar el definitivo: MariaDB lo arranca con `--skip-networking` y PostgreSQL con
/// `listen_addresses=''`, y en los dos casos el temporal crea la base del `MARIADB_DATABASE` /
/// `POSTGRES_DB` antes de apagarse. La sonda **nombra la base** ([`sonda_de_mariadb`],
/// [`sonda_de_postgres`]), y eso la protege del temporal *antes* de que la base exista, pero no
/// de su tramo final: un acierto único puede ser contra un servidor que se apaga un instante
/// después, y quien siembra entonces se encuentra el socket ausente (`ERROR 2002`, `ENOENT`).
/// Exigir que el acierto se **repita** tras una espera distingue al definitivo —que sigue en
/// pie— del temporal, que ya no está.
///
/// Es la espera que usan los dos motores con servidor temporal, y **no** la de un arranque que
/// no lo tenga: [`Motor::esperar_orden`] se queda como está para lo demás —el `alpine` con el
/// cliente SQLite, sin servidor—, y esta espera no le añade a ese caso una latencia ni un modo
/// de fallo que no le corresponden.
///
/// # Por qué el sondeo y la espera se inyectan
///
/// El **sondeo** llega como clausura y la **espera** como parámetro —no como constantes leídas
/// dentro—, igual que el `reintentar` del #141 recibe el «intento»: así una prueba simula la
/// secuencia entera —acierto durante el temporal, fallo después, acierto estable al final— sin
/// contenedores y sin esperar la espera de producción, marcando la que quiere medir y contando
/// cuántas veces se sondeó. El `tokio::time` vive **solo aquí**, en el borde que duerme de
/// verdad; el resto es el bucle y su contrato.
///
/// # El plazo acota el primer acierto, no la confirmación
///
/// `plazo` es el presupuesto visible para que la sonda acierte **por primera vez**; la
/// confirmación —la espera y el segundo sondeo— dispone de su propia holgura, acotada por
/// `espera`. Por eso el límite interno del bucle es `plazo + espera` y no `plazo`: si el
/// definitivo aparece al final del plazo, su acierto **se confirma** en vez de fallar por unos
/// milisegundos. La espera entre sondeos no consume el plazo.
///
/// # Errors
///
/// Con el **último motivo** que dio el sondeo cuando vence el límite, nombrando el contenedor,
/// la orden y `plazo` (el presupuesto visible) —el mismo contrato que [`Motor::esperar_orden`]—
/// en vez de dar el arranque por bueno.
pub async fn esperar_arranque<F, Fut>(
    contenedor: &str,
    cmd: &[&str],
    plazo: Duration,
    espera: Duration,
    mut sonda: F,
) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let inicio = Instant::now();
    // El presupuesto visible es `plazo`, pero la confirmación de un acierto que llegue justo al
    // final necesita su propia holgura: por eso el bucle se corta en `plazo + espera`.
    let limite = plazo + espera;
    let mut ultimo = String::new();

    loop {
        let confirmado = match sonda().await {
            // El servidor contestó: para darlo por definitivo tiene que seguir en pie después de
            // la espera. El temporal del arranque se apaga justo aquí, así que no confirma.
            Ok(()) => {
                tokio::time::sleep(espera).await;

                match sonda().await {
                    Ok(()) => true,
                    Err(motivo) => {
                        ultimo = motivo;
                        false
                    }
                }
            }
            Err(motivo) => {
                ultimo = motivo;
                false
            }
        };

        if confirmado {
            return Ok(());
        }

        if inicio.elapsed() >= limite {
            return Err(format!(
                "`{contenedor}` no respondió a `{cmd:?}` en {plazo:?}: {ultimo}"
            ));
        }

        tokio::time::sleep(espera).await;
    }
}

// ── El servidor S3 ─────────────────────────────────────────────────────────

/// La identidad con la que el servidor S3 de la batería admite peticiones.
///
/// Es un fichero de configuración del servidor, no un secreto: se escribe en el directorio
/// temporal de la ejecución y se monta dentro del contenedor.
pub fn configuracion_del_s3() -> String {
    serde_json::to_string_pretty(&json!({
        "identities": [{
            "name": "kesto-e2e",
            "credentials": [{
                "accessKey": ACCESO_S3,
                "secretKey": SECRETO_S3,
            }],
            "actions": ["Admin", "Read", "Write", "List", "Tagging"],
        }]
    }))
    .expect("la configuración del servidor S3 de la batería es serializable")
}

/// Espera a que el servidor S3 conteste, y **no** degrada a un almacén local si no llega.
///
/// Es una función pura sobre el endpoint para que el escenario «un servidor S3 que no llega
/// falla con motivo» pueda afirmarla contra un puerto donde no hay nada.
///
/// # Errors
///
/// Con el endpoint y el último motivo cuando vence el plazo.
pub async fn esperar_el_s3(endpoint: &str, plazo: Duration) -> Result<(), String> {
    let cliente = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|causa| format!("no se pudo preparar el sondeo del servidor S3: {causa}"))?;

    let inicio = Instant::now();

    loop {
        // El servidor contesta lo que sea —un `403` es su forma de decir que está y no
        // admite anónimos—: lo que se espera es que **conteste**.
        let ultimo = match cliente.get(format!("{endpoint}/")).send().await {
            Ok(respuesta)
                if respuesta.status().is_success()
                    || respuesta.status() == StatusCode::FORBIDDEN =>
            {
                return Ok(());
            }
            Ok(respuesta) => format!("contestó {}", respuesta.status()),
            Err(causa) => causa.to_string(),
        };

        if inicio.elapsed() >= plazo {
            return Err(format!(
                "el servidor S3 de la batería no llegó a estar disponible en `{endpoint}` en \
                 {plazo:?}: {ultimo}. La batería no degrada a un almacén local: sin servidor S3 \
                 no hay nada que probar"
            ));
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

// ── El binario de Kesto y su API ───────────────────────────────────────────

/// El binario de Kesto arrancado para una prueba, con su base de datos y su puerto.
pub struct Kesto {
    hijo: Child,
    raiz: PathBuf,
    /// Puerto donde escucha.
    pub puerto: u16,
    cliente: reqwest::Client,
}

impl Drop for Kesto {
    fn drop(&mut self) {
        // El proceso muere pase lo que pase, incluso si la prueba entró en pánico.
        let _ = self.hijo.kill();
        let _ = self.hijo.wait();
        let _ = std::fs::remove_dir_all(&self.raiz);
    }
}

impl Kesto {
    /// Arranca el binario de verdad, con su entorno, y espera a que sirva.
    ///
    /// El modo es `Disabled` a propósito —no se declara ninguna `OIDC_*`—: sin proveedor de
    /// identidad la mitad protegida no se monta y todo `/api/v1/*` queda abierto, que es justo
    /// lo que la batería necesita para hablar con la API sin fabricar sesiones. Y como el modo
    /// es `Disabled`, el proceso declara el permiso explícito —`KESTO_ALLOW_INSECURE=1`— con un
    /// bind de loopback: sin ellos la aplicación se niega a arrancar.
    ///
    /// El almacén de la batería es un servidor S3 en `http://127.0.0.1:<puerto>` —esquema en claro
    /// y anfitrión de loopback—, así que el borde rechazaría su `endpoint` con un `400` sin el
    /// permiso del `endpoint`. Se declara con la misma forma que `KESTO_ALLOW_INSECURE` y por el
    /// mismo motivo —`KESTO_ALLOW_INSECURE_ENDPOINT=1`—, porque la batería es un laboratorio local
    /// y no un despliegue.
    ///
    /// `raiz_de_copias` es la raíz bajo la que el motor acepta el `host_path` de un target
    /// `directory` —se declara como `KESTO_BACKUP_ROOT`—: es la raíz temporal de la batería, y
    /// los directorios que los escenarios copian cuelgan de ella.
    ///
    /// # Errors
    ///
    /// Si el proceso no arranca o no llega a contestar la salud.
    pub async fn arrancar(socket: &str, raiz_de_copias: &Path) -> Result<Self, String> {
        let puerto = puerto_libre();
        let raiz = std::env::temp_dir().join(format!("kesto-e2e-binario-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&raiz)
            .map_err(|causa| format!("no se pudo crear `{}`: {causa}", raiz.display()))?;

        let registro = File::create(raiz.join("kesto.log"))
            .map_err(|causa| format!("no se pudo abrir el registro de la prueba: {causa}"))?;

        let raiz_de_copias = raiz_de_copias
            .to_str()
            .ok_or_else(|| {
                format!(
                    "la raíz de copias `{}` no es UTF-8",
                    raiz_de_copias.display()
                )
            })?
            .to_owned();

        let hijo = Command::new(env!("CARGO_BIN_EXE_kesto"))
            // `env_clear` deja la prueba hermética: lo que el binario ve es solo lo de aquí.
            .env_clear()
            .env("KESTO_MASTER_KEY", CLAVE_MAESTRA)
            .env("KESTO_ALLOW_INSECURE", "1")
            .env("KESTO_ALLOW_INSECURE_ENDPOINT", "1")
            .env("BIND_ADDRESS", format!("127.0.0.1:{puerto}"))
            .env(
                "DATABASE_URL",
                format!("sqlite://{}/kesto.db", raiz.display()),
            )
            .env("KESTO_BACKUP_ROOT", raiz_de_copias)
            .env("RUST_LOG", "info")
            .env(ENV_DOCKER_HOST, socket)
            .stdin(Stdio::null())
            .stdout(
                registro
                    .try_clone()
                    .map_err(|causa| format!("no se pudo duplicar el registro: {causa}"))?,
            )
            .stderr(registro)
            .spawn()
            .map_err(|causa| {
                format!(
                    "no se pudo lanzar el binario de Kesto (`{}`): {causa}",
                    env!("CARGO_BIN_EXE_kesto")
                )
            })?;

        let cliente = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|causa| format!("no se pudo preparar el cliente HTTP: {causa}"))?;

        let kesto = Self {
            hijo,
            raiz,
            puerto,
            cliente,
        };

        kesto.esperar_la_salud().await?;

        Ok(kesto)
    }

    /// La ruta del registro del proceso, para poder leerlo cuando algo falle.
    pub fn registro(&self) -> PathBuf {
        self.raiz.join("kesto.log")
    }

    /// Espera a que el binario conteste la salud.
    async fn esperar_la_salud(&self) -> Result<(), String> {
        let inicio = Instant::now();

        while inicio.elapsed() < PLAZO_DE_ARRANQUE {
            if let Ok(respuesta) = self.cliente.get(self.url("/api/v1/health")).send().await
                && respuesta.status().is_success()
            {
                return Ok(());
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        Err(format!(
            "el binario de Kesto no contestó a `GET /api/v1/health` en `127.0.0.1:{}` en \
             {PLAZO_DE_ARRANQUE:?}. Su registro está en `{}`",
            self.puerto,
            self.registro().display()
        ))
    }

    /// La URL de una ruta de la API.
    pub fn url(&self, ruta: &str) -> String {
        format!("http://127.0.0.1:{}{ruta}", self.puerto)
    }

    /// Una petición a la API: devuelve su código y su cuerpo.
    pub async fn pedir(
        &self,
        metodo: Method,
        ruta: &str,
        cuerpo: Option<&Value>,
    ) -> (StatusCode, Value) {
        let mut peticion = self.cliente.request(metodo, self.url(ruta));

        if let Some(cuerpo) = cuerpo {
            peticion = peticion.json(cuerpo);
        }

        let respuesta = peticion
            .send()
            .await
            .unwrap_or_else(|causa| panic!("la petición a `{ruta}` no salió: {causa}"));

        let estado = respuesta.status();
        let texto = respuesta
            .text()
            .await
            .unwrap_or_else(|causa| panic!("no se pudo leer el cuerpo de `{ruta}`: {causa}"));
        let valor = serde_json::from_str(&texto).unwrap_or(Value::Null);

        (estado, valor)
    }

    /// Una petición que tiene que salir bien: devuelve su `data`.
    pub async fn datos(
        &self,
        contexto: &str,
        metodo: Method,
        ruta: &str,
        cuerpo: Option<&Value>,
    ) -> Value {
        // El motivo se arma **antes** de la petición: `Method` viaja por valor y después del
        // `pedir` ya no está para escribirlo.
        let que = format!("{contexto}: `{metodo} {ruta}`");

        let (estado, valor) = self.pedir(metodo, ruta, cuerpo).await;

        assert!(
            estado.is_success(),
            "{que}: se esperaba un 2xx y llegó {estado} con {valor}"
        );

        valor.get("data").cloned().unwrap_or(Value::Null)
    }

    /// Da de alta un repositorio contra el servidor S3 de la batería.
    ///
    /// Reintenta de forma acotada ante un `502` —el `init` de rustic contra un SeaweedFS recién
    /// arrancado se pasa del plazo bajo carga, y un `502` deja cero filas escritas (ver
    /// [`reintentar`])— y, agotados los intentos, falla en voz alta con el último código y su
    /// cuerpo.
    pub async fn crear_repositorio(&self, endpoint: &str, cubo: &str) -> Value {
        let cuerpo = json!({
            "name": format!("e2e-{cubo}"),
            "endpoint": endpoint,
            "bucket": cubo,
            "region": REGION,
            "access_key_id": ACCESO_S3,
            "secret_access_key": SECRETO_S3,
            "passphrase": FRASE_DE_PASO,
        });

        let (estado, valor) = reintentar(INTENTOS_DEL_ALTA, ESPERA_ENTRE_ALTA, || {
            self.pedir(Method::POST, "/api/v1/repositories", Some(&cuerpo))
        })
        .await;

        assert!(
            estado.is_success(),
            "el alta del repositorio: `POST /api/v1/repositories` con {INTENTOS_DEL_ALTA} \
             intentos se quedó en {estado} con {valor}"
        );

        valor.get("data").cloned().unwrap_or(Value::Null)
    }

    /// Cambia la credencial de un repositorio, dejando los dos secretos guardados.
    ///
    /// El endpoint y el cubo se conservan: lo que se estropea es la credencial y nada más, que
    /// es lo único que el escenario quiere aislar.
    pub async fn estropear_la_credencial(
        &self,
        repositorio: &str,
        endpoint: &str,
        cubo: &str,
        acceso: &str,
    ) -> Value {
        self.datos(
            "el cambio de credencial del repositorio",
            Method::PUT,
            &format!("/api/v1/repositories/{repositorio}"),
            Some(&json!({
                "name": format!("e2e-{cubo}"),
                "endpoint": endpoint,
                "bucket": cubo,
                "region": REGION,
                "access_key_id": acceso,
            })),
        )
        .await
    }

    /// Da de alta un target y devuelve su identificador.
    pub async fn crear_target(&self, cuerpo: Value) -> String {
        let data = self
            .datos(
                "el alta del target",
                Method::POST,
                "/api/v1/targets",
                Some(&cuerpo),
            )
            .await;

        identificador_de(&data, "el target")
    }

    /// Da de alta una tarea y devuelve su identificador.
    pub async fn crear_tarea(
        &self,
        target: &str,
        repositorio: &str,
        nombre: &str,
        keep_daily: i64,
    ) -> String {
        let data = self
            .datos(
                "el alta de la tarea",
                Method::POST,
                "/api/v1/jobs",
                Some(&json!({
                    "target_id": target,
                    "repository_id": repositorio,
                    "cron_expression": "0 0 3 * * *",
                    "keep_daily": keep_daily,
                    "keep_weekly": 0,
                    "keep_monthly": 0,
                    "enabled": false,
                    "name": nombre,
                })),
            )
            .await;

        identificador_de(&data, "la tarea")
    }

    /// Lanza una tarea a mano y devuelve el identificador de su ejecución.
    pub async fn lanzar(&self, tarea: &str) -> String {
        let data = self
            .datos(
                "el lanzamiento de la tarea",
                Method::POST,
                &format!("/api/v1/jobs/{tarea}/run"),
                None,
            )
            .await;

        identificador_de(&data, "la ejecución")
    }

    /// Lanza una tarea y espera a que su ejecución acabe.
    pub async fn copiar(&self, tarea: &str, plazo: Duration) -> Value {
        let ejecucion = self.lanzar(tarea).await;

        self.esperar_la_ejecucion(tarea, &ejecucion, plazo).await
    }

    /// Lanza una tarea, espera a su ejecución y **exige** que acabara bien.
    pub async fn copiar_exigiendo_exito(&self, tarea: &str, plazo: Duration) -> Value {
        let fila = self.copiar(tarea, plazo).await;

        assert_eq!(
            fila["status"], "success",
            "la copia de la tarea `{tarea}` tenía que terminar en `success` y terminó en \
             `{}` con el motivo {:?}",
            fila["status"], fila["error_message"]
        );

        fila
    }

    /// Espera a que una ejecución de tarea termine, y devuelve su fila.
    pub async fn esperar_la_ejecucion(
        &self,
        tarea: &str,
        ejecucion: &str,
        plazo: Duration,
    ) -> Value {
        let inicio = Instant::now();
        let mut ultima = Value::Null;

        loop {
            let filas = self
                .datos(
                    "el listado de ejecuciones",
                    Method::GET,
                    &format!("/api/v1/executions?job_id={tarea}&limit=50"),
                    None,
                )
                .await;

            if let Some(fila) = filas["executions"]
                .as_array()
                .and_then(|filas| filas.iter().find(|fila| fila["id"] == ejecucion))
            {
                ultima = fila.clone();

                if let Some("success" | "failed") = fila["status"].as_str() {
                    return fila.clone();
                }
            }

            assert!(
                inicio.elapsed() < plazo,
                "la ejecución `{ejecucion}` de la tarea `{tarea}` no terminó en {plazo:?}: su \
                 última fila fue {ultima}"
            );

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Las instantáneas que el repositorio tiene en el almacén.
    pub async fn instantaneas(&self, repositorio: &str) -> Vec<Value> {
        let data = self
            .datos(
                "el listado de instantáneas",
                Method::GET,
                &format!("/api/v1/snapshots?repository_id={repositorio}"),
                None,
            )
            .await;

        data["snapshots"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| panic!("el listado de instantáneas no trajo `snapshots`: {data}"))
    }

    /// Pide restaurar una instantánea. Devuelve lo que contestó la API tal cual, para que el
    /// escenario pueda afirmar sobre un rechazo.
    pub async fn restaurar(
        &self,
        instantanea: &str,
        repositorio: &str,
        sobrescribir: bool,
    ) -> (StatusCode, Value) {
        self.pedir(
            Method::POST,
            &format!("/api/v1/snapshots/{instantanea}/restore"),
            Some(&json!({
                "repository_id": repositorio,
                "sobrescribir": sobrescribir,
            })),
        )
        .await
    }

    /// Restaura y espera a que la restauración acabe, exigiendo que saliera bien.
    pub async fn restaurar_exigiendo_exito(
        &self,
        instantanea: &str,
        repositorio: &str,
        sobrescribir: bool,
        plazo: Duration,
    ) -> Value {
        let (estado, valor) = self.restaurar(instantanea, repositorio, sobrescribir).await;

        assert!(
            estado.is_success(),
            "la restauración de `{instantanea}` tenía que lanzarse y contestó {estado} con \
             {valor}"
        );

        let ejecucion = valor["data"]["execution_id"]
            .as_str()
            .unwrap_or_else(|| panic!("la restauración no trajo `execution_id`: {valor}"))
            .to_owned();

        let fila = self.esperar_la_restauracion(&ejecucion, plazo).await;

        assert_eq!(
            fila["status"], "success",
            "la restauración `{ejecucion}` tenía que terminar en `success` y terminó en `{}` \
             con el motivo {:?}",
            fila["status"], fila["error_message"]
        );

        fila
    }

    /// Espera a que una restauración termine, y devuelve su fila.
    pub async fn esperar_la_restauracion(&self, ejecucion: &str, plazo: Duration) -> Value {
        let inicio = Instant::now();
        let mut ultima = Value::Null;

        loop {
            let data = self
                .datos(
                    "el listado de restauraciones",
                    Method::GET,
                    "/api/v1/restorations?limit=50",
                    None,
                )
                .await;

            if let Some(fila) = data["restorations"]
                .as_array()
                .and_then(|filas| filas.iter().find(|fila| fila["id"] == ejecucion))
            {
                ultima = fila.clone();

                if let Some("success" | "failed") = fila["status"].as_str() {
                    return fila.clone();
                }
            }

            assert!(
                inicio.elapsed() < plazo,
                "la restauración `{ejecucion}` no terminó en {plazo:?}: su última fila fue \
                 {ultima}"
            );

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// La fila de un repositorio, tal y como la API la devuelve.
    pub async fn repositorio(&self, repositorio: &str) -> (StatusCode, Value) {
        self.pedir(
            Method::GET,
            &format!("/api/v1/repositories/{repositorio}"),
            None,
        )
        .await
    }
}

/// El identificador que una respuesta de alta tiene que traer.
#[track_caller]
fn identificador_de(data: &Value, que: &str) -> String {
    data["id"]
        .as_str()
        .unwrap_or_else(|| panic!("el alta de {que} no trajo `id`: {data}"))
        .to_owned()
}

// ── El reintento del alta ──────────────────────────────────────────────────

/// Cuántas veces se reenvía el alta de un repositorio antes de darla por perdida.
///
/// Un número **pequeño** a propósito: lo que se tolera es un destino recién arrancado que aún no
/// contesta, no un destino caído. Tres intentos cubren el arranque en frío bajo carga (el `init`
/// de rustic puede pasarse de `TOPE_DEL_DESTINO` la primera vez) sin convertir un fallo real en
/// una espera larga.
pub const INTENTOS_DEL_ALTA: u32 = 3;

/// La espera del primer reintento del alta; la de después dobla a la anterior (backoff).
///
/// Con [`INTENTOS_DEL_ALTA`] = 3 las esperas son 2 s y 4 s. Se doblan en vez de repetirse porque
/// el motivo que se tolera —un destino que se está poniendo en pie— se resuelve con el tiempo, y
/// una espera que crece acorta el caso bueno sin alargar de más el malo.
pub const ESPERA_ENTRE_ALTA: Duration = Duration::from_secs(2);

/// Reintenta un «intento» **solo** mientras conteste `502`, con backoff entre intentos.
///
/// El bucle no sabe nada de HTTP más allá del código: recibe el intento como clausura y devuelve
/// la **última** respuesta tal cual. Eso lo hace puro y comprobable sin levantar un servidor —los
/// tests de este módulo simulan un `502` seguido de un `200` con un contador— y deja que el
/// llamador decida qué hacer con el resultado final.
///
/// - Devuelve en cuanto el código **no** es `502` (incluido un `2xx`).
/// - Ante `502`, espera `espera` —2 s la primera vez, el doble cada vez— y reintenta.
/// - Agotados los `intentos`, devuelve el **último `502` con su cuerpo**, sin entrar en pánico:
///   el llamador falla en voz alta con ese código y ese cuerpo.
///
/// # Por qué reintentar el alta es seguro
///
/// Un `502` equivale a **cero filas escritas**: en el alta la fila de base de datos se escribe
/// *después* de abrir el destino, así que si la apertura agota su plazo no queda estado parcial
/// que un segundo intento pudiera duplicar. Se reenvía la misma petición y nada más.
///
/// `intentos` es el número total de llamadas; un valor de `0` se trata como `1`.
async fn reintentar<F, Fut>(intentos: u32, espera: Duration, intento: F) -> (StatusCode, Value)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = (StatusCode, Value)>,
{
    let intentos = intentos.max(1);
    let mut respuesta = intento().await;
    let mut numero = 1;

    while respuesta.0 == StatusCode::BAD_GATEWAY && numero < intentos {
        // Backoff exponencial: 2 s, 4 s, 8 s… El factor se satura para que un número de intentos
        // grande no desborde el desplazamiento.
        let factor = 1u32.checked_shl(numero - 1).unwrap_or(u32::MAX);
        tokio::time::sleep(espera.saturating_mul(factor)).await;

        numero += 1;
        respuesta = intento().await;
    }

    respuesta
}

// ── El mundo de una prueba ─────────────────────────────────────────────────

/// Todo lo que un escenario necesita: motor, servidor S3, binario y repositorio.
///
/// Se monta **una vez por escenario**, y por eso cada escenario arranca con un cubo propio y
/// una base de datos propia: dos escenarios no comparten ni almacén ni histórico.
pub struct Mundo {
    /// El motor de contenedores.
    pub motor: Motor,
    /// El binario de Kesto, arrancado.
    pub kesto: Kesto,
    /// El nombre del contenedor del servidor S3.
    pub s3: String,
    /// El endpoint HTTP del servidor S3.
    pub endpoint: String,
    /// El cubo del repositorio de este escenario.
    pub cubo: String,
    /// El identificador del repositorio de este escenario.
    pub repositorio: String,
    marca: String,
    raiz: PathBuf,
}

impl Mundo {
    /// Monta el mundo: motor, S3, cubo, binario y repositorio.
    ///
    /// # Panics
    ///
    /// Con el motivo por delante cuando falta algo del entorno —`DOCKER_HOST`, el socket, el
    /// servidor S3—. Es la forma de que un escenario **falle** en vez de saltarse.
    pub async fn nuevo() -> Self {
        let marca = Uuid::new_v4().simple().to_string()[..8].to_owned();
        let raiz = std::env::temp_dir().join(format!("kesto-e2e-{marca}"));
        std::fs::create_dir_all(&raiz)
            .unwrap_or_else(|causa| panic!("no se pudo crear `{}`: {causa}", raiz.display()));

        let mut motor = Motor::conectar()
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"));

        let configuracion = raiz.join("s3.json");
        std::fs::write(&configuracion, configuracion_del_s3())
            .unwrap_or_else(|causa| panic!("no se pudo escribir la configuración del S3: {causa}"));

        let puerto = puerto_libre();
        let nombre = format!("kesto-e2e-{marca}-s3");
        let imagen_del_s3 = imagen_del_s3();
        let s3 = motor
            .crear(Receta {
                nombre: &nombre,
                imagen: &imagen_del_s3,
                cmd: &[
                    "server",
                    "-dir=/data",
                    "-s3",
                    "-s3.port=8333",
                    "-s3.config=/etc/seaweedfs/s3.json",
                ],
                env: &[],
                puerto: Some((8333, puerto)),
                montaje: Some((
                    configuracion
                        .to_str()
                        .expect("la ruta de la configuración del S3 es UTF-8"),
                    "/etc/seaweedfs/s3.json",
                )),
                // El servidor S3 **no** es fuente de copia: no se copia ni se restaura en él, así
                // que no lleva la etiqueta y el motor no le crearía ningún `exec`.
                fuente_de_copia: false,
            })
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"));

        let endpoint = format!("http://127.0.0.1:{puerto}");
        esperar_el_s3(&endpoint, PLAZO_DEL_S3)
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"));

        let cubo = format!("kesto-e2e-{marca}");
        motor
            .ejecutar_guion(
                &s3,
                &format!("echo 's3.bucket.create -name {cubo}' | weed shell"),
            )
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"));

        // Y no se da por creado: se pregunta. Un cubo que no está haría fracasar la puesta en
        // marcha del repositorio con un motivo del almacén, que es más difícil de leer.
        let cubos = motor
            .ejecutar_guion(&s3, "echo 's3.bucket.list' | weed shell")
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"));
        assert!(
            cubos.salida.contains(&cubo),
            "el cubo `{cubo}` no está en el servidor S3 de la batería: {}",
            cubos.salida
        );

        // El binario, contra el mismo socket del motor. Y sin proveedor de identidad: modo de
        // desarrollo, donde toda la API está abierta.
        let socket = motor.socket().to_owned();
        let kesto = Kesto::arrancar(&socket, &raiz)
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"));

        let repositorio = kesto.crear_repositorio(&endpoint, &cubo).await;

        Self {
            motor,
            kesto,
            s3,
            endpoint,
            cubo,
            repositorio: identificador_de(&repositorio, "el repositorio"),
            marca,
            raiz,
        }
    }

    /// Un directorio de trabajo propio de este escenario.
    #[track_caller]
    pub fn trabajo(&self, grupo: &str, nombre: &str) -> PathBuf {
        let ruta = self.raiz.join(grupo).join(nombre);
        std::fs::create_dir_all(&ruta)
            .unwrap_or_else(|causa| panic!("no se pudo crear `{}`: {causa}", ruta.display()));

        ruta
    }

    /// El nombre de un contenedor de este escenario.
    pub fn nombre_de_contenedor(&self, sufijo: &str) -> String {
        format!("kesto-e2e-{}-{sufijo}", self.marca)
    }

    /// Crea un contenedor más para este escenario.
    pub async fn contenedor(&mut self, receta: Receta<'_>) -> String {
        self.motor
            .crear(receta)
            .await
            .unwrap_or_else(|motivo| panic!("{motivo}"))
    }

    /// Recoge lo que el escenario creó.
    pub async fn cerrar(mut self) {
        self.motor.recoger().await;
        let _ = std::fs::remove_dir_all(&self.raiz);
    }
}

// ── Comparaciones ──────────────────────────────────────────────────────────

/// Compara dos árboles de directorios **fichero a fichero y byte a byte**.
///
/// Es la garantía de que la batería no se conforma con el código de salida del proceso: lo
/// que se compara es el contenido, y la primera diferencia se cuenta con nombre y camino.
/// Cuenta las tres cosas que pueden ir mal —lo que falta, lo que sobra y lo que cambió—, y
/// devuelve la primera que encuentre.
///
/// # Errors
///
/// Con la diferencia encontrada, nombrando el fichero.
pub fn comparar_arboles(esperado: &Path, obtenido: &Path) -> Result<(), String> {
    let mut suyos = ficheros_de(esperado)?;
    let mut nuestros = ficheros_de(obtenido)?;

    suyos.sort();
    nuestros.sort();

    for ruta in &suyos {
        if !nuestros.contains(ruta) {
            return Err(format!(
                "falta `{}` en `{}`: el original lo tiene y lo restaurado no",
                ruta.display(),
                obtenido.display()
            ));
        }
    }

    for ruta in &nuestros {
        if !suyos.contains(ruta) {
            return Err(format!(
                "sobra `{}` en `{}`: lo restaurado tiene un fichero que el original no tiene",
                ruta.display(),
                obtenido.display()
            ));
        }
    }

    for ruta in &suyos {
        let esperado = std::fs::read(esperado.join(ruta))
            .map_err(|causa| format!("no se pudo leer `{}`: {causa}", ruta.display()))?;
        let obtenido = std::fs::read(obtenido.join(ruta))
            .map_err(|causa| format!("no se pudo leer `{}`: {causa}", ruta.display()))?;

        if esperado != obtenido {
            return Err(format!(
                "`{}` no coincide: el original tiene {} bytes y lo restaurado {}, y el primer \
                 byte distinto es el {}",
                ruta.display(),
                esperado.len(),
                obtenido.len(),
                primer_byte_distinto(&esperado, &obtenido)
                    .map(|posicion| posicion.to_string())
                    .unwrap_or_else(|| String::from("final del más corto")),
            ));
        }
    }

    Ok(())
}

/// La posición del primer byte distinto entre dos contenidos.
fn primer_byte_distinto(uno: &[u8], otro: &[u8]) -> Option<usize> {
    uno.iter()
        .zip(otro.iter())
        .position(|(uno, otro)| uno != otro)
}

/// Todas las rutas de fichero de un árbol, **relativas** a su raíz.
fn ficheros_de(raiz: &Path) -> Result<Vec<PathBuf>, String> {
    let mut encontrados = Vec::new();
    let mut pendientes = vec![raiz.to_path_buf()];

    while let Some(directorio) = pendientes.pop() {
        let entradas = std::fs::read_dir(&directorio)
            .map_err(|causa| format!("no se pudo recorrer `{}`: {causa}", directorio.display()))?;

        for entrada in entradas {
            let entrada = entrada.map_err(|causa| {
                format!(
                    "no se pudo leer una entrada de `{}`: {causa}",
                    raiz.display()
                )
            })?;
            let ruta = entrada.path();

            if ruta.is_dir() {
                pendientes.push(ruta);
            } else {
                let relativa = ruta
                    .strip_prefix(raiz)
                    .map_err(|_| format!("`{}` no cuelga de `{}`", ruta.display(), raiz.display()))?
                    .to_path_buf();
                encontrados.push(relativa);
            }
        }
    }

    Ok(encontrados)
}

/// Compara dos listados de filas —el del origen y el del destino restaurado— y cuenta la
/// primera diferencia.
///
/// Es la misma garantía que [`comparar_arboles`] para un volcado de base de datos: lo que se
/// compara es el contenido, no el código de salida de la restauración.
///
/// # Errors
///
/// Con la diferencia encontrada, nombrando la fila.
pub fn comparar_filas(esperado: &str, obtenido: &str) -> Result<(), String> {
    let suyas: Vec<&str> = esperado.lines().map(str::trim_end).collect();
    let nuestras: Vec<&str> = obtenido.lines().map(str::trim_end).collect();

    for (numero, fila) in suyas.iter().enumerate() {
        if nuestras.get(numero) != Some(fila) {
            return Err(format!(
                "la fila {} no coincide: el origen tiene {fila:?} y lo restaurado {:?}",
                numero + 1,
                nuestras.get(numero).copied().unwrap_or("(no hay tal fila)")
            ));
        }
    }

    if nuestras.len() > suyas.len() {
        return Err(format!(
            "lo restaurado tiene {} filas y el origen {}: sobran {:?}",
            nuestras.len(),
            suyas.len(),
            &nuestras[suyas.len()..]
        ));
    }

    Ok(())
}

/// Copia un árbol de ficheros, para poder comparar después contra el original.
pub fn copiar_arbol(origen: &Path, destino: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destino)
        .map_err(|causa| format!("no se pudo crear `{}`: {causa}", destino.display()))?;

    for ruta in ficheros_de(origen)? {
        let de = origen.join(&ruta);
        let a = destino.join(&ruta);

        if let Some(padre) = a.parent() {
            std::fs::create_dir_all(padre)
                .map_err(|causa| format!("no se pudo crear `{}`: {causa}", padre.display()))?;
        }

        std::fs::copy(&de, &a)
            .map_err(|causa| format!("no se pudo copiar `{}`: {causa}", de.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    /// La espera inyectada en las pruebas: diminuta, para que el backoff se pueda medir sin
    /// que la batería pierda segundos esperándolo.
    const ESPERA_DE_PRUEBA: Duration = Duration::from_millis(20);

    /// El plazo de arranque de las pruebas: corto, para que un sondeo que nunca sobrevive
    /// agote el plazo en milisegundos en vez de en los 180 s de [`PLAZO_DE_ARRANQUE`].
    const PLAZO_DE_PRUEBA: Duration = Duration::from_millis(200);

    /// Un contador de llamadas compartido con la clausura del intento.
    fn contador_de_llamadas() -> (Rc<Cell<usize>>, Rc<Cell<usize>>) {
        let llamadas = Rc::new(Cell::new(0));

        (Rc::clone(&llamadas), llamadas)
    }

    #[tokio::test]
    async fn un_502_seguido_de_un_2xx_reintenta_y_devuelve_los_datos() {
        let (para_la_clausura, llamadas) = contador_de_llamadas();

        let (estado, valor) = reintentar(INTENTOS_DEL_ALTA, ESPERA_DE_PRUEBA, move || {
            let intento = para_la_clausura.get();
            para_la_clausura.set(intento + 1);

            async move {
                if intento == 0 {
                    (StatusCode::BAD_GATEWAY, json!({ "ok": false }))
                } else {
                    (
                        StatusCode::OK,
                        json!({ "ok": true, "data": { "id": "uno" } }),
                    )
                }
            }
        })
        .await;

        assert_eq!(estado, StatusCode::OK);
        assert_eq!(valor["data"]["id"], "uno");
        assert_eq!(llamadas.get(), 2, "un `502` y un `2xx` son dos llamadas");
    }

    #[tokio::test]
    async fn un_2xx_no_se_reintenta() {
        let (para_la_clausura, llamadas) = contador_de_llamadas();

        let (estado, valor) = reintentar(INTENTOS_DEL_ALTA, ESPERA_DE_PRUEBA, move || {
            para_la_clausura.set(para_la_clausura.get() + 1);

            async move {
                (
                    StatusCode::OK,
                    json!({ "ok": true, "data": { "id": "uno" } }),
                )
            }
        })
        .await;

        assert_eq!(estado, StatusCode::OK);
        assert_eq!(valor["data"]["id"], "uno");
        assert_eq!(llamadas.get(), 1, "un `2xx` no se reintenta");
    }

    #[tokio::test]
    async fn un_codigo_que_no_es_502_no_se_reintenta() {
        let (para_la_clausura, llamadas) = contador_de_llamadas();

        let (estado, valor) = reintentar(INTENTOS_DEL_ALTA, ESPERA_DE_PRUEBA, move || {
            para_la_clausura.set(para_la_clausura.get() + 1);

            async move {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "ok": false, "error": "otra cosa" }),
                )
            }
        })
        .await;

        assert_eq!(estado, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(valor["error"], "otra cosa");
        assert_eq!(llamadas.get(), 1, "solo el `502` se reintenta");
    }

    #[tokio::test]
    async fn un_502_persistente_agota_los_intentos_y_devuelve_el_ultimo() {
        let (para_la_clausura, llamadas) = contador_de_llamadas();

        let (estado, valor) = reintentar(INTENTOS_DEL_ALTA, ESPERA_DE_PRUEBA, move || {
            let intento = para_la_clausura.get();
            para_la_clausura.set(intento + 1);

            async move {
                (
                    StatusCode::BAD_GATEWAY,
                    json!({ "ok": false, "error": format!("el intento {intento}") }),
                )
            }
        })
        .await;

        assert_eq!(estado, StatusCode::BAD_GATEWAY);
        assert_eq!(
            valor["error"],
            format!("el intento {}", INTENTOS_DEL_ALTA - 1),
            "agotados los intentos devuelve el último, no uno vacío"
        );
        assert_eq!(
            llamadas.get(),
            INTENTOS_DEL_ALTA as usize,
            "un `502` persistente gasta los {INTENTOS_DEL_ALTA} intentos"
        );
    }

    /// Mide que la espera entre reintentos **crece** (backoff exponencial) y no es constante.
    ///
    /// Con 3 intentos el helper duerme `ESPERA_DE_PRUEBA` antes del segundo intento y
    /// `ESPERA_DE_PRUEBA * 2` antes del tercero, es decir `20 + 2×20 = 60 ms` en total. El umbral
    /// es `ESPERA_DE_PRUEBA * 3` (60 ms): una espera **constante** solo acumularía `2 × 20 = 40 ms`
    /// y quedaría por debajo, así que el umbral discrimina de verdad backoff de espera fija.
    ///
    /// La aserción es una **cota inferior** (`>=`): la carga de la máquina solo puede aumentar el
    /// tiempo transcurrido, nunca reducirlo, así que no introduce flakiness.
    #[tokio::test]
    async fn el_backoff_espera_entre_intentos() {
        let inicio = Instant::now();

        let (estado, _) = reintentar(3, ESPERA_DE_PRUEBA, || async {
            (StatusCode::BAD_GATEWAY, Value::Null)
        })
        .await;

        assert_eq!(estado, StatusCode::BAD_GATEWAY);

        let transcurrido = inicio.elapsed();
        // Backoff real: 20 ms (2.º intento) + 40 ms (3.er intento) = 60 ms; una espera constante
        // daría 40 ms.
        let minimo = ESPERA_DE_PRUEBA * 3;

        assert!(
            transcurrido >= minimo,
            "con 3 intentos y una espera de {ESPERA_DE_PRUEBA:?} el backoff acumula al menos \
             {minimo:?} entre reintentos y pasaron {transcurrido:?}"
        );
    }

    // ══ Requisito: la espera de arranque no se conforma con el servidor temporal ══

    /// Escenario: el servidor temporal no da la base por arrancada.
    ///
    /// El temporal del entrypoint oficial contesta con la base ya creada y se apaga un instante
    /// después: un único acierto no puede dar el arranque por bueno, porque el que contestó ya
    /// no está cuando llega la siembra. Aquí el sondeo acierta la primera vez y falla después
    /// —el temporal que se apaga—, así que el ayudante tiene que seguir sondeando y, agotado el
    /// plazo, fallar con el último motivo en vez de devolver un `Ok` que no le corresponde.
    #[tokio::test]
    async fn el_servidor_temporal_no_da_el_arranque_por_bueno() {
        let (para_la_sonda, sondeos) = contador_de_llamadas();

        let desenlace = esperar_arranque(
            "maria",
            &["mariadb", "-e", "SELECT 1"],
            PLAZO_DE_PRUEBA,
            ESPERA_DE_PRUEBA,
            move || {
                let sondeo = para_la_sonda.get();
                para_la_sonda.set(sondeo + 1);

                async move {
                    if sondeo == 0 {
                        // El temporal contesta una vez, con la base ya creada…
                        Ok(())
                    } else {
                        // …y se apaga: el socket desaparece.
                        Err("ERROR 2002: el socket no existe".to_owned())
                    }
                }
            },
        )
        .await;

        let motivo =
            desenlace.expect_err("un acierto que no sobrevive no da el arranque por bueno");

        assert!(
            motivo.contains("ERROR 2002"),
            "el motivo tiene que ser el último del sondeo, no un ok: {motivo}"
        );
        assert!(
            sondeos.get() >= 2,
            "el ayudante tiene que exigir un segundo sondeo y solo hizo {}",
            sondeos.get()
        );
    }

    /// Escenario: el servidor definitivo da la base por arrancada.
    ///
    /// El definitivo sigue en pie, así que su acierto se repite: el ayudante sondea **dos veces**
    /// separadas por la espera y da el arranque por bueno. El número de sondeos es exactamente el
    /// mínimo —no se sondea de más una vez confirmado—.
    #[tokio::test]
    async fn el_servidor_definitivo_da_el_arranque_por_bueno_tras_dos_sondeos() {
        let (para_la_sonda, sondeos) = contador_de_llamadas();

        let desenlace = esperar_arranque(
            "maria",
            &["mariadb", "-e", "SELECT 1"],
            PLAZO_DE_PRUEBA,
            ESPERA_DE_PRUEBA,
            move || {
                para_la_sonda.set(para_la_sonda.get() + 1);

                async { Ok(()) }
            },
        )
        .await;

        assert!(
            desenlace.is_ok(),
            "el definitivo, que sigue en pie, da el arranque por bueno: {desenlace:?}"
        );
        assert_eq!(
            sondeos.get(),
            2,
            "dos aciertos separados por la espera son el mínimo: no se sondea de más"
        );
    }

    /// Escenario: la espera entre sondeos es la inyectada, no una constante interna.
    ///
    /// La espera llega **inyectada** como parámetro, no como una constante leída dentro: con una
    /// espera marcada **muy por debajo** de la de producción ([`ESPERA_ENTRE_SONDEOS`], 2 s) y un
    /// sondeo que falla la primera vez, el ayudante la consume antes del segundo sondeo. Se
    /// afirman **las dos cotas**: transcurrió al menos la inyectada y bastante menos que la de
    /// producción. Si el ayudante ignorase el parámetro y leyera la constante interna, la cota
    /// superior fallaría —con 2 s por espera, dos esperas son ~4 s—, así que la prueba sí
    /// discrimina. La cota inferior no introduce flakiness, como en el backoff del #141: la carga
    /// de la máquina solo puede aumentar el tiempo transcurrido, nunca reducirlo.
    #[tokio::test]
    async fn la_espera_entre_sondeos_es_la_inyectada() {
        const ESPERA_INYECTADA: Duration = Duration::from_millis(20);
        // Muy por debajo de la de producción: así una implementación que leyera la constante
        // interna (2 s) se delata en la cota superior.
        const COTA_SUPERIOR: Duration = Duration::from_millis(500);
        let (para_la_sonda, sondeos) = contador_de_llamadas();

        let inicio = Instant::now();
        let desenlace = esperar_arranque(
            "maria",
            &["mariadb", "-e", "SELECT 1"],
            PLAZO_DE_PRUEBA,
            ESPERA_INYECTADA,
            move || {
                let sondeo = para_la_sonda.get();
                para_la_sonda.set(sondeo + 1);

                async move {
                    if sondeo == 0 {
                        Err("todavía no".to_owned())
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;
        let transcurrido = inicio.elapsed();

        assert!(
            desenlace.is_ok(),
            "el segundo acierto da el arranque: {desenlace:?}"
        );
        assert!(
            transcurrido >= ESPERA_INYECTADA,
            "la espera entre sondeos es la inyectada ({ESPERA_INYECTADA:?}) y pasaron \
             {transcurrido:?}"
        );
        assert!(
            transcurrido < COTA_SUPERIOR,
            "la espera inyectada ({ESPERA_INYECTADA:?}) tiene que mandar sobre la de producción \
             ({ESPERA_ENTRE_SONDEOS:?}), y pasaron {transcurrido:?}"
        );
        assert!(sondeos.get() >= 2, "hubo al menos un segundo sondeo");
    }

    /// Escenario: agotado el plazo, se falla en voz alta.
    ///
    /// Con un sondeo que nunca sobrevive, el ayudante no devuelve un ok silencioso: vencido el
    /// plazo falla nombrando el contenedor, la orden y el plazo —el mismo contrato que
    /// [`Motor::esperar_orden`]— con el **último** motivo que dio el sondeo.
    #[tokio::test]
    async fn agotado_el_plazo_falla_con_el_ultimo_motivo() {
        const PLAZO: Duration = Duration::from_millis(150);
        let (para_la_sonda, sondeos) = contador_de_llamadas();

        let desenlace = esperar_arranque(
            "maria",
            &["mariadb", "-e", "SELECT 1"],
            PLAZO,
            ESPERA_DE_PRUEBA,
            move || {
                let sondeo = para_la_sonda.get();
                para_la_sonda.set(sondeo + 1);

                async move { Err(format!("intento {sondeo}")) }
            },
        )
        .await;

        let motivo = desenlace.expect_err("agotado el plazo no se da el arranque por bueno");
        let ultimo = format!("intento {}", sondeos.get().saturating_sub(1));

        assert!(
            motivo.contains("maria"),
            "el motivo nombra el contenedor: {motivo}"
        );
        assert!(
            motivo.contains("mariadb"),
            "el motivo nombra la orden: {motivo}"
        );
        assert!(
            motivo.contains("150ms"),
            "el motivo nombra el plazo: {motivo}"
        );
        assert!(
            motivo.contains(&ultimo),
            "el motivo es el último del sondeo (`{ultimo}`), no uno anterior: {motivo}"
        );
    }
}
