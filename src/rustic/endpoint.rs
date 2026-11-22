//! Qué `endpoint` de S3 es admisible.
//!
//! El `endpoint` lo escribe el operador y Kesto lo usa para **conectar**: es el único campo
//! de una fila de repositorio que decide a dónde va el proceso. Sin comprobación, quien ya
//! tiene una sesión de operador puede apuntarlo al loopback del propio Kesto —su API, su
//! base, el socket del daemon— o a la dirección de metadatos de la instancia, donde viven
//! las credenciales de la máquina.
//!
//! Lo que cierra es el valor **literal**: el esquema, las credenciales embebidas —que acabarían
//! en la fila y en los registros—, el loopback —con el nombre `localhost`—, la dirección de
//! enlace local, la de metadatos y la no especificada. El permiso para destinos inseguros solo
//! rebaja el esquema y el loopback; el resto no se rebaja nunca.
//!
//! Lo que este módulo **no** hace es resolver el nombre: un `endpoint` con nombre de host se
//! acepta por su forma, así que un nombre que resuelva a una dirección de enlace local no se
//! detecta. Cerrarlo exigiría resolver en el momento del alta y **fijar** la dirección usada
//! en la conexión —entre la comprobación y la conexión cabe un cambio de DNS—, y el cliente
//! S3 no ofrece ese punto de enganche. Queda declarado como limitación.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Por qué no se admite un `endpoint`.
///
/// Se distinguen los motivos porque el operador arregla cada uno de una manera: un esquema
/// `http` se arregla poniendo `https` —o declarando el permiso—, una dirección de metadatos
/// no se arregla declarando nada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motivo {
    /// El valor no es una URL absoluta con esquema y host.
    NoEsUnaUrlAbsoluta,
    /// El esquema no es `https` ni, con el permiso declarado, `http`.
    EsquemaNoAdmitido,
    /// El host es una dirección de enlace local (`169.254.0.0/16`, `fe80::/10`).
    EnlaceLocal,
    /// El host es la dirección de metadatos de la instancia.
    Metadatos,
    /// El host es la dirección no especificada (`0.0.0.0`, `::`).
    NoEspecificada,
    /// La URL lleva credenciales embebidas.
    CredencialesEmbebidas,
    /// El host es el loopback y no hay permiso para destinos inseguros.
    Loopback,
    /// El host es un nombre local y no hay permiso para destinos inseguros.
    NombreLocal,
}

impl std::fmt::Display for Motivo {
    fn fmt(&self, formateador: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let texto = match self {
            Self::NoEsUnaUrlAbsoluta => "no es una URL absoluta con esquema y host",
            Self::EsquemaNoAdmitido => "el esquema tiene que ser `https`",
            Self::EnlaceLocal => "el host es una dirección de enlace local",
            Self::Metadatos => "el host es una dirección de metadatos de la instancia",
            Self::NoEspecificada => "el host es la dirección no especificada",
            Self::CredencialesEmbebidas => "la URL no puede llevar credenciales embebidas",
            Self::Loopback => "el host es el loopback y no hay permiso para destinos inseguros",
            Self::NombreLocal => "el nombre es local y no hay permiso para destinos inseguros",
        };

        formateador.write_str(texto)
    }
}

/// El `endpoint` de una fila de repositorio no se puede admitir.
///
/// El mensaje **no** repite la URL recibida: la escribió el operador, puede llevar
/// credenciales embebidas y acabaría en un registro o en el cuerpo de una respuesta.
#[derive(Debug, thiserror::Error)]
#[error("el campo `endpoint` no es admisible: {motivo}")]
pub struct ErrorDeEndpoint {
    /// Qué hace inadmisible el valor.
    pub motivo: Motivo,
}

