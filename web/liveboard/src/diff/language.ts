export type DiffSyntaxLanguage =
  | 'rust'
  | 'typescript'
  | 'javascript'
  | 'go'
  | 'bash'
  | 'python'
  | 'json'
  | 'ini'

const EXTENSION_TO_LANGUAGE: Readonly<Record<string, DiffSyntaxLanguage>> = {
  bash: 'bash',
  cjs: 'javascript',
  cts: 'typescript',
  fish: 'bash',
  go: 'go',
  ini: 'ini',
  js: 'javascript',
  json: 'json',
  jsonc: 'json',
  jsx: 'javascript',
  mjs: 'javascript',
  mts: 'typescript',
  py: 'python',
  pyw: 'python',
  rs: 'rust',
  sh: 'bash',
  toml: 'ini',
  ts: 'typescript',
  tsx: 'typescript',
  zsh: 'bash',
}

export const EAGER_DIFF_LANGUAGES: readonly DiffSyntaxLanguage[] = [
  'rust',
  'typescript',
  'javascript',
  'go',
  'bash',
  'python',
  'json',
  'ini',
]

export function resolveDiffSyntaxLanguage(filePath: string): DiffSyntaxLanguage | null {
  const fileName = filePath.trim().toLowerCase().split('/').filter(Boolean).pop()
  if (!fileName) return null
  const dotIndex = fileName.lastIndexOf('.')
  if (dotIndex < 0 || dotIndex === fileName.length - 1) return null
  return EXTENSION_TO_LANGUAGE[fileName.slice(dotIndex + 1)] ?? null
}
