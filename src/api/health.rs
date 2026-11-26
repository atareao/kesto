//! Salud del proceso: vida, no disponibilidad, y sin describir el despliegue.

use axum::extract::State;
use axum::http::StatusCode;

use crate::api::response::{ApiResponse, Data};
use crate::state::AppState;

/// `GET /api/v1/health`, sin sesión: contesta solo el estado.
///
/// No declara la versión, ni el modo de autenticación, ni la dirección de
/// escucha, ni la cadena de base de datos, ni el emisor OIDC: es un endpoint
/// sin autenticar que no filtra metadatos.
pub async fn health(State(_state): State<AppState>) -> ApiResponse {
    ApiResponse::new(
        StatusCode::OK,
        "servicio en marcha",
        Data::Some(serde_json::json!({})),
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
        OIDC_ISSUER_URL, OIDC_REDIRECT_URL, base_con_clave, body_json, body_text, estado_de_prueba,
        get, lookup_from,
    };

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
    ///
    /// Declara el permiso explícito del modo sin autenticación: sin las `OIDC_*` la
    /// aplicación solo arranca con `KESTO_ALLOW_INSECURE=1` y un bind de loopback. Va
    /// en este ayudante —y no en `state()`, que comparten los de OIDC— porque es el
    /// único que construye una configuración `Disabled`; en modo `Oidc` el permiso ni
    /// se consulta, así que declararlo allí no describiría nada.
    async fn desarrollo() -> AppState {
        state(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_ALLOW_INSECURE", "1"),
        ])
        .await
    }

    /// Configuración con la autenticación encendida.
    async fn con_oidc() -> AppState {
        state(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("KESTO_SECRET", SESSION_SECRET),
            ("OIDC_ISSUER_URL", OIDC_ISSUER_URL),
            ("OIDC_CLIENT_ID", OIDC_CLIENT_ID),
            ("OIDC_CLIENT_SECRET", OIDC_CLIENT_SECRET),
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_URL),
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
            ("OIDC_REDIRECT_URL", OIDC_REDIRECT_URL),
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
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "la envoltura de una respuesta atendida debe traer message: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn la_salud_no_debe_incluir_version_auth_mode() {
        let respuesta = get(api_router(desarrollo().await), "/api/v1/health").await;
        let cuerpo = body_json(respuesta).await;

        let datos = cuerpo["data"]
            .as_object()
            .expect("data debe ser un objeto JSON");

        assert!(
            !datos.contains_key("version"),
            "data no debe contener la clave version: {cuerpo}"
        );
        assert!(
            !datos.contains_key("auth_mode"),
            "data no debe contener la clave auth_mode: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn la_salud_declara_el_modo_de_autenticacion() {
        // auth_mode no debe estar en data tras el cambio de SEC-21.
        let cuerpo = body_json(get(api_router(desarrollo().await), "/api/v1/health").await).await;
        assert!(
            !cuerpo["data"]
                .as_object()
                .expect("data debe ser un objeto JSON")
                .contains_key("auth_mode"),
            "data no debe contener la clave auth_mode: {cuerpo}"
        );

        let cuerpo = body_json(get(api_router(con_oidc().await), "/api/v1/health").await).await;
        assert!(
            !cuerpo["data"]
                .as_object()
                .expect("data debe ser un objeto JSON")
                .contains_key("auth_mode"),
            "data no debe contener la clave auth_mode: {cuerpo}"
        );
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

        // Tras SEC-21, `data` no lleva `version` ni `auth_mode`, así que el conjunto de claves
        // es vacío.
        assert_eq!(
            claves,
            Vec::<String>::new(),
            "la salud debe limitar su cuerpo a un objeto vacío: {cuerpo}"
        );
    }

    #[tokio::test]
    async fn con_una_clave_debil_no_hay_salud_que_contestar() {
        // Este test fijaba que una clave débil solo avisaba y el servicio
        // arrancaba y contestaba la salud. Ahora una clave débil es puerta de
        // arranque: no hay salud que contestar porque el proceso no llega a
        // levantarse. La aserción se invierte a conciencia —la clave deja de
        // asumir que arranca—; no la «restaures» pensando que era intencional.
        let clave_debil = "c".repeat(12);
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", &clave_debil),
            ("KESTO_ALLOW_INSECURE", "1"),
        ]);

        let error = Config::from_lookup(&lookup)
            .expect_err("una clave débil no puede arrancar, así que no contesta la salud");

        assert!(
            error.to_string().contains("KESTO_MASTER_KEY"),
            "el rechazo debe nombrar la variable débil: {error}"
        );
        assert!(
            !error.to_string().contains(clave_debil.as_str()),
            "el rechazo no puede publicar el valor de la clave: {error}"
        );
    }
}
