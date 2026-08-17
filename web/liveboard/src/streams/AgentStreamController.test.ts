import { describe, expect, it } from 'vitest'

import type { ApiTimelinePage, PresentationRecord } from '../api/client'
import { createAgentStreamController } from './AgentStreamController'

function command(
  presentationId: string,
  startedAtMs: number,
  overrides: Partial<PresentationRecord> = {},
): PresentationRecord {
  return {
    presentation_id: presentationId,
    primary_invocation_id: Number(presentationId.replace('inv-', '')),
    raw_evidence_count: 1,
    raw_invocation_ids: [Number(presentationId.replace('inv-', ''))],
    raw_invocation_ids_truncated: false,
    agent_id: 'a111',
    declared_workdir: '/repo',
    normalized_workdir: '/repo',
    new_workdir: null,
    started_at_ms: startedAtMs,
    duration_ms: null,
    evidence: {
      evidence_state: 'complete',
      capture_state: 'complete',
      degraded: false,
      reason: null,
    },
    kind: 'command',
    command: `echo ${presentationId}`,
    status: 'running',
    effective_cwd: '/repo',
    exit_code: null,
    termination_reason: null,
    output: null,
    output_truncated: false,
    polls: null,
    ...overrides,
  } as PresentationRecord
}

function page(
  records: PresentationRecord[],
  hasMore: boolean,
  nextCursor: string | null,
): ApiTimelinePage {
  return {
    schema_version: 1,
    presentation_version: 2,
    runtime_id: 'runtime-one',
    records,
    has_more: hasMore,
    next_cursor: nextCursor,
  }
}

const outputLoaders = {
  loadOutputMetadata: async () => {
    throw new Error('output metadata not expected in this controller test')
  },
  loadDisplayOutputPage: async () => {
    throw new Error('output page not expected in this controller test')
  },
}

