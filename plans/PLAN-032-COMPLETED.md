# PLAN-032 — Migración de la UI a la nueva identidad visual (issue #131)

## Objetivo

Sustituir en el panel los assets del logo antiguo (cubo teal/verde) por el arte nuevo
(cubo K oscuro con circuitos naranja), de forma coherente en `ui/public`, `ui/src/assets`
y la imagen Open Graph.

## Estado de partida (verificado)

- **0 de 8** hashes coinciden entre `assets/public/assets/` (arte nuevo) y `ui/public/` (arte viejo).
- `ui/src/assets/kesto-icon.png`, `kesto-icon-dark.png` y `kesto-emblema.png` también son arte viejo.
- `assets/docs/assets/` ya está generado del arte nuevo: **no se toca**.

## Fuente de verdad del arte

| Uso | Fichero | Notas |
|---|---|---|
| Icono | `assets/icono-3000.svg` | 3000×3000, transparente, `viewBox 0 0 793.75 793.75`; idéntico (sha `7935c86…`) a `assets/public/assets/icon.svg` y `assets/docs/assets/logo.svg` |
| Lockup | `assets/logo.svg` | 956×441: cubo + «KESTO» gris + «BACKUP ORCHESTRATOR» naranja |

Renderizadores disponibles: `resvg`, `rsvg-convert`, `inkscape`, `magick`.

## Tareas

### 1. `ui/public/` — favicons y assets PWA

Fondo por defecto del panel: `#0D1B2A`. Color de tile de Windows: `#123048`.

- [x] 1.1 Regenerar los PNG desde el icono nuevo:
  ```
  SRC=assets/icono-3000.svg
  rsvg-convert -w 16 -h 16 "$SRC" -o ui/public/favicon-16x16.png
  rsvg-convert -w 32 -h 32 "$SRC" -o ui/public/favicon-32x32.png
  rsvg-convert -w 48 -h 48 "$SRC" -o ui/public/favicon-48x48.png
  rsvg-convert -w 64 -h 64 "$SRC" -o ui/public/favicon-64x64.png
  rsvg-convert -w 192 -h 192 "$SRC" -o ui/public/android-chrome-192x192.png
  rsvg-convert -w 512 -h 512 "$SRC" -o ui/public/android-chrome-512x512.png
  ```
- [x] 1.2 `favicon.ico` multirresolución (16 + 32 + 48):
  ```
  magick ui/public/favicon-16x16.png ui/public/favicon-32x32.png ui/public/favicon-48x48.png ui/public/favicon.ico
  ```
- [x] 1.3 `apple-touch-icon.png` (180×180) opaco sobre `#0D1B2A` (iOS no respeta transparencia):
  ```
  rsvg-convert -w 180 -h 180 "$SRC" -o /tmp/opencode/kesto-assets/apple.png
  magick /tmp/opencode/kesto-assets/apple.png -background '#0D1B2A' -flatten ui/public/apple-touch-icon.png
  ```
- [x] 1.4 `maskable-icon-512x512.png`: fondo opaco `#0D1B2A` y arte al 70 % (358 px) centrado, dentro de la zona segura circular del 80 %:
  ```
  rsvg-convert -w 358 -h 358 "$SRC" -o /tmp/opencode/kesto-assets/maskable-art.png
  magick -size 512x512 xc:'#0D1B2A' /tmp/opencode/kesto-assets/maskable-art.png -gravity center -composite ui/public/maskable-icon-512x512.png
  ```
- [x] 1.5 `mstile-150x150.png`: fondo `#123048` y arte a 105 px centrado:
  ```
  rsvg-convert -w 105 -h 105 "$SRC" -o /tmp/opencode/kesto-assets/mstile-art.png
  magick -size 150x150 xc:'#123048' /tmp/opencode/kesto-assets/mstile-art.png -gravity center -composite ui/public/mstile-150x150.png
  ```
- [x] 1.6 `ui/public/icon.svg`: copia del SVG nuevo (`cp "$SRC" ui/public/icon.svg`).
- [x] 1.7 `og-image-dark.png` (1200×630): fondo `#0D1B2A` y lockup centrado a 900 px de ancho:
  ```
  rsvg-convert -w 900 assets/logo.svg -o /tmp/opencode/kesto-assets/lockup.png
  magick -size 1200x630 xc:'#0D1B2A' /tmp/opencode/kesto-assets/lockup.png -gravity center -composite ui/public/og-image-dark.png
  ```

### 2. `ui/src/assets/` — arte dentro del panel

Los dos iconos se pintan a 32×32 CSS (`.logo-del-panel`) y el emblema a 256×256 CSS
(`portada.tsx`), así que se generan a 4× y 2× para pantallas de alta densidad.

- [x] 2.1 `kesto-icon.png` (tema claro): 128×128, transparente, sobre superficie clara se lee solo.
- [x] 2.2 `kesto-icon-dark.png` (tema oscuro): 128×128 con plato claro sutil, para que el cubo oscuro no se funda con el `Sider` oscuro:
  ```
  magick -size 128x128 xc:none -fill 'rgba(255,255,255,0.10)' -draw 'roundrectangle 0,0,127,127,16,16' /tmp/opencode/kesto-assets/icon-128.png -gravity center -composite ui/src/assets/kesto-icon-dark.png
  ```
- [x] 2.3 `kesto-emblema.png`: 512×512 con el mismo plato (se ve a 256×256; sirve en claro y en oscuro).

### 3. Código

- [x] 3.1 `ui/src/pages/portada.tsx`: el emblema pasa de `width={256} height={225}` a `256×256` (el arte nuevo es cuadrado 1:1) y se corrige el comentario de la proporción.
- [x] 3.2 `ui/src/pages/portada.test.tsx`: la aserción de proporción pasa a `height` `256`.
- [x] 3.3 `ui/src/identidadDeMarca.test.ts`: añadir `icon.svg` a la lista de assets servidos desde `public/`.
- [x] 3.4 `ui/index.html`: añadir `<link rel="icon" type="image/svg+xml" href="/icon.svg" />` junto a los demás favicons.

### 4. Verificación

- [x] 4.1 `just ui-check` (typecheck, typecheck:test, lint, test, build) en verde.
- [x] 4.2 Comprobación visual de los renders con `identify` (dimensiones y canales) y revisión de las imágenes generadas.

### 5. Cierre

- [x] 5.1 `docs/FEATURE-032-nueva-identidad-visual.md` con qué es, ficheros implicados, comandos de generación y follow-ups.
- [x] 5.2 PR a `development` citando el issue #131.
- [x] 5.3 Renombrar este plan a `plans/PLAN-032-COMPLETED.md` y marcar las tareas.

## Fuera de alcance

- Copy y meta: `site.webmanifest` (`"description": "Backups & Data Security"`) y `og:title` siguen con la tagline antigua, mientras el lockup nuevo dice «BACKUP ORCHESTRATOR». Se decide en otro cambio.
- `assets/` no se modifica: es la fuente de verdad.
- No se commitea nada de `ui/dist` ni `ui/node_modules`.

## Riesgos

- El arte nuevo es oscuro: sobre el `Sider` oscuro se pierde sin el plato claro del punto 2.2 (por eso existe).
- `maskable` y `mstile` no tienen equivalente en el set nuevo; son derivados y hay que revisarlos a ojo.