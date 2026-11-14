/**
 * El proveedor del tema claro/oscuro (§ identidad visual y tema).
 *
 * Monta el contexto con el modo activo y `alternar()`. Al alternar persiste la
 * elección en `localStorage` y el efecto sincroniza `document.documentElement`
 * (`data-tema`) para que `estilos.css` ajuste el fondo del `body` con una
 * variable CSS: así el color de fondo no depende de un valor fijo a un tema.
 *
 * Este fichero exporta un componente y solo un componente, que es lo que exige
 * `react-refresh/only-export-components`; el tipo, la clave, el lector, el
 * contexto, el hook y los tokens viven en `useTema.ts`.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  CLAVE_DE_TEMA,
  ContextoDelTema,
  FONDO_DEL_TEMA,
  leerTemaGuardado,
} from './useTema'
import type { ModoDeTema, ValorDelTema } from './useTema'

/** Persiste la elección; un almacenamiento no disponible no debe romper el tema. */
function guardarTema(modo: ModoDeTema): void {
  try {
    window.localStorage.setItem(CLAVE_DE_TEMA, modo)
  } catch {
    // Sin persistencia el tema sigue funcionando durante la sesión.
  }
}

/**
 * Refleja el modo activo en el documento: `data-tema` para el CSS y el
 * `theme-color` para que la barra del navegador (móvil) case con el fondo real
 * en vez de quedarse en el valor del arranque.
 */
function sincronizarDocumento(modo: ModoDeTema): void {
  document.documentElement.dataset.tema = modo

  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta !== null) {
    meta.setAttribute('content', FONDO_DEL_TEMA[modo])
  }
}

export function ProveedorDeTema({ children }: { children: ReactNode }) {
  const [modo, setModo] = useState<ModoDeTema>(leerTemaGuardado)

  // El documento refleja el modo activo para que el CSS lo pueda consultar.
  useEffect(() => {
    sincronizarDocumento(modo)
  }, [modo])

  const alternar = useCallback(() => {
    const siguiente: ModoDeTema = modo === 'oscuro' ? 'claro' : 'oscuro'
    guardarTema(siguiente)
    setModo(siguiente)
  }, [modo])

  const valor = useMemo<ValorDelTema>(() => ({ modo, alternar }), [modo, alternar])

  return <ContextoDelTema.Provider value={valor}>{children}</ContextoDelTema.Provider>
}
