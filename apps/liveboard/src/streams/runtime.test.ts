import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  ApiAgent,
  ApiStatus,
  ApiTimelineDetail,
  ApiTimelinePage,
  HistoryLiveEvent,
  PresentationRecord,
  TimelineQuery,
} from '../api/client'
import {
  createRuntimeConnection,
  type EventSourceLike,
  type RuntimeApi,
} from './runtime'

function agent(id: string): ApiAgent {
  return {
    id,
    first_seen_at_ms: 10,
    last_seen_at_ms: 20,
    seen_in_current_runtime: true,
    active_process_count: 0,
    workdirs: [],
  }
}

function status(runtimeId = 'runtime-one'): ApiStatus {
  return {
    schema_version: 1,
    api_version: 1,
    presentation_version: 3,
    runtime_id: runtimeId,
    current_runtime_agent_count: 2,
    active_process_count: 0,
  }
}

function record(id: number, agentId: string, startedAtMs = id * 10): PresentationRecord {
  return {
    presentation_id: `inv-${id}`,
    primary_invocation_id: id,
    raw_evidence_count: 1,
    raw_invocation_ids: [id],
    raw_invocation_ids_truncated: false,
    agent_id: agentId,
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
    kind: 'generic',
    tool_name: 'fixture',
    status: 'success',
    summary: null,
  }
}

function fileRecord(
  id: number,
  agentId: string,
  diffLinesIncluded: boolean,
): PresentationRecord {
  return {
    ...record(id, agentId),
    kind: 'file_changes',
    source_tool: 'apply_patch',
    changes: [
      {
        operation: 'edited',
        path: `/repo/file-${id}.ts`,
        old_path: null,
        write_mode: null,
        added: 1,
        removed: 1,
        diff_truncated: false,
        diff_lines_included: diffLinesIncluded,
        lines: diffLinesIncluded
          ? [
              { kind: 'remove', old_line: 1, new_line: null, text: 'before' },
              { kind: 'add', old_line: null, new_line: 1, text: 'after' },
            ]
          : [],
      },
    ],
  } as PresentationRecord
}

function page(runtimeId: string, records: PresentationRecord[]): ApiTimelinePage {
  return {
    schema_version: 1,
    presentation_version: 3,
    runtime_id: runtimeId,
    records,
    has_more: false,
    next_cursor: null,
  }
}

function liveEvent(input: Partial<HistoryLiveEvent> & Pick<HistoryLiveEvent, 'sequence' | 'event_type'>) {
  return {
    schema_version: input.schema_version ?? 2,
    runtime_id: input.runtime_id ?? 'runtime-one',
    sequence: input.sequence,
    emitted_at_ms: input.emitted_at_ms ?? 1_100 + input.sequence,
    event_type: input.event_type,
    agent_id: input.agent_id ?? null,
    invocation_id: input.invocation_id ?? null,
    presentation_id: input.presentation_id ?? null,
    presentation_revision: input.presentation_revision ?? 3,
    payload: input.payload ?? {},
  } satisfies HistoryLiveEvent
}

class FakeEventSource implements EventSourceLike {
  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  readonly listeners = new Map<string, EventListener[]>()
  closed = false

  constructor(readonly url: string) {}

  addEventListener(type: string, listener: EventListener) {
    const listeners = this.listeners.get(type) ?? []
    listeners.push(listener)
    this.listeners.set(type, listeners)
  }

  open() {
    this.onopen?.(new Event('open'))
  }

  error() {
    this.onerror?.(new Event('error'))
  }

  emit(event: HistoryLiveEvent) {
    for (const listener of this.listeners.get(event.event_type) ?? []) {
      listener({ data: JSON.stringify(event) } as unknown as Event)
    }
  }

  close() {
    this.closed = true
  }
}

