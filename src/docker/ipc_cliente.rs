//! Cliente de la API para hablar con `kesto-motor` por el socket IPC.
//!
//! Es la única pieza del proceso de la API que conoce la ruta del canal; no hay
//! aquí ni un solo tipo del daemon de contenedores. La etiqueta de fuente de
//! copia y el código de salida los comprueba el motor antes de contestar.
//!
//! La lectura la hace una tarea dedicada que empuja respuestas **completas** por
//! un canal. Un `select!` que cancele `read_exact` a mitad de un mensaje dejaría
//! el socket desincronizado; aquí solo se cancelan recepciones de canal, que no
//! pierden bytes.

use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;

use crate::ipc::{
    CanalIpc, ClaseFallo, Error as ErrorIpc, OrdenIpc, RespuestaMotor, SolicitudMotor, conectar,
    enviar, recibir,
};

use super::{Canal, Error, Orden, Trozo};

/// Tamaño de cada trozo de `stdin` que viaja hacia el motor.
const TROZO_DE_ENTRADA: usize = 8 * 1024;

/// Cliente del motor de copias, en el proceso sin privilegios.
pub struct Cliente {
    /// Ruta del socket IPC compartido con el motor.
    socket: String,
    /// Tiempo máximo de conexión (el drenaje lo acota la inactividad).
    timeout: Duration,
}

impl Cliente {
    /// Construye el cliente; no abre el socket hasta la primera operación.
    ///
    /// # Errors
    ///
    /// Ninguno: la comprobación del socket se hace al usar el cliente, igual
    /// que en el transporte que sustituye.
    pub fn conectar(socket: &str, timeout: Duration) -> Result<Self, Error> {
        Ok(Self {
            socket: socket.to_owned(),
            timeout,
        })
    }

    /// Comprueba que el motor contesta.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::MotorNoDisponible`] si el motor no responde.
    pub async fn comprobar(&self) -> Result<(), Error> {
        let mut canal = CanalCliente::abrir(&self.socket).await?;
        canal.enviar(&SolicitudMotor::Comprobar).await?;
        match canal.esperar(Duration::ZERO).await? {
            RespuestaMotor::Aceptado => Ok(()),
            RespuestaMotor::Fallo {
                clase,
                mensaje,
                status_code,
            } => Err(self.del_fallo(clase, &mensaje, status_code, None, false)),
            _ => Err(self.del_protocolo()),
        }
    }

    /// Detiene un contenedor en marcha antes de un backup.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::MotorNoDisponible`] si el motor no responde, o el fallo
    /// del daemon si no puede detener el contenedor.
    pub async fn stop_container(&self, contenedor: &str) -> Result<(), Error> {
        let mut canal = CanalCliente::abrir(&self.socket).await?;
        canal
            .enviar(&SolicitudMotor::DetenerContenedor {
                nombre: contenedor.to_owned(),
            })
            .await?;
        match canal.esperar(self.timeout).await? {
            RespuestaMotor::Aceptado => Ok(()),
            RespuestaMotor::Fallo {
                clase,
                mensaje,
                status_code,
            } => Err(self.del_fallo(clase, &mensaje, status_code, Some(contenedor), false)),
            _ => Err(self.del_protocolo()),
        }
    }

    /// Arranca un contenedor previamente detenido.
    ///
    /// # Errors
    ///
    /// Devuelve [`Error::MotorNoDisponible`] si el motor no responde, o el fallo
    /// del daemon si no puede arrancar el contenedor.
    pub async fn start_container(&self, contenedor: &str) -> Result<(), Error> {
        let mut canal = CanalCliente::abrir(&self.socket).await?;
        canal
            .enviar(&SolicitudMotor::IniciarContenedor {
                nombre: contenedor.to_owned(),
            })
            .await?;
        match canal.esperar(self.timeout).await? {
            RespuestaMotor::Aceptado => Ok(()),
            RespuestaMotor::Fallo {
                clase,
                mensaje,
                status_code,
            } => Err(self.del_fallo(clase, &mensaje, status_code, Some(contenedor), false)),
            _ => Err(self.del_protocolo()),
        }
    }

