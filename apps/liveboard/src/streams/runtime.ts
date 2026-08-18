import { createSignal, type Accessor } from 'solid-js'

import {
  eventStreamUrl,
  fetchCurrentAgents,
  fetchOutputMetadata,
  fetchOutputPage,
  fetchStatus,
  fetchTimeline,
  fetchTimelineDetail,
  fetchTimelineDiffBatch,
  presentationRecordFromLiveEvent,
  validateLiveEvent,
  type ApiAgent,
  type ApiStatus,
  type DiffProjection,
  type HistoryLiveEvent,
  type TimelineQuery,
} from '../api/client'
import {
  createAgentStreamController,
  type AgentStreamController,
} from './AgentStreamController'

const EVENT_TYPES = [
  'agent_first_seen',
  'agent_workdir_added',
  'invocation_started',
  'invocation_completed',
  'presentation_updated',
  'output',
  'output_complete',
  'process_started',
  'process_ended',
  'gap',
] as const
const HISTORY_PAGE_SIZE = 20
const RECOVERY_PAGE_SIZE = 50
const SEEN_SEQUENCE_LIMIT = 1_024

export type RuntimeConnectionState =
  | 'connecting'
  | 'connected'
  | 'recovering'
  | 'disconnected'
  | 'incompatible'

export interface EventSourceLike {
  onopen: ((event: Event) => void) | null
  onerror: ((event: Event) => void) | null
  addEventListener(type: string, listener: EventListener): void
  close(): void
}

export interface RuntimeApi {
  fetchStatus: () => Promise<ApiStatus>
  fetchCurrentAgents: (runtimeId: string) => Promise<{ agents: ApiAgent[] }>
  fetchTimeline: (
    query: TimelineQuery,
    runtimeId: string,
  ) => ReturnType<typeof fetchTimeline>
  fetchTimelineDetail: (
    presentationId: string,
    runtimeId: string,
  ) => ReturnType<typeof fetchTimelineDetail>
  fetchTimelineDiffBatch: (
    presentationIds: readonly string[],
    runtimeId: string,
  ) => ReturnType<typeof fetchTimelineDiffBatch>
  fetchOutputMetadata: (
    invocationId: number,
    runtimeId: string,
  ) => ReturnType<typeof fetchOutputMetadata>
  fetchOutputPage: (
    invocationId: number,
    input: { cursor: number; limit?: number; view: 'raw' | 'display' },
    runtimeId: string,
  ) => ReturnType<typeof fetchOutputPage>
  openEventSource: (url: string) => EventSourceLike
}

export interface RuntimeConnection {
  runtimeId: Accessor<string>
  agents: Accessor<ApiAgent[]>
  connectionState: Accessor<RuntimeConnectionState>
  connectionError: Accessor<string | undefined>
  visibleAgentIds: Accessor<string[]>
  setVisibleAgentIds: (ids: readonly string[]) => void
  setDiffProjection: (projection: DiffProjection) => void
  controllerFor: (agentId: string) => AgentStreamController
  start: () => void
  dispose: () => void
}

interface RuntimeConnectionOptions {
  initialStatus: ApiStatus
  initialAgents: readonly ApiAgent[]
  initialVisibleAgentIds: readonly string[]
  initialDiffProjection?: DiffProjection
  viewerAttachWatermarkMs: number
  api?: RuntimeApi
}

const browserRuntimeApi: RuntimeApi = {
  fetchStatus,
  fetchCurrentAgents,
  fetchTimeline,
  fetchTimelineDetail,
  fetchTimelineDiffBatch,
  fetchOutputMetadata,
  fetchOutputPage,
  openEventSource: (url) => new EventSource(url),
}

function sameIdMembership(left: readonly string[], right: readonly string[]) {
  if (left.length !== right.length) return false
  const rightIds = new Set(right)
  return left.every((value) => rightIds.has(value))
}

