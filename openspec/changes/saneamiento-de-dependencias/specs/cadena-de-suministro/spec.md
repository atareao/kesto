# cadena-de-suministro Specification (delta)

## ADDED Requirements

### Requirement: Las dependencias con advisory conocido se cierran o se declaran con su motivo

Ninguna dependencia del workspace SHALL llevar un advisory de RustSec que afecte a código alcanzable. Cuando un advisory no se pueda cerrar SHALL declararse como excepción en `deny.toml`, con su identificador y su **motivo** por escrito, y el motivo NO SHALL limitarse a silenciar el aviso. Hay dos tipos de excepción y el motivo SHALL nombrar la causa que corresponde a cada uno:

- un aviso **no alcanzable** —el camino vulnerable no se ejecuta, o un tercero retiene la versión—: el motivo SHALL nombrar **quién retiene la versión** o **qué camino no se usa**;
- un crate ***unmaintained*** **sin versión que lo cierre**: el motivo SHALL nombrar la **migración** que haría falta para sustituirlo y **por qué queda fuera del alcance**.

Cada excepción SHALL ser refutable contra el árbol con un `cargo tree -i`, y SHALL retirarse en cuanto su causa desaparezca.

#### Scenario: Un advisory alcanzable se cierra subiendo la dependencia
- **GIVEN** un árbol con `zstd 0.13.3` y `zstd-safe 7.3.0`
- **WHEN** se resuelve con las versiones declaradas
- **THEN** `zstd` SHALL ser `>= 0.13.4` y `zstd-safe` SHALL ser `>= 7.4`
- **AND** `cargo deny check advisories` NO SHALL reportar RUSTSEC-2026-0391, RUSTSEC-2026-0398 ni RUSTSEC-2026-0403

#### Scenario: Una excepción se declara con su motivo
- **GIVEN** el advisory RUSTSEC-2023-0071 sobre `rsa 0.9.10`
- **WHEN** se ejecuta `cargo deny check advisories`
- **THEN** SHALL terminar sin error
- **AND** la entrada de `ignore` SHALL llevar el identificador y un motivo que nombre a `reqsign-core` como quien fija la versión y el descifrado PKCS#1 v1.5 como el camino que no se ejecuta

#### Scenario: Un crate *unmaintained* se declara nombrando la migración que haría falta
- **GIVEN** el advisory RUSTSEC-2026-0543 sobre `chrono 0.4.45`, que ninguna versión cierra
- **WHEN** se ejecuta `cargo deny check advisories`
- **THEN** SHALL terminar sin error
- **AND** la entrada de `ignore` SHALL llevar el identificador y un motivo que nombre la migración a `time`/`jiff` como salida y su exclusión del alcance de este change

#### Scenario: Un advisory nuevo detiene la comprobación
- **GIVEN** un advisory que no está declarado en `deny.toml`
- **WHEN** se ejecuta `cargo deny check advisories`
- **THEN** SHALL terminar con error nombrando el identificador

### Requirement: El CI vigila la cadena de suministro en cada propuesta de cambio

El CI SHALL ejecutar `cargo deny check` sobre el workspace en cada propuesta de cambio y en cada empujón a `development` y a `main`, con el `deny.toml` versionado y con `--locked`. SHALL comprobar las cuatro familias: advisories, licencias, prohibiciones y fuentes.

Las licencias SHALL estar en una allowlist **explícita**: una licencia que aparezca sin estar declarada SHALL hacer fallar el trabajo. Las fuentes SHALL ser solo `crates.io`: un registro o un repositorio git ajeno SHALL hacer fallar el trabajo.

#### Scenario: Una licencia no declarada detiene el CI
- **GIVEN** una dependencia nueva cuya licencia no está en la allowlist
- **WHEN** corre el trabajo de dependencias
- **THEN** SHALL fallar nombrando la licencia y el crate

#### Scenario: Una fuente ajena a crates.io detiene el CI
- **GIVEN** una dependencia que no venga de crates.io
- **WHEN** corre el trabajo de dependencias
- **THEN** SHALL fallar nombrando la fuente

#### Scenario: Una duplicidad de terceros avisa y no detiene el CI
- **GIVEN** dos versiones del mismo crate que entran por árboles de terceros
- **WHEN** corre el trabajo de dependencias
- **THEN** SHALL avisar
- **AND** NO SHALL fallar

### Requirement: El CI compila y prueba todos los miembros del workspace

Los pasos de análisis y de prueba del CI SHALL cubrir **todos** los miembros del workspace, de modo que `kesto-mcp` —donde vive el SDK del MCP— NO SHALL quedar sin compilar ni sin probar. Lo que el CI ejecuta SHALL coincidir con lo que ejecutan las recetas locales: una divergencia entre ambos deja fuera del CI justo lo que el desarrollador sí comprueba.

#### Scenario: Un cambio que rompe el crate del MCP detiene el CI
- **GIVEN** un cambio que no compila en `kesto-mcp`
- **WHEN** corre el trabajo de backend
- **THEN** SHALL fallar en el análisis o en las pruebas

#### Scenario: La receta local y el CI coinciden
- **GIVEN** las recetas `just clippy` y `just test`
- **WHEN** se comparan con los pasos de análisis y prueba del CI
- **THEN** los dos SHALL cubrir el workspace entero

### Requirement: Las actualizaciones de dependencias llegan por PR revisable

El repositorio SHALL declarar la configuración de Dependabot para los tres ecosistemas que tiene: `cargo` —una entrada en la raíz cubre el workspace entero—, `github-actions` y el gestor de paquetes del panel en `/ui`. La periodicidad SHALL ser fija y las actualizaciones SHALL llegar agrupadas por naturaleza, para que un PR no mezcle un parche con un salto mayor.

Ninguna actualización SHALL fusionarse sola: cada PR SHALL pasar por la misma puerta que un cambio escrito a mano.

#### Scenario: Dependabot cubre los tres ecosistemas
- **GIVEN** el fichero de configuración de Dependabot
- **WHEN** se lee
- **THEN** SHALL declarar `cargo`, `github-actions` y `npm` en `/ui`
- **AND** SHALL fijar una periodicidad y un límite de PR abiertos

#### Scenario: Una actualización pasa por la misma puerta
- **GIVEN** una propuesta de cambio abierta por Dependabot
- **WHEN** corre el CI
- **THEN** SHALL ejecutar los mismos trabajos que en cualquier otra propuesta

### Requirement: Una prueba que falla hace fallar la receta que la ejecuta

Las recetas locales que ejecutan una batería de pruebas SHALL propagar el fallo de cualquiera de sus elementos: el código de salida de la receta NO SHALL depender del orden de la batería ni del último elemento ejecutado. Una receta que devuelve éxito con una prueba en rojo convierte la puerta en un adorno.

#### Scenario: Un script en rojo hace fallar la receta
- **GIVEN** la batería de `scripts/*.test.sh` con uno de los scripts en rojo
- **WHEN** se ejecuta la receta que los corre
- **THEN** SHALL terminar con error
