//! El binario: elige el transporte, arranca y espera.
//!
//! Sin argumentos habla el protocolo MCP por la entrada y la salida estándar —que es como
//! lo lanzan opencode o Claude Desktop—; con `--transport http` sirve el mismo catálogo por
//! `Streamable HTTP` bajo `/mcp`, que es lo que necesita un uso remoto.
//!
//! El registro va **a stderr** y no a stdout, y eso no es una preferencia de estilo: en
//! `stdio` el protocolo **es** stdout, así que una línea de traza escrita ahí corrompe el
//! flujo y el cliente deja de entender el servidor.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::process::ExitCode;

use kesto_mcp::client::ApiClient;
use kesto_mcp::config::{Config, KESTO_MCP_BIND_ADDRESS};
use kesto_mcp::http;
use kesto_mcp::server::KestoMcp;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tokio_util::sync::CancellationToken;

/// Lo que se enseña con `--help`.
const AYUDA: &str = "\
kesto-mcp — servidor MCP de Kesto

USO:
  kesto-mcp                      arranca por `stdio` (el defecto)
  kesto-mcp --transport http     arranca por Streamable HTTP

OPCIONES:
  --transport <stdio|http>       el transporte; `stdio` si no se dice nada
  --bind <dirección>             dirección del transporte HTTP
  -h, --help                     esta ayuda

ENTORNO:
  KESTO_URL                      obligatoria: URL base de la API de Kesto
  KESTO_API_TOKEN                token por defecto; en HTTP, el de la cabecera manda
  KESTO_MCP_BIND_ADDRESS         dirección del transporte HTTP (127.0.0.1:8931)
  KESTO_MCP_ALLOWED_HOSTS        hosts admitidos por HTTP, separados por comas
  RUST_LOG                       nivel de registro; el protocolo va por stdout";

/// El transporte elegido en la línea de órdenes.
#[derive(Debug, PartialEq, Eq)]
enum Transporte {
    /// Entrada y salida estándar: el defecto.
    Stdio,
    /// `Streamable HTTP` en la dirección indicada.
    Http(String),
}

/// Los argumentos ya interpretados.
#[derive(Debug, Default)]
struct Argumentos {
    ayuda: bool,
    http: bool,
    bind: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    iniciar_registro();

    match arrancar().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // El motivo se publica por stderr: stdout podría ser el protocolo.
            eprintln!("kesto-mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Monta el registro, siempre por stderr.
///
/// Un `RUST_LOG` que no se entiende no impide arrancar: se cae al nivel por defecto. Es la
/// misma regla que el resto de Kesto sigue con sus avisos —no tumbar un servicio por algo
/// que solo afecta a lo que se lee—, y aquí importa más que allí, porque el servidor tiene
/// que hablar el protocolo aunque nadie mire los registros.
fn iniciar_registro() {
    let filtro = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filtro)
        // El color solo cuando alguien mira el registro: sin esto, un registro
        // redirigido a un fichero o leído por una herramienta queda lleno de secuencias
        // de escape que hay que limpiar antes de poder leerlo.
        .with_ansi(std::io::stderr().is_terminal())
        .with_writer(std::io::stderr)
        .init();
}

/// Arranca el servidor con el transporte elegido y espera a que termine.
async fn arrancar() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // `args_os` y no `args`: `args` entra en pánico con un argumento que no sea texto
    // UTF-8, y un pánico al arrancar deja al cliente MCP con un proceso muerto y sin
    // motivo. Aquí un argumento ilegible es un error contado, no un volcado.
    let argumentos: Vec<OsString> = std::env::args_os().skip(1).collect();

    let Some(transporte) = interpretar(&argumentos)? else {
        println!("{AYUDA}");
        return Ok(());
    };

    let config = Config::from_env()?;
    let cliente = ApiClient::new(&config)?;
    let servidor = KestoMcp::new(cliente);

    match transporte {
        Transporte::Stdio => {
            tracing::info!("kesto-mcp hablando por stdio");
            let corriendo = servidor
                .serve(stdio())
                .await
                .map_err(|error| std::io::Error::other(format!("stdio: {error:?}")))?;
            corriendo
                .waiting()
                .await
                .map_err(|error| std::io::Error::other(format!("stdio: {error:?}")))?;
        }
        Transporte::Http(direccion) => {
            let cierre = CancellationToken::new();
            let entorno = |nombre: &str| std::env::var(nombre).ok();
            let router = http::router(
                servidor,
                http::configuracion(cierre.clone(), Config::allowed_hosts(&entorno)),
            );
            let escucha = tokio::net::TcpListener::bind(&direccion)
                .await
                .map_err(|error| {
                    std::io::Error::other(format!("no se pudo escuchar en `{direccion}`: {error}"))
                })?;
            // Se anuncia la dirección **resuelta** y no la pedida: con `--bind 127.0.0.1:0`
            // el puerto lo elige el sistema y el que se pidió no dice nada. Es además lo
            // que permite a las pruebas encontrar al servidor sin adivinar el puerto.
            let escuchando = escucha
                .local_addr()
                .map_err(|error| std::io::Error::other(format!("sin dirección local: {error}")))?;

            tracing::info!(
                "kesto-mcp escuchando en http://{escuchando}{} (Streamable HTTP)",
                http::CAMINO
            );

            axum::serve(escucha, router)
                .with_graceful_shutdown(async move {
                    if let Err(error) = tokio::signal::ctrl_c().await {
                        tracing::warn!("no se pudo esperar la señal de cierre: {error}");
                    }
                    cierre.cancel();
                })
                .await
                .map_err(|error| std::io::Error::other(format!("HTTP: {error}")))?;
        }
    }

