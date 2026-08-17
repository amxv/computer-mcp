import { defineConfig } from 'vite'
import solid from 'vite-plugin-solid'

function attachedLiveboardProxy() {
  const raw = process.env.LIVEBOARD_DEV_UPSTREAM
  if (!raw) return undefined

  const upstream = new URL(raw)
  const capabilityPath = upstream.pathname.endsWith('/')
    ? upstream.pathname
    : `${upstream.pathname}/`
  const proxy = {
    target: upstream.origin,
    changeOrigin: true,
    proxyTimeout: 0,
    timeout: 0,
    rewrite: (path: string) =>
      `${capabilityPath}${path.replace(/^\//, '')}`,
    configure: (server: {
      on: (
        event: 'proxyReq',
        callback: (request: { setHeader: (name: string, value: string) => void }) => void,
      ) => void
    }) => {
      server.on('proxyReq', (request) => {
        request.setHeader('origin', upstream.origin)
      })
    },
  }

  return {
    '/api': proxy,
    '/preferences': proxy,
  }
}

export default defineConfig({
  base: './',
  plugins: [solid()],
  server: {
    host: '127.0.0.1',
    proxy: attachedLiveboardProxy(),
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
    assetsDir: 'assets',
    emptyOutDir: true,
  },
  worker: {
    format: 'es',
  },
})
