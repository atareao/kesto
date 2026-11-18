# portada-publica — delta

## ADDED Requirements

### Requirement: La raíz presenta una portada pública en modo `oidc` sin sesión

En modo `oidc`, sin sesión y una vez conocido el modo, la ruta `/` SHALL mostrar una portada pública que presente Kesto y ofrezca entrar. El botón de entrada SHALL **navegar** a `/api/v1/auth/login` (una navegación del navegador, no una petición de datos) y NO SHALL mostrarse ninguna pantalla del panel ni dato de la instalación. Mientras `/health` o la sesión están en el aire, la raíz SHALL mostrar la carga ya existente, no la portada.

#### Scenario: La raíz sin sesión muestra la portada
- **GIVEN** el modo `oidc` y ninguna sesión válida
- **WHEN** el visitante abre `/`
- **THEN** SHALL mostrarse la portada pública
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

### Requirement: La raíz entra al panel en modo `disabled` y con sesión

En modo `disabled`, `/` SHALL mostrar el panel (el Dashboard), sin capa de sesión. En modo `oidc` con sesión válida, `/` SHALL mostrar el panel. La portada NO SHALL aparecer en ninguno de los dos casos.

#### Scenario: En desarrollo la raíz entra directa al panel
- **GIVEN** el modo `disabled`
- **WHEN** se abre `/`
- **THEN** SHALL mostrarse el Dashboard
- **AND** NO SHALL mostrarse la portada

#### Scenario: Con sesión la raíz entra al panel
- **GIVEN** el modo `oidc` y una sesión válida
- **WHEN** se abre `/`
- **THEN** SHALL mostrarse el Dashboard
- **AND** NO SHALL mostrarse la portada

### Requirement: `/sesion` conserva su ruta y su comportamiento

La pantalla de sesión SHALL seguir disponible en `/sesion`, fuera de la guarda, y SHALL conservar su comportamiento actual. Las rutas protegidas sin sesión SHALL seguir desviándose a `/sesion`. La portada NO SHALL sustituir ni mover esa ruta.

#### Scenario: La ruta de sesión no se mueve
- **GIVEN** el modo `oidc` sin sesión
- **WHEN** se abre `/sesion`
- **THEN** SHALL mostrarse la pantalla de sesión con su oferta de entrar
- **AND** la ruta SHALL seguir siendo `/sesion`

#### Scenario: Una ruta protegida sin sesión sigue desviándose a `/sesion`
- **GIVEN** el modo `oidc` sin sesión
- **WHEN** se abre una ruta protegida distinta de `/`
- **THEN** SHALL navegarse a `/sesion`
- **AND** NO SHALL mostrarse la portada

### Requirement: La portada no añade peticiones a la API

La portada SHALL decidir su contenido con `useSesion()`, reutilizando la consulta de salud y de sesión ya existente. La portada NO SHALL introducir consultas, mutaciones ni peticiones propias, y el cambio de ruta índice NO SHALL duplicar la consulta de sesión.

#### Scenario: La portada se pinta con la sesión ya consultada
- **GIVEN** la portada visible en `/`
- **WHEN** se inspeccionan las peticiones hechas al abrirla
- **THEN** las peticiones SHALL ser las que ya hacía `useSesion()`
- **AND** NO SHALL aparecer ninguna petición nueva atribuible a la portada