/// Comprueba que un `endpoint` se puede usar.
///
/// El permiso `permitir_inseguro` rebaja **dos** cosas a la vez: el esquema (`http` se admite)
/// y el loopback —con su nombre `localhost`—. Son el mismo caso, «el destino está en mi propia
/// máquina y en claro», que es lo que hacen el desarrollo local y la batería. Lo que **no**
/// rebaja nunca es la dirección de enlace local, la de metadatos ni la no especificada: no hay
/// laboratorio que necesite alcanzar el servicio de metadatos, y dejarlas fuera mantiene
/// cerrado el agujero más caro incluso con el permiso puesto.
///
/// # Errors
///
/// Devuelve [`ErrorDeEndpoint`] nombrando el motivo cuando el valor no es admisible.
pub fn validar(valor: &str, permitir_inseguro: bool) -> Result<(), ErrorDeEndpoint> {
    let url = Url::parse(valor.trim()).map_err(|_| rechazo(Motivo::NoEsUnaUrlAbsoluta))?;

    // Las credenciales embebidas se rechazan **siempre**, también con el permiso declarado:
    // acabarían en la fila y en los registros, y el destino no las necesita —la credencial del
    // repositorio es un campo aparte—.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(rechazo(Motivo::CredencialesEmbebidas));
    }

    match url.scheme() {
        "https" => {}
        "http" if permitir_inseguro => {}
        _ => return Err(rechazo(Motivo::EsquemaNoAdmitido)),
    }

    let Some(host) = url.host() else {
        return Err(rechazo(Motivo::NoEsUnaUrlAbsoluta));
    };

    match host {
        Host::Ipv4(direccion) => validar_direccion(IpAddr::V4(direccion), permitir_inseguro),
        Host::Ipv6(direccion) => validar_direccion(IpAddr::V6(direccion), permitir_inseguro),
        Host::Domain(nombre) => validar_nombre(nombre, permitir_inseguro),
    }
}

/// Construye el rechazo de un motivo.
fn rechazo(motivo: Motivo) -> ErrorDeEndpoint {
    ErrorDeEndpoint { motivo }
}

/// Desmapea la IPv4 embebida en una IPv6, si la lleva.
///
/// Una IPv6 puede **contener** una IPv4, y en Linux esa dirección conecta a la IPv4 real: sin
/// desmapearla, `is_loopback()`, `is_unspecified()` y la comprobación de enlace local miran la
/// IPv6 —donde las tres dan `false`— y `https://[::ffff:127.0.0.1]:9000/` se saltaría justo la
/// regla que este módulo existe para hacer cumplir. Se juzga, por tanto, la dirección que de
/// verdad se va a alcanzar.
///
/// Cubre las dos formas que enrutan a una IPv4: la **IPv4-mapeada** `::ffff:a.b.c.d` (RFC 4291),
/// que es la que usa el doble pila, y el prefijo **NAT64** bien conocido `64:ff9b::/96` (RFC
/// 6052), donde el traductor vuelve a poner esa IPv4 en el destino. La forma **IPv4-compatible**
/// `::a.b.c.d` (RFC 4291, obsoleta y sin ruta en Linux) se deja sin desmapear a propósito: no
/// conecta a ninguna IPv4 y tratarla como tal confundiría `::1` con `0.0.0.1`.
fn ip_sin_envolver(ip: IpAddr) -> IpAddr {
    let IpAddr::V6(direccion) = ip else {
        return ip;
    };

    if let Some(mapeada) = direccion.to_ipv4_mapped() {
        return IpAddr::V4(mapeada);
    }

    // `64:ff9b::/96`: los seis primeros segmentos son el prefijo; los dos últimos, la IPv4.
    let segmentos = direccion.segments();
    if segmentos[..6] == [0x0064, 0xff9b, 0, 0, 0, 0] {
        let embebida = u32::from(segmentos[6]) << 16 | u32::from(segmentos[7]);
        return IpAddr::V4(Ipv4Addr::from(embebida));
    }

    ip
}

/// Comprueba una dirección literal.
///
/// El orden de las comprobaciones es el orden en que se explica el rechazo: primero lo que no
/// se rebaja nunca, después lo que sí.
fn validar_direccion(ip: IpAddr, permitir_inseguro: bool) -> Result<(), ErrorDeEndpoint> {
    // Se juzga la dirección que de verdad se va a alcanzar: una IPv6 puede llevar dentro una
    // IPv4, y las comprobaciones de abajo no la verían.
    let ip = ip_sin_envolver(ip);

    if es_de_metadatos(ip) {
        return Err(rechazo(Motivo::Metadatos));
    }

    if es_de_enlace_local(ip) {
        return Err(rechazo(Motivo::EnlaceLocal));
    }

    if ip.is_unspecified() {
        return Err(rechazo(Motivo::NoEspecificada));
    }

    // El loopback es lo que el permiso rebaja: apuntar al propio Kesto —su API, su base, el
    // socket del daemon— es lo que un despliegue no puede permitirse y lo que un laboratorio
    // local hace a diario.
    if ip.is_loopback() && !permitir_inseguro {
        return Err(rechazo(Motivo::Loopback));
    }

    Ok(())
}

/// Comprueba un nombre de host.
///
/// El nombre **no** se resuelve: se acepta por su forma. Un nombre que resuelva a una dirección
/// de enlace local no se detecta, y es la limitación declarada del módulo.
fn validar_nombre(nombre: &str, permitir_inseguro: bool) -> Result<(), ErrorDeEndpoint> {
    let nombre = nombre.trim_end_matches('.').to_ascii_lowercase();

    let es_local = nombre == "localhost" || nombre.ends_with(".localhost");

    if es_local && !permitir_inseguro {
        return Err(rechazo(Motivo::NombreLocal));
    }

    Ok(())
}

