# FEATURE-010 — Guard de `latest`: la etiqueta apunta siempre a la release más alta

**Issue:** [#32](https://github.com/atareao/kesto/issues/32) · **Plan:** `plans/PLAN-010.md` · **Change OpenSpec:** `openspec/changes/latest-tag-guard/`

## Qué es

El `compose.yaml` del §8 despliega con `image: ghcr.io/atareao/kesto:latest`. Hasta ahora el workflow de release publicaba esa etiqueta `latest` **incondicionalmente** en cada release; con construcciones multi-arquitectura solapadas (la arm64 va bajo QEMU y tarda horas) la etiqueta acababa apuntando a la release que terminaba **última**, que no tiene por qué ser la más nueva. En la tanda 0.3.2/0.3.3/0.4.0, `latest` quedó en **0.3.3** en vez de **0.4.0**.

Esta feature hace que `latest` resuelva **siempre** al manifiesto de la versión semver más alta publicada.

## Ficheros implicados

- `scripts/latest-tag-guard.sh` — decide si una versión es la mayor del conjunto de tags (orden semver).
- `scripts/latest-tag-guard.test.sh` — pruebas del anterior.
- `.github/workflows/release.yml` — job `image`: paso `Compute latest tag guard` + `enable` condicional en `docker/metadata-action` + `concurrency` del job.

## Cómo funciona

1. Al publicarse un tag `vX.Y.Z`, `release.yml` entra en el job `image`.
2. El paso **`Compute latest tag guard`** lista los tags del remoto con `git ls-remote --tags origin 'refs/tags/v*'` (no depende de un clon completo), los normaliza (quita `refs/tags/` y el sufijo `^{}`) e invoca `scripts/latest-tag-guard.sh "${GITHUB_REF_NAME#v}" <tags...>`. El resultado (`true`/`false`) queda en el output `is_latest`.
3. `scripts/latest-tag-guard.sh` imprime `true` solo si la versión publicada es la mayor por orden semver (`sort -V`, prefijo `v` indiferente), y `false` en caso contrario.
4. En `docker/metadata-action`, la línea de `latest` se publica solo con `enable=${{ steps.latest_guard.outputs.is_latest == 'true' }}`. Las etiquetas `{{version}}` y `{{major}}.{{minor}}` se publican siempre.
5. El job declara `concurrency: { group: release-image, cancel-in-progress: false }`, de modo que dos releases no construyen la imagen a la vez.

Resultado: una release antigua que termine más tarde **no** mueve `latest` hacia atrás; la más alta gana siempre.

## Configuración / variables

No introduce variables de entorno nuevas. Depende de `GITHUB_REF_NAME` (lo provee el runner) y del remoto `origin` (lo configura `actions/checkout`).

## Cómo verificar

```bash
# Pruebas del guard (deben pasar los 4 casos)
bash scripts/latest-tag-guard.test.sh

# Simulación contra los tags reales del remoto
tags="$(git ls-remote --tags origin 'refs/tags/v*' | awk '{print $2}' | sed 's#refs/tags/##; s/\^{}//')"
scripts/latest-tag-guard.sh 0.3.3 $tags   # -> false (no toca latest)
scripts/latest-tag-guard.sh 0.4.0 $tags   # -> true  (publica latest)

# Estado del registro
podman manifest inspect ghcr.io/atareao/kesto:latest
```

## Decisiones

- **Guard por semver, no solo concurrencia.** Serializar los builds bastaría para el caso observado, pero no si un tag antiguo se empuja más tarde; el guard lo resuelve en cualquier orden.
- **`git ls-remote` en vez de `git fetch --tags`.** Evita depender de un clon completo y funciona con el checkout por defecto.
- **Solo condiciona `latest`.** Las etiquetas por versión y por `major.minor` se publican siempre: son inmutables y su publicación nunca degrada nada.
- **`podman`, no `docker`**, en este entorno de desarrollo.

## Follow-ups

- **Remedio del registro:** `latest` quedó en 0.3.3 tras la release 0.4.0. Reetiquetar a 0.4.0 requiere credenciales con `write:packages` (el token `gho_` de `gh` no las tiene):
  ```bash
  skopeo copy --all docker://ghcr.io/atareao/kesto:0.4.0 docker://ghcr.io/atareao/kesto:latest
  ```
- Valorar si `compose.yaml` debería fijar una versión concreta en vez de `latest` para despliegues reproducibles.
- Repetir el patrón de guard si aparecen otros alias de canal (por ejemplo `stable`).