function messageFrom(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export function createRuntimeConnection(
  options: RuntimeConnectionOptions,
): RuntimeConnection {
  const api = options.api ?? browserRuntimeApi
  const [runtimeId, setRuntimeId] = createSignal(options.initialStatus.runtime_id)
  const [agents, setAgents] = createSignal<ApiAgent[]>([...options.initialAgents])
  const [connectionState, setConnectionState] =
    createSignal<RuntimeConnectionState>('connecting')
  const [connectionError, setConnectionError] = createSignal<string>()
  const [visibleAgentIds, setVisibleIds] = createSignal<string[]>([
    ...options.initialVisibleAgentIds,
  ])
  const [diffProjection, setDiffProjectionSignal] = createSignal<DiffProjection>(
    options.initialDiffProjection ?? 'full',
  )
  const controllers = new Map<string, AgentStreamController>()
  const hydratedAgents = new Set<string>()
  const hydratingAgents = new Set<string>()
  const seenSequences = new Set<number>()
  const seenSequenceOrder = new Array<number>(SEEN_SEQUENCE_LIMIT)
  let seenSequenceCount = 0
  let seenSequenceCursor = 0
  const detailVersions = new Map<string, number>()
  const detailPendingWatermarks = new Map<string, number>()
  const detailQueue = new Set<string>()
  let nextDetailVersion = 1
  let detailFrame: number | undefined
  const fullPresentationQueue = new Set<string>()
  const fullPresentationVersions = new Map<string, number>()
  let nextFullPresentationVersion = 1
  let fullPresentationFrame: number | undefined
  let agentRefreshFrame: number | undefined
  let activeSource: EventSourceLike | undefined
  let pendingSource: EventSourceLike | undefined
  let pendingRetry: ReturnType<typeof setTimeout> | undefined
  let started = false
  let disposed = false
  let initialRecoveryComplete = false
  let runtimeAttachWatermarkMs = options.viewerAttachWatermarkMs
  let candidateWatermarkMs = options.viewerAttachWatermarkMs
  let disconnectStartedAtMs: number | undefined
  let synchronization = Promise.resolve()

  const rememberSequence = (sequence: number) => {
    if (seenSequences.has(sequence)) return false
    if (seenSequenceCount < SEEN_SEQUENCE_LIMIT) {
      seenSequenceOrder[seenSequenceCount] = sequence
      seenSequenceCount += 1
    } else {
      const removed = seenSequenceOrder[seenSequenceCursor]
      if (removed !== undefined) seenSequences.delete(removed)
      seenSequenceOrder[seenSequenceCursor] = sequence
      seenSequenceCursor = (seenSequenceCursor + 1) % SEEN_SEQUENCE_LIMIT
    }
    seenSequences.add(sequence)
    return true
  }

  const safeRecoveryWatermark = () => {
    let watermark = candidateWatermarkMs
    for (const pending of detailPendingWatermarks.values()) {
      watermark = Math.min(watermark, pending)
    }
    return watermark
  }

  const loadHistoryPage = async (input: {
    agentId: string
    beforeMs: number
    cursor?: string
  }) =>
    api.fetchTimeline(
      {
        agentId: input.agentId,
        beforeMs: input.beforeMs,
        cursor: input.cursor,
        limit: HISTORY_PAGE_SIZE,
        diffs: diffProjection(),
      },
      runtimeId(),
    )

  const recoverAgent = async (
    agentId: string,
    sinceMs: number,
    controller = controllers.get(agentId),
  ) => {
    if (!controller || hydratingAgents.has(agentId) || disposed) return
    hydratingAgents.add(agentId)
    try {
      let cursor: string | undefined
      for (;;) {
        const page = await api.fetchTimeline(
          {
            agentId,
            recoverySinceMs: sinceMs,
            cursor,
            limit: RECOVERY_PAGE_SIZE,
            diffs: diffProjection(),
          },
          runtimeId(),
        )
        controller.mergeRecovery(page.records)
        if (!page.has_more || !page.next_cursor) break
        cursor = page.next_cursor
      }
      hydratedAgents.add(agentId)
    } finally {
      hydratingAgents.delete(agentId)
    }
  }

  const ensureController = (agentId: string) => {
    const existing = controllers.get(agentId)
    if (existing) return existing
    const controller = createAgentStreamController({
      agentId,
      attachWatermarkMs: runtimeAttachWatermarkMs,
      loadHistoryPage,
      loadOutputMetadata: (invocationId) =>
        api.fetchOutputMetadata(invocationId, runtimeId()),
      loadDisplayOutputPage: (invocationId, cursor, limit) =>
        api.fetchOutputPage(
          invocationId,
          { cursor, limit, view: 'display' },
          runtimeId(),
        ),
      requestFullPresentation,
    })
    controllers.set(agentId, controller)
    if (initialRecoveryComplete && !hydratedAgents.has(agentId)) {
      void recoverAgent(agentId, runtimeAttachWatermarkMs, controller).catch((error) => {
        setConnectionError(`Agent recovery failed: ${messageFrom(error)}`)
      })
    }
    return controller
  }

  const recoverVisible = async (sinceMs: number) => {
    await Promise.all(
      visibleAgentIds().map((agentId) =>
        recoverAgent(agentId, sinceMs, ensureController(agentId)),
      ),
    )
  }

  const refreshAgents = async (expectedRuntimeId = runtimeId()) => {
    const list = await api.fetchCurrentAgents(expectedRuntimeId)
    if (expectedRuntimeId !== runtimeId()) return
    setAgents([...list.agents])
  }

  const patchAgentFromEvent = (event: HistoryLiveEvent) => {
    const agentId = event.agent_id
    if (!agentId) return
    if (event.event_type === 'agent_first_seen' || event.event_type === 'agent_workdir_added') {
      if (agentRefreshFrame === undefined) {
        agentRefreshFrame = requestAnimationFrame(() => {
          agentRefreshFrame = undefined
          void refreshAgents().catch((error) => {
            setConnectionError(`Agent refresh failed: ${messageFrom(error)}`)
          })
        })
      }
      return
    }
    setAgents((current) =>
      current.map((agent) => {
        if (agent.id !== agentId) return agent
        if (event.event_type === 'invocation_started') {
          return {
            ...agent,
            last_seen_at_ms: Math.max(agent.last_seen_at_ms, event.emitted_at_ms),
          }
        }
        if (event.event_type === 'process_started' || event.event_type === 'process_ended') {
          const count = event.payload.agent_active_process_count
          if (typeof count === 'number') {
            return { ...agent, active_process_count: count }
          }
        }
        return agent
      }),
    )
  }

  const finishDetailRefresh = (presentationId: string, version: number) => {
    if (detailVersions.get(presentationId) === version) {
      detailPendingWatermarks.delete(presentationId)
      detailVersions.delete(presentationId)
    }
  }

  const invalidateFullPresentationRequest = (presentationId: string) => {
    fullPresentationQueue.delete(presentationId)
    fullPresentationVersions.delete(presentationId)
  }

  const flushFullPresentationQueue = () => {
    fullPresentationFrame = undefined
    if (disposed || fullPresentationQueue.size === 0) return
    const ids = [...fullPresentationQueue].slice(0, 100)
    for (const id of ids) fullPresentationQueue.delete(id)
    const versions = new Map(
      ids.map((id) => [id, fullPresentationVersions.get(id) ?? 0] as const),
    )
    void api
      .fetchTimelineDiffBatch(ids, runtimeId())
      .then((batch) => {
        const returned = new Set<string>()
        for (const record of batch.records) {
          const presentationId = record.presentation_id
          returned.add(presentationId)
          const expected = versions.get(presentationId)
          if (
            expected === undefined ||
            fullPresentationVersions.get(presentationId) !== expected
          ) {
            continue
          }
          fullPresentationVersions.delete(presentationId)
          const agentId = record.agent_id
          if (agentId && visibleAgentIds().includes(agentId)) {
            ensureController(agentId).upsert(record, false)
          }
        }
        for (const [presentationId, expected] of versions) {
          if (returned.has(presentationId)) continue
          if (fullPresentationVersions.get(presentationId) === expected) {
            fullPresentationVersions.delete(presentationId)
          }
        }
      })
      .catch((error) => {
        let relevant = false
        for (const [presentationId, expected] of versions) {
          if (fullPresentationVersions.get(presentationId) === expected) {
            fullPresentationVersions.delete(presentationId)
            relevant = true
          }
        }
        if (relevant) setConnectionError(`Diff hydration failed: ${messageFrom(error)}`)
      })
      .finally(() => {
        if (!disposed && fullPresentationQueue.size > 0 && fullPresentationFrame === undefined) {
          fullPresentationFrame = requestAnimationFrame(flushFullPresentationQueue)
        }
      })
  }

  function requestFullPresentation(presentationId: string) {
    if (disposed || fullPresentationVersions.has(presentationId)) return
    fullPresentationVersions.set(presentationId, nextFullPresentationVersion++)
    fullPresentationQueue.add(presentationId)
    if (fullPresentationFrame === undefined) {
      fullPresentationFrame = requestAnimationFrame(flushFullPresentationQueue)
    }
  }

  const refreshDetail = async (presentationId: string, version: number) => {
    try {
      const detail = await api.fetchTimelineDetail(presentationId, runtimeId())
      if (detailVersions.get(presentationId) !== version) return
      const agentId = detail.record.agent_id
      if (agentId && visibleAgentIds().includes(agentId)) {
        ensureController(agentId).upsert(detail.record, true)
      }
      finishDetailRefresh(presentationId, version)
    } catch (error) {
      if (detailVersions.get(presentationId) === version) {
        detailVersions.delete(presentationId)
        setConnectionError(`Timeline refresh failed: ${messageFrom(error)}`)
      }
    }
  }

  const flushDetailQueue = () => {
    detailFrame = undefined
    const ids = [...detailQueue]
    detailQueue.clear()
    for (const presentationId of ids) {
      const version = detailVersions.get(presentationId) ?? 0
      void refreshDetail(presentationId, version)
    }
  }

  const scheduleDetailRefresh = (event: HistoryLiveEvent) => {
    const presentationId = event.presentation_id
    const agentId = event.agent_id
    if (!presentationId || !agentId || !visibleAgentIds().includes(agentId)) return
    const previous = detailPendingWatermarks.get(presentationId)
    detailPendingWatermarks.set(
      presentationId,
      previous === undefined ? event.emitted_at_ms : Math.min(previous, event.emitted_at_ms),
    )
    detailVersions.set(presentationId, nextDetailVersion++)
    detailQueue.add(presentationId)
    if (detailFrame === undefined) detailFrame = requestAnimationFrame(flushDetailQueue)
  }

  const resetRuntime = (nextRuntimeId: string, nextAgents: readonly ApiAgent[], boundaryMs: number) => {
    for (const controller of controllers.values()) controller.dispose()
    controllers.clear()
    hydratedAgents.clear()
    hydratingAgents.clear()
    seenSequences.clear()
    seenSequenceCount = 0
    seenSequenceCursor = 0
    detailQueue.clear()
    detailVersions.clear()
    detailPendingWatermarks.clear()
    fullPresentationQueue.clear()
    fullPresentationVersions.clear()
    setRuntimeId(nextRuntimeId)
    setAgents([...nextAgents])
    setVisibleIds((ids) => ids.filter((id) => nextAgents.some((agent) => agent.id === id)))
    runtimeAttachWatermarkMs = boundaryMs
    candidateWatermarkMs = boundaryMs
    initialRecoveryComplete = false
  }

  const synchronizeAfterOpen = async (firstOpen: boolean) => {
    if (disposed) return
    setConnectionState(firstOpen ? 'connecting' : 'recovering')
    setConnectionError(undefined)
    try {
      if (!firstOpen) {
        for (const controller of controllers.values()) {
          controller.markOutputRecoveryNeeded()
        }
      }
      const status = await api.fetchStatus()
      const runtimeChanged = status.runtime_id !== runtimeId()
      if (runtimeChanged) {
        const boundary = disconnectStartedAtMs ?? Date.now()
        const list = await api.fetchCurrentAgents(status.runtime_id)
        resetRuntime(status.runtime_id, list.agents, boundary)
      } else {
        await refreshAgents(status.runtime_id)
      }
      const recoveryBoundary = firstOpen
        ? runtimeAttachWatermarkMs
        : safeRecoveryWatermark()
      await recoverVisible(recoveryBoundary)
      initialRecoveryComplete = true
      candidateWatermarkMs = Math.max(candidateWatermarkMs, recoveryBoundary)
      disconnectStartedAtMs = undefined
      setConnectionState('connected')
    } catch (error) {
      const message = messageFrom(error)
      setConnectionError(message)
      setConnectionState(message.includes('incompatible') ? 'incompatible' : 'disconnected')
    }
  }

  const recoverAfterGap = (event: HistoryLiveEvent) => {
    const boundary = safeRecoveryWatermark()
    synchronization = synchronization.then(async () => {
      if (disposed) return
      setConnectionState('recovering')
      try {
        for (const controller of controllers.values()) {
          controller.markOutputRecoveryNeeded()
        }
        await refreshAgents()
        await recoverVisible(boundary)
        candidateWatermarkMs = Math.max(candidateWatermarkMs, event.emitted_at_ms)
        setConnectionError(undefined)
        setConnectionState('connected')
      } catch (error) {
        setConnectionError(`Gap recovery failed: ${messageFrom(error)}`)
        setConnectionState('disconnected')
      }
    })
  }

  const handleRuntimeMismatch = (event: HistoryLiveEvent) => {
    synchronization = synchronization.then(async () => {
      if (disposed || event.runtime_id === runtimeId()) return
      setConnectionState('recovering')
      try {
        const status = await api.fetchStatus()
        if (status.runtime_id !== event.runtime_id) return
        const list = await api.fetchCurrentAgents(status.runtime_id)
        const boundary = disconnectStartedAtMs ?? event.emitted_at_ms
        resetRuntime(status.runtime_id, list.agents, boundary)
        await recoverVisible(boundary)
        initialRecoveryComplete = true
        candidateWatermarkMs = Math.max(candidateWatermarkMs, event.emitted_at_ms)
        setConnectionError(undefined)
        setConnectionState('connected')
      } catch (error) {
        setConnectionError(`Runtime recovery failed: ${messageFrom(error)}`)
        setConnectionState('disconnected')
      }
    })
  }

  const handleLiveEvent = (message: MessageEvent<string>) => {
    if (disposed) return
    try {
      const event = JSON.parse(message.data) as HistoryLiveEvent
      validateLiveEvent(event)
      if (event.runtime_id !== runtimeId()) {
        handleRuntimeMismatch(event)
        return
      }
      if (!rememberSequence(event.sequence)) return
      if (event.event_type === 'gap') {
        recoverAfterGap(event)
        return
      }
      candidateWatermarkMs = Math.max(candidateWatermarkMs, event.emitted_at_ms)
      patchAgentFromEvent(event)

      if (event.event_type === 'output' || event.event_type === 'output_complete') {
        if (event.agent_id && event.presentation_id && visibleAgentIds().includes(event.agent_id)) {
          const controller = ensureController(event.agent_id)
          if (event.event_type === 'output' && event.invocation_id !== null) {
            const sequence = event.payload.output_sequence
            const text = event.payload.text
            const displayState = event.payload.display_state
            const displayReason = event.payload.display_reason
            if (typeof sequence === 'number' && typeof text === 'string') {
              controller.appendLiveOutput({
                presentationId: event.presentation_id,
                invocationId: event.invocation_id,
                sequence,
                text,
                displayState:
                  typeof displayState === 'string' ? displayState : undefined,
                displayReason:
                  typeof displayReason === 'string' ? displayReason : undefined,
              })
            }
          } else {
            const displayState = event.payload.display_state
            const displayReason = event.payload.display_reason
            controller.completeLiveOutput(
              event.presentation_id,
              typeof displayState === 'string' ? displayState : undefined,
              typeof displayReason === 'string' ? displayReason : undefined,
            )
          }
        }
        return
      }

      if (event.event_type === 'presentation_updated') {
        const presentationId = event.presentation_id
        if (presentationId) invalidateFullPresentationRequest(presentationId)
        const record = presentationRecordFromLiveEvent(event)
        if (record) {
          if (presentationId) {
            detailQueue.delete(presentationId)
            detailVersions.delete(presentationId)
            detailPendingWatermarks.delete(presentationId)
          }
          const agentId = record.agent_id ?? event.agent_id
          if (agentId && visibleAgentIds().includes(agentId)) {
            ensureController(agentId).upsert(record, true)
          }
          return
        }
        scheduleDetailRefresh(event)
        return
      }

      // Lifecycle events update the Agent summary immediately. Canonical card
      // state arrives on the materialized `presentation_updated` event.
    } catch (error) {
      setConnectionError(`Live event rejected: ${messageFrom(error)}`)
    }
  }

  const attachListeners = (source: EventSourceLike) => {
    for (const type of EVENT_TYPES) {
      source.addEventListener(type, handleLiveEvent as unknown as EventListener)
    }
  }

  const openSource = (purpose: 'initial' | 'handover') => {
    if (disposed) return
    const source = api.openEventSource(eventStreamUrl(visibleAgentIds(), diffProjection()))
    if (purpose === 'initial') activeSource = source
    attachListeners(source)
    let openedOnce = false
    source.onopen = () => {
      if (disposed) {
        source.close()
        return
      }
      const wasReconnect = openedOnce
      openedOnce = true
      if (purpose === 'handover' && pendingSource === source) {
        activeSource?.close()
        activeSource = source
        pendingSource = undefined
        setConnectionState('connected')
        setConnectionError(undefined)
        return
      }
      synchronization = synchronization.then(() => synchronizeAfterOpen(!wasReconnect && !initialRecoveryComplete))
    }
    source.onerror = () => {
      if (disposed) return
      if (pendingSource === source) {
        source.close()
        pendingSource = undefined
        if (pendingRetry !== undefined) clearTimeout(pendingRetry)
        pendingRetry = setTimeout(() => {
          pendingRetry = undefined
          if (!disposed) requestHandover()
        }, 500)
        return
      }
      if (activeSource === source) {
        disconnectStartedAtMs ??= Date.now()
        setConnectionState('disconnected')
      }
    }
    if (purpose === 'handover') pendingSource = source
  }

  function requestHandover() {
    if (!started || disposed || !activeSource) return
    pendingSource?.close()
    pendingSource = undefined
    openSource('handover')
  }

  const setVisibleAgentIds = (ids: readonly string[]) => {
    const unique = [...new Set(ids)]
    if (sameIdMembership(unique, visibleAgentIds())) return
    const removed = visibleAgentIds().filter((id) => !unique.includes(id))
    setVisibleIds(unique)
    for (const agentId of removed) {
      controllers.get(agentId)?.dispose()
      controllers.delete(agentId)
      hydratedAgents.delete(agentId)
      hydratingAgents.delete(agentId)
    }
    for (const agentId of unique) ensureController(agentId)
    requestHandover()
  }

  const setDiffProjection = (projection: DiffProjection) => {
    if (projection === diffProjection()) return
    setDiffProjectionSignal(projection)
    if (projection === 'summary') {
      if (fullPresentationFrame !== undefined) {
        cancelAnimationFrame(fullPresentationFrame)
        fullPresentationFrame = undefined
      }
      fullPresentationQueue.clear()
      fullPresentationVersions.clear()
      for (const controller of controllers.values()) controller.dropDiffBodies()
    }
    requestHandover()
  }

  return {
    runtimeId,
    agents,
    connectionState,
    connectionError,
    visibleAgentIds,
    setVisibleAgentIds,
    setDiffProjection,
    controllerFor: ensureController,
    start: () => {
      if (started || disposed) return
      started = true
      for (const agentId of visibleAgentIds()) ensureController(agentId)
      openSource('initial')
    },
    dispose: () => {
      disposed = true
      activeSource?.close()
      pendingSource?.close()
      if (pendingRetry !== undefined) clearTimeout(pendingRetry)
      if (detailFrame !== undefined) cancelAnimationFrame(detailFrame)
      if (fullPresentationFrame !== undefined) cancelAnimationFrame(fullPresentationFrame)
      if (agentRefreshFrame !== undefined) cancelAnimationFrame(agentRefreshFrame)
      for (const controller of controllers.values()) controller.dispose()
      controllers.clear()
    },
  }
}