    Ok(())
}

/// Interpreta los argumentos.
///
/// Devuelve `Ok(None)` cuando lo que se pidió es la ayuda: no es un fallo ni un arranque.
fn interpretar(argumentos: &[OsString]) -> Result<Option<Transporte>, ErrorDeArgumentos> {
    let mut leidos = Argumentos::default();
    let mut pendientes = argumentos.iter();

    while let Some(argumento) = pendientes.next() {
        match texto(argumento)?.as_str() {
            "-h" | "--help" => leidos.ayuda = true,
            "--transport" => {
                let valor = pendientes.next().ok_or_else(|| {
                    ErrorDeArgumentos::de("`--transport` necesita un valor: `stdio` o `http`")
                })?;

                match texto(valor)?.as_str() {
                    "http" => leidos.http = true,
                    "stdio" => leidos.http = false,
                    otro => {
                        return Err(ErrorDeArgumentos::de(format!(
                            "transporte desconocido `{otro}`: usa `stdio` o `http`"
                        )));
                    }
                }
            }
            "--bind" => {
                let direccion = pendientes
                    .next()
                    .ok_or_else(|| ErrorDeArgumentos::de("`--bind` necesita una dirección"))?;

                leidos.bind = Some(texto(direccion)?);
            }
            otro => {
                return Err(ErrorDeArgumentos::de(format!(
                    "argumento desconocido `{otro}`: prueba con `--help`"
                )));
            }
        }
    }

    if leidos.ayuda {
        return Ok(None);
    }

    let entorno = |nombre: &str| std::env::var(nombre).ok();

    if leidos.http {
        let direccion = leidos
            .bind
            .unwrap_or_else(|| Config::bind_address(&entorno));

        return Ok(Some(Transporte::Http(direccion)));
    }

    // `--bind` sin transporte HTTP no configura nada: se avisa en vez de ignorarlo en
    // silencio, que es la misma regla que sigue la API con sus parámetros de consulta.
    if leidos.bind.is_some() {
        return Err(ErrorDeArgumentos::de(format!(
            "`--bind` solo vale con `--transport http` (o con `{KESTO_MCP_BIND_ADDRESS}`)"
        )));
    }

    Ok(Some(Transporte::Stdio))
}

/// El texto de un argumento, o el error que dice que no es texto.
///
/// El sistema no promete que sus argumentos sean UTF-8 —un nombre de fichero o una ruta
/// pueden llevar cualquier byte—, así que el argumento ilegible se cuenta en vez de
/// reventar. El mensaje nombra el argumento, que es lo único que permite ver cuál era.
fn texto(argumento: &OsString) -> Result<String, ErrorDeArgumentos> {
    argumento.to_str().map(str::to_owned).ok_or_else(|| {
        ErrorDeArgumentos::de(format!(
            "argumento ilegible, no es texto UTF-8: {argumento:?}"
        ))
    })
}

/// Un argumento que no se puede atender.
///
/// Es un tipo y no un `String` suelto para que el mensaje tenga un único constructor: así
/// ningún sitio del binario puede inventarse un error sin mensaje.
#[derive(Debug)]
struct ErrorDeArgumentos {
    mensaje: String,
}

impl ErrorDeArgumentos {
    /// Un error con su motivo.
    fn de(mensaje: impl Into<String>) -> Self {
        Self {
            mensaje: mensaje.into(),
        }
    }
}

impl std::fmt::Display for ErrorDeArgumentos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.mensaje)
    }
}

impl std::error::Error for ErrorDeArgumentos {}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};

    use super::{Transporte, interpretar};

    /// Los argumentos como los entrega el sistema: sin exigirles que sean texto.
    fn argumentos(valores: &[&str]) -> Vec<OsString> {
        valores
            .iter()
            .map(OsStr::new)
            .map(OsStr::to_owned)
            .collect()
    }

    #[test]
    fn sin_argumentos_el_transporte_es_stdio() {
        let leidos = interpretar(&argumentos(&[])).expect("sin argumentos es válido");

        assert_eq!(leidos, Some(Transporte::Stdio));
    }

    #[test]
    fn el_transporte_http_necesita_su_direccion() {
        assert_eq!(
            interpretar(&argumentos(&[
                "--transport",
                "http",
                "--bind",
                "0.0.0.0:9000"
            ]))
            .expect("es válido"),
            Some(Transporte::Http("0.0.0.0:9000".to_owned()))
        );
    }

    #[test]
    fn un_transporte_desconocido_se_rechaza() {
        let error =
            interpretar(&argumentos(&["--transport", "grpc"])).expect_err("no es un transporte");

        assert!(error.to_string().contains("grpc"));
    }

    #[test]
    fn la_ayuda_no_arranca_nada() {
        assert_eq!(
            interpretar(&argumentos(&["--help"])).expect("es válido"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn un_argumento_ilegible_no_paniquea_y_lo_dice() {
        use std::os::unix::ffi::OsStringExt;

        let argumento = OsString::from_vec(vec![b'-', b'-', 0xff, 0xfe]);
        let error = interpretar(&[argumento]).expect_err("no es un argumento legible");

        assert!(
            error.to_string().contains("UTF-8"),
            "el error tiene que decir por qué no vale: {error}"
        );
    }

    #[test]
    fn un_bind_sin_http_se_rechaza() {
        let error = interpretar(&argumentos(&["--bind", "127.0.0.1:9000"]))
            .expect_err("`--bind` solo vale con HTTP");

        assert!(error.to_string().contains("--bind"));
    }
}
