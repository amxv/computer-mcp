import { describe, expect, it, vi } from 'vitest'

import type { ApiOutputPage } from '../api/client'
import { CommandOutputState } from './CommandOutputState'

function outputPage(
  chunks: Array<{ sequence: number; text: string }>,
  displayState = 'available',
): ApiOutputPage {
  return {
    schema_version: 1,
    runtime_id: 'runtime-one',
    invocation_id: 10,
    view: 'display',
    chunks: chunks.map((chunk) => ({ ...chunk, observed_at_ms: chunk.sequence })),
    next_cursor: null,
    display_state: displayState,
  }
}

describe('CommandOutputState', () => {
  it('hydrates one bounded recent display page from last_cursor without raw detail', async () => {
    const metadata = vi.fn(async () => ({
      schema_version: 1,
      runtime_id: 'runtime-one',
      invocation_id: 10,
      output: {
        available: true,
        chunk_count: 500,
        size_bytes: 50_000,
        capture_state: 'complete',
        capture_reason: null,
        first_cursor: 1,
        last_cursor: 500,
      },
    }))
    const page = vi.fn(async (_id: number, cursor: number, limit: number) =>
      outputPage(
        Array.from({ length: 64 }, (_, index) => ({
          sequence: cursor + index,
          text: `${cursor + index}\n`,
        })),
      ),
    )
    const state = new CommandOutputState(
      'inv-10',
      10,
      { loadMetadata: metadata, loadDisplayPage: page },
      () => undefined,
    )
    await state.ensureRecentTail()
    expect(metadata).toHaveBeenCalledTimes(1)
    expect(page).toHaveBeenCalledWith(10, 437, 64)
    expect(state.materialize()).toContain('437\n')
    expect(state.materialize()).toContain('500\n')
    expect(state.materialize()).not.toContain('436\n')
  })

  it('merges live overlap, recovers a sequence hole, and exposes unavailable display truth', async () => {
    let unavailable = false
    const state = new CommandOutputState(
      'inv-10',
      10,
      {
        loadMetadata: async () => ({
          schema_version: 1,
          runtime_id: 'runtime-one',
          invocation_id: 10,
          output: {
            available: true,
            chunk_count: 4,
            size_bytes: 10,
            capture_state: 'complete',
            capture_reason: null,
            first_cursor: 1,
            last_cursor: 4,
          },
        }),
        loadDisplayPage: async () =>
          unavailable
            ? { ...outputPage([], 'unavailable'), display_reason: 'sequence hole' }
            : outputPage([
                { sequence: 1, text: 'a' },
                { sequence: 2, text: 'b' },
                { sequence: 3, text: 'c' },
                { sequence: 4, text: 'd' },
              ]),
      },
      () => undefined,
    )
    state.appendLive(1, 'a')
    expect(state.appendLive(4, 'd').sequenceGap).toBe(true)
    await state.ensureRecentTail()
    expect(state.materialize()).toBe('abcd')
    expect(state.needsRecovery()).toBe(false)

    state.markRecoveryNeeded()
    unavailable = true
    await state.ensureRecentTail()
    expect(state.isDisplayUnavailable()).toBe(true)
    expect(state.displayUnavailableReason()).toBe('sequence hole')
  })

  it('reconciles the durable tail at EOF when the final live chunk was missed', async () => {
    let lastCursor = 2
    const page = vi.fn(async () =>
      outputPage(
        lastCursor === 2
          ? [
              { sequence: 1, text: 'a' },
              { sequence: 2, text: 'b' },
            ]
          : [
              { sequence: 1, text: 'a' },
              { sequence: 2, text: 'b' },
              { sequence: 3, text: 'c' },
            ],
      ),
    )
    const state = new CommandOutputState(
      'inv-10',
      10,
      {
        loadMetadata: async () => ({
          schema_version: 1,
          runtime_id: 'runtime-one',
          invocation_id: 10,
          output: {
            available: true,
            chunk_count: lastCursor,
            size_bytes: lastCursor,
            capture_state: 'complete',
            capture_reason: null,
            first_cursor: 1,
            last_cursor: lastCursor,
          },
        }),
        loadDisplayPage: page,
      },
      () => undefined,
    )
    await state.ensureRecentTail()
    expect(state.materialize()).toBe('ab')
    expect(state.needsRecovery()).toBe(false)

    const unsubscribe = state.subscribe(() => undefined)
    lastCursor = 3
    state.markComplete('available')
    await vi.waitFor(() => expect(state.materialize()).toBe('abc'))
    expect(page).toHaveBeenCalledTimes(2)
    expect(state.needsRecovery()).toBe(false)
    unsubscribe()
  })

  it('releases a final buffer only after the last expanded subscriber detaches', () => {
    const released = vi.fn()
    const state = new CommandOutputState(
      'inv-10',
      10,
      {
        loadMetadata: async () => {
          throw new Error('not expected')
        },
        loadDisplayPage: async () => {
          throw new Error('not expected')
        },
      },
      released,
    )
    const unsubscribe = state.subscribe(() => undefined)
    state.markFinal()
    expect(released).not.toHaveBeenCalled()
    unsubscribe()
    expect(released).toHaveBeenCalledTimes(1)
  })

  it('contains automatic recovery failures and keeps them visible for retry', async () => {
    const state = new CommandOutputState(
      'inv-10',
      10,
      {
        loadMetadata: async () => {
          throw new Error('observer temporarily unavailable')
        },
        loadDisplayPage: async () => {
          throw new Error('not expected')
        },
      },
      () => undefined,
    )
    const unsubscribe = state.subscribe(() => undefined)
    state.appendLive(1, 'live')
    await vi.waitFor(() =>
      expect(state.recoveryErrorMessage()).toBe('observer temporarily unavailable'),
    )
    expect(state.needsRecovery()).toBe(true)
    unsubscribe()
  })
})