    /// Ejecuta una orden y entrega cada trozo de salida según llega.
    ///
    /// # Errors
    ///
    /// Los errores del transporte, y [`Error::SinCodigoDeSalida`] si el motor
    /// no pudo determinar cómo acabó la orden.
    pub async fn ejecutar(
        &self,
        contenedor: &str,
        orden: &Orden,
        inactividad_maxima: Duration,
        mut al_llegar: impl FnMut(Trozo),
    ) -> Result<i32, Error> {
        let mut canal = CanalCliente::abrir(&self.socket).await?;
        self.crear_exec(&mut canal, contenedor, orden, false)
            .await?;
        canal
            .enviar(&SolicitudMotor::IniciarExec {
                id: String::new(),
                entrada: None,
            })
            .await?;
        self.drenar(&mut canal, contenedor, inactividad_maxima, &mut al_llegar)
            .await
    }

    /// Ejecuta una orden alimentando su `stdin` con `lector`, sin ficheros temporales.
    ///
    /// # Errors
    ///
    /// [`Error::EntradaNoEntregada`] si el volcado no se pudo entregar entero,
    /// además de los errores de [`Self::ejecutar`].
    pub async fn ejecutar_con_entrada(
        &self,
        contenedor: &str,
        orden: &Orden,
        inactividad_maxima: Duration,
        mut lector: impl AsyncRead + Unpin,
        mut al_llegar: impl FnMut(Trozo),
    ) -> Result<i32, Error> {
        let mut canal = CanalCliente::abrir(&self.socket).await?;
        self.crear_exec(&mut canal, contenedor, orden, true).await?;

        // La entrega y la recepción corren a la vez: serializarlas bloquearía,
        // porque el proceso espera stdin mientras nadie lee su salida. El canal
        // de respuestas se lee en `lector.esperar`, que es cancel-safe: un
        // timeout nunca descarta bytes ya recibidos.
        let CanalCliente {
            escritura,
            lector: respuestas,
            ..
        } = &mut canal;
        let entrega = async {
            let mut buffer = vec![0_u8; TROZO_DE_ENTRADA];
            loop {
                let leidos =
                    lector
                        .read(&mut buffer)
                        .await
                        .map_err(|causa| ErrorIpc::NoDisponible {
                            socket: self.socket.clone(),
                            causa,
                        })?;
                if leidos == 0 {
                    break;
                }
                let trozo = buffer[..leidos].to_vec();
                enviar(
                    escritura,
                    &SolicitudMotor::IniciarExec {
                        id: String::new(),
                        entrada: Some(trozo),
                    },
                )
                .await?;
            }
            enviar(
                escritura,
                &SolicitudMotor::IniciarExec {
                    id: String::new(),
                    entrada: None,
                },
            )
            .await
        };
        tokio::pin!(entrega);

        let mut estado_de_entrega: Option<Result<(), ErrorIpc>> = None;
        let desenlace = loop {
            tokio::select! {
                respuesta = respuestas.esperar(&self.socket, inactividad_maxima) => {
                    match respuesta {
                        Ok(RespuestaMotor::Trozo { canal, bytes }) => al_llegar(Trozo {
                            canal: if canal == CanalIpc::Salida { Canal::Salida } else { Canal::Error },
                            bytes,
                        }),
                        Ok(RespuestaMotor::Terminado { codigo }) => break Ok(codigo),
                        Ok(RespuestaMotor::Fallo { clase, mensaje, status_code }) => {
                            break Err(self.del_fallo(
                                clase, &mensaje, status_code, Some(contenedor), true,
                            ));
                        }
                        Ok(_) => break Err(self.del_protocolo()),
                        Err(error) => break Err(error),
                    }
                }
                resultado = &mut entrega, if estado_de_entrega.is_none() => {
                    estado_de_entrega = Some(resultado);
                }
            }
        };

        let codigo = desenlace?;
        // Si la orden terminó antes de que el volcado se entregara por completo,
        // no damos por buena una restauración a medias.
        let entrega_final = match estado_de_entrega {
            Some(resultado) => resultado,
            None => match tokio::time::timeout(inactividad_maxima, &mut entrega).await {
                Ok(resultado) => resultado,
                Err(_) => Err(ErrorIpc::CanalCerrado),
            },
        };
        match entrega_final {
            Ok(()) if codigo >= 0 => Ok(codigo),
            Ok(()) => Err(Error::SinCodigoDeSalida {
                contenedor: contenedor.to_owned(),
            }),
            Err(causa) => Err(Error::EntradaNoEntregada {
                contenedor: contenedor.to_owned(),
                causa: io::Error::other(causa.to_string()),
            }),
        }
    }

