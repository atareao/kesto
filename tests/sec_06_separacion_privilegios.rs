//! SEC-06 — fase RED: contratos verificables de la separación de privilegios.
//!
//! Las pruebas de este fichero son de **origen y manifiesto** por una razón
//! concreta: el binario `kesto-motor` no existe hoy, así que referirse a él con
//! `env!("CARGO_BIN_EXE_kesto-motor")` o a `kesto::ipc::SolicitudMotor` haría
//! que el fichero no compilara en RED. Aquí se comprueban los contratos que la
//! feature tiene que dejar visibles en `Cargo.toml`, `compose.yaml` y `src/`,
//! con mensajes que nombran qué `SHALL` falta. Todas compilan hoy y fallen hoy;
//! ninguna lleva `#[ignore]`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const RAIZ: &str = env!("CARGO_MANIFEST_DIR");

/// Devuelve el texto de un fichero, o vacío si no existe: la aserción que sigue
/// es la que nombra el contrato ausente, no un error de E/S críptico.
fn texto(ruta: &Path) -> String {
    fs::read_to_string(ruta).unwrap_or_default()
}

/// Bloques de un manifiesto con una cabecera dada (puede haber varios `[[bin]]`).
fn bloques_toml(texto: &str, cabecera: &str) -> Vec<String> {
    let lineas: Vec<&str> = texto.lines().collect();
    let mut bloques = Vec::new();
    for (i, linea) in lineas.iter().enumerate() {
        if linea.trim() != cabecera {
            continue;
        }
        let mut bloque = Vec::new();
        for siguiente in lineas.iter().skip(i + 1) {
            let recortada = siguiente.trim();
            if recortada.starts_with('[') && recortada.ends_with(']') {
                break;
            }
            bloque.push(*siguiente);
        }
        bloques.push(bloque.join("\n"));
    }
    bloques
}

/// Cuerpo de un servicio de `compose.yaml` indentado con dos espacios.
fn seccion_yaml(texto: &str, nombre: &str) -> Option<String> {
    let cabecera = format!("  {nombre}:");
    let mut dentro = false;
    let mut cuerpo = Vec::new();
    for linea in texto.lines() {
        if !dentro {
            if linea == cabecera {
                dentro = true;
            }
            continue;
        }
        let util = !linea.trim().is_empty() && !linea.trim_start().starts_with('#');
        if util && !linea.starts_with("    ") {
            break;
        }
        cuerpo.push(linea);
    }
    dentro.then(|| cuerpo.join("\n"))
}

