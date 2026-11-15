// Los matchers de jest-dom (`toBeInTheDocument`, …) se registran en `expect` de
// Vitest: la entrada `/vitest` es la que declara la augmentación de tipos (la
// raíz del paquete augmenta el `expect` de Jest y no compila aquí).
import '@testing-library/jest-dom/vitest'

// jsdom no implementa `window.matchMedia`, y Ant Design lo consulta para el
// `breakpoint` del `Sider`. Este doble evalúa las consultas `min-width`/
// `max-width` contra `window.innerWidth`, que es lo que necesita la prueba del
// menú plegable en ventana estrecha.
if (typeof window !== 'undefined' && typeof window.matchMedia !== 'function') {
  window.matchMedia = ((consulta: string) => {
    const maximo = /max-width:\s*([\d.]+)px/.exec(consulta)
    const minimo = /min-width:\s*([\d.]+)px/.exec(consulta)

    const evaluar = (): boolean => {
      let casa = true
      if (maximo !== null) casa = casa && window.innerWidth <= Number(maximo[1])
      if (minimo !== null) casa = casa && window.innerWidth >= Number(minimo[1])
      return casa
    }

    const oyentes = new Set<(evento: MediaQueryListEvent) => void>()
    const lista = {
      get matches() {
        return evaluar()
      },
      media: consulta,
      onchange: null,
      addEventListener: (_tipo: string, oyente: (evento: MediaQueryListEvent) => void) => {
        oyentes.add(oyente)
      },
      removeEventListener: (_tipo: string, oyente: (evento: MediaQueryListEvent) => void) => {
        oyentes.delete(oyente)
      },
      addListener: (oyente: (evento: MediaQueryListEvent) => void) => {
        oyentes.add(oyente)
      },
      removeListener: (oyente: (evento: MediaQueryListEvent) => void) => {
        oyentes.delete(oyente)
      },
      dispatchEvent: () => false,
    }
    return lista as unknown as MediaQueryList
  }) as typeof window.matchMedia
}
