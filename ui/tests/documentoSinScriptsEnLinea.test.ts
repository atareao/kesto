import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * Frontera de la política de seguridad de contenido
 * (§ endurecimiento-cabeceras-y-panel): el servidor sirve `script-src 'self'`,
 * sin `'unsafe-inline'` ni hashes. El documento del panel no puede llevar por
 * tanto NINGÚN `<script>` con cuerpo en línea: todo `<script>` debe tener `src`
 * y apuntar a un fichero del mismo origen.
 *
 * El arranque de tema va en `ui/public/tema-inicial.js` y se carga de forma
 * síncrona desde el `<head>` para fijar el tema antes del primer pintado.
 *
 * Este test vive fuera de `ui/src` y lee el HTML del disco con `node:fs`, el
 * mismo estilo que los tests de tooling.
 */

const rutaIndice = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'index.html')
const rutaArranqueDeTema = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  'public',
  'tema-inicial.js',
)
const documento = readFileSync(rutaIndice, 'utf8')

/**
 * El documento sin sus comentarios HTML: un comentario que documente
 * `<script>…</script>` no es un script del panel y no debe teñir el barrido de
 * rojo (falso positivo por comentarios).
 */
const documentoActivo = documento.replace(/<!--[\s\S]*?-->/g, '')

/** Etiquetas de apertura de `<script …>`, en orden de aparición. */
const aperturasDeScript = documentoActivo.match(/<script\b[^>]*>/gi) ?? []

/** La apertura concreta que debe materializar el arranque de tema. */
const SCRIPT_DE_TEMA = /<script\s+src="\/tema-inicial\.js"\s*>/i

/** Cadenas que debe contener el arranque de tema para fijarlo antes de pintar. */
const CADENAS_DE_TEMA = ['kesto-tema', 'dataset.tema', 'theme-color'] as const

describe("documento del panel bajo `script-src 'self'`", () => {
  it('no contiene ningún `<script>` con cuerpo en línea', () => {
    const enLinea = aperturasDeScript.filter((apertura) => !/\bsrc\s*=/i.test(apertura))

    expect(
      enLinea,
      `Hay <script> en línea que romperían la CSP (script-src 'self'):\n${enLinea.join('\n')}`,
    ).toEqual([])
  })

  it('carga el arranque de tema desde un fichero propio del mismo origen', () => {
    expect(
      aperturasDeScript,
      'No se encontró un `<script src="/tema-inicial.js">` en el documento.',
    ).toContainEqual(expect.stringMatching(SCRIPT_DE_TEMA))

    // Síncrono y clásico: si llevara `defer`/`type="module"` correría tras el
    // primer pintado y habría destello de tema.
    expect(documentoActivo).toMatch(SCRIPT_DE_TEMA)
  })

  it('el arranque de tema conserva el fijado de tema antes del primer pintado', () => {
    let arranque: string
    try {
      arranque = readFileSync(rutaArranqueDeTema, 'utf8')
    } catch {
      throw new Error(
        `No se pudo leer el arranque de tema en ${rutaArranqueDeTema}: el destello de tema volvería sin aviso.`,
      )
    }

    // El fichero vacío deja el `<script src=…>` apuntando a la nada y el tema
    // se fijaría tarde: el destello volvería sin tocar el HTML.
    expect(
      arranque.replace(/\s/g, ''),
      'El arranque de tema está vacío: el destello de tema volvería sin aviso.',
    ).not.toBe('')

    for (const cadena of CADENAS_DE_TEMA) {
      expect(
        arranque,
        `El arranque de tema no contiene \`${cadena}\`: no fijaría el tema antes del primer pintado.`,
      ).toContain(cadena)
    }
  })
})