/// Líneas con datos: sin comentarios, que suelen describir el contrato viejo.
fn solo_datos(seccion: &str) -> String {
    seccion
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Volúmenes nombrados (no montajes de host) declarados por un servicio.
fn volumenes_nombrados(seccion: &str) -> BTreeSet<String> {
    solo_datos(seccion)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .filter_map(|entrada| entrada.split(':').next())
        .filter(|origen| !origen.starts_with('/') && !origen.starts_with('.'))
        .map(str::to_owned)
        .collect()
}

/// Código de producción: sin comentarios y sin los módulos `#[cfg(test)]`.
fn codigo_produccion(texto: &str) -> String {
    let mut salida = Vec::new();
    let mut profundidad = 0_i32;
    let mut omitiendo = false;
    for linea in texto.lines() {
        let recortada = linea.trim();
        if !omitiendo && recortada.starts_with("#[cfg(test)]") {
            omitiendo = true;
            profundidad = 0;
            continue;
        }
        if omitiendo {
            for c in linea.chars() {
                match c {
                    '{' => profundidad += 1,
                    '}' => profundidad -= 1,
                    _ => {}
                }
            }
            if profundidad <= 0 {
                omitiendo = false;
            }
            continue;
        }
        if recortada.starts_with("//") {
            continue;
        }
        salida.push(linea.split("//").next().unwrap_or(linea).to_string());
    }
    salida.join("\n")
}

fn archivos_rs(dir: &Path, salida: &mut Vec<PathBuf>) {
    if let Ok(entradas) = fs::read_dir(dir) {
        for entrada in entradas.flatten() {
            let ruta = entrada.path();
            if ruta.is_dir() {
                archivos_rs(&ruta, salida);
            } else if ruta.extension().and_then(|e| e.to_str()) == Some("rs") {
                salida.push(ruta);
            }
        }
    }
}

/// Fuentes de producción (el arnés de pruebas lee `DOCKER_HOST` legítimamente).
fn fuentes_produccion() -> Vec<PathBuf> {
    let mut fuentes = Vec::new();
    archivos_rs(&Path::new(RAIZ).join("src"), &mut fuentes);
    fuentes.retain(|p| !p.to_string_lossy().contains("test_support"));
    fuentes.sort();
    fuentes
}

/// Cuerpo de una declaración `enum Nombre { … }`.
fn cuerpo_enum(texto: &str, nombre: &str) -> Option<String> {
    let inicio = texto.find(&format!("enum {nombre}"))?;
    let apertura = inicio + texto[inicio..].find('{')?;
    let mut profundidad = 0_i32;
    for (desplazamiento, c) in texto[apertura..].char_indices() {
        match c {
            '{' => profundidad += 1,
            '}' => {
                profundidad -= 1;
                if profundidad == 0 {
                    return Some(texto[apertura + 1..apertura + desplazamiento].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Variantes de primer nivel de un cuerpo de enum, sin campos anidados.
fn variantes_de_primer_nivel(cuerpo: &str) -> BTreeSet<String> {
    let mut variantes = BTreeSet::new();
    let mut profundidad = 0_i32;
    for linea in cuerpo.lines() {
        let util = linea.split("//").next().unwrap_or("").trim();
        if profundidad == 0 && !util.is_empty() && !util.starts_with('#') && !util.starts_with('/')
        {
            let id: String = util
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if id.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                variantes.insert(id);
            }
        }
        for c in linea.chars() {
            match c {
                '{' => profundidad += 1,
                '}' => profundidad -= 1,
                _ => {}
            }
        }
    }
    variantes
}

#[test]
fn cargo_declara_el_binario_kesto_motor_tras_la_feature_motor() {
    let manifiesto = texto(&Path::new(RAIZ).join("Cargo.toml"));
    let bin_motor = bloques_toml(&manifiesto, "[[bin]]")
        .into_iter()
        .find(|b| b.lines().any(|l| l.trim() == "name = \"kesto-motor\""));

    assert!(
        bin_motor.is_some(),
        "SEC-06 falta el contrato de proceso: Cargo.toml no declara un [[bin]] `kesto-motor`"
    );
    let bin_motor = bin_motor.unwrap_or_default();
    assert!(
        bin_motor
            .lines()
            .any(|l| l.contains("required-features") && l.contains("\"motor\"")),
        "SEC-06 `kesto-motor` no declara `required-features = [\"motor\"]`: la API podría enlazar el código del daemon"
    );
}

#[test]
fn bollard_es_opcional_y_solo_se_activa_con_la_feature_motor() {
    let manifiesto = texto(&Path::new(RAIZ).join("Cargo.toml"));
    let dependencias = bloques_toml(&manifiesto, "[dependencies]");
    let opcional_en_linea = dependencias.iter().any(|b| {
        b.lines().any(|l| {
            let l = l.trim();
            l.starts_with("bollard") && l.contains("optional") && l.contains("true")
        })
    });
    let opcional_en_tabla = bloques_toml(&manifiesto, "[dependencies.bollard]")
        .iter()
        .any(|b| b.lines().any(|l| l.replace(' ', "") == "optional=true"));

    assert!(
        opcional_en_linea || opcional_en_tabla,
        "SEC-06 `bollard` sigue siendo obligatoria: debe declararse `optional = true` para que la API no enlace el cliente Docker"
    );

    let motor = bloques_toml(&manifiesto, "[features]")
        .into_iter()
        .next()
        .unwrap_or_default();
    assert!(
        motor
            .lines()
            .any(|l| l.trim_start().starts_with("motor") && l.contains("bollard")),
        "SEC-06 no existe la feature `motor` que active `dep:bollard`"
    );
}

#[test]
fn compose_no_entrega_el_socket_del_daemon_a_la_api() {
    let compose = texto(&Path::new(RAIZ).join("compose.yaml"));
    let api = seccion_yaml(&compose, "kesto").unwrap_or_default();
    assert!(
        !api.is_empty(),
        "SEC-06 falta el servicio `kesto` en compose.yaml: no se puede comprobar el aislamiento de la API"
    );
    let datos = solo_datos(&api);
    assert!(
        !datos.contains("/var/run/docker.sock"),
        "SEC-06 el servicio `kesto` (kesto-app) monta `/var/run/docker.sock`: un fallo en la API entregaría el daemon"
    );
    assert!(
        !datos.contains("DOCKER_HOST"),
        "SEC-06 el servicio `kesto` declara `DOCKER_HOST`: la API no debe conocer el socket del daemon"
    );
}

#[test]
fn compose_monta_el_socket_solo_en_el_motor_y_comparte_la_ipc() {
    let compose = texto(&Path::new(RAIZ).join("compose.yaml"));
    let api = seccion_yaml(&compose, "kesto").unwrap_or_default();
    let motor = seccion_yaml(&compose, "kesto-motor").unwrap_or_default();
    assert!(
        !motor.is_empty(),
        "SEC-06 compose.yaml no declara el servicio `kesto-motor`: el socket del daemon sigue en el proceso de la API"
    );

    let datos_motor = solo_datos(&motor);
    assert!(
        datos_motor.contains("/var/run/docker.sock:/var/run/docker.sock:ro"),
        "SEC-06 `kesto-motor` no monta `/var/run/docker.sock` en solo lectura"
    );
    assert!(
        datos_motor
            .lines()
            .any(|l| l.trim_start().starts_with("user:") && !l.contains("root")),
        "SEC-06 `kesto-motor` no declara un usuario no root"
    );
    assert!(
        datos_motor.contains("KESTO_API_UID"),
        "SEC-06 `kesto-motor` no declara `KESTO_API_UID` para verificar al par IPC"
    );
    assert!(
        datos_motor.contains("KESTO_MOTOR_SOCKET")
            && solo_datos(&api).contains("KESTO_MOTOR_SOCKET"),
        "SEC-06 API y motor no comparten la ruta del socket IPC `KESTO_MOTOR_SOCKET`"
    );

    let compartidos: BTreeSet<_> = volumenes_nombrados(&motor)
        .intersection(&volumenes_nombrados(&api))
        .cloned()
        .collect();
    assert!(
        !compartidos.is_empty(),
        "SEC-06 API y motor no comparten ningún volumen para el socket IPC (defecto `/run/kesto/motor.sock`)"
    );
}

#[test]
fn la_configuracion_expone_la_ipc_y_exige_el_uid_en_el_motor() {
    let fuentes = fuentes_produccion();
    let codigo: Vec<_> = fuentes
        .iter()
        .map(|p| (p, codigo_produccion(&texto(p))))
        .collect();

    assert!(
        codigo
            .iter()
            .any(|(_, c)| c.contains("\"KESTO_MOTOR_SOCKET\"")),
        "SEC-06 la configuración no expone `KESTO_MOTOR_SOCKET`"
    );
    assert!(
        codigo
            .iter()
            .any(|(_, c)| c.contains("/run/kesto/motor.sock")),
        "SEC-06 falta el defecto `/run/kesto/motor.sock` del canal IPC"
    );

    let con_uid: Vec<_> = codigo
        .iter()
        .filter(|(p, c)| {
            c.contains("\"KESTO_API_UID\"")
                && (p.to_string_lossy().contains("motor") || p.ends_with("main.rs"))
        })
        .collect();
    assert!(
        !con_uid.is_empty(),
        "SEC-06 el motor no lee `KESTO_API_UID`: no puede identificar al par por UID"
    );
    assert!(
        con_uid.iter().any(|(_, c)| {
            c.contains("return Err")
                || c.contains("Err(")
                || c.contains("ok_or")
                || c.contains("required")
        }),
        "SEC-06 `KESTO_API_UID` no es obligatoria en el motor: falta una ruta de error cuando no está"
    );
}

#[test]
fn la_api_no_lee_docker_host() {
    let sensibles = ["src/config.rs", "src/main.rs"];
    for relativo in sensibles {
        let ruta = Path::new(RAIZ).join(relativo);
        let codigo = codigo_produccion(&texto(&ruta));
        assert!(
            !codigo.contains("\"DOCKER_HOST\"") && !codigo.contains("ENV_DOCKER_HOST"),
            "SEC-06 `{relativo}` todavía lee `DOCKER_HOST`: la API no debe construir el cliente Docker"
        );
    }
}

#[test]
fn la_lista_blanca_ipc_tiene_exactamente_seis_operaciones() {
    let esperadas = BTreeSet::from([
        "Comprobar".to_owned(),
        "CrearContenedor".to_owned(),
        "IniciarContenedor".to_owned(),
        "CrearExec".to_owned(),
        "InspeccionarExec".to_owned(),
        "IniciarExec".to_owned(),
    ]);

    let mut cuerpo_encontrado = None;
    for ruta in fuentes_produccion() {
        if let Some(cuerpo) = cuerpo_enum(&texto(&ruta), "SolicitudMotor") {
            cuerpo_encontrado = Some(cuerpo);
            break;
        }
    }
    let cuerpo = cuerpo_encontrado.unwrap_or_default();
    assert!(
        !cuerpo.is_empty(),
        "SEC-06 no existe `enum SolicitudMotor`: no hay lista blanca cerrada de operaciones del daemon"
    );
    assert_eq!(
        variantes_de_primer_nivel(&cuerpo),
        esperadas,
        "SEC-06 la lista blanca IPC no contiene exactamente las seis operaciones permitidas"
    );
}

#[test]
fn el_framing_ipc_rechaza_longitudes_invalidas_y_mayores_a_1_mib_sin_panico() {
    let codigo: String = fuentes_produccion()
        .iter()
        .filter(|p| {
            let t = texto(p);
            t.contains("SolicitudMotor") || t.contains("RespuestaMotor")
        })
        .map(|p| codigo_produccion(&texto(p)))
        .collect();

    assert!(
        !codigo.is_empty(),
        "SEC-06 no hay tipos `SolicitudMotor`/`RespuestaMotor`: no existe el contrato IPC con framing"
    );
    assert!(
        codigo.contains("from_be_bytes") || codigo.contains("read_u32"),
        "SEC-06 el framing IPC no lee una longitud `u32` big-endian"
    );
    assert!(
        codigo.contains("1048576")
            || codigo.contains("1_048_576")
            || codigo.replace(' ', "").contains("1024*1024")
            || codigo.contains("1 << 20"),
        "SEC-06 el framing IPC no aplica el tope de 1 MiB al cuerpo del mensaje"
    );
    assert!(
        codigo.contains("Err(") || codigo.contains("return Err"),
        "SEC-06 una longitud inválida o fuera de tope no se descarta con un error tipado"
    );
    assert!(
        !codigo.contains(".unwrap()") && !codigo.contains(".expect(") && !codigo.contains("panic!"),
        "SEC-06 el camino de lectura del framing puede entrar en pánico con una longitud hostil"
    );
}

// ── Pruebas de comportamiento del protocolo, no de texto ──────────────────
// El contrato se incluye por ruta: es el código exacto que compilan la API y
// el motor, no una copia del Test. Las órdenes viajan por un `UnixStream`
// real con el framing `u32` big-endian del producción.

#[allow(dead_code)]
#[path = "../src/ipc/mensaje.rs"]
mod ipc_contrato;

use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;

static SOCKETS: AtomicUsize = AtomicUsize::new(0);

fn ruta_socket_unica() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kesto-sec06-{}-{}.sock",
        std::process::id(),
        SOCKETS.fetch_add(1, Ordering::Relaxed)
    ))
}

#[tokio::test]
async fn una_longitud_cero_hostil_o_mayor_al_tope_falla_sin_panico() {
    for longitud in [0_u32, (ipc_contrato::TOPE_CUERPO + 1) as u32, u32::MAX] {
        let (mut escritor, mut lector) = UnixStream::pair().expect("par de sockets");
        let lectura = tokio::spawn(async move { ipc_contrato::leer(&mut lector).await });
        escritor
            .write_all(&longitud.to_be_bytes())
            .await
            .expect("escribe la cabecera hostil");

        let desenlace = tokio::time::timeout(std::time::Duration::from_secs(1), lectura).await;
        let resultado = desenlace
            .expect("la lectura no se cuelga")
            .expect("la tarea no panica");
        assert!(
            matches!(resultado, Err(ipc_contrato::Error::LongitudInvalida { longitud: recibida }) if recibida == longitud),
            "la longitud {longitud} no se descartó con un error tipado: {resultado:?}"
        );
    }
}

#[test]
fn el_debug_de_la_orden_ipc_redacta_las_credenciales_del_entorno() {
    let orden = ipc_contrato::OrdenIpc {
        cmd: vec![String::from("psql")],
        env: vec![String::from("PGPASSWORD=clave-secreta")],
    };
    const SECRETO: &str = "clave-secreta";
    let impreso = format!("{orden:?}");
    assert!(
        impreso.contains("PGPASSWORD"),
        "el Debug debe seguir siendo útil"
    );
    assert!(
        !impreso.contains(SECRETO),
        "el Debug vertió la credencial: {impreso}"
    );
}

#[tokio::test]
async fn las_seis_operaciones_de_la_lista_blanca_cruzan_el_canal_y_las_demas_no() {
    let cuerpo = br#"[
        "Comprobar",
        {"CrearContenedor":{"nombre":"c","imagen":"i","etiquetas":[]}},
        {"IniciarContenedor":{"nombre":"c"}},
        {"CrearExec":{"contenedor":"c","orden":{"cmd":["sh"],"env":[]},"con_entrada":false}},
        {"InspeccionarExec":{"id":"e"}},
        {"IniciarExec":{"id":"e","entrada":null}}
    ]"#;
    let solicitudes: Vec<ipc_contrato::SolicitudMotor> =
        serde_json::from_slice(cuerpo).expect("las seis operaciones son del contrato");
    assert_eq!(solicitudes.len(), 6);

    let esperadas = solicitudes.clone();
    let ruta = ruta_socket_unica();
    let escucha = tokio::net::UnixListener::bind(&ruta).expect("se crea el socket falso");
    let servidor = tokio::spawn(async move {
        let (mut canal, _) = escucha.accept().await.expect("acepta al cliente");
        let mut recibidas = 0;
        while recibidas < 6 {
            let cuerpo = ipc_contrato::leer(&mut canal)
                .await
                .expect("lee la solicitud");
            let solicitud: ipc_contrato::SolicitudMotor =
                serde_json::from_slice(&cuerpo).expect("operación de la lista blanca");
            assert_eq!(solicitud, esperadas[recibidas]);
            recibidas += 1;
        }
        recibidas
    });

    let mut canal = UnixStream::connect(&ruta)
        .await
        .expect("conecta con el servidor falso");
    for solicitud in &solicitudes {
        let cuerpo = serde_json::to_vec(solicitud).expect("serializa la solicitud");
        ipc_contrato::escribir(&mut canal, &cuerpo)
            .await
            .expect("escribe la solicitud");
    }
    assert_eq!(servidor.await.expect("el servidor no panica"), 6);

    assert!(
        serde_json::from_slice::<ipc_contrato::SolicitudMotor>(br#"{"BorrarContenedor":{}}"#)
            .is_err(),
        "una operación fuera de la lista blanca fue aceptada por el contrato"
    );
    let _ = std::fs::remove_file(ruta);
}

#[test]
fn cargo_tree_comprueba_que_la_api_no_enlaza_bollard_y_el_motor_si() {
    let salida = std::process::Command::new("cargo")
        .args(["tree", "-e", "normal", "-p", "kesto"])
        .output()
        .expect("cargo tree se ejecuta");
    assert!(salida.status.success(), "cargo tree falló");
    let arbol = String::from_utf8_lossy(&salida.stdout);
    assert!(
        !arbol.lines().any(|linea| linea.contains("bollard")),
        "el binario de la API sigue enlazando bollard: {arbol}"
    );

    let salida = std::process::Command::new("cargo")
        .args(["tree", "-e", "normal", "-p", "kesto", "--features", "motor"])
        .output()
        .expect("cargo tree del motor se ejecuta");
    assert!(salida.status.success(), "cargo tree --features motor falló");
    let arbol = String::from_utf8_lossy(&salida.stdout);
    assert!(
        arbol.lines().any(|linea| linea.contains("bollard")),
        "el motor no enlaza bollard: el IPC no podría hablar con el daemon"
    );
}

#[tokio::test]
async fn una_respuesta_recibida_a_medias_no_se_descarta_al_cerrar_el_canal() {
    let (mut escritor, mut lector) = UnixStream::pair().expect("par de sockets");
    let lectura = tokio::spawn(async move {
        let primera = ipc_contrato::leer(&mut lector).await.expect("primera");
        let segunda = ipc_contrato::leer(&mut lector).await.expect("segunda");
        (primera, segunda)
    });
    let cuerpo = serde_json::to_vec(&ipc_contrato::RespuestaMotor::Aceptado).expect("serializa");
    let mut mensaje = (cuerpo.len() as u32).to_be_bytes().to_vec();
    mensaje.extend_from_slice(&cuerpo);
    let (primera, segunda) = mensaje.split_at(5);
    escritor.write_all(primera).await.expect("cabecera");
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    escritor.write_all(segunda).await.expect("cuerpo");
    escritor.write_all(&mensaje).await.expect("segundo mensaje");

    let (recibida, otra) = tokio::time::timeout(std::time::Duration::from_secs(1), lectura)
        .await
        .expect("no se cuelga")
        .expect("no panica");
    assert_eq!(recibida, cuerpo);
    assert_eq!(otra, cuerpo);
}