    /// Crea el `exec` y espera su identificador antes de cualquier otra cosa.
    async fn crear_exec(
        &self,
        canal: &mut CanalCliente,
        contenedor: &str,
        orden: &Orden,
        con_entrada: bool,
    ) -> Result<(), Error> {
        canal
            .enviar(&SolicitudMotor::CrearExec {
                contenedor: contenedor.to_owned(),
                orden: OrdenIpc {
                    cmd: orden.cmd.clone(),
                    env: orden.env.clone(),
                },
                con_entrada,
            })
            .await?;

        match canal.esperar(self.timeout).await? {
            RespuestaMotor::ExecCreado { .. } => Ok(()),
            RespuestaMotor::Fallo {
                clase,
                mensaje,
                status_code,
            } => Err(self.del_fallo(clase, &mensaje, status_code, Some(contenedor), false)),
            _ => Err(self.del_protocolo()),
        }
    }

    /// Drena las respuestas del `exec` hasta su código de salida.
    async fn drenar(
        &self,
        canal: &mut CanalCliente,
        contenedor: &str,
        inactividad: Duration,
        al_llegar: &mut impl FnMut(Trozo),
    ) -> Result<i32, Error> {
        loop {
            match canal.esperar(inactividad).await? {
                RespuestaMotor::Trozo { canal, bytes } => al_llegar(Trozo {
                    canal: if canal == CanalIpc::Salida {
                        Canal::Salida
                    } else {
                        Canal::Error
                    },
                    bytes,
                }),
                RespuestaMotor::Terminado { codigo } => return Ok(codigo),
                RespuestaMotor::Fallo {
                    clase,
                    mensaje,
                    status_code,
                } => {
                    return Err(self.del_fallo(
                        clase,
                        &mensaje,
                        status_code,
                        Some(contenedor),
                        false,
                    ));
                }
                _ => return Err(self.del_protocolo()),
            }
        }
    }

    fn del_protocolo(&self) -> Error {
        del_protocolo_de(&self.socket)
    }

    fn del_fallo(
        &self,
        clase: ClaseFallo,
        mensaje: &str,
        status_code: Option<u16>,
        contenedor: Option<&str>,
        con_entrada: bool,
    ) -> Error {
        let causa = ErrorIpc::DelMotor {
            clase,
            mensaje: mensaje.to_owned(),
            status_code,
        };
        match clase {
            ClaseFallo::Socket => Error::SocketInexistente {
                socket: self.socket.clone(),
            },
            ClaseFallo::Etiqueta => Error::ContenedorNoEsFuenteDeCopia {
                contenedor: contenedor.unwrap_or("").to_owned(),
            },
            ClaseFallo::Contenedor => match status_code {
                Some(status_code) => Error::MotorContesta {
                    contenedor: contenedor.unwrap_or("").to_owned(),
                    status_code,
                    message: mensaje.to_owned(),
                },
                None => Error::MotorNoContesta {
                    contenedor: contenedor.unwrap_or("").to_owned(),
                    causa,
                },
            },
            ClaseFallo::Inactividad => {
                let inactividad = Duration::from_secs(1);
                let contenedor = contenedor.unwrap_or("").to_owned();
                if con_entrada {
                    Error::RestauracionEstancada {
                        contenedor,
                        inactividad,
                    }
                } else {
                    Error::ExtraccionEstancada {
                        contenedor,
                        inactividad,
                    }
                }
            }
            ClaseFallo::Entrada => Error::EntradaNoEntregada {
                contenedor: contenedor.unwrap_or("").to_owned(),
                causa: io::Error::other(mensaje.to_owned()),
            },
            ClaseFallo::Codigo => Error::SinCodigoDeSalida {
                contenedor: contenedor.unwrap_or("").to_owned(),
            },
            ClaseFallo::Motor | ClaseFallo::Protocolo => Error::MotorNoDisponible {
                socket: self.socket.clone(),
                causa,
            },
        }
    }
}

