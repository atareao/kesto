import { existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/**
 * Guarda de la identidad de marca del panel (§ identidad visual y tema): el
 * documento declara el favicon, el manifiesto, `theme-color` y las meta de Open
 * Graph, y los assets se sirven desde `public/` (Vite los publica en la raíz).
 *
 * Los ficheros se resuelven desde la raíz de `ui/` (`process.cwd()` en la
 * prueba): `import.meta.url` no sirve aquí porque jsdom no lo deja como URL
 * `file:`.
 */
const INDICE = readFileSync(resolve(process.cwd(), 'index.html'), 'utf8')
const MANIFIESTO = readFileSync(resolve(process.cwd(), 'public', 'site.webmanifest'), 'utf8')

function assetPublico(ruta: string): boolean {
  return existsSync(resolve(process.cwd(), 'public', ruta))
}

describe('index.html declara la identidad de marca', () => {
  it('declara el favicon y el manifiesto', () => {
    expect(INDICE).toMatch(/<link[^>]+rel="icon"/i)
    expect(INDICE).toMatch(/<link[^>]+rel="manifest"[^>]+href="\/site\.webmanifest"/i)
  })

  it('declara el color de tema alineado con el fondo oscuro por defecto', () => {
    expect(INDICE).toMatch(/<meta[^>]+name="theme-color"[^>]+content="#0D1B2A"/i)
  })

  it('declara las meta de Open Graph', () => {
    expect(INDICE).toMatch(/<meta[^>]+property="og:title"/i)
    expect(INDICE).toMatch(/<meta[^>]+property="og:description"/i)
    expect(INDICE).toMatch(/<meta[^>]+property="og:image"/i)
  })

  it('el manifiesto usa el fondo oscuro por defecto', () => {
    expect(MANIFIESTO).toMatch(/"theme_color":\s*"#0D1B2A"/i)
    expect(MANIFIESTO).toMatch(/"background_color":\s*"#0D1B2A"/i)
  })
})

describe('los assets de marca se sirven desde public/', () => {
  it.each([
    'favicon.ico',
    'favicon-16x16.png',
    'favicon-32x32.png',
    'favicon-48x48.png',
    'favicon-64x64.png',
    'apple-touch-icon.png',
    'android-chrome-192x192.png',
    'android-chrome-512x512.png',
    'maskable-icon-512x512.png',
    'mstile-150x150.png',
    'site.webmanifest',
    'og-image-dark.png',
  ])('existe public/%s', (ruta) => {
    expect(assetPublico(ruta)).toBe(true)
  })
})