beforeEach(() => {
  vi.stubGlobal(
    'requestAnimationFrame',
    (callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 0) as unknown as number,
  )
  vi.stubGlobal('cancelAnimationFrame', (id: number) => clearTimeout(id))
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('runtime connection', () => {
  it('recovers only visible Agents, coalesces canonical detail, and overlaps output-selection handoff', async () => {
    const sources: FakeEventSource[] = []
    const timelineQueries: TimelineQuery[] = []
    const detailRequests: string[] = []
    const details = new Map<string, PresentationRecord>([
      ['inv-3', record(3, 'a111')],
      ['inv-4', record(4, 'b222')],
    ])
    const api: RuntimeApi = {
      fetchStatus: async () => status(),
      fetchCurrentAgents: async () => ({ agents: [agent('a111'), agent('b222')] }),
      fetchTimeline: async (query, runtimeId) => {
        timelineQueries.push({ ...query })
        const records =
          query.agentId === 'a111'
            ? [record(1, 'a111')]
            : query.agentId === 'b222'
              ? [record(2, 'b222')]
              : []
        return page(runtimeId, records)
      },
      fetchTimelineDetail: async (presentationId, runtimeId): Promise<ApiTimelineDetail> => {
        detailRequests.push(presentationId)
        return {
          schema_version: 1,
          presentation_version: 3,
          runtime_id: runtimeId,
          record: details.get(presentationId)!,
        }
      },
      fetchTimelineDiffBatch: async (_presentationIds, runtimeId) => ({
        schema_version: 1,
        presentation_version: 3,
        runtime_id: runtimeId,
        records: [],
      }),
      fetchOutputMetadata: async () => {
        throw new Error('output metadata not expected')
      },
      fetchOutputPage: async () => {
        throw new Error('output page not expected')
      },
      openEventSource: (url) => {
        const source = new FakeEventSource(url)
        sources.push(source)
        return source
      },
    }

    const runtime = createRuntimeConnection({
      initialStatus: status(),
      initialAgents: [agent('a111'), agent('b222')],
      initialVisibleAgentIds: ['a111'],
      viewerAttachWatermarkMs: 1_000,
      api,
    })
    const firstController = runtime.controllerFor('a111')
    runtime.start()
    expect(sources[0]?.url).toBe('api/events?output_agent_ids=a111&diffs=full')
    sources[0]!.open()
    await vi.waitFor(() => expect(runtime.connectionState()).toBe('connected'))
    expect(firstController.orderedIds()).toEqual(['inv-1'])
    expect(timelineQueries).toContainEqual({
      agentId: 'a111',
      recoverySinceMs: 1_000,
      cursor: undefined,
      limit: 50,
      diffs: 'full',
    })
    expect(timelineQueries.some((query) => query.agentId === 'b222')).toBe(false)

    const refresh = liveEvent({
      sequence: 1,
      event_type: 'presentation_updated',
      agent_id: 'a111',
      invocation_id: 3,
      presentation_id: 'inv-3',
    })
    sources[0]!.emit(refresh)
    sources[0]!.emit(refresh)
    await vi.waitFor(() => expect(firstController.orderedIds()).toContain('inv-3'))
    expect(detailRequests.filter((id) => id === 'inv-3')).toHaveLength(1)

    sources[0]!.emit(
      liveEvent({
        sequence: 2,
        event_type: 'presentation_updated',
        agent_id: 'b222',
        invocation_id: 4,
        presentation_id: 'inv-4',
      }),
    )
    await new Promise((resolve) => setTimeout(resolve, 5))
    expect(detailRequests).not.toContain('inv-4')

    runtime.setVisibleAgentIds(['a111', 'b222'])
    await vi.waitFor(() => expect(sources).toHaveLength(2))
    expect(sources[1]?.url).toBe('api/events?output_agent_ids=a111%2Cb222&diffs=full')
    expect(sources[0]?.closed).toBe(false)
    await vi.waitFor(() =>
      expect(timelineQueries.some((query) => query.agentId === 'b222')).toBe(true),
    )
    expect(runtime.controllerFor('b222').orderedIds()).toEqual(['inv-2'])
    sources[1]!.open()
    expect(sources[0]?.closed).toBe(true)
    expect(sources[1]?.closed).toBe(false)

    const sourceCountBeforeUiReorder = sources.length
    runtime.setVisibleAgentIds(['b222', 'a111'])
    await new Promise((resolve) => setTimeout(resolve, 5))
    expect(sources).toHaveLength(sourceCountBeforeUiReorder)
    expect(runtime.visibleAgentIds()).toEqual(['a111', 'b222'])

    const oldA = runtime.controllerFor('a111')
    runtime.setVisibleAgentIds(['b222'])
    expect(oldA.orderedIds()).toEqual([])
    runtime.dispose()
  })

  it('uses explicit gaps for durable recovery and resets runtime-scoped controllers on replacement', async () => {
    const sources: FakeEventSource[] = []
    const recoveryQueries: TimelineQuery[] = []
    let currentStatus = status()
    let currentAgents = [agent('a111')]
    const api: RuntimeApi = {
      fetchStatus: async () => currentStatus,
      fetchCurrentAgents: async () => ({ agents: currentAgents }),
      fetchTimeline: async (query, runtimeId) => {
        recoveryQueries.push({ ...query })
        return page(runtimeId, query.agentId === 'a111' ? [record(1, 'a111')] : [])
      },
      fetchTimelineDetail: async (presentationId, runtimeId) => ({
        schema_version: 1,
        presentation_version: 3,
        runtime_id: runtimeId,
        record: record(Number(presentationId.slice(4)), 'a111'),
      }),
      fetchTimelineDiffBatch: async (_presentationIds, runtimeId) => ({
        schema_version: 1,
        presentation_version: 3,
        runtime_id: runtimeId,
        records: [],
      }),
      fetchOutputMetadata: async () => {
        throw new Error('output metadata not expected')
      },
      fetchOutputPage: async () => {
        throw new Error('output page not expected')
      },
      openEventSource: (url) => {
        const source = new FakeEventSource(url)
        sources.push(source)
        return source
      },
    }
    const runtime = createRuntimeConnection({
      initialStatus: status(),
      initialAgents: currentAgents,
      initialVisibleAgentIds: ['a111'],
      viewerAttachWatermarkMs: 1_000,
      api,
    })
    const oldController = runtime.controllerFor('a111')
    runtime.start()
    sources[0]!.open()
    await vi.waitFor(() => expect(runtime.connectionState()).toBe('connected'))
    const initialRecoveryCount = recoveryQueries.length

    sources[0]!.emit(
      liveEvent({
        sequence: 1,
        event_type: 'output',
        agent_id: 'a111',
        invocation_id: 1,
        presentation_id: 'inv-1',
        emitted_at_ms: 1_200,
        payload: { output_sequence: 7, text: 'x' },
      }),
    )
    sources[0]!.emit(
      liveEvent({
        sequence: 2,
        event_type: 'gap',
        emitted_at_ms: 1_300,
        payload: { skipped_events: 3 },
      }),
    )
    await vi.waitFor(() => expect(recoveryQueries.length).toBeGreaterThan(initialRecoveryCount))
    expect(recoveryQueries.at(-1)?.recoverySinceMs).toBe(1_200)

    currentStatus = status('runtime-two')
    currentAgents = [agent('c333')]
    sources[0]!.emit(
      liveEvent({
        schema_version: 2,
        runtime_id: 'runtime-two',
        sequence: 1,
        event_type: 'agent_first_seen',
        agent_id: 'c333',
        emitted_at_ms: 2_000,
      }),
    )
    await vi.waitFor(() => expect(runtime.runtimeId()).toBe('runtime-two'))
    expect(oldController.orderedIds()).toEqual([])
    expect(runtime.agents().map((item) => item.id)).toEqual(['c333'])
    expect(runtime.visibleAgentIds()).toEqual([])

    runtime.setVisibleAgentIds(['c333'])
    await vi.waitFor(() => expect(sources.length).toBeGreaterThanOrEqual(2))
    expect(sources.at(-1)?.url).toBe('api/events?output_agent_ids=c333&diffs=full')
    runtime.dispose()
  })

  it('ignores an older overlapping detail response after a newer refresh wins', async () => {
    const sources: FakeEventSource[] = []
    const detailResolvers: Array<(detail: ApiTimelineDetail) => void> = []
    const api: RuntimeApi = {
      fetchStatus: async () => status(),
      fetchCurrentAgents: async () => ({ agents: [agent('a111')] }),
      fetchTimeline: async (_query, runtimeId) => page(runtimeId, []),
      fetchTimelineDetail: (presentationId, runtimeId) =>
        new Promise<ApiTimelineDetail>((resolve) => {
          detailResolvers.push(resolve)
        }).then((detail) => ({ ...detail, runtime_id: runtimeId, record: { ...detail.record, presentation_id: presentationId } })),
      fetchTimelineDiffBatch: async (_presentationIds, runtimeId) => ({
        schema_version: 1,
        presentation_version: 3,
        runtime_id: runtimeId,
        records: [],
      }),
      fetchOutputMetadata: async () => {
        throw new Error('output metadata not expected')
      },
      fetchOutputPage: async () => {
        throw new Error('output page not expected')
      },
      openEventSource: (url) => {
        const source = new FakeEventSource(url)
        sources.push(source)
        return source
      },
    }
    const runtime = createRuntimeConnection({
      initialStatus: status(),
      initialAgents: [agent('a111')],
      initialVisibleAgentIds: ['a111'],
      viewerAttachWatermarkMs: 1_000,
      api,
    })
    const controller = runtime.controllerFor('a111')
    const currentSummary = () => {
      const current = controller.record('inv-3')
      return current?.kind === 'generic' ? current.summary : undefined
    }
    runtime.start()
    sources[0]!.open()
    await vi.waitFor(() => expect(runtime.connectionState()).toBe('connected'))

    const refresh = (sequence: number) =>
      liveEvent({
        sequence,
        event_type: 'presentation_updated',
        agent_id: 'a111',
        invocation_id: 3,
        presentation_id: 'inv-3',
      })
    sources[0]!.emit(refresh(1))
    await vi.waitFor(() => expect(detailResolvers).toHaveLength(1))
    sources[0]!.emit(refresh(2))
    await vi.waitFor(() => expect(detailResolvers).toHaveLength(2))

    const newer = {
      ...record(3, 'a111'),
      summary: 'newer detail',
    } as PresentationRecord
    detailResolvers[1]!({
      schema_version: 1,
      presentation_version: 3,
      runtime_id: 'runtime-one',
      record: newer,
    })
    await vi.waitFor(() => expect(currentSummary()).toBe('newer detail'))

    const older = {
      ...record(3, 'a111'),
      summary: 'older detail',
    } as PresentationRecord
    detailResolvers[0]!({
      schema_version: 1,
      presentation_version: 3,
      runtime_id: 'runtime-one',
      record: older,
    })
    await new Promise((resolve) => setTimeout(resolve, 5))
    expect(currentSummary()).toBe('newer detail')
    runtime.dispose()
  })

  it('projects summary diffs live, skips detail GETs, batches body hydration, and hands over projection', async () => {
    const sources: FakeEventSource[] = []
    const detailRequests: string[] = []
    const batchRequests: string[][] = []
    const api: RuntimeApi = {
      fetchStatus: async () => status(),
      fetchCurrentAgents: async () => ({ agents: [agent('a111')] }),
      fetchTimeline: async (_query, runtimeId) =>
        page(runtimeId, [fileRecord(10, 'a111', false)]),
      fetchTimelineDetail: async (presentationId, runtimeId) => {
        detailRequests.push(presentationId)
        return {
          schema_version: 1,
          presentation_version: 3,
          runtime_id: runtimeId,
          record: fileRecord(Number(presentationId.slice(4)), 'a111', true),
        }
      },
      fetchTimelineDiffBatch: async (presentationIds, runtimeId) => {
        batchRequests.push([...presentationIds])
        return {
          schema_version: 1,
          presentation_version: 3,
          runtime_id: runtimeId,
          records: presentationIds.map((id) =>
            fileRecord(Number(id.slice(4)), 'a111', true),
          ),
        }
      },
      fetchOutputMetadata: async () => {
        throw new Error('output metadata not expected')
      },
      fetchOutputPage: async () => {
        throw new Error('output page not expected')
      },
      openEventSource: (url) => {
        const source = new FakeEventSource(url)
        sources.push(source)
        return source
      },
    }
    const runtime = createRuntimeConnection({
      initialStatus: status(),
      initialAgents: [agent('a111')],
      initialVisibleAgentIds: ['a111'],
      initialDiffProjection: 'summary',
      viewerAttachWatermarkMs: 1_000,
      api,
    })
    const controller = runtime.controllerFor('a111')
    runtime.start()
    expect(sources[0]?.url).toBe('api/events?output_agent_ids=a111&diffs=summary')
    sources[0]!.open()
    await vi.waitFor(() => expect(runtime.connectionState()).toBe('connected'))
    const initialSummary = controller.record('inv-10')
    expect(
      initialSummary?.kind === 'file_changes'
        ? initialSummary.changes[0]?.diff_lines_included
        : undefined,
    ).toBe(false)

    const live = fileRecord(11, 'a111', false)
    sources[0]!.emit(
      liveEvent({
        sequence: 1,
        event_type: 'presentation_updated',
        agent_id: 'a111',
        invocation_id: 11,
        presentation_id: 'inv-11',
        payload: { record: live },
      }),
    )
    await vi.waitFor(() => expect(controller.orderedIds()).toContain('inv-11'))
    expect(detailRequests).toHaveLength(0)

    controller.requestFullPresentation('inv-10')
    controller.requestFullPresentation('inv-11')
    await vi.waitFor(() => expect(batchRequests).toHaveLength(1))
    expect(batchRequests[0]).toEqual(['inv-10', 'inv-11'])
    await vi.waitFor(() => {
      const current = controller.record('inv-11')
      expect(current?.kind).toBe('file_changes')
      expect(current?.kind === 'file_changes' && current.changes[0]?.diff_lines_included).toBe(true)
    })

    runtime.setDiffProjection('full')
    await vi.waitFor(() => expect(sources).toHaveLength(2))
    expect(sources[1]?.url).toBe('api/events?output_agent_ids=a111&diffs=full')
    runtime.setDiffProjection('summary')
    const dropped = controller.record('inv-11')
    expect(dropped?.kind === 'file_changes' && dropped.changes[0]?.diff_lines_included).toBe(false)
    expect(dropped?.kind === 'file_changes' ? dropped.changes[0]?.lines : undefined).toEqual([])
    runtime.dispose()
  })

  it('does not let concurrent recovery replace newer repository stream frames', async () => {
    const sources: FakeEventSource[] = []
    const recoveryResolvers = new Map<string, (page: ApiTimelinePage) => void>()
    const api: RuntimeApi = {
      fetchStatus: async () => status(),
      fetchCurrentAgents: async () => ({ agents: [agent('a111'), agent('b222')] }),
      fetchTimeline: (query, runtimeId) =>
        new Promise<ApiTimelinePage>((resolve) => {
          recoveryResolvers.set(query.agentId!, resolve)
        }).then((result) => ({ ...result, runtime_id: runtimeId })),
      fetchTimelineDetail: async () => {
        throw new Error('timeline detail not expected')
      },
      fetchTimelineDiffBatch: async (_presentationIds, runtimeId) => ({
        schema_version: 1,
        presentation_version: 3,
        runtime_id: runtimeId,
        records: [],
      }),
      fetchOutputMetadata: async () => {
        throw new Error('output metadata not expected')
      },
      fetchOutputPage: async () => {
        throw new Error('output page not expected')
      },
      openEventSource: (url) => {
        const source = new FakeEventSource(url)
        sources.push(source)
        return source
      },
    }
    const runtime = createRuntimeConnection({
      initialStatus: status(),
      initialAgents: [agent('a111'), agent('b222')],
      initialVisibleAgentIds: ['a111', 'b222'],
      viewerAttachWatermarkMs: 1_000,
      api,
    })
    const controllerA = runtime.controllerFor('a111')
    const controllerB = runtime.controllerFor('b222')
    const liveA = {
      ...record(1, 'a111'),
      normalized_workdir: '/repos/alpha',
      summary: 'alpha live frame',
    } as PresentationRecord
    const liveB = {
      ...record(2, 'b222'),
      normalized_workdir: '/repos/beta',
      summary: 'beta live frame',
    } as PresentationRecord

    runtime.start()
    sources[0]!.open()
    await vi.waitFor(() => expect(recoveryResolvers.size).toBe(2))
    sources[0]!.emit(
      liveEvent({
        sequence: 1,
        event_type: 'presentation_updated',
        agent_id: 'a111',
        invocation_id: 1,
        presentation_id: 'inv-1',
        payload: { record: liveA },
      }),
    )
    sources[0]!.emit(
      liveEvent({
        sequence: 2,
        event_type: 'presentation_updated',
        agent_id: 'b222',
        invocation_id: 2,
        presentation_id: 'inv-2',
        payload: { record: liveB },
      }),
    )

    recoveryResolvers.get('a111')!(
      page('runtime-one', [
        {
          ...liveA,
          summary: 'alpha stale recovery frame',
        } as PresentationRecord,
      ]),
    )
    recoveryResolvers.get('b222')!(
      page('runtime-one', [
        {
          ...liveB,
          summary: 'beta stale recovery frame',
        } as PresentationRecord,
      ]),
    )
    await vi.waitFor(() => expect(runtime.connectionState()).toBe('connected'))

    expect(controllerA.record('inv-1')).toMatchObject({
      agent_id: 'a111',
      normalized_workdir: '/repos/alpha',
      summary: 'alpha live frame',
    })
    expect(controllerB.record('inv-2')).toMatchObject({
      agent_id: 'b222',
      normalized_workdir: '/repos/beta',
      summary: 'beta live frame',
    })
    expect(controllerA.record('inv-2')).toBeUndefined()
    expect(controllerB.record('inv-1')).toBeUndefined()
    runtime.dispose()
  })
})