/// Conexión con el motor: mitad de escritura y lector en su propia tarea.
struct CanalCliente {
    /// Ruta del socket, para traducir los errores.
    socket: String,
    /// Mitad de escritura del canal.
    escritura: OwnedWriteHalf,
    /// Receptor de respuestas completas.
    lector: ReceptorRespuestas,
}

impl CanalCliente {
    /// Abre el socket y arranca el lector antes de enviar nada.
    async fn abrir(socket: &str) -> Result<Self, Error> {
        let canal = conectar(socket)
            .await
            .map_err(|causa| del_socket_de(socket, causa))?;
        let (lectura, escritura) = canal.into_split();
        Ok(Self {
            socket: socket.to_owned(),
            escritura,
            lector: ReceptorRespuestas::nuevo(lectura),
        })
    }

    /// Envía una solicitud completa con el framing del contrato.
    async fn enviar(&mut self, solicitud: &SolicitudMotor) -> Result<(), Error> {
        enviar(&mut self.escritura, solicitud)
            .await
            .map_err(|causa| del_socket_de(&self.socket, causa))
    }

    /// Espera una respuesta, con inactividad opcional.
    async fn esperar(&mut self, inactividad: Duration) -> Result<RespuestaMotor, Error> {
        self.lector.esperar(&self.socket, inactividad).await
    }
}

/// Lee el socket en una tarea que nunca se cancela a mitad de un mensaje.
struct ReceptorRespuestas {
    /// Respuestas ya decodificadas, o el error que cerró el canal.
    respuestas: mpsc::Receiver<Result<RespuestaMotor, ErrorIpc>>,
}

impl ReceptorRespuestas {
    /// Arranca la tarea de lectura y devuelve el extremo receptor.
    fn nuevo(lectura: OwnedReadHalf) -> Self {
        let (emisor, respuestas) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut lectura = lectura;
            loop {
                let respuesta = recibir(&mut lectura).await;
                let termino = respuesta.is_err();
                if emisor.send(respuesta).await.is_err() || termino {
                    break;
                }
            }
        });
        Self { respuestas }
    }

    /// Recibe la respuesta siguiente; un timeout no pierde lo ya leído.
    async fn esperar(
        &mut self,
        socket: &str,
        inactividad: Duration,
    ) -> Result<RespuestaMotor, Error> {
        let recepcion = self.respuestas.recv();
        let recibido = if inactividad.is_zero() {
            recepcion.await
        } else {
            match tokio::time::timeout(inactividad, recepcion).await {
                Ok(recibido) => recibido,
                Err(_) => {
                    return Err(Error::ExtraccionEstancada {
                        contenedor: String::new(),
                        inactividad,
                    });
                }
            }
        };
        match recibido {
            Some(Ok(respuesta)) => Ok(respuesta),
            Some(Err(causa)) => Err(del_socket_de(socket, causa)),
            None => Err(del_socket_de(socket, ErrorIpc::CanalCerrado)),
        }
    }
}

fn del_socket_de(socket: &str, causa: ErrorIpc) -> Error {
    match causa {
        ErrorIpc::SocketInexistente { socket } => Error::SocketInexistente { socket },
        causa => Error::MotorNoDisponible {
            socket: socket.to_owned(),
            causa,
        },
    }
}

fn del_protocolo_de(socket: &str) -> Error {
    Error::MotorNoDisponible {
        socket: socket.to_owned(),
        causa: ErrorIpc::DelMotor {
            clase: ClaseFallo::Protocolo,
            mensaje: String::from("respuesta fuera del contrato IPC"),
            status_code: None,
        },
    }
}

