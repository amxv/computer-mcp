import { describe, expect, it } from 'vitest'

import {
  focusedLiveboardUrl,
  parseLiveboardView,
  unifiedLiveboardUrl,
} from './view'

describe('Liveboard view URLs', () => {
  it('parses unified and focused views strictly', () => {
    expect(parseLiveboardView('')).toEqual({ kind: 'unified' })
    expect(parseLiveboardView('?agent=k7m2')).toEqual({ kind: 'focused', agentId: 'k7m2' })
    expect(() => parseLiveboardView('?agent=K7M2')).toThrow()
    expect(() => parseLiveboardView('?agent=k7m2&agent=a111')).toThrow()
  })

  it('builds canonical focus and clear-focus URLs', () => {
    const current = 'http://127.0.0.1:43123/capability/?junk=1#old'
    expect(focusedLiveboardUrl('k7m2', current)).toBe(
      'http://127.0.0.1:43123/capability/?agent=k7m2',
    )
    expect(unifiedLiveboardUrl(current)).toBe('http://127.0.0.1:43123/capability/')
  })
})
