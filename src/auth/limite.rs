//! Limitador de tasa de la ruta de entrada: ventana fija por IP.
//!
//! `GET /api/v1/auth/login` es pública y cada petición escribe en memoria del
//! proceso, así que sin límite un caudal alto y no autenticado hace crecer el mapa
//! de logins sin más freno que la caducidad. El límite se hace **por IP** y no
//! global: uno global convertiría el propio límite en una denegación de servicio
//! —un atacante lo agota y deja fuera a los usuarios legítimos—, mientras que por
//! IP el daño se acota a quien abusa.
//!
//! La ventana es **fija** y no un cubo de fichas: se explica en una frase y se
//! prueba con un reloj inyectado, sin dormir. Aquí no se busca suavizar el caudal
//! sino **acotarlo**.
//!
//! El reloj entra por parámetro —[`Instant`]— en vez de leerse dentro: es lo que
//! permite probar la renovación de la ventana sin esperar un minuto de verdad.
//!
//! La IP sale del socket de la conexión y nunca de una cabecera, y donde no hay
//! socket —las pruebas despachan el router con `oneshot`— se cuenta bajo la
//! centinela: sin ella el limitador solo sería ejercitable abriendo un puerto.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

/// Peticiones de login que se atienden por IP dentro de la ventana.
///
/// Un humano hace unos pocos logins por minuto; veinte dejan sitio a un reintento y
/// a una pestaña duplicada, y cortan en seco un caudal automatizado.
pub const MAX_PETICIONES_DE_LOGIN: u32 = 20;

/// Duración de la ventana del límite de login.
pub const VENTANA_DE_LOGIN: Duration = Duration::from_secs(60);

/// Tope de ventanas vivas en el mapa.
///
/// El limitador también es memoria del proceso: sin tope, un caudal desde muchas
/// direcciones de origen haría crecer su mapa sin límite y el propio limitador
/// sería el sumidero.
pub const MAX_VENTANAS: usize = 1024;

/// IP con la que se cuentan las peticiones cuando la conexión no trae socket.
///
/// Las pruebas despachan el router con `oneshot` y no abren socket, así que no hay
/// información de conexión; con una IP centinela compartida el limitador sigue
/// siendo ejercitable, y todas esas peticiones caen en el mismo cubo.
pub const IP_CENTINELA: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Una ventana de conteo de una IP: cuándo empezó y cuántas peticiones lleva.
#[derive(Debug, Clone, Copy)]
struct Ventana {
    /// Instante en que se abrió la ventana.
    inicio: Instant,
    /// Peticiones atendidas dentro de la ventana.
    peticiones: u32,
}

/// Limitador de tasa de ventana fija por IP.
#[derive(Debug, Default)]
pub struct LimitadorDeLogin {
    /// Ventanas vivas, una por IP vista dentro de la ventana en curso.
    ventanas: HashMap<IpAddr, Ventana>,
}

impl LimitadorDeLogin {
    /// Construye un limitador sin ninguna ventana abierta.
    pub fn new() -> Self {
        Self::default()
    }

    /// ¿Se atiende esta petición de `ip` en `ahora`?
    ///
    /// La ventana es fija: la primera petición de una IP abre la suya y las siguientes
    /// cuentan dentro; pasada [`VENTANA_DE_LOGIN`] desde su apertura, la ventana se
    /// considera vencida y la siguiente petición abre una nueva con el cupo entero. Se
    /// purgan **todas** las vencidas —no solo la de esta IP— antes de contar, así que el
    /// mapa no arrastra lo que ya nadie va a consultar.
    ///
    /// Devuelve `false` cuando la IP ya ha agotado [`MAX_PETICIONES_DE_LOGIN`] dentro de su
    /// ventana, y en ese caso **no** cuenta la petición rechazada: si la contara, un caudal
    /// sostenido mantendría la ventana agotada indefinidamente.
    pub fn permitir(&mut self, ip: IpAddr, ahora: Instant) -> bool {
        // Primero la purga: el tope solo entra si, aun sin las vencidas, siguen sobrando
        // ventanas vivas. Recorre el mapa entero en cada petición, pero el coste está acotado
        // por `MAX_VENTANAS` y el mapa es pequeño: no hace falta purgar por umbral ni llevar
        // un registro aparte de vencimientos.
        self.ventanas.retain(|_, ventana| {
            ahora.saturating_duration_since(ventana.inicio) < VENTANA_DE_LOGIN
        });

        if let Some(ventana) = self.ventanas.get_mut(&ip) {
            if ventana.peticiones >= MAX_PETICIONES_DE_LOGIN {
                return false;
            }

            ventana.peticiones = ventana.peticiones.saturating_add(1);

            return true;
        }

        // El tope del mapa es el último recurso y no rechaza la petición: se desaloja la
        // ventana más antigua, que es la que menos valor tiene —está más cerca de vencer—.
        // Rechazar la petición nueva convertiría el tope en una denegación de servicio.
        if self.ventanas.len() >= MAX_VENTANAS
            && let Some(mas_antigua) = self
                .ventanas
                .iter()
                .min_by_key(|(_, ventana)| ventana.inicio)
                .map(|(clave, _)| *clave)
        {
            self.ventanas.remove(&mas_antigua);
        }

        self.ventanas.insert(
            ip,
            Ventana {
                inicio: ahora,
                peticiones: 1,
            },
        );

        true
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant};

