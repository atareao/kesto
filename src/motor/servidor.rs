//! Servidor IPC del proceso privilegiado.
//!
//! Solo este módulo abre el socket del daemon de contenedores. La API entrega
//! órdenes por el canal tipado y nunca ve un cliente HTTP ni una credencial.

use std::collections::HashMap;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bollard::container::LogOutput;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, StartContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use thiserror::Error;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

use crate::ipc::{
    CanalIpc, ClaseFallo, Error as ErrorIpc, RespuestaMotor, SolicitudMotor, escribir,
    recibir_solicitud,
};
use crate::motor::config::AjustesMotor;

/// Plazo de conexión con el daemon, en segundos.
const SEGUNDOS_DE_CONEXION: u64 = 30;
/// Plazo para recibir el mensaje siguiente de un par IPC.
const TIEMPO_DE_LECTURA: Duration = Duration::from_secs(30);
/// Tope de conexiones IPC atendidas a la vez.
const MAX_CONEXIONES: usize = 32;

/// Fallos del servidor IPC.
#[derive(Debug, Error)]
pub enum Error {
    /// Fallo del canal IPC.
    #[error(transparent)]
    Ipc(#[from] ErrorIpc),
    /// Fallo de entrada/salida del socket IPC.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Fallo traducido del transporte de Kesto.
    #[error(transparent)]
    Docker(#[from] crate::docker::Error),
    /// El par IPC tardó demasiado en enviar una solicitud.
    #[error("tiempo de lectura IPC agotado")]
    TiempoDeLectura,
    /// El canal de solicitudes se cerró antes de terminar.
    #[error("el canal IPC se cerró antes de terminar")]
    CanalCerrado,
}

/// Aborta la tarea de lectura cuando la conexión termina, también por `?`.
struct AbortarLector(tokio::task::JoinHandle<()>);

impl Drop for AbortarLector {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Estado del `exec` que se está conductiendo en una conexión.
struct ExecEnCurso {
    /// Identificador dado por el daemon.
    id: String,
    /// Contenedor en el que se ejecuta.
    contenedor: String,
    /// Si la orden recibe el volcado por `stdin`.
    con_entrada: bool,
    /// Entrada abierta del `exec`, hasta que llegue el fin del volcado.
    entrada: Option<Pin<Box<dyn AsyncWrite + Send + Unpin>>>,
}

/// Escucha en el socket IPC y atiende a un par por conexión.
///
/// # Errors
///
/// Falla si no se puede crear el socket o aceptar una conexión; un cliente
/// individual que falle se descarta sin tumbar el servidor.
pub async fn servir(ajustes: AjustesMotor) -> Result<(), Error> {
    let ruta = ajustes.socket_ipc.clone();
    preparar_ruta(&ruta)?;

    // Enlazar antes de hablar con el daemon deja el socket observable desde el
    // primer instante: un arranque que falla al conectar no deja al cliente
    // esperando sobre un fichero que nunca se creó. Si la conexión falla, se
    // retira el enlace para no dejar un socket colgante.
    let listener = UnixListener::bind(&ruta)?;
    std::fs::set_permissions(&ruta, std::fs::Permissions::from_mode(0o660))?;

    let cliente = crate::docker::Cliente::conectar(
        &ajustes.docker_host,
        std::time::Duration::from_secs(SEGUNDOS_DE_CONEXION),
    );
    let cliente = match cliente {
        Ok(cliente) => cliente,
        Err(error) => {
            drop(listener);
            let _ = std::fs::remove_file(&ruta);
            return Err(Error::Docker(error));
        }
    };
    let docker = Arc::new(cliente.docker().clone());

    let conexiones = Arc::new(Semaphore::new(MAX_CONEXIONES));
    let raiz = Arc::new(ajustes.raiz_de_copias.clone());

    loop {
        let (stream, _) = listener.accept().await?;
        // El socket solo lo puede abrir el UID de la API; una credencial que
        // no se puede leer no se admite, igual que un UID distinto.
        let admitido = stream
            .peer_cred()
            .map(|credenciales| credenciales.uid() == ajustes.uid_api)
            .unwrap_or(false);
        if !admitido {
            tracing::warn!("conexión IPC rechazada: el par no es el UID de la API");
            drop(stream);
            continue;
        }

        // El cupo se toma **después** de identificar al par: un extraño no
        // puede agotar las plazas de los clientes legítimos.
        let Ok(permit) = Arc::clone(&conexiones).try_acquire_owned() else {
            tracing::warn!("conexión IPC rechazada: demasiadas conexiones");
            drop(stream);
            continue;
        };

        let docker = Arc::clone(&docker);
        let raiz = Arc::clone(&raiz);
        tokio::spawn(async move {
            let _permit: OwnedSemaphorePermit = permit;
            if let Err(error) = atender(stream, docker, &raiz).await {
                tracing::warn!(%error, "conexión IPC cerrada con error");
            }
        });
    }
}

/// Deja el directorio listo y retira un socket colgante de otra ejecución.
fn preparar_ruta(ruta: &Path) -> Result<(), Error> {
    if let Some(carpeta) = ruta.parent() {
        std::fs::create_dir_all(carpeta)?;
    }
    if ruta.exists() {
        std::fs::remove_file(ruta)?;
    }
    Ok(())
}

/// Atiende una conexión completa.
///
/// La lectura corre en una tarea propia y nunca se cancela: `read_exact` no es
/// cancel-safe, y un `select!` que interrumpiera un mensaje a medias dejaría el
/// canal desincronizado para siempre. Lo que se selecciona aquí son dos
/// *receptores* —solicitudes ya completas y eventos del `exec`—, no lecturas.
async fn atender(
    stream: UnixStream,
    docker: Arc<bollard::Docker>,
    raiz: &Path,
) -> Result<(), Error> {
    let (lectura, mut escritura) = tokio::io::split(stream);
    let (emisor_solicitudes, mut solicitudes) = mpsc::channel::<Result<SolicitudMotor, Error>>(16);
    let lector =
        tokio::spawn(async move {
            let mut lectura = lectura;
            loop {
                let solicitud =
                    match tokio::time::timeout(TIEMPO_DE_LECTURA, recibir_solicitud(&mut lectura))
                        .await
                    {
                        Ok(Ok(solicitud)) => Ok(solicitud),
                        Ok(Err(error)) => Err(Error::Ipc(error)),
                        Err(_) => Err(Error::TiempoDeLectura),
                    };
                let termino = solicitud.is_err();
                if emisor_solicitudes.send(solicitud).await.is_err() || termino {
                    break;
                }
            }
        });
    let _lector = AbortarLector(lector);

    let (emisor, mut eventos) = mpsc::channel::<RespuestaMotor>(64);
    let mut actual: Option<ExecEnCurso> = None;
    let desenlace = loop {
        tokio::select! {
            solicitud = solicitudes.recv() => {
                let solicitud = solicitud.ok_or(Error::CanalCerrado)??;
                match solicitud {
                    SolicitudMotor::Comprobar => {
                        responder(&mut escritura, &RespuestaMotor::Aceptado).await?;
                    }
                    SolicitudMotor::CrearContenedor { nombre, imagen, etiquetas } => {
                        let resultado = crear_contenedor(&docker, &nombre, &imagen, &etiquetas).await;
                        responder(&mut escritura, &resultado).await?;
                    }
                    SolicitudMotor::IniciarContenedor { nombre } => {
                        let resultado = iniciar_contenedor(&docker, &nombre).await;
                        responder(&mut escritura, &resultado).await?;
                    }
                    SolicitudMotor::CrearExec { contenedor, orden, con_entrada } => {
                        let resultado = crear_exec(&docker, &contenedor, &orden, con_entrada, raiz).await;
                        if let RespuestaMotor::ExecCreado { ref id } = resultado {
                            actual = Some(ExecEnCurso {
                                id: id.clone(),
                                contenedor: contenedor.clone(),
                                con_entrada,
                                entrada: None,
                            });
                        }
                        responder(&mut escritura, &resultado).await?;
                    }
                    SolicitudMotor::InspeccionarExec { id } => {
                        let resultado = inspeccionar_exec(&docker, &id).await;
                        responder(&mut escritura, &resultado).await?;
                    }
                    SolicitudMotor::IniciarExec { id: _, entrada } => {
                        let respuesta = match actual.as_mut() {
                            Some(estado) => iniciar_exec(
                                &docker, estado, &emisor, entrada,
                            ).await,
                            None => Some(fallo(
                                ClaseFallo::Protocolo,
                                "no hay ningún exec creado",
                                None,
                            )),
                        };
                        if let Some(respuesta) = respuesta {
                            responder(&mut escritura, &respuesta).await?;
                        }
                    }
                }
            }
            evento = eventos.recv() => {
                match evento {
                    Some(respuesta) => {
                        let finalizo = matches!(respuesta, RespuestaMotor::Terminado { .. });
                        responder(&mut escritura, &respuesta).await?;
                        if finalizo {
                            break Ok(());
                        }
                    }
                    None => break Ok(()),
                }
            }
        }
    };
    desenlace
}

/// Escribe una respuesta con el framing del contrato.
async fn responder<W>(escritura: &mut W, respuesta: &RespuestaMotor) -> Result<(), Error>
where
    W: AsyncWrite + Unpin,
{
    let cuerpo =
        serde_json::to_vec(respuesta).map_err(|causa| ErrorIpc::MensajeInvalido { causa })?;
    escribir(escritura, &cuerpo).await?;
    Ok(())
}

/// Crea un contenedor. No es una operación que la API use hoy, pero forma
/// parte de la lista blanca del canal.
async fn crear_contenedor(
    docker: &bollard::Docker,
    nombre: &str,
    imagen: &str,
    etiquetas: &[(String, String)],
) -> RespuestaMotor {
    let configuracion = ContainerConfig {
        image: Some(imagen.to_owned()),
        labels: Some(
            etiquetas
                .iter()
                .map(|(clave, valor)| (clave.clone(), valor.clone()))
                .collect::<HashMap<_, _>>(),
        ),
        ..Default::default()
    };
    match docker
        .create_container(
            Some(CreateContainerOptions {
                name: nombre.to_owned(),
                platform: None,
            }),
            configuracion,
        )
        .await
    {
        Ok(_) => RespuestaMotor::Aceptado,
        Err(causa) => fallo_de_docker(crate::docker::Error::del_contenedor(nombre, causa)),
    }
}

async fn iniciar_contenedor(docker: &bollard::Docker, nombre: &str) -> RespuestaMotor {
    match docker
        .start_container(nombre, None::<StartContainerOptions<String>>)
        .await
    {
        Ok(()) => RespuestaMotor::Aceptado,
        Err(causa) => fallo_de_docker(crate::docker::Error::del_contenedor(nombre, causa)),
    }
}

/// Comprueba la etiqueta de fuente y crea el `exec` en el daemon.
async fn crear_exec(
    docker: &bollard::Docker,
    contenedor: &str,
    orden: &crate::ipc::OrdenIpc,
    con_entrada: bool,
    raiz: &Path,
) -> RespuestaMotor {
    // La comprobación de la etiqueta es fail-closed y vive en el transporte,
    // antes de crear cualquier `exec`.
    let inspeccion = match docker
        .inspect_container(
            contenedor,
            None::<bollard::container::InspectContainerOptions>,
        )
        .await
    {
        Ok(inspeccion) => inspeccion,
        Err(causa) => {
            return fallo_de_docker(crate::docker::Error::del_contenedor(contenedor, causa));
        }
    };
    if let Err(error) = crate::docker::exigir_la_etiqueta_de_fuente(contenedor, &inspeccion) {
        return fallo_de_docker(error);
    }

    // La API no es de confiar: ella ya comprobó la forma de la ruta, pero aquí
    // se vuelve a confinar contra la raíz del propio motor, resolviendo enlaces.
    if !confinamiento_valido(raiz, &orden.cmd) {
        return fallo(
            ClaseFallo::Protocolo,
            "la orden contiene una ruta fuera de la raíz de copias",
            None,
        );
    }

    let opciones = CreateExecOptions {
        attach_stdin: Some(con_entrada),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        tty: Some(false),
        env: Some(orden.env.iter().map(String::as_str).collect()),
        cmd: Some(orden.cmd.iter().map(String::as_str).collect()),
        ..Default::default()
    };

    match docker.create_exec(contenedor, opciones).await {
        Ok(creado) => RespuestaMotor::ExecCreado { id: creado.id },
        Err(causa) => fallo_de_docker(crate::docker::Error::del_contenedor(contenedor, causa)),
    }
}

/// Revalida el confinamiento de las rutas bajo la raíz que viajan en la orden.
///
/// La comprobación textual del borde HTTP no basta: el motor es el único que
/// puede resolver enlaces y `..` en el sistema de ficheros que realmente se
/// archiva. Solo se confinan los trozos que declaran una ruta bajo la raíz de
/// copias; las rutas internas del contenedor (`/var/lib/…`, `db_path`) no son
/// rutas del host y no pueden resolverse aquí.
fn confinamiento_valido(raiz: &Path, cmd: &[String]) -> bool {
    cmd.iter()
        .flat_map(|argumento| {
            argumento.split(|caracter: char| {
                caracter.is_whitespace()
                    || matches!(
                        caracter,
                        ';' | '|' | '&' | '"' | '\'' | '=' | '(' | ')' | ',' | ':'
                    )
            })
        })
        .filter(|trozo| trozo.starts_with('/'))
        .all(|declarado| {
            if !Path::new(declarado).starts_with(raiz) {
                return true;
            }
            crate::frontera::confinar_host_path(raiz, declarado).is_ok()
        })
}

/// Arranca el `exec`, entrega un trozo de `stdin` (si lo hay) y lanza el drenaje.
async fn iniciar_exec(
    docker: &bollard::Docker,
    estado: &mut ExecEnCurso,
    emisor: &mpsc::Sender<RespuestaMotor>,
    entrada: Option<Vec<u8>>,
) -> Option<RespuestaMotor> {
    if estado.entrada.is_none() {
        let iniciado = match docker
            .start_exec(
                &estado.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: false,
                    output_capacity: None,
                }),
            )
            .await
        {
            Ok(iniciado) => iniciado,
            Err(causa) => {
                return Some(fallo_de_docker(crate::docker::Error::del_contenedor(
                    &estado.contenedor,
                    causa,
                )));
            }
        };

        let StartExecResults::Attached { input, output } = iniciado else {
            return Some(fallo(ClaseFallo::Motor, "el exec no dejó salida", None));
        };
        estado.entrada = Some(Box::pin(input));
        drenar(docker.clone(), estado.id.clone(), output, emisor.clone());
    }

    if estado.con_entrada {
        match (estado.entrada.as_mut(), entrada) {
            (Some(canal), Some(trozo)) => {
                if let Err(causa) = canal.write_all(&trozo).await {
                    tracing::warn!(%causa, "fallo al escribir en el stdin del exec");
                    return Some(fallo(
                        ClaseFallo::Entrada,
                        "no se pudo escribir el volcado en la entrada del exec",
                        None,
                    ));
                }
            }
            (Some(canal), None) => {
                let _ = canal.shutdown().await;
                estado.entrada = None;
            }
            (None, Some(_)) => {
                return Some(fallo(
                    ClaseFallo::Protocolo,
                    "la entrada del exec ya estaba cerrada",
                    None,
                ));
            }
            (None, None) => {}
        }
    }
    None
}

/// Drena la salida del `exec` en segundo plano y avisa cuando termina.
fn drenar(
    docker: bollard::Docker,
    id: String,
    output: impl futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>>
    + Send
    + Unpin
    + 'static,
    emisor: mpsc::Sender<RespuestaMotor>,
) {
    tokio::spawn(async move {
        let mut output = Box::pin(output);
        loop {
            match output.next().await {
                Some(Ok(LogOutput::StdOut { message })) => {
                    let _ = emisor
                        .send(RespuestaMotor::Trozo {
                            canal: CanalIpc::Salida,
                            bytes: message.to_vec(),
                        })
                        .await;
                }
                Some(Ok(LogOutput::StdErr { message })) => {
                    let _ = emisor
                        .send(RespuestaMotor::Trozo {
                            canal: CanalIpc::Error,
                            bytes: message.to_vec(),
                        })
                        .await;
                }
                Some(Ok(_)) => {}
                Some(Err(causa)) => {
                    let _ = emisor
                        .send(fallo_de_docker(crate::docker::Error::del_contenedor(
                            &id, causa,
                        )))
                        .await;
                    return;
                }
                None => break,
            }
        }

        let respuesta = match docker.inspect_exec(&id).await {
            Ok(estado) => match crate::docker::codigo_de_salida(&estado) {
                Some(codigo) => RespuestaMotor::Terminado { codigo },
                None => fallo(ClaseFallo::Codigo, "sin código de salida", None),
            },
            Err(causa) => fallo_de_docker(crate::docker::Error::del_contenedor(&id, causa)),
        };
        let _ = emisor.send(respuesta).await;
    });
}

/// Código de salida de un `exec` ya terminado, para la operación explícita.
async fn inspeccionar_exec(docker: &bollard::Docker, id: &str) -> RespuestaMotor {
    match docker.inspect_exec(id).await {
        Ok(estado) => match crate::docker::codigo_de_salida(&estado) {
            Some(codigo) => RespuestaMotor::Terminado { codigo },
            None => fallo(ClaseFallo::Codigo, "sin código de salida", None),
        },
        Err(causa) => fallo_de_docker(crate::docker::Error::del_contenedor(id, causa)),
    }
}

/// Construye un fallo del contrato.
fn fallo(clase: ClaseFallo, mensaje: &str, status_code: Option<u16>) -> RespuestaMotor {
    RespuestaMotor::Fallo {
        clase,
        mensaje: mensaje.to_owned(),
        status_code,
    }
}

/// Traduce un error del transporte al contrato IPC **sin reenviar su texto**.
///
/// El `Display` de `crate::docker::Error` nombra la ruta del socket y puede
/// llevar fragmentos de una URL con credenciales. Nada de eso puede cruzar el
/// canal hacia la API: se envía la familia del fallo, el código HTTP cuando lo
/// hay y un mensaje fijo que no interpola la causa.
fn fallo_de_docker(error: crate::docker::Error) -> RespuestaMotor {
    match error {
        crate::docker::Error::MotorContesta { status_code, .. } => fallo(
            ClaseFallo::Contenedor,
            "el motor de contenedores rechazó la operación",
            Some(status_code),
        ),
        crate::docker::Error::ContenedorNoEsFuenteDeCopia { .. } => fallo(
            ClaseFallo::Etiqueta,
            "el contenedor no se declara fuente de copia",
            None,
        ),
        crate::docker::Error::EntradaNoEntregada { .. } => fallo(
            ClaseFallo::Entrada,
            "no se pudo entregar el volcado al exec",
            None,
        ),
        crate::docker::Error::ExtraccionEstancada { .. } => fallo(
            ClaseFallo::Inactividad,
            "la orden dejó de producir salida",
            None,
        ),
        crate::docker::Error::RestauracionEstancada { .. } => fallo(
            ClaseFallo::Inactividad,
            "la orden de restauración dejó de producir salida",
            None,
        ),
        _ => fallo(
            ClaseFallo::Motor,
            "el motor de contenedores no está disponible",
            None,
        ),
    }
}

#[cfg(test)]
mod pruebas {
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::ipc::{conectar, escribir, recibir};

