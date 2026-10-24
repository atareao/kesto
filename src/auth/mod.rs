//! Dominio de autenticación: la sesión del operador y el flujo OIDC.
//!
//! `src/auth/` no es un módulo del §7 del SRS: lo declara `main.rs` como los
//! demás. Su frontera con `src/api/` es la de siempre —aquí el flujo y la
//! sesión, allí el HTTP—: `src/api/auth.rs` traducirá peticiones a llamadas de
//! este módulo, y no al revés.

pub mod middleware;
pub mod oidc;
pub mod session;

use crate::state::AuthState;

/// Intenta el descubrimiento del proveedor al arrancar.
///
/// Devuelve el aviso a registrar cuando el proveedor no contesta, o `None`
/// cuando contestó y el resultado quedó cacheado. **No** impide arrancar, y el
/// porqué se deja escrito para que nadie lo «arregle» mañana: Kesto es el
/// orquestador de las **copias**, y negarse a arrancar porque el proveedor de
/// identidad está caído convertiría un problema de autenticación en ausencia de
/// copias. Es el mismo criterio por el que una credencial débil avisa y arranca.
/// El reintento llega con el primer login.
pub async fn descubrimiento_al_arrancar(auth: &AuthState) -> Option<String> {
    match oidc::discovery_cached(auth).await {
        Ok(_) => None,
        // La causa entra en el aviso para que el operador la vea en el log; es
        // un `String` de `AppError`, que solo lleva el fallo del descubrimiento
        // —URL del emisor y error de red—, nunca el `client_secret`.
        Err(error) => Some(format!(
            "no se pudo descubrir el proveedor de identidad al arrancar; se reintentará en el primer login: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::api::api_router;
    use crate::test_support::{
        emisor_caido, estado_oidc_con_emisor, get, proveedor_falso::ProveedorFalso,
        router_oidc_con_emisor,
    };

    #[tokio::test]
    async fn un_descubrimiento_fallido_no_impide_arrancar() {
        // Escenario 1.5.
        let estado = estado_oidc_con_emisor(&emisor_caido());
        let auth = estado
            .auth
            .as_ref()
            .expect("en modo Oidc hay estado de autenticación");

        let aviso = super::descubrimiento_al_arrancar(auth).await;

        let aviso = aviso.expect("un descubrimiento fallido debe producir un aviso");
        assert!(
            aviso.contains("descubrimiento"),
            "el aviso no declara el fallo del descubrimiento: {aviso}"
        );
        assert!(
            aviso.contains("proveedor"),
            "el aviso no nombra al proveedor de identidad: {aviso}"
        );

        // Y el router se construye igualmente: el arranque no depende del proveedor.
        let router = api_router(estado);
        let salud = get(router, "/api/v1/health").await;

        assert_eq!(
            salud.status(),
            StatusCode::OK,
            "un proveedor caído no puede impedir que la salud conteste"
        );
    }

    #[tokio::test]
    async fn el_primer_login_reintenta_el_descubrimiento_y_lo_cachea() {
        // Escenario 1.6 — el candado de la caché.
        //
        // El GIVEN literal del escenario es «un arranque cuyo descubrimiento
        // falló»; aquí se aproxima por «la caché del descubrimiento está vacía».
        // Para la caché los dos estados son el mismo —tras un fallo, `discovery`
        // queda en `None`, que es como arranca el estado—, por eso las aserciones
        // de abajo son las correctas y no vacuas. No se reproduce literalmente
        // porque recrearlo exigiría construir el estado con un emisor caído,
        // dejar que el descubrimiento fallara y luego reapuntarlo al proveedor
        // falso, y `settings` no se muta tras construir el estado: la forma de
        // forzarlo sería un emisor que primero falla y luego contesta, más arnés
        // del que el escenario necesita para quedar probado.
        //
        // La única forma de probar que el resultado se cachea de verdad es
        // contar las peticiones que llegan al proveedor: si la caché
        // desapareciera, el segundo login volvería a pedirlo y el contador
        // subiría a dos.
        let proveedor = ProveedorFalso::servir().await;
        let router = router_oidc_con_emisor(&proveedor.issuer);

        let primera = get(router.clone(), "/api/v1/auth/login").await;

        assert_eq!(
            primera.status(),
            StatusCode::FOUND,
            "el login debe redirigir"
        );
        assert_eq!(
            proveedor.peticiones_de_descubrimiento().await,
            1,
            "el primer login debe pedir el descubrimiento"
        );

        let segunda = get(router, "/api/v1/auth/login").await;

        assert_eq!(
            segunda.status(),
            StatusCode::FOUND,
            "el login debe redirigir"
        );
        assert_eq!(
            proveedor.peticiones_de_descubrimiento().await,
            1,
            "el segundo login NO debe repetir la petición: el resultado debe estar cacheado"
        );
    }
}
