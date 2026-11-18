---
# FEATURE-032 — Nueva identidad visual del panel (issue #131)

## Qué es

El panel servía el **logo antiguo** (cubo K teal/verde) en todos sus assets, mientras en
`assets/` ya vivía la identidad nueva: el **cubo K oscuro con circuitos y detalles naranja**.
Esta feature regenera favicons, iconos PWA, el logo del lateral, el emblema de la portada y
la imagen de Open Graph desde el arte nuevo, para que no convivan dos identidades.

## Arte de referencia

| Uso | Fichero | Notas |
|---|---|---|
| Icono | `assets/icono-3000.svg` | 3000×3000, transparente; idéntico a `assets/public/assets/icon.svg` y `assets/docs/assets/logo.svg` |
| Lockup | `assets/logo.svg` | 956×441: cubo + «KESTO» gris + «BACKUP ORCHESTRATOR» naranja |

`assets/public/assets/` es la copia publicable del set nuevo; `assets/docs/assets/` ya estaba
generada del arte nuevo y no se toca.

## Ficheros implicados

Regenerados (binarios):

- `ui/public/`: `favicon.ico`, `favicon-16x16.png`, `favicon-32x32.png`, `favicon-48x48.png`,
  `favicon-64x64.png`, `apple-touch-icon.png`, `android-chrome-192x192.png`,
  `android-chrome-512x512.png`, `maskable-icon-512x512.png`, `mstile-150x150.png`,
  `og-image-dark.png` y `icon.svg` (nuevo).
- `ui/src/assets/`: `kesto-icon.png` (128×128, transparente), `kesto-icon-dark.png`
  (128×128 con plato `rgba(255,255,255,0.10)`), `kesto-emblema.png` (512×512 con el mismo plato).

Código:

- `ui/index.html`: añade `<link rel="icon" type="image/svg+xml" href="/icon.svg" />`.
- `ui/src/pages/portada.tsx`: el emblema pasa de 256×225 a 256×256 (el arte nuevo es cuadrado).
- `ui/src/pages/portada.test.tsx`: la aserción de proporción se ajusta a `height=256`.
- `ui/src/identidadDeMarca.test.ts`: `icon.svg` entra en la lista de assets servidos desde `public/`.

## Cómo funciona

Vite sirve `ui/public/` en la raíz del sitio; `index.html` declara los favicons, el manifiesto
y las meta de Open Graph, y `site.webmanifest` referencia `android-chrome-*` y el icono
`maskable`. Las imágenes de `ui/src/assets/` se importan desde el código y Vite las procesa
como módulos (hash de contenido).

## Cómo regenerar

Fondo por defecto del panel `#0D1B2A`; color de tile de Windows `#123048`. Renderizadores
usados: `rsvg-convert` y `magick`.

```
SRC=assets/icono-3000.svg
rsvg-convert -w 16 -h 16 "$SRC" -o ui/public/favicon-16x16.png
rsvg-convert -w 32 -h 32 "$SRC" -o ui/public/favicon-32x32.png
rsvg-convert -w 48 -h 48 "$SRC" -o ui/public/favicon-48x48.png
rsvg-convert -w 64 -h 64 "$SRC" -o ui/public/favicon-64x64.png
rsvg-convert -w 192 -h 192 "$SRC" -o ui/public/android-chrome-192x192.png
rsvg-convert -w 512 -h 512 "$SRC" -o ui/public/android-chrome-512x512.png
magick ui/public/favicon-16x16.png ui/public/favicon-32x32.png ui/public/favicon-48x48.png ui/public/favicon.ico

# apple-touch-icon: iOS no respeta transparencia
rsvg-convert -w 180 -h 180 "$SRC" -o /tmp/apple.png
magick /tmp/apple.png -background '#0D1B2A' -flatten -alpha off ui/public/apple-touch-icon.png

# maskable: arte al 70 % (358 px) dentro de la zona segura del 80 %
rsvg-convert -w 358 -h 358 "$SRC" -o /tmp/maskable-art.png
magick -size 512x512 xc:'#0D1B2A' /tmp/maskable-art.png -gravity center -composite ui/public/maskable-icon-512x512.png

# mosaico de Windows
rsvg-convert -w 105 -h 105 "$SRC" -o /tmp/mstile-art.png
magick -size 150x150 xc:'#123048' /tmp/mstile-art.png -gravity center -composite ui/public/mstile-150x150.png

cp "$SRC" ui/public/icon.svg

# Open Graph 1200×630 con el lockup centrado
rsvg-convert -w 900 assets/logo.svg -o /tmp/lockup.png
magick -size 1200x630 xc:'#0D1B2A' /tmp/lockup.png -gravity center -composite ui/public/og-image-dark.png

# Arte del panel
rsvg-convert -w 128 -h 128 "$SRC" -o ui/src/assets/kesto-icon.png
rsvg-convert -w 128 -h 128 "$SRC" -o /tmp/icon-128.png
magick -size 128x128 xc:none -fill 'rgba(255,255,255,0.10)' -draw 'roundrectangle 0,0,127,127,16,16' /tmp/icon-128.png -gravity center -composite ui/src/assets/kesto-icon-dark.png
rsvg-convert -w 512 -h 512 "$SRC" -o /tmp/icon-512.png
magick -size 512x512 xc:none -fill 'rgba(255,255,255,0.10)' -draw 'roundrectangle 0,0,511,511,64,64' /tmp/icon-512.png -gravity center -composite ui/src/assets/kesto-emblema.png
```

## Decisiones

- **Plato claro al 10 % en el arte oscuro dentro del panel.** El cubo nuevo es oscuro: sobre el
  `Sider` oscuro y sobre la portada (fondo `#0D1B2A`) se fundiría con el fondo. Un plato blanco
  al 10 % se ve solo sobre fondo oscuro y es invisible sobre fondo claro, así que el mismo
  asset sirve en los dos temas.
- **`apple-touch-icon`, `maskable` y `mstile` opacos**: iOS, Android y Windows no respetan la
  transparencia en estos formatos; se aplanan sobre el color de fondo correspondiente.
- **Tamaños 4×/2×**: los iconos del lateral se pintan a 32×32 CSS (se generan a 128) y el
  emblema a 256×256 CSS (se genera a 512), para pantallas de alta densidad.
- **`og-image`**: se recompone con el lockup sobre el fondo oscuro del panel; el texto «KESTO»
  del lockup es gris oscuro y sobre navy queda apagado pero legible (verificado a ojo).

## Verificación

`just ui-check` en verde: typecheck, typecheck de tests, lint, **307 tests** y build.

## Follow-ups

- **Copy desalineado:** `site.webmanifest` (`"description": "Backups & Data Security"`) y
  `og:title` (`Kesto — Backups & Data Security`) siguen con la tagline antigua, mientras el
  lockup dice «BACKUP ORCHESTRATOR». Pendiente de decisión.
- El favicon de 16×16 es poco legible por el detalle del arte; es inevitable a ese tamaño y
  el navegador moderno usará `icon.svg` cuando pueda.
- `assets/` sigue siendo la fuente de verdad: si el arte cambia, hay que repetir los comandos
  de arriba (no hay receta `just` para ello todavía).
---