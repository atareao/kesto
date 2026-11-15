## ADDED Requirements

### Requirement: El código de aplicación no depende de tipos de Node

El proyecto de UI SHALL separar la configuración de TypeScript por fronteras: aplicación, tooling y tests. El `tsconfig.json` de la **aplicación** (`ui/src`) NO SHALL declarar tipos de Node ni globales de Vitest. Los tipos de Node SHALL acotarse a los dos proyectos que sí los necesitan y SHALL declararse en el tsconfig de cada uno, no en el de la aplicación: el proyecto de **tooling** (`ui/tsconfig.node.json`, que typechequea `vite.config.ts`, ejecutado en Node) y el de **tests** (`ui/tsconfig.test.json`, que extiende del de aplicación y declara `"types": ["vitest/globals", "node"]`). El código de aplicación NO SHALL importar ni referenciar APIs o tipos de Node, y el andamiaje de pruebas (setup y dobles) SHALL vivir fuera de `ui/src`. El typecheck de la aplicación SHALL usar el tsconfig de aplicación y el de tests SHALL usar el tsconfig de tests.

El porqué: el panel es código de navegador; permitir tipos de Node en toda la app invita a dependencias accidentales del entorno y oculta errores que solo aparecerían en el bundle. Acotarlos al tooling y a los tests mantiene la frontera explícita: el tooling los necesita para `vite.config.ts` (que corre en Node) y los tests para leer el sistema de ficheros.

#### Scenario: La aplicación compila sin tipos de Node
- **GIVEN** el código de aplicación de `ui/src`
- **WHEN** se ejecuta el typecheck con el tsconfig de aplicación
- **THEN** la compilación SHALL completarse sin resolver tipos de Node
- **AND** el tsconfig de aplicación NO SHALL contener `"types": ["node"]` ni `"types": ["vitest/globals"]`

#### Scenario: El proyecto de tooling conserva sus tipos de Node
- **GIVEN** `ui/vite.config.ts`, que se ejecuta en Node
- **WHEN** se resuelve el tsconfig de ese proyecto de tooling (`ui/tsconfig.node.json`)
- **THEN** los tipos de Node SHALL estar disponibles para él
- **AND** esos tipos SHALL declararse en el tsconfig de tooling, no en el de aplicación

#### Scenario: Los tests conservan los tipos de Node y de Vitest
- **GIVEN** los tests de UI
- **WHEN** se ejecutan con el tsconfig de tests
- **THEN** los tipos de Node y los globales de Vitest SHALL estar disponibles
- **AND** el tsconfig de tests SHALL extender del tsconfig de aplicación

### Requirement: El tema conserva su comportamiento tras el refactor de memoización

El provider y el hook de tema SHALL mantener su API pública y su comportamiento observable: tema oscuro por defecto sin elección guardada, persistencia de la elección en `localStorage` bajo la clave `kesto-tema`, y conmutación inmediata y accesible. La eliminación de `useCallback`/`useMemo` redundantes NO SHALL alterar el tema aplicado, la persistencia ni la accesibilidad del control.

El porqué: la memoización sobrante añade complejidad sin beneficio; retirarla no debe cambiar lo que el operador ve ni cómo se guarda su elección.

#### Scenario: El tema por defecto sigue siendo oscuro
- **GIVEN** un navegador sin elección guardada
- **WHEN** se carga el panel
- **THEN** el tema activo SHALL ser el oscuro

#### Scenario: La elección persiste tras el refactor
- **GIVEN** el panel en tema claro
- **WHEN** se recarga el panel
- **THEN** el tema activo SHALL seguir siendo el claro
- **AND** `localStorage` SHALL conservar la clave `kesto-tema`

#### Scenario: El conmutador sigue siendo accesible
- **GIVEN** el panel cargado
- **WHEN** se navega con teclado hasta el control de tema y se activa
- **THEN** el tema SHALL cambiar
- **AND** el control SHALL anunciar el tema hacia el que cambia