/// ¿Es una dirección de enlace local (`169.254.0.0/16`, `fe80::/10`)?
fn es_de_enlace_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(direccion) => direccion.is_link_local(),
        // `fe80::/10`: los diez bits de más peso del primer segmento.
        IpAddr::V6(direccion) => (direccion.segments()[0] & 0xffc0) == 0xfe80,
    }
}

/// ¿Es una dirección de metadatos de la instancia?
///
/// La de IPv4 vive dentro del enlace local, pero se nombra aparte para que el motivo diga lo
/// que es: es la dirección donde el proveedor de la máquina deja sus credenciales.
fn es_de_metadatos(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(direccion) => direccion == Ipv4Addr::new(169, 254, 169, 254),
        // `fd00:ec2::254`, la dirección de metadatos IPv6 de AWS.
        IpAddr::V6(direccion) => direccion == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254),
    }
}

#[cfg(test)]
mod tests {
    use super::{Motivo, validar};

    /// Un `endpoint` público por `https`: el caso bueno.
    const PUBLICO: &str = "https://minio.example.com/";

    /// El motivo con el que se rechaza un valor, o un fallo de la prueba si se aceptó.
    fn motivo(valor: &str, permitir_inseguro: bool) -> Motivo {
        validar(valor, permitir_inseguro)
            .expect_err(&format!("`{valor}` tenía que rechazarse"))
            .motivo
    }

    #[test]
    fn un_endpoint_por_https_se_acepta() {
        assert!(
            validar(PUBLICO, false).is_ok(),
            "un host público por https se admite"
        );
        assert!(
            validar("https://10.0.0.5:9000/", false).is_ok(),
            "el rango privado se admite: es el caso del S3 autoalojado, el uso principal"
        );
        assert!(
            validar("https://192.168.1.10:9000/", false).is_ok(),
            "el rango privado se admite"
        );
        assert!(
            validar("https://[fc00::1]:9000/", false).is_ok(),
            "el rango privado IPv6 también se admite"
        );
    }

    #[test]
    fn el_esquema_http_se_rechaza_salvo_con_permiso() {
        assert_eq!(
            motivo("http://minio.example.com/", false),
            Motivo::EsquemaNoAdmitido,
            "sin permiso, el esquema tiene que ser https"
        );
        assert!(
            validar("http://minio.example.com/", true).is_ok(),
            "con el permiso declarado, el esquema http se admite"
        );
    }

    #[test]
    fn el_loopback_se_rechaza_salvo_con_permiso() {
        assert_eq!(
            motivo("https://127.0.0.1:9000/", false),
            Motivo::Loopback,
            "el loopback IPv4 se rechaza sin permiso"
        );
        assert_eq!(
            motivo("https://[::1]:9000/", false),
            Motivo::Loopback,
            "el loopback IPv6 se rechaza sin permiso"
        );
        assert_eq!(
            motivo("https://localhost:9000/", false),
            Motivo::NombreLocal,
            "el nombre localhost se rechaza sin permiso"
        );
        assert_eq!(
            motivo("https://algo.localhost/", false),
            Motivo::NombreLocal,
            "cualquier nombre bajo localhost se rechaza sin permiso"
        );

        for valor in [
            "https://127.0.0.1:9000/",
            "https://[::1]:9000/",
            "https://localhost:9000/",
            "https://algo.localhost/",
        ] {
            assert!(
                validar(valor, true).is_ok(),
                "`{valor}` tiene que admitirse con el permiso declarado"
            );
        }
    }

    #[test]
    fn lo_que_nunca_se_admite_se_rechaza_tambien_con_permiso() {
        // El permiso rebaja el esquema y el loopback; la dirección de enlace local, la de
        // metadatos y la no especificada no se rebajan nunca: no hay laboratorio que
        // necesite alcanzar el servicio de metadatos.
        for valor in [
            "https://169.254.169.254/latest/meta-data/",
            "https://[fe80::1]/",
            "https://[fd00:ec2::254]/",
            "https://0.0.0.0/",
            "https://[::]/",
        ] {
            assert!(
                validar(valor, true).is_err(),
                "`{valor}` no puede admitirse ni con el permiso declarado"
            );
        }

        assert_eq!(
            motivo("https://169.254.169.254/", true),
            Motivo::Metadatos,
            "la dirección de metadatos de la nube se nombra por lo que es"
        );
        assert_eq!(
            motivo("https://[fe80::1]/", true),
            Motivo::EnlaceLocal,
            "el enlace local IPv6 se rechaza siempre"
        );
        assert_eq!(
            motivo("https://[fd00:ec2::254]/", true),
            Motivo::Metadatos,
            "la dirección de metadatos IPv6 de AWS se rechaza siempre"
        );
        assert_eq!(
            motivo("https://0.0.0.0/", true),
            Motivo::NoEspecificada,
            "la dirección no especificada IPv4 se rechaza siempre"
        );
        assert_eq!(
            motivo("https://[::]/", true),
            Motivo::NoEspecificada,
            "la dirección no especificada IPv6 se rechaza siempre"
        );
    }

