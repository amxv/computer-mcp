import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'
import solid from 'vite-plugin-solid'

const runtimeProcess = (globalThis as {
  process?: { env?: Record<string, string | undefined> }
}).process
const browser = runtimeProcess?.env?.LIVEBOARD_BROWSER === 'webkit' ? 'webkit' : 'chromium'

export default defineConfig({
  plugins: [solid()],
  optimizeDeps: {
    include: [
      'highlight.js/lib/core',
      'highlight.js/lib/languages/bash',
      'highlight.js/lib/languages/go',
      'highlight.js/lib/languages/ini',
      'highlight.js/lib/languages/javascript',
      'highlight.js/lib/languages/json',
      'highlight.js/lib/languages/python',
      'highlight.js/lib/languages/rust',
      'highlight.js/lib/languages/typescript',
    ],
  },
  test: {
    name: 'browser',
    include: ['src/**/*.browser.test.tsx'],
    browser: {
      enabled: true,
      headless: true,
      provider: playwright(),
      instances: [{ browser }],
    },
  },
})
