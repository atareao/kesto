## Purpose

El empaquetado y el despliegue de Kesto: la imagen de contenedor que reúne el binario y el panel compilado sin toolchain y sin privilegios, el fichero de compose del §8 que la pone en marcha, la forma en que la configuración y los secretos entran en el contenedor, y la verificación de que el artefacto arranca y sirve de verdad. Fija el contrato entre lo que se construye, lo que se despliega y lo que el operador tiene que proveer.

## ADDED Requirements

### Requirement: La imagen multi-arquitectura se construye nativamente y se publica como manifest list

El workflow de release SHALL construir cada arquitectura de la imagen (`linux/amd64` y `linux/arm64`) en un runner de la **misma** arquitectura —amd64 en `ubuntu-26.04` y arm64 en `ubuntu-26.04-arm`—, de modo que la capa `arm64` NO SHALL compilarse por emulación. Cada construcción SHALL publicar su imagen por digest y un paso posterior SHALL ensamblar ambos digests en un único manifest list.

#### Scenario: amd64 y arm64 se construyen sin emulación
- **GIVEN** una release `vX.Y.Z` publicada
- **WHEN** arranca el job de imagen
- **THEN** la arquitectura amd64 SHALL construirse en un runner x64
- **AND** la arquitectura arm64 SHALL construirse en el runner nativo `ubuntu-26.04-arm`
- **AND** la construcción arm64 NO SHALL depender de emulación

#### Scenario: el manifest publicado contiene las dos arquitecturas
- **GIVEN** la release `vX.Y.Z` terminada
- **WHEN** se inspecciona `ghcr.io/atareao/kesto:X.Y.Z`
- **THEN** el manifest SHALL contener `linux/amd64` y `linux/arm64`
- **AND** `X.Y.Z`, `X.Y` y `latest` SHALL resolver al mismo manifest list

#### Scenario: el contrato de tags no cambia
- **GIVEN** una release `vX.Y.Z` que es la más alta publicada
- **WHEN** termina la publicación
- **THEN** los tags SHALL ser `X.Y.Z`, `X.Y` y `latest`, igual que antes
- **AND** NO SHALL publicarse etiquetas nuevas ni retirarse las existentes

### Requirement: La compilación Rust se cachea entre ejecuciones

El workflow SHALL reutilizar los artefactos de compilación Rust entre ejecuciones mediante caché persistente: una capa de dependencias en el `Dockerfile` y **`sccache`** con backend de caché de GitHub Actions. Una release NO SHALL recompilar las dependencias que no hayan cambiado.

#### Scenario: las dependencias se reutilizan entre releases
- **GIVEN** dos releases consecutivas con las dependencias sin cambios
- **WHEN** se construye la segunda
- **THEN** las dependencias Rust SHALL reutilizarse de la caché
- **AND** el tiempo de compilación SHALL reducirse respecto a un build en frío

#### Scenario: sccache reutiliza artefactos ante un cambio de código
- **GIVEN** la caché de `sccache` activada
- **WHEN** cambia solo el código del crate y no sus dependencias
- **THEN** `sccache` SHALL reutilizar los artefactos de las dependencias
- **AND** NO SHALL recompilarlas

### Requirement: El guard de `latest` y el smoke test siguen vigentes

El workflow de release SHALL mantener el guard que publica `latest` solo cuando el tag es la versión semver más alta, con independencia del orden en que terminen las construcciones, y SHALL ejecutar el smoke test de arranque sobre la imagen publicada. La construcción por arquitectura NO SHALL alterar esa lógica.

#### Scenario: una release no-máxima no publica `latest`
- **GIVEN** `latest` publicado en `X.Y.Z`
- **WHEN** termina la construcción de una release anterior
- **THEN** el workflow NO SHALL publicar `latest` desde esa release
- **AND** `latest` SHALL seguir resolviendo a `X.Y.Z`

#### Scenario: la imagen publicada arranca
- **GIVEN** la imagen `X.Y.Z` publicada
- **WHEN** se arranca el contenedor con la configuración de prueba
- **THEN** SHALL aplicar las migraciones y escuchar en el puerto HTTP
- **AND** el smoke test SHALL pasar en el job amd64

### Requirement: La imagen publicada es funcionalmente idéntica

La imagen SHALL contener el mismo binario y el mismo panel compilado que antes, sin toolchain en el runtime y corriendo sin privilegios, de modo que la aceleración del build NO SHALL cambiar el artefacto entregado.

#### Scenario: el runtime sigue sin toolchain y sin privilegios
- **GIVEN** la imagen `X.Y.Z` publicada
- **WHEN** se inspecciona el runtime
- **THEN** NO SHALL contener el toolchain de compilación
- **AND** el proceso SHALL correr con el usuario sin privilegios
