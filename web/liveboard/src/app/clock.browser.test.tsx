import { createRoot, type Accessor } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { createCoarseClock } from './clock'

afterEach(() => {
  vi.useRealTimers()
})

describe('coarse shared clock', () => {
  it('ticks once per interval while visible, pauses while hidden, and refreshes on return', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(1_000)
    const descriptor = Object.getOwnPropertyDescriptor(document, 'hidden')
    let hidden = false
    Object.defineProperty(document, 'hidden', {
      configurable: true,
      get: () => hidden,
    })

    let now!: Accessor<number>
    const dispose = createRoot((rootDispose) => {
      now = createCoarseClock(1_000)
      return rootDispose
    })
    expect(now()).toBe(1_000)

    await vi.advanceTimersByTimeAsync(1_000)
    expect(now()).toBe(2_000)

    hidden = true
    document.dispatchEvent(new Event('visibilitychange'))
    const hiddenValue = now()
    vi.setSystemTime(7_000)
    await vi.advanceTimersByTimeAsync(5_000)
    expect(now()).toBe(hiddenValue)

    hidden = false
    document.dispatchEvent(new Event('visibilitychange'))
    expect(now()).toBe(12_000)

    dispose()
    if (descriptor) {
      Object.defineProperty(document, 'hidden', descriptor)
    } else {
      Reflect.deleteProperty(document, 'hidden')
    }
  })
})