describe('AgentStreamController', () => {
  it('orders by stable presentation identity and updates one card without duplicating it', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })

    controller.upsert(command('inv-3', 300))
    controller.upsert(command('inv-1', 100))
    controller.upsert(command('inv-2', 200))
    expect(controller.orderedIds()).toEqual(['inv-1', 'inv-2', 'inv-3'])

    controller.upsert(
      command('inv-2', 200, {
        kind: 'command',
        status: 'exited',
        exit_code: 0,
      }),
    )
    expect(controller.orderedIds()).toEqual(['inv-1', 'inv-2', 'inv-3'])
    const updated = controller.record('inv-2')
    expect(updated?.kind).toBe('command')
    expect(updated?.kind === 'command' ? updated.status : undefined).toBe('exited')
  })

  it('counts one unseen activity per card while paused, including live output growth', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })
    controller.upsert(command('inv-1', 100), false)
    controller.setFollowing(false)

    controller.upsert(command('inv-1', 100, { duration_ms: 20 }), true)
    controller.appendLiveOutput({
      presentationId: 'inv-1',
      invocationId: 1,
      sequence: 4,
      text: 'a',
    })
    controller.appendLiveOutput({
      presentationId: 'inv-1',
      invocationId: 1,
      sequence: 5,
      text: 'b',
    })
    expect(controller.unseenCount()).toBe(1)
    expect(controller.lastLiveOutputSequence('inv-1')).toBe(5)

    controller.upsert(command('inv-2', 200), true)
    expect(controller.unseenCount()).toBe(2)
    controller.returnToLive()
    expect(controller.following()).toBe(true)
    expect(controller.unseenCount()).toBe(0)
  })

  it('activates history only on request, follows opaque cursors, dedupes, and exhausts', async () => {
    const calls: Array<{ beforeMs: number; cursor?: string }> = []
    const responses = [
      page([command('inv-1', 100), command('inv-2', 200)], true, 'cursor-one'),
      page([command('inv-0', 50), command('inv-1', 100)], false, null),
    ]
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async (input) => {
        calls.push({ beforeMs: input.beforeMs, cursor: input.cursor })
        return responses[calls.length - 1]!
      },
      ...outputLoaders,
    })
    controller.mergeRecovery([command('inv-2', 200), command('inv-3', 300)])
    expect(controller.historyActivated()).toBe(false)

    expect(await controller.loadEarlier()).toBe(1)
    expect(controller.historyActivated()).toBe(true)
    expect(controller.orderedIds()).toEqual(['inv-1', 'inv-2', 'inv-3'])
    expect(calls[0]).toEqual({ beforeMs: 1_000, cursor: undefined })

    expect(await controller.loadEarlier()).toBe(1)
    expect(calls[1]).toEqual({ beforeMs: 1_000, cursor: 'cursor-one' })
    expect(controller.orderedIds()).toEqual(['inv-0', 'inv-1', 'inv-2', 'inv-3'])
    expect(controller.historyExhausted()).toBe(true)
    expect(await controller.loadEarlier()).toBe(0)
    expect(calls).toHaveLength(2)
  })

  it('disposes only its own card state', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })
    controller.upsert(command('inv-1', 100))
    controller.dispose()
    expect(controller.orderedIds()).toEqual([])
    expect(controller.record('inv-1')).toBeUndefined()
    expect(controller.upsert(command('inv-2', 200))).toBe(false)
  })

  it('keeps command expansion overrides local until the next global command action', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })
    const first = controller.commandExpanded('inv-1')
    const second = controller.commandExpanded('inv-2')
    expect(first()).toBe(false)
    expect(second()).toBe(false)

    controller.toggleCommandExpansion('inv-1')
    expect(first()).toBe(true)
    expect(second()).toBe(false)

    controller.setCommandExpansionDefault(true)
    expect(first()).toBe(true)
    expect(second()).toBe(true)
    controller.toggleCommandExpansion('inv-1')
    expect(first()).toBe(false)
    expect(second()).toBe(true)

    controller.setCommandExpansionDefault(false)
    expect(first()).toBe(false)
    expect(second()).toBe(false)
  })

  it('keeps diff expansion overrides local and resets them on each global diff action', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })
    const first = controller.diffExpanded('inv-1:file-change:0')
    const second = controller.diffExpanded('inv-2:file-change:0')
    expect(first()).toBe(true)
    expect(second()).toBe(true)

    controller.toggleDiffExpansion('inv-1:file-change:0')
    expect(first()).toBe(false)
    expect(second()).toBe(true)

    controller.setDiffExpansionDefault(false)
    expect(first()).toBe(false)
    expect(second()).toBe(false)
    controller.toggleDiffExpansion('inv-1:file-change:0')
    expect(first()).toBe(true)
    expect(second()).toBe(false)

    controller.setDiffExpansionDefault(true)
    expect(first()).toBe(true)
    expect(second()).toBe(true)
  })

  it('releases a finalized command live buffer while retaining canonical card truth', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })
    controller.upsert(command('inv-1', 100), false)
    controller.appendLiveOutput({
      presentationId: 'inv-1',
      invocationId: 1,
      sequence: 9,
      text: 'tail',
    })
    expect(controller.lastLiveOutputSequence('inv-1')).toBe(9)

    controller.upsert(
      command('inv-1', 100, {
        kind: 'command',
        status: 'exited',
        exit_code: 0,
      }),
      false,
    )
    expect(controller.lastLiveOutputSequence('inv-1')).toBeUndefined()
    controller.appendLiveOutput({
      presentationId: 'inv-1',
      invocationId: 1,
      sequence: 10,
      text: 'late collapsed tail',
    })
    expect(controller.lastLiveOutputSequence('inv-1')).toBeUndefined()
    expect(controller.record('inv-1')?.presentation_id).toBe('inv-1')
  })

  it('keeps trailing PTY bytes visible when process finalization races an expanded reader tail', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
      ...outputLoaders,
    })
    controller.upsert(command('inv-1', 100), false)
    const state = controller.outputState('inv-1', 1)
    const unsubscribe = state.subscribe(() => undefined)
    controller.appendLiveOutput({
      presentationId: 'inv-1',
      invocationId: 1,
      sequence: 1,
      text: 'before exit\n',
    })

    controller.upsert(
      command('inv-1', 100, {
        kind: 'command',
        status: 'exited',
        exit_code: 0,
      }),
      false,
    )
    controller.appendLiveOutput({
      presentationId: 'inv-1',
      invocationId: 1,
      sequence: 2,
      text: 'after exit\n',
    })
    expect(state.materialize()).toBe('before exit\nafter exit\n')
    expect(controller.lastLiveOutputSequence('inv-1')).toBe(2)

    unsubscribe()
    expect(controller.lastLiveOutputSequence('inv-1')).toBeUndefined()
  })
})
