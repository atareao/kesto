# Design

## Contexto

El panel es un SPA de React 19 + Antd v5 (`ui/`) servido por el binario. Hoy no hay `ConfigProvider`, así que Antd pinta su tema claro por defecto y la marca es inexistente. Los assets viven fuera del repo (`~/Descargas/kesto-web-assets/`) y hay que incorporarlos.

## Decisiones de diseño

### Mapeo de la paleta a tokens de Antd

La paleta oficial (`brand-tokens.json`) se traduce a tokens de Antd v5 en un único punto:

- `colorPrimary: #2194AA` (cyan).
- `colorSuccess: #7EBC50`, `colorWarning: #F08724`, `colorInfo: #33998D`, `colorLink: #2194AA`.
- Oscuro (`algorithm: darkAlgorithm`): `colorBgLayout: #0D1B2A`, `colorBgContainer: #123048` (navy).
- Claro (`algorithm: defaultAlgorithm`): `colorBgLayout: #F8FAFC`, `colorBgContainer: #FFFFFF`.
- `borderRadius` un poco mayor que el defecto para un aire más cuidado; tipografía por defecto.

### Arquitectura del tema

- Un módulo `ui/src/services/tema.tsx` con: el tipo `ModoDeTema` (`'oscuro' | 'claro'`), la clave de `localStorage` (`kesto-tema`), el lector `leerTemaGuardado()` —**oscuro por defecto**—, un `ProveedorDeTema` (contexto React con el modo y `alternar()`, que persiste) y el hook `useTema()`.
- La función que construye el `ThemeConfig` de Antd a partir del modo vive junto al proveedor.
- `main.tsx` envuelve el árbol: `ProveedorDeTema` → componente que lee `useTema()` y monta el `ConfigProvider` → `QueryClientProvider` → `App`.
- Sincronización con el documento: el proveedor escribe `document.documentElement.dataset.tema` para que `estilos.css` pueda ajustar el fondo del `body` con una variable CSS (evita el destello blanco tras el `body`).

### Assets

- Favicons y `site.webmanifest` van a `ui/public/` (Vite los sirve en la raíz: `/favicon.ico`, `/site.webmanifest`…). El manifest referencia rutas absolutas, coherentes con `public/`.
- Logos e imágenes de marca van a `ui/src/assets/` y se **importan** para que Vite las huella y las sirva con el resto del bundle.

### Layout y estados

- `Layout` deja de fijar `theme="light"` y toma el color de la cabecera y del lateral de los tokens (`theme.useToken()`), de modo que ambos temas sean coherentes sin colores a mano.
- El conmutador de tema vive en la cabecera, junto al botón de plegado; es un control accesible (con nombre, alcanzable y activable con teclado) que anuncia su estado.
- El logo del lateral usa la variante oscura o clara según el modo.

## Estrategia de pruebas

- Unidad del módulo de tema: por defecto oscuro sin nada guardado; se respeta un valor guardado; `alternar()` cambia y persiste.
- Componente de toggle: se pinta con nombre accesible y alterna al activarse con teclado.
- Regresión del documento: `index.html` declara el favicon y el manifest; los assets existen en `public/`.
- Los tests existentes de `Layout` y de las pantallas siguen en verde (no se rompe el comportamiento ya especificado en `ui`).
