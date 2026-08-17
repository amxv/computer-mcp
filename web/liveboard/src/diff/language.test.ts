import { describe, expect, it } from 'vitest'

import { EAGER_DIFF_LANGUAGES, resolveDiffSyntaxLanguage } from './language'

describe('diff language mapping', () => {
  it('maps the eager common Zodex file set without a network language load', () => {
    expect(resolveDiffSyntaxLanguage('src/main.rs')).toBe('rust')
    expect(resolveDiffSyntaxLanguage('ui/View.tsx')).toBe('typescript')
    expect(resolveDiffSyntaxLanguage('ui/worker.mts')).toBe('typescript')
    expect(resolveDiffSyntaxLanguage('legacy/bridge.cts')).toBe('typescript')
    expect(resolveDiffSyntaxLanguage('client.jsx')).toBe('javascript')
    expect(resolveDiffSyntaxLanguage('worker.mjs')).toBe('javascript')
    expect(resolveDiffSyntaxLanguage('config.cjs')).toBe('javascript')
    expect(resolveDiffSyntaxLanguage('cmd/server.go')).toBe('go')
    expect(resolveDiffSyntaxLanguage('scripts/build.sh')).toBe('bash')
    expect(resolveDiffSyntaxLanguage('shell/login.zsh')).toBe('bash')
    expect(resolveDiffSyntaxLanguage('shell/config.fish')).toBe('bash')
    expect(resolveDiffSyntaxLanguage('tools/check.pyw')).toBe('python')
    expect(resolveDiffSyntaxLanguage('tsconfig.jsonc')).toBe('json')
    expect(resolveDiffSyntaxLanguage('Cargo.toml')).toBe('ini')
    expect(resolveDiffSyntaxLanguage('.config/app.ini')).toBe('ini')
    expect(resolveDiffSyntaxLanguage('README.md')).toBeNull()
    expect(EAGER_DIFF_LANGUAGES).toEqual([
      'rust',
      'typescript',
      'javascript',
      'go',
      'bash',
      'python',
      'json',
      'ini',
    ])
  })
})
