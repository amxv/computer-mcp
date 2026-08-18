import { defineConfig, type ProxyOptions } from 'vite'
import solid from 'vite-plugin-solid'

function attachedLiveboardProxy(): Record<string, string | ProxyOptions> | undefined {
  const raw = process.env.LIVEBOARD_DEV_UPSTREAM
  if (!raw) return undefined

  const upstream = new URL(raw)
  const capabilityPath = upstream.pathname.endsWith('/')
    ? upstream.pathname
    : `${upstream.pathname}/`
  const proxy: ProxyOptions = {
    target: upstream.origin,
    changeOrigin: true,
    proxyTimeout: 0,
    timeout: 0,
    rewrite: (path: string) =>
      `${capabilityPath}${path.replace(/^\//, '')}`,
    configure: (server) => {
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