    use super::*;

    static CONTADOR: AtomicUsize = AtomicUsize::new(0);

    struct SocketDePrueba(PathBuf, PathBuf);

    impl SocketDePrueba {
        fn nuevo() -> Self {
            let prefijo = format!(
                "kesto-motor-{}-{}",
                std::process::id(),
                CONTADOR.fetch_add(1, Ordering::Relaxed)
            );
            let ruta = std::env::temp_dir().join(format!("{prefijo}.sock"));
            let docker = std::env::temp_dir().join(format!("{prefijo}-docker.sock"));
            let _ = std::fs::remove_file(&ruta);
            // `bollard` comprueba que la ruta existe al construir el cliente;
            // estas pruebas no hablan con el daemon, solo necesitan que el
            // servidor llegue a aceptar conexiones.
            std::fs::write(&docker, b"").expect("crea el socket de Docker de prueba");
            Self(ruta, docker)
        }
    }

    impl Drop for SocketDePrueba {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(&self.1);
        }
    }

    /// Espera acotada a que el servidor enlace el socket; no fija un plazo
    /// arbitrario de `sleep` y falla con mensaje si no llega a enlazarlo.
    async fn esperar_socket(ruta: &Path) {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut interval = tokio::time::interval(Duration::from_millis(5));
            loop {
                if ruta.exists() {
                    return;
                }
                interval.tick().await;
            }
        })
        .await
        .expect("el servidor enlazó el socket IPC dentro del plazo");
    }

    fn ajustes(ruta: &SocketDePrueba, uid_api: u32) -> AjustesMotor {
        AjustesMotor {
            docker_host: format!("unix://{}", ruta.1.display()),
            socket_ipc: ruta.0.clone(),
            uid_api,
            raiz_de_copias: std::env::temp_dir().join("kesto-raiz-de-prueba"),
        }
    }

    #[tokio::test]
    async fn un_par_con_uid_distinto_es_desconectado_sin_procesar() {
        let ruta = SocketDePrueba::nuevo();
        let uid_actual = std::fs::metadata("/proc/self")
            .expect("/proc/self existe")
            .uid();
        let servidor = tokio::spawn(servir(ajustes(&ruta, uid_actual.wrapping_add(1))));
        esperar_socket(&ruta.0).await;

        let mut canal = conectar(&ruta.0.to_string_lossy())
            .await
            .expect("el socket del motor existe");
        let cuerpo = serde_json::to_vec(&SolicitudMotor::Comprobar).expect("serializa");
        escribir(&mut canal, &cuerpo)
            .await
            .expect("el par escribe antes de ser rechazado");
        let desenlace = tokio::time::timeout(Duration::from_millis(300), recibir(&mut canal)).await;
        assert!(
            desenlace.is_err()
                || matches!(
                    desenlace,
                    Ok(Err(ErrorIpc::CanalCerrado)) | Ok(Err(ErrorIpc::NoDisponible { .. }))
                ),
            "un UID ajeno recibió respuesta: el motor lo procesó"
        );
        servidor.abort();
    }

    #[tokio::test]
    async fn un_par_con_el_uid_correcto_recibe_latido_y_una_orden_fuera_de_la_lista_cierra() {
        let ruta = SocketDePrueba::nuevo();
        let uid_actual = std::fs::metadata("/proc/self")
            .expect("/proc/self existe")
            .uid();
        let servidor = tokio::spawn(servir(ajustes(&ruta, uid_actual)));
        esperar_socket(&ruta.0).await;

        let mut canal = conectar(&ruta.0.to_string_lossy())
            .await
            .expect("el socket del motor existe");
        let cuerpo = serde_json::to_vec(&SolicitudMotor::Comprobar).expect("serializa");
        escribir(&mut canal, &cuerpo)
            .await
            .expect("envía el latido");
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), recibir(&mut canal))
                .await
                .expect("el motor contesta"),
            Ok(RespuestaMotor::Aceptado)
        ));
        drop(canal);

        // Una variante que no está en la lista blanca no llega a ningún
        // manejador: el decoder la rechaza y la conexión se cierra.
        let mut canal = conectar(&ruta.0.to_string_lossy())
            .await
            .expect("el socket del motor existe");
        escribir(&mut canal, br#"{"BorrarContenedor":{}}"#)
            .await
            .expect("envía la operación hostil");
        let desenlace = tokio::time::timeout(Duration::from_millis(300), recibir(&mut canal)).await;
        assert!(
            desenlace.is_err() || matches!(desenlace, Ok(Err(_))),
            "una operación fuera de la lista blanca obtuvo una respuesta"
        );
        servidor.abort();
    }

    #[test]
    fn un_fallo_del_socket_no_publica_la_ruta_ni_la_credencial() {
        let respuesta = fallo_de_docker(crate::docker::Error::MotorNoDisponible {
            socket: String::from("unix:///run/secreto/docker.sock?token=CLAVE"),
            causa: bollard::errors::Error::RequestTimeoutError,
        });

        let RespuestaMotor::Fallo { mensaje, .. } = respuesta else {
            panic!("esperábamos un fallo del contrato");
        };
        assert!(
            !mensaje.contains("/run/secreto") && !mensaje.contains("CLAVE"),
            "la respuesta IPC publicó la ruta o la credencial: {mensaje}"
        );
    }

    #[test]
    fn una_ruta_que_escapa_de_la_raiz_se_rechaza_antes_del_exec() {
        let raiz = std::env::temp_dir().join("kesto-raiz-de-prueba");
        let cmd = vec![
            String::from("sh"),
            String::from("-c"),
            format!("cat {}/../secreto", raiz.display()),
        ];
        assert!(!confinamiento_valido(&raiz, &cmd));
    }

    #[test]
    fn una_ruta_dentro_de_la_raiz_y_una_ruta_del_contenedor_se_aceptan() {
        let raiz = std::env::temp_dir().join("kesto-raiz-de-prueba");
        let cmd = vec![
            String::from("sqlite3"),
            format!("{}/base.db", raiz.display()),
            String::from("/var/lib/otra-base.db"),
        ];
        assert!(confinamiento_valido(&raiz, &cmd));
    }
}
