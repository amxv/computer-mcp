import { describe, expect, it } from 'vitest'

import { connectionSummary } from './bootstrap'

describe('connectionSummary', () => {
  it('keeps the bootstrap status compact and deterministic', () => {
    expect(connectionSummary('runtime-12345678', 1, 2)).toBe(
      'runtime- · 1 Agent · 2 active processes',
    )
    expect(connectionSummary('abcd', 4, 1)).toBe('abcd · 4 Agents · 1 active process')
  })
})