    use super::{LimitadorDeLogin, MAX_PETICIONES_DE_LOGIN, MAX_VENTANAS, VENTANA_DE_LOGIN};

    /// IP de origen de las pruebas: una dirección de documentación.
    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7));

    #[test]
    fn el_cupo_se_agota_y_la_siguiente_no_pasa() {
        let mut limitador = LimitadorDeLogin::new();
        let ahora = Instant::now();

        for intento in 0..MAX_PETICIONES_DE_LOGIN {
            assert!(
                limitador.permitir(IP, ahora),
                "la petición {intento} está dentro del cupo y tiene que pasar"
            );
        }

        assert!(
            !limitador.permitir(IP, ahora),
            "agotado el cupo, la siguiente petición de la misma IP no puede pasar"
        );
    }

    #[test]
    fn pasada_la_ventana_el_cupo_se_renueva() {
        let mut limitador = LimitadorDeLogin::new();
        let ahora = Instant::now();

        for _ in 0..MAX_PETICIONES_DE_LOGIN {
            assert!(limitador.permitir(IP, ahora));
        }
        assert!(
            !limitador.permitir(IP, ahora),
            "el cupo tiene que estar agotado antes de avanzar el reloj"
        );

        let despues = ahora + VENTANA_DE_LOGIN;

        assert!(
            limitador.permitir(IP, despues),
            "pasada la ventana, la misma IP recupera su cupo"
        );
    }

    #[test]
    fn una_ip_distinta_no_consume_el_cupo_de_la_primera() {
        let mut limitador = LimitadorDeLogin::new();
        let ahora = Instant::now();
        let otra = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 8));

        for _ in 0..MAX_PETICIONES_DE_LOGIN {
            assert!(limitador.permitir(IP, ahora));
        }
        assert!(
            !limitador.permitir(IP, ahora),
            "el cupo de la primera tiene que estar agotado"
        );

        assert!(
            limitador.permitir(otra, ahora),
            "una IP distinta tiene su propio cupo: el límite es por IP y no global"
        );
    }

    #[test]
    fn el_mapa_de_ventanas_se_poda_y_no_crece_sin_limite() {
        let mut limitador = LimitadorDeLogin::new();
        let ahora = Instant::now();

        // Muchas IPs distintas dentro de la misma ventana: el mapa no puede crecer
        // con cada una, o el propio limitador sería el sumidero.
        let ips: Vec<IpAddr> = (0..MAX_VENTANAS + 100)
            .map(|i| IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8)))
            .collect();

        for ip in &ips {
            limitador.permitir(*ip, ahora);
        }

        assert!(
            limitador.ventanas.len() <= MAX_VENTANAS,
            "el mapa de ventanas no puede pasar de MAX_VENTANAS entradas, y tiene {}",
            limitador.ventanas.len()
        );

        // Pasada la ventana, una petición poda las vencidas en vez de arrastrarlas.
        let despues = ahora + VENTANA_DE_LOGIN;
        limitador.permitir(ips[0], despues);

        assert_eq!(
            limitador.ventanas.len(),
            1,
            "pasada la ventana solo tiene que quedar la ventana que acaba de llegar"
        );
    }

    #[test]
    fn con_el_mapa_lleno_se_desaloja_la_ventana_mas_antigua_y_la_nueva_entra() {
        let mut limitador = LimitadorDeLogin::new();
        let ahora = Instant::now();

        // Se llena el mapa con MAX_VENTANAS IPs distintas, cada una un milisegundo después de la
        // anterior: dentro de la ventana (nunca la vence), pero con marcas de inicio **distintas**
        // para que «la más antigua» sea una sola y no un empate.
        let ips: Vec<IpAddr> = (0..MAX_VENTANAS)
            .map(|i| IpAddr::V4(Ipv4Addr::new(10, 1, (i / 256) as u8, (i % 256) as u8)))
            .collect();

        for (i, ip) in ips.iter().enumerate() {
            let instante = ahora + Duration::from_millis(i as u64);
            assert!(
                limitador.permitir(*ip, instante),
                "la IP {i} tiene que entrar mientras el mapa no está lleno"
            );
        }

        assert_eq!(
            limitador.ventanas.len(),
            MAX_VENTANAS,
            "el mapa tiene que estar lleno antes de la inserción que desaloja"
        );

        let nueva = IpAddr::V4(Ipv4Addr::new(10, 2, 0, 0));
        let instante = ahora + Duration::from_millis(MAX_VENTANAS as u64);

        assert!(
            limitador.permitir(nueva, instante),
            "la IP nueva tiene que entrar aunque el mapa esté lleno: el tope desaloja, no rechaza"
        );

        assert_eq!(
            limitador.ventanas.len(),
            MAX_VENTANAS,
            "el mapa sigue teniendo MAX_VENTANAS entradas, no una más"
        );
        assert!(
            !limitador.ventanas.contains_key(&ips[0]),
            "la ventana más antigua —la primera IP— es la que se desaloja"
        );
        assert!(
            limitador.ventanas.contains_key(&nueva),
            "la IP nueva tiene que quedar registrada"
        );
    }
}
