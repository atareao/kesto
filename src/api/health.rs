//! Salud del proceso: vida, no disponibilidad, y sin describir el despliegue.

use axum::extract::State;
use axum::http::StatusCode;

use crate::api::response::{ApiResponse, Data};
use crate::state::AppState;

/// `GET /api/v1/health`, sin sesión: contesta la versión y el modo de autenticación.
///
/// No declara la dirección de escucha, ni la cadena de base de datos, ni el
/// emisor OIDC: es un endpoint sin autenticar.
pub async fn health(State(state): State<AppState>) -> ApiResponse {
    ApiResponse::new(
        StatusCode::OK,
        "servicio en marcha",
        Data::Some(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "auth_mode": state.config.auth.as_str(),
        })),
    )
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::api::api_router;
    use crate::config::Config;
    use crate::state::{AppState, EstadoDeBase};
    // El emisor de ejemplo se toma de `test_support` en vez de redeclararlo: era
    // una segunda fuente de verdad del mismo literal, y cambiar una y no la otra
    // dejaría los dos tests mintiendo en silencio.
    use crate::test_support::{
        OIDC_ISSUER_URL, base_con_clave, body_json, body_text, estado_con_base, estado_de_prueba,
        get, lookup_from,
    };

    /// Versión que tiene que declarar la salud: la del binario.
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    /// Clave maestra larga, para que ningún aviso se cuele en la respuesta.
    const MASTER_KEY: &str = "clave-maestra-distinguible-000000000000000000000000000000000000";
    /// Clave de firma larga, por el mismo motivo.
    const SESSION_SECRET: &str = "clave-de-firma-distinguible-0000000000000000000000000000000";
    /// Secreto de cliente OIDC: solo tiene que estar presente.
    const OIDC_CLIENT_SECRET: &str = "secreto-de-cliente-oidc-distinguible";
    /// Identificador público del cliente OIDC de pruebas, por el mismo motivo.
    const OIDC_CLIENT_ID: &str = "kesto-panel";
    /// Dirección de escucha distinguible, para detectarla si se publica.
    const BIND_ADDRESS: &str = "127.0.0.1:19090";
    /// Puerto de la dirección anterior: es lo que se busca en el cuerpo.
    const BIND_PORT: &str = "19090";
    /// Cadena de base de datos distinguible, por el mismo motivo.
    const DATABASE_URL: &str = "sqlite:///tmp/kesto-distinguible.db";
    /// Fichero de la cadena anterior: es lo que se busca en el cuerpo.
    const DATABASE_FILE: &str = "kesto-distinguible.db";

    /// Construye el estado a partir de una lectura inyectada.
    ///
    /// El estado lleva la base **ya abierta y migrada**, como en producción: el pool llega
    /// de una base temporal de verdad —es el patrón de la prueba 1.1 de `src/state.rs`— y
    /// su guarda se retiene durante la corrida, porque el pool sobrevive al ayudante. La
    /// configuración entra por la lectura inyectada tal cual, así que su `DATABASE_URL`
    /// puede ser la que la prueba declare: lo que el estado usa es el pool, no la cadena.
    async fn state(pares: &[(&str, &str)]) -> AppState {
        let lookup = lookup_from(pares);
        let config = Config::from_lookup(&lookup).expect("configuración de prueba válida");

        let (_directorio, pool, clave) = base_con_clave().await;

        estado_de_prueba(config, EstadoDeBase::nuevo(pool, clave))
    }

    /// Configuración de desarrollo: sin OIDC.
    async fn desarrollo() -> AppState {
        state(&[("KESTO_MASTER_KEY", MASTER_KEY)]).await
    }

    /// Configuración con la autenticación encendida.
    async fn con_oidc() -> AppState {
        state(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER_URL),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
        ])
        .await
    }

    /// Configuración con la autenticación encendida y un despliegue distinguible:
    /// es la que tiene algo que callar, tanto de OIDC como del entorno.
    async fn con_oidc_y_despliegue_propio() -> AppState {
        state(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER_URL),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
            ("BIND_ADDRESS", BIND_ADDRESS),
            ("DATABASE_URL", DATABASE_URL),
        ])
        .await
    }

    #[tokio::test]
    async fn la_salud_contesta_sin_credenciales() {
        let router = api_router(desarrollo().await);

        let respuesta = get(router, "/api/v1/health").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 200);
        assert_eq!(cuerpo["data"]["version"], VERSION);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "la envoltura de una respuesta atendida debe traer message: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn la_salud_declara_el_modo_de_autenticacion() {
        let cuerpo = body_json(get(api_router(desarrollo().await), "/api/v1/health").await).await;
        assert_eq!(cuerpo["data"]["auth_mode"], "disabled");

        let cuerpo = body_json(get(api_router(con_oidc().await), "/api/v1/health").await).await;
        assert_eq!(cuerpo["data"]["auth_mode"], "oidc");
    }

    #[tokio::test]
    async fn la_salud_no_describe_el_despliegue() {
        let respuesta = get(
            api_router(con_oidc_y_despliegue_propio().await),
            "/api/v1/health",
        )
        .await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let texto = body_text(respuesta).await;

        for (que, valor) in [
            ("la dirección de escucha", BIND_PORT),
            ("la cadena de base de datos", DATABASE_FILE),
            ("el emisor OIDC", OIDC_ISSUER_URL),
            ("el identificador del cliente OIDC", OIDC_CLIENT_ID),
        ] {
            assert!(!texto.contains(valor), "la salud publica {que}: {texto}");
        }

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("la salud debe traer JSON en el cuerpo");

        // «Limitar su cuerpo» solo es comprobable si el conjunto de claves está
        // fijado: cualquier campo nuevo tiene que romper este test y obligar a
        // decidir si se publica.
        let mut claves: Vec<String> = cuerpo["data"]
            .as_object()
            .expect("data debe ser un objeto JSON")
            .keys()
            .cloned()
            .collect();
        claves.sort();

        assert_eq!(
            claves,
            ["auth_mode", "version"],
            "la salud debe limitar su cuerpo a la versión y el modo de autenticación: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn con_una_clave_debil_la_salud_contesta_igual() {
        // El escenario del aviso por credencial débil tiene dos mitades: el
        // aviso y que el servicio arranque y conteste la salud.
        let clave_debil = "c".repeat(12);
        let lookup = lookup_from(&[("KESTO_MASTER_KEY", &clave_debil)]);
        let config = Config::from_lookup(&lookup).expect("una clave débil no impide arrancar");

        let avisos = config.warnings();

        assert!(
            avisos
                .iter()
                .any(|aviso| aviso.contains("KESTO_MASTER_KEY")),
            "una clave maestra débil debe avisar: {avisos:?}"
        );

        let estado = estado_con_base(config).await;

        let respuesta = get(api_router(estado), "/api/v1/health").await;

        assert_eq!(respuesta.status(), StatusCode::OK);

        let cuerpo = body_json(respuesta).await;

        assert_eq!(cuerpo["status"], 200);
        assert_eq!(cuerpo["data"]["version"], VERSION);
        assert_eq!(cuerpo["data"]["auth_mode"], "disabled");
    }
}
