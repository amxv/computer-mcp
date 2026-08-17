import { defineConfig } from 'vitest/config'
import solid from 'vite-plugin-solid'

export default defineConfig({
  plugins: [solid()],
  test: {
    name: 'unit',
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.browser.test.tsx'],
  },
})
