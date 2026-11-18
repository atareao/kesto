# portada-publica Specification

## MODIFIED Requirements

### Requirement: La raíz presenta una portada pública en modo `oidc` sin sesión

En modo `oidc`, sin sesión y una vez conocido el modo, la ruta `/` SHALL mostrar una portada pública que presente la identidad de marca de Kesto —logo, nombre «Kesto» y una línea de descripción— y ofrezca entrar. El botón de entrada SHALL **navegar** a `/api/v1/auth/login` (una navegación del navegador, no una petición de datos) y NO SHALL mostrarse ninguna pantalla del panel ni dato de la instalación. En modo `disabled` o con sesión la raíz SHALL mostrar el Dashboard, no la portada. Mientras `/health` o la sesión están en el aire, la raíz SHALL mostrar la carga ya existente, no la portada.

#### Scenario: La raíz sin sesión muestra la portada
- **GIVEN** el modo `oidc` y ninguna sesión válida
- **WHEN** el visitante abre `/`
- **THEN** SHALL mostrarse la portada pública
- **AND** SHALL mostrarse la identidad de marca de Kesto
- **AND** NO SHALL mostrarse ninguna pantalla del panel

#### Scenario: El botón de la portada navega al login
- **GIVEN** la portada pública visible en `/`
- **WHEN** el visitante pulsa el botón de entrada
- **THEN** SHALL navegarse a `/api/v1/auth/login`
- **AND** NO SHALL hacerse ninguna petición nueva a la API desde la portada

#### Scenario: La raíz con la sesión en el aire no enseña la portada
- **GIVEN** el modo aún desconocido o la sesión sin contestar
- **WHEN** se abre `/`
- **THEN** SHALL mostrarse la indicación de carga
- **AND** NO SHALL mostrarse la portada como si no hubiera sesión

## ADDED Requirements

### Requirement: La portada muestra la marca de Kesto

La portada pública SHALL mostrar la identidad de marca de Kesto: el logo de Kesto —con `data-testid="logo-de-la-portada"`—, un único asset con fondo transparente, legible en el tema activo (claro u oscuro), junto al nombre «Kesto» como encabezado de nivel 1 y una línea de descripción (tagline). El logo NO SHALL tener variantes por tema: un solo asset SHALL verse legible tanto en claro como en oscuro.

#### Scenario: La portada enseña el logo y el nombre
- **GIVEN** el modo `oidc` sin sesión y la portada visible en `/`
- **WHEN** se pinta la portada
- **THEN** SHALL verse el logo de Kesto (`data-testid="logo-de-la-portada"`)
- **AND** SHALL verse el nombre «Kesto» como encabezado de nivel 1
- **AND** SHALL verse una línea de descripción

#### Scenario: El logo es legible en ambos temas
- **GIVEN** la portada visible en `/`
- **WHEN** se pinta el logo con el tema claro y con el tema oscuro activos
- **THEN** el mismo asset del logo SHALL verse legible en tema claro
- **AND** SHALL verse legible en tema oscuro
- **AND** NO SHALL cambiarse de asset entre temas

### Requirement: Sin sesión, las rutas protegidas desvían a la portada

En modo `oidc` sin sesión, abrir una ruta protegida distinta de `/` SHALL navegar a `/`, no a `/sesion`. La portada es el único punto de entrada antes de la autenticación.

#### Scenario: Una ruta protegida sin sesión desvía a la portada
- **GIVEN** el modo `oidc` sin sesión
- **WHEN** se abre una ruta protegida distinta de `/`
- **THEN** SHALL navegarse a `/`
- **AND** NO SHALL navegarse a `/sesion`
- **AND** NO SHALL mostrarse ninguna pantalla del panel

### Requirement: La barra lateral no se monta sin sesión

En modo `oidc` sin sesión, y mientras el modo o la sesión se están resolviendo, ninguna ruta SHALL renderizar el menú lateral (`navigation` «Principal»). En modo `disabled` o en `oidc` con sesión la barra lateral SHALL estar presente. La ausencia del menú lateral no SHALL afectar a la portada, que se muestra sin chasis.

#### Scenario: Sin sesión no hay menú lateral
- **GIVEN** el modo `oidc` sin sesión
- **WHEN** se abre cualquier ruta, incluida la portada
- **THEN** NO SHALL renderizarse el menú lateral (`navigation` «Principal»)

#### Scenario: Mientras se resuelve el modo o la sesión tampoco hay menú lateral
- **GIVEN** el modo aún desconocido o la sesión sin contestar
- **WHEN** se abre cualquier ruta
- **THEN** NO SHALL renderizarse el menú lateral (`navigation` «Principal»)

#### Scenario: Con panel sí hay menú lateral
- **GIVEN** el modo `disabled` o el modo `oidc` con sesión válida
- **WHEN** se abre cualquier ruta del panel
- **THEN** el menú lateral (`navigation` «Principal») SHALL estar presente

## REMOVED Requirements

### Requirement: `/sesion` conserva su ruta y su comportamiento
