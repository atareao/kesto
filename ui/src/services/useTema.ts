/**
 * Núcleo del tema del panel (§ identidad visual y tema): el tipo de modo, la
 * clave de persistencia, el lector del valor guardado, el contexto y el hook, y
 * la traducción de la paleta de marca a un `ThemeConfig` de Antd.
 *
 * Vive separado de `tema.tsx` —que exporta **solo** el componente
 * `ProveedorDeTema`— por la misma razón que `useEventStream.ts` lo está de
 * `events.tsx`: `react-refresh/only-export-components` exige que un módulo de
 * componentes no exporte además funciones, constantes ni contextos.
 */

import { createContext, useContext } from 'react'
import { theme } from 'antd'
import type { ThemeConfig } from 'antd'

/** Los dos modos que ofrece el panel. */
export type ModoDeTema = 'oscuro' | 'claro'

/** Clave de `localStorage` donde se recuerda la elección del operador. */
export const CLAVE_DE_TEMA = 'kesto-tema'

/** El modo por defecto del panel: oscuro, sin seguir la preferencia del sistema. */
const MODO_POR_DEFECTO: ModoDeTema = 'oscuro'

/** ¿El valor leído de `localStorage` es un modo válido? */
function esModoDeTema(valor: string | null): valor is ModoDeTema {
  return valor === 'oscuro' || valor === 'claro'
}

/**
 * Lee la elección guardada. Devuelve **oscuro** si no hay nada guardado, si el
 * valor no es válido o si el almacenamiento no está disponible.
 */
export function leerTemaGuardado(): ModoDeTema {
  try {
    const guardado = window.localStorage.getItem(CLAVE_DE_TEMA)
    return esModoDeTema(guardado) ? guardado : MODO_POR_DEFECTO
  } catch {
    // Almacenamiento no disponible (modo privado, políticas del navegador…).
    return MODO_POR_DEFECTO
  }
}

/** Lo que el contexto entrega a los consumidores del tema. */
export interface ValorDelTema {
  /** El modo activo. */
  modo: ModoDeTema
  /** Cambia al modo contrario y persiste la elección. */
  alternar: () => void
}

/** Contexto del tema. `null` fuera del proveedor: el hook avisa de ello. */
export const ContextoDelTema = createContext<ValorDelTema | null>(null)

/**
 * Acceso al tema activo. Lanza si se usa fuera de `ProveedorDeTema`: sin el
 * proveedor no hay modo que leer y devolver un valor por defecto escondería el
 * fallo de montaje.
 */
export function useTema(): ValorDelTema {
  const contexto = useContext(ContextoDelTema)
  if (contexto === null) {
    throw new Error('useTema debe usarse dentro de <ProveedorDeTema>')
  }
  return contexto
}

/** Cyan de marca (`colorPrimary`), un valor común a los dos modos. */
const CYAN_DE_MARCA = '#2194AA'

/**
 * Fondo del `body` y `theme-color` por modo: las superficies de marca
 * (`surface_dark`/`surface_light` de `brand-tokens.json`).
 */
export const FONDO_DEL_TEMA: Record<ModoDeTema, string> = {
  oscuro: '#0D1B2A',
  claro: '#F8FAFC',
}

/**
 * Acento de **texto** por modo (enlaces y texto sobre selección). El cyan de
 * marca no vale como color de texto: `#2194AA` sobre el navy del panel da
 * ≈3.8:1 y sobre blanco ≈3.6:1, por debajo del 4.5:1 de WCAG AA. En oscuro se
 * aclara el cyan y en claro se oscurece, hasta superar AA (lo comprueba
 * `tema.test.tsx`).
 */
const ACENTO_DE_TEXTO: Record<ModoDeTema, string> = {
  oscuro: '#5CC8DC',
  claro: '#18788C',
}

/**
 * Colores de la entrada de **menú seleccionada** por modo. En oscuro no se usa
 * `colorPrimary` como fondo con texto blanco (≈3.6:1): se pinta un navy algo más
 * claro con el cyan claro de texto. En claro, el fondo es el cyan oscuro con
 * texto blanco. Ambos pares superan AA.
 */
const MENU_SELECCIONADO: Record<ModoDeTema, { fondo: string; texto: string }> = {
  oscuro: { fondo: '#1B3B57', texto: '#5CC8DC' },
  claro: { fondo: '#18788C', texto: '#FFFFFF' },
}

/**
 * Texto sobre el cyan de marca cuando este es **fondo** (botón sólido). Blanco
 * sobre `#2194AA` da ≈3.6:1; el navy casi negro lo lleva a ≈4.9:1.
 */
const TEXTO_SOBRE_PRIMARIO = '#0D1B2A'

/**
 * Construye el `ThemeConfig` de Antd para un modo: el algoritmo de Antd, los
 * tokens de marca y los ajustes de componente que hacen legible el acento de
 * marca en cada tema (menú seleccionado y botón sólido).
 */
export function temaDeAntd(modo: ModoDeTema): ThemeConfig {
  const esOscuro = modo === 'oscuro'
  const seleccionado = MENU_SELECCIONADO[modo]

  return {
    algorithm: esOscuro ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      colorPrimary: CYAN_DE_MARCA,
      colorSuccess: '#7EBC50',
      colorWarning: '#F08724',
      colorInfo: '#33998D',
      colorLink: ACENTO_DE_TEXTO[modo],
      colorBgLayout: FONDO_DEL_TEMA[modo],
      colorBgContainer: esOscuro ? '#123048' : '#FFFFFF',
      borderRadius: 6,
    },
    components: {
      // El botón sólido usa `colorPrimary` de fondo: su texto debe contrastar.
      Button: { primaryColor: TEXTO_SOBRE_PRIMARIO },
      // El `Menu` del panel se pinta en `theme="dark"` u `theme="light"` según el
      // modo, así que se fijan las cuatro variantes para no arrastrar el defecto.
      Menu: {
        itemSelectedBg: seleccionado.fondo,
        itemSelectedColor: seleccionado.texto,
        darkItemSelectedBg: seleccionado.fondo,
        darkItemSelectedColor: seleccionado.texto,
      },
    },
  }
}
