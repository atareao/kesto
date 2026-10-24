//! API HTTP de Kesto: el router de `/api/v1` y la envoltura de sus respuestas.

pub mod health;
pub mod response;

use axum::Router;
use axum::routing::get;

use crate::error::AppError;
use crate::state::AppState;

/// Router de `/api/v1`.
///
/// La salud se declara sin barra final y las rutas desconocidas contestan con
/// la misma envoltura de error que el resto de la API.
pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health::health))
        // Dos fallbacks, no uno: el de rutas cubre lo que no está declarado, y
        // el de métodos cubre lo que sí lo está con otro verbo. Sin el segundo,
        // axum contesta `405` con el cuerpo vacío y rompe la envoltura única.
        .fallback(not_found)
        .method_not_allowed_fallback(metodo_no_permitido)
        .with_state(state)
}

/// Fallback de las rutas desconocidas: `404` con la envoltura de error.
async fn not_found() -> AppError {
    AppError::NotFound("la ruta solicitada no existe".to_owned())
}

/// Fallback de los métodos no declarados: `405` con la envoltura de error.
async fn metodo_no_permitido() -> AppError {
    AppError::MethodNotAllowed("el método no está permitido en esta ruta".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::StatusCode;

    use super::api_router;
    use crate::config::Config;
    use crate::state::AppState;
    use crate::test_support::{body_text, get, lookup_from, post};

    /// Clave maestra larga, para que ningún aviso se cuele en la respuesta.
    const MASTER_KEY: &str = "clave-maestra-distinguible-000000000000000000000000000000000000";

    /// Estado de prueba: escucha en un puerto efímero, sin base de datos real.
    fn app_state() -> AppState {
        let lookup = lookup_from(&[
            ("KESTO_MASTER_KEY", MASTER_KEY),
            ("BIND_ADDRESS", "127.0.0.1:0"),
            ("DATABASE_URL", "sqlite:///tmp/kesto-pruebas.db"),
        ]);
        let config = Config::from_lookup(&lookup).expect("configuración de prueba válida");

        AppState {
            config: Arc::new(config),
        }
    }

    #[tokio::test]
    async fn una_ruta_desconocida_contesta_404_con_envoltura() {
        let respuesta = get(api_router(app_state()), "/api/v1/no-existe").await;

        assert_eq!(respuesta.status(), StatusCode::NOT_FOUND);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 404 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 404 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 404);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 404 debe traer un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn un_metodo_no_declarado_contesta_405_con_envoltura() {
        let respuesta = post(api_router(app_state()), "/api/v1/health").await;

        assert_eq!(respuesta.status(), StatusCode::METHOD_NOT_ALLOWED);

        let texto = body_text(respuesta).await;

        assert!(!texto.is_empty(), "el 405 llegó con el cuerpo vacío");

        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).expect("el 405 debe traer JSON en el cuerpo");

        assert_eq!(cuerpo["status"], 405);
        assert!(
            cuerpo["message"]
                .as_str()
                .is_some_and(|mensaje| !mensaje.is_empty()),
            "el 405 debe traer un mensaje: {texto}"
        );
    }

    #[tokio::test]
    async fn el_servidor_devuelve_cuando_la_senal_de_cierre_se_resuelve() {
        let (senal, espera) = tokio::sync::oneshot::channel::<()>();

        let servidor = tokio::spawn(async move {
            crate::serve(app_state(), async move {
                let _ = espera.await;
            })
            .await
        });

        senal.send(()).expect("la señal de cierre debe entregarse");

        let transcurrido = tokio::time::timeout(Duration::from_secs(5), servidor).await;

        transcurrido
            .expect("el servidor no devolvió al resolverse la señal de cierre")
            .expect("la tarea del servidor no debe entrar en pánico")
            .expect("el servidor debe cerrar sin error");
    }
}
