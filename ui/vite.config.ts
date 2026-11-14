import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

// Panel de Kesto. En desarrollo no hay CORS: Vite sirve el SPA y reenvía
// `/api/*` al backend en 127.0.0.1:8080. El proxy tiene que dejar pasar el
// stream SSE (`/api/v1/events`) sin acumularlo en un buffer, o el panel no
// recibiría los eventos hasta que la conexión se cerrase.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on('proxyReq', (proxyReq) => {
            // Sin compresión en la respuesta hacia el proxy: nada que acumule.
            proxyReq.setHeader('accept-encoding', 'identity')
          })
          proxy.on('proxyRes', (proxyRes) => {
            const contentType = String(proxyRes.headers['content-type'] ?? '')
            if (contentType.includes('text/event-stream')) {
              // Un `content-length` desconocido y anuncios de no-buffering a
              // cualquier intermediario delante del SPA.
              delete proxyRes.headers['content-length']
              proxyRes.headers['cache-control'] = 'no-cache'
              proxyRes.headers['x-accel-buffering'] = 'no'
            }
          })
        },
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './tests/setupTests.ts',
    globals: true,
    css: true,
  },
})
