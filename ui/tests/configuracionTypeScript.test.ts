import { existsSync, readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * Frontera de TypeScript (§ refactor-tema-memoization): el código de aplicación
 * no debe ver los tipos de Node (ni los globales de Vitest). Solo lo necesitan
 * el proyecto de tooling (`vite.config.ts`) y los tests.
 *
 * Este test vive fuera de `ui/src` para quedar fuera del `tsconfig.json` de
 * aplicación y dentro de `ui/tsconfig.test.json`: es justo la frontera que
 * comprueba.
 */

const raizUi = resolve(dirname(fileURLToPath(import.meta.url)), '..')

interface Tsconfig {
  extends?: string
  include?: string[]
  exclude?: string[]
  compilerOptions?: { types?: string[]; tsBuildInfoFile?: string }
}

function leerTsconfig(nombre: string): Tsconfig {
  return JSON.parse(readFileSync(resolve(raizUi, nombre), 'utf8')) as Tsconfig
}

const TODOS = ['tsconfig.json', 'tsconfig.node.json', 'tsconfig.test.json']

describe('configuración de TypeScript', () => {
  it('existen los tsconfig de aplicación, tooling y tests', () => {
    for (const nombre of TODOS) {
      expect(existsSync(resolve(raizUi, nombre)), nombre).toBe(true)
    }
  })

  describe('tsconfig de aplicación', () => {
    const app = leerTsconfig('tsconfig.json')

    it('no expone los tipos de Node ni los globales de Vitest', () => {
      const tipos = app.compilerOptions?.types ?? []
      expect(tipos).not.toContain('node')
      expect(tipos).not.toContain('vitest/globals')
    })

    it('excluye los tests de `src` del programa de aplicación', () => {
      const exclude = app.exclude ?? []
      expect(exclude).toContain('src/**/*.test.ts')
      expect(exclude).toContain('src/**/*.test.tsx')
    })
  })

  describe('proyecto de tooling', () => {
    const tooling = leerTsconfig('tsconfig.node.json')

    it('conserva los tipos de Node para `vite.config.ts`', () => {
      expect(tooling.compilerOptions?.types ?? []).toContain('node')
    })
  })

  describe('tsconfig de tests', () => {
    const tests = leerTsconfig('tsconfig.test.json')

    it('extiende del de aplicación y declara Node y los globales de Vitest', () => {
      expect(tests.extends).toBe('./tsconfig.json')

      const tipos = tests.compilerOptions?.types ?? []
      expect(tipos).toContain('node')
      expect(tipos).toContain('vitest/globals')
    })

    it('no hereda el `exclude` de la aplicación (los tests sí entran)', () => {
      expect(tests.exclude).toEqual([])
    })

    it('tiene un `tsBuildInfoFile` propio, distinto del de aplicación', () => {
      const app = leerTsconfig('tsconfig.json')
      const propio = tests.compilerOptions?.tsBuildInfoFile

      expect(propio).toBeTruthy()
      expect(propio).not.toBe(app.compilerOptions?.tsBuildInfoFile)
    })
  })
})