impl std::fmt::Debug for Cliente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cliente")
            .field("socket", &self.socket)
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[cfg(test)]
mod pruebas {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;
    use tokio::sync::oneshot;

    use super::*;
    use crate::ipc::escribir;

    static CONTADOR: AtomicUsize = AtomicUsize::new(0);

    struct SocketDePrueba(PathBuf);

    impl SocketDePrueba {
        fn nuevo() -> Self {
            let ruta = std::env::temp_dir().join(format!(
                "kesto-ipc-cliente-{}-{}.sock",
                std::process::id(),
                CONTADOR.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_file(&ruta);
            Self(ruta)
        }
    }

    impl Drop for SocketDePrueba {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    async fn responder_prueba(canal: &mut tokio::net::UnixStream, respuesta: &RespuestaMotor) {
        let cuerpo = serde_json::to_vec(respuesta).expect("serializa la respuesta de prueba");
        escribir(canal, &cuerpo)
            .await
            .expect("escribe la respuesta de prueba");
    }

    fn orden_de_prueba() -> Orden {
        Orden {
            cmd: vec![String::from("sqlite3"), String::from("-readonly")],
            env: vec![String::from("MYSQL_PWD=secreto-de-la-prueba")],
        }
    }

    #[tokio::test]
    async fn el_cliente_ipc_envia_la_orden_y_drena_el_exec_contra_un_servidor_falso() {
        let ruta = SocketDePrueba::nuevo();
        let escucha = UnixListener::bind(&ruta.0).expect("se puede crear el socket de prueba");
        let servidor = tokio::spawn(async move {
            let (mut canal, _) = escucha.accept().await.expect("acepta la conexión");
            let solicitud = crate::ipc::recibir_solicitud(&mut canal)
                .await
                .expect("lee la solicitud");
            match solicitud {
                SolicitudMotor::CrearExec { orden, .. } => {
                    assert_eq!(
                        orden,
                        OrdenIpc {
                            cmd: vec![String::from("sqlite3"), String::from("-readonly")],
                            env: vec![String::from("MYSQL_PWD=secreto-de-la-prueba")],
                        }
                    );
                }
                _ => panic!("primero tiene que llegar CrearExec"),
            }
            responder_prueba(
                &mut canal,
                &RespuestaMotor::ExecCreado {
                    id: String::from("exec-1"),
                },
            )
            .await;

            let solicitud = crate::ipc::recibir_solicitud(&mut canal)
                .await
                .expect("lee el inicio");
            assert!(matches!(
                solicitud,
                SolicitudMotor::IniciarExec { entrada: None, .. }
            ));
            responder_prueba(
                &mut canal,
                &RespuestaMotor::Trozo {
                    canal: CanalIpc::Salida,
                    bytes: b"volcado".to_vec(),
                },
            )
            .await;
            responder_prueba(
                &mut canal,
                &RespuestaMotor::Trozo {
                    canal: CanalIpc::Error,
                    bytes: b"aviso".to_vec(),
                },
            )
            .await;
            responder_prueba(&mut canal, &RespuestaMotor::Terminado { codigo: 0 }).await;
        });

        let cliente = Cliente::conectar(&ruta.0.to_string_lossy(), Duration::from_secs(1))
            .expect("el cliente se construye sin conectar");
        let mut trozos = Vec::new();
        let codigo = cliente
            .ejecutar(
                "contenedor",
                &orden_de_prueba(),
                Duration::from_secs(1),
                |trozo| trozos.push((trozo.canal, trozo.bytes)),
            )
            .await
            .expect("el cliente drena la respuesta");

        assert_eq!(codigo, 0);
        assert_eq!(
            trozos,
            vec![
                (Canal::Salida, b"volcado".to_vec()),
                (Canal::Error, b"aviso".to_vec()),
            ]
        );
        servidor.await.expect("el servidor termina");
    }

    #[tokio::test]
    async fn una_respuesta_partial_no_descarta_la_siguiente() {
        let ruta = SocketDePrueba::nuevo();
        let escucha = UnixListener::bind(&ruta.0).expect("se puede crear el socket de prueba");
        let servidor = tokio::spawn(async move {
            let (mut canal, _) = escucha.accept().await.expect("acepta la conexión");
            let _ = crate::ipc::recibir_solicitud(&mut canal)
                .await
                .expect("lee CrearExec");
            responder_prueba(
                &mut canal,
                &RespuestaMotor::ExecCreado {
                    id: String::from("exec-2"),
                },
            )
            .await;
            let _ = crate::ipc::recibir_solicitud(&mut canal)
                .await
                .expect("lee IniciarExec");

            // Un cuerpo partido en dos escrituras: la biblioteca de framing
            // tiene que esperar al resto en vez de dar el mensaje por perdido.
            let cuerpo = serde_json::to_vec(&RespuestaMotor::Trozo {
                canal: CanalIpc::Salida,
                bytes: b"abc".to_vec(),
            })
            .expect("serializa");
            let mut mensaje = (cuerpo.len() as u32).to_be_bytes().to_vec();
            mensaje.extend_from_slice(&cuerpo);
            canal.write_all(&mensaje[..5]).await.expect("cabecera");
            tokio::time::sleep(Duration::from_millis(20)).await;
            canal
                .write_all(&mensaje[5..])
                .await
                .expect("resto del cuerpo");
            responder_prueba(&mut canal, &RespuestaMotor::Terminado { codigo: 4 }).await;
        });

        let cliente = Cliente::conectar(&ruta.0.to_string_lossy(), Duration::from_secs(1))
            .expect("el cliente se construye sin conectar");
        let mut trozos = Vec::new();
        let codigo = cliente
            .ejecutar(
                "contenedor",
                &orden_de_prueba(),
                Duration::from_secs(1),
                |trozo| trozos.push(trozo.bytes),
            )
            .await
            .expect("el mensaje partido se entrega entero");

        assert_eq!(codigo, 4);
        assert_eq!(trozos, vec![b"abc".to_vec()]);
        servidor.await.expect("el servidor termina");
    }

    #[tokio::test]
    async fn un_timeout_no_desincroniza_la_lectura_del_socket() {
        let ruta = SocketDePrueba::nuevo();
        let escucha = UnixListener::bind(&ruta.0).expect("se puede crear el socket de prueba");
        let (señal, espera) = oneshot::channel::<()>();
        let servidor = tokio::spawn(async move {
            let (mut canal, _) = escucha.accept().await.expect("acepta la conexión");
            for _ in 0..2 {
                let _ = crate::ipc::recibir_solicitud(&mut canal)
                    .await
                    .expect("lee la solicitud");
            }
            responder_prueba(&mut canal, &RespuestaMotor::Aceptado).await;
            espera.await.expect("el cliente ya venció el timeout");
            responder_prueba(&mut canal, &RespuestaMotor::Aceptado).await;
        });

        let cliente = Cliente::conectar(&ruta.0.to_string_lossy(), Duration::from_millis(40))
            .expect("el cliente se construye sin conectar");
        let mut canal = CanalCliente::abrir(&cliente.socket)
            .await
            .expect("abre el canal");
        canal
            .enviar(&SolicitudMotor::Comprobar)
            .await
            .expect("envía el primero");
        canal
            .enviar(&SolicitudMotor::Comprobar)
            .await
            .expect("envía el segundo");
        assert!(matches!(
            canal.esperar(Duration::from_millis(40)).await,
            Ok(RespuestaMotor::Aceptado)
        ));
        assert!(
            canal.esperar(Duration::from_millis(40)).await.is_err(),
            "el segundo mensaje aún no llegó"
        );
        señal.send(()).expect("el servidor sigue vivo");
        assert!(matches!(
            canal
                .esperar(Duration::from_secs(1))
                .await
                .expect("la respuesta tardía se entrega"),
            RespuestaMotor::Aceptado
        ));
        servidor.await.expect("el servidor termina");
    }
}
