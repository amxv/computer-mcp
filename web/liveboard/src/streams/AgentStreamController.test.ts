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

describe('AgentStreamController', () => {
  it('orders by stable presentation identity and updates one card without duplicating it', () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 1_000,
      loadHistoryPage: async () => page([], false, null),
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
    })
    controller.upsert(command('inv-1', 100), false)
    controller.setFollowing(false)

    controller.upsert(command('inv-1', 100, { duration_ms: 20 }), true)
    controller.noteLiveOutput('inv-1', 4)
    controller.noteLiveOutput('inv-1', 5)
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
    })
    controller.upsert(command('inv-1', 100))
    controller.dispose()
    expect(controller.orderedIds()).toEqual([])
    expect(controller.record('inv-1')).toBeUndefined()
    expect(controller.upsert(command('inv-2', 200))).toBe(false)
  })
})
