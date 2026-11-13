# Spec Delta

## Purpose

El empaquetado y el despliegue de Kesto: la imagen de contenedor que reúne el binario y el panel compilado sin toolchain y sin privilegios, el fichero de compose del §8 que la pone en marcha, la forma en que la configuración y los secretos entran en el contenedor, y la verificación de que el artefacto arranca y sirve de verdad. Fija el contrato entre lo que se construye, lo que se despliega y lo que el operador tiene que proveer.

## ADDED Requirements

### Requirement: La etiqueta `latest` de la imagen apunta siempre a la release más alta

La publicación de la imagen en GHCR SHALL mantener `latest` resolviendo **siempre** al manifiesto de la versión semver más alta publicada, con independencia del orden en que terminen las construcciones. El workflow de release NO SHALL publicar `latest` cuando el tag que se está publicando no sea esa versión más alta, porque el `compose.yaml` del §8 usa `image: ghcr.io/atareao/kesto:latest` y un despliegue que siga esa etiqueta recibiría una versión anterior.

El workflow SHALL serializar las construcciones de imagen entre releases, de modo que dos releases no compitan en paralelo por la misma etiqueta y el resultado no dependa de cuál acabe antes.

#### Scenario: `latest` apunta a la versión más alta
- **GIVEN** las releases `0.3.3` y `0.4.0` publicadas y sus construcciones terminadas
- **WHEN** se inspecciona `ghcr.io/atareao/kesto:latest`
- **THEN** SHALL resolver al mismo digest que `ghcr3.io/atareao/kesto:0.4.0`
- **AND** NO SHALL resolver al digest de `0.3.3`

#### Scenario: Una release no-máxima no degrada `latest`
- **GIVEN** `latest` publicado en 0.4.0
- **WHEN** termina la construcción de una release anterior (por ejemplo `0.3.3`)
- **THEN** el workflow NO SHALL publicar `latest` desde esa release
- **AND** `latest` SHALL seguir resolviendo al digest de 0.4.0

#### Scenario: Builds solapados no dejan `latest` en una versión antigua
- **GIVEN** dos construcciones de release solapadas en el tiempo
- **WHEN** ambas terminan
- **THEN** `latest` SHALL quedar en la versión más alta de las dos
- **AND** NO SHALL quedar en la que terminó última por azar