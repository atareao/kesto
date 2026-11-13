## Purpose

El empaquetado y el despliegue de Kesto: la imagen de contenedor que reúne el binario y el panel compilado sin toolchain y sin privilegios, el fichero de compose del §8 que la pone en marcha, la forma en que la configuración y los secretos entran en el contenedor, y la verificación de que el artefacto arranca y sirve de verdad. Fija el contrato entre lo que se construye, lo que se despliega y lo que el operador tiene que proveer.

## ADDED Requirements

### Requirement: La imagen contiene el binario real del crate

El `Dockerfile` SHALL garantizar que el binario de release se compila a partir del código fuente real y NO SHALL publicar un artefacto de la caché de compilación. Cuando la etapa de dependencias use el patrón dummy, el build SHALL invalidar la huella de Cargo del crate tras copiar el código, de modo que Cargo lo recompile.

#### Scenario: la imagen arranca con el binario real
- **GIVEN** el `Dockerfile` con caché de dependencias por el patrón dummy
- **WHEN** se construye y se arranca la imagen
- **THEN** el binario SHALL corresponder al código fuente real
- **AND** el contenedor SHALL escuchar en el puerto HTTP (smoke test superado)

#### Scenario: el guard estático detecta la regresión
- **GIVEN** `scripts/release-image-workflow.test.sh`
- **WHEN** el `Dockerfile` copia el código real sin invalidar la huella de Cargo
- **THEN** el test SHALL fallar