    #[test]
    fn una_ipv4_embebida_en_una_ipv6_se_juzga_por_la_ipv4() {
        // El agujero que cierra esta prueba: una IPv6 con una IPv4 dentro conecta a esa IPv4 en
        // Linux, así que `[::ffff:127.0.0.1]` tiene que caer en la misma regla que `127.0.0.1`.
        assert_eq!(
            motivo("https://[::ffff:127.0.0.1]:9000/", false),
            Motivo::Loopback,
            "la IPv4-mapeada del loopback se rechaza sin permiso"
        );
        assert_eq!(
            motivo("https://[0:0:0:0:0:ffff:7f00:1]:9000/", false),
            Motivo::Loopback,
            "la forma larga de la IPv4-mapeada del loopback se rechaza igual"
        );
        assert_eq!(
            motivo("https://[::ffff:169.254.169.254]/", true),
            Motivo::Metadatos,
            "la dirección de metadatos embebida no se rebaja ni con permiso"
        );
        assert_eq!(
            motivo("https://[::ffff:0.0.0.0]/", true),
            Motivo::NoEspecificada,
            "la no especificada embebida no se rebaja ni con permiso"
        );

        // NAT64: `64:ff9b::/96` con la IPv4 en los dos últimos segmentos.
        assert_eq!(
            motivo("https://[64:ff9b::7f00:1]:9000/", false),
            Motivo::Loopback,
            "el loopback tras NAT64 se rechaza sin permiso"
        );
        assert_eq!(
            motivo("https://[64:ff9b::a9fe:a9fe]/", true),
            Motivo::Metadatos,
            "la de metadatos tras NAT64 no se rebaja ni con permiso"
        );

        // Y el permiso sigue rebajando la IPv4-mapeada del loopback, como rebaja `127.0.0.1`.
        assert!(
            validar("https://[::ffff:127.0.0.1]:9000/", true).is_ok(),
            "con el permiso declarado, el loopback embebido se admite como el directo"
        );
    }

    #[test]
    fn lo_que_no_es_una_url_absoluta_o_lleva_credenciales_se_rechaza() {
        assert_eq!(
            motivo("/var/lib/minio", false),
            Motivo::NoEsUnaUrlAbsoluta,
            "una ruta del sistema de ficheros no es una URL"
        );
        assert_eq!(
            motivo("minio.example.com", false),
            Motivo::NoEsUnaUrlAbsoluta,
            "un host suelto no es una URL absoluta"
        );
        assert_eq!(
            motivo("", false),
            Motivo::NoEsUnaUrlAbsoluta,
            "el valor vacío no es una URL"
        );
        assert_eq!(
            motivo("ftp://minio.example.com/", false),
            Motivo::EsquemaNoAdmitido,
            "un esquema que no es http ni https se rechaza"
        );
        assert_eq!(
            motivo("file:///var/lib/minio", false),
            Motivo::EsquemaNoAdmitido,
            "el esquema file se rechaza"
        );
        assert_eq!(
            motivo("https://usuario:clave@minio.example.com/", false),
            Motivo::CredencialesEmbebidas,
            "las credenciales embebidas acabarían en la fila y en los registros"
        );
        assert_eq!(
            motivo("https://usuario:clave@minio.example.com/", true),
            Motivo::CredencialesEmbebidas,
            "el permiso no rebaja las credenciales embebidas"
        );
    }

    #[test]
    fn el_mensaje_nombra_el_campo_y_no_repite_la_url() {
        let error = validar("http://minio.example.com/", false)
            .expect_err("el esquema http sin permiso tiene que rechazarse");
        let mensaje = error.to_string();

        assert!(
            mensaje.contains("endpoint"),
            "el mensaje tiene que nombrar el campo: {mensaje}"
        );
        assert!(
            !mensaje.contains("minio.example.com"),
            "el mensaje no puede repetir la URL recibida: {mensaje}"
        );
    }
}
