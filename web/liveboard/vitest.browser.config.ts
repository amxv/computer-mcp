import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'
import solid from 'vite-plugin-solid'

const runtimeProcess = (globalThis as {
  process?: { env?: Record<string, string | undefined> }
}).process
const browser = runtimeProcess?.env?.LIVEBOARD_BROWSER === 'webkit' ? 'webkit' : 'chromium'

export default defineConfig({
  plugins: [solid()],
  test: {
    name: 'browser',
    include: ['src/**/*.browser.test.tsx'],
    browser: {
      enabled: true,
      provider: playwright(),
      instances: [{ browser }],
    },
  },
})
