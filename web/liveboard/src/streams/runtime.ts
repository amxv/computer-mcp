import { createSignal, type Accessor } from 'solid-js'

import {
  eventStreamUrl,
  fetchCurrentAgents,
  fetchStatus,
  fetchTimeline,
  fetchTimelineDetail,
  validateLiveEvent,
  type ApiAgent,
  type ApiStatus,
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
  openEventSource: (url: string) => EventSourceLike
}

export interface RuntimeConnection {
  runtimeId: Accessor<string>
  agents: Accessor<ApiAgent[]>
  connectionState: Accessor<RuntimeConnectionState>
  connectionError: Accessor<string | undefined>
  visibleAgentIds: Accessor<string[]>
  setVisibleAgentIds: (ids: readonly string[]) => void
  controllerFor: (agentId: string) => AgentStreamController
  start: () => void
  dispose: () => void
}

interface RuntimeConnectionOptions {
  initialStatus: ApiStatus
  initialAgents: readonly ApiAgent[]
  initialVisibleAgentIds: readonly string[]
  viewerAttachWatermarkMs: number
  api?: RuntimeApi
}

const browserRuntimeApi: RuntimeApi = {
  fetchStatus,
  fetchCurrentAgents,
  fetchTimeline,
  fetchTimelineDetail,
  openEventSource: (url) => new EventSource(url),
}

function sameIds(left: readonly string[], right: readonly string[]) {
  return left.length === right.length && left.every((value, index) => value === right[index])
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
  const controllers = new Map<string, AgentStreamController>()
  const hydratedAgents = new Set<string>()
  const hydratingAgents = new Set<string>()
  const seenSequences = new Set<number>()
  const seenSequenceOrder: number[] = []
  const detailVersions = new Map<string, number>()
  const detailPendingWatermarks = new Map<string, number>()
  const detailQueue = new Set<string>()
  let detailFrame: number | undefined
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
    seenSequences.add(sequence)
    seenSequenceOrder.push(sequence)
    while (seenSequenceOrder.length > SEEN_SEQUENCE_LIMIT) {
      const removed = seenSequenceOrder.shift()
      if (removed !== undefined) seenSequences.delete(removed)
    }
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
        limit: RECOVERY_PAGE_SIZE,
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
    }
  }

  const refreshDetail = async (presentationId: string, version: number) => {
    try {
      const detail = await api.fetchTimelineDetail(presentationId, runtimeId())
      const agentId = detail.record.agent_id
      if (agentId && visibleAgentIds().includes(agentId)) {
        ensureController(agentId).upsert(detail.record, true)
      }
      finishDetailRefresh(presentationId, version)
    } catch (error) {
      setConnectionError(`Timeline refresh failed: ${messageFrom(error)}`)
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
    detailVersions.set(presentationId, (detailVersions.get(presentationId) ?? 0) + 1)
    detailQueue.add(presentationId)
    if (detailFrame === undefined) detailFrame = requestAnimationFrame(flushDetailQueue)
  }

  const resetRuntime = (nextRuntimeId: string, nextAgents: readonly ApiAgent[], boundaryMs: number) => {
    for (const controller of controllers.values()) controller.dispose()
    controllers.clear()
    hydratedAgents.clear()
    hydratingAgents.clear()
    seenSequences.clear()
    seenSequenceOrder.length = 0
    detailQueue.clear()
    detailVersions.clear()
    detailPendingWatermarks.clear()
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
          const sequence = event.payload.output_sequence
          ensureController(event.agent_id).noteLiveOutput(
            event.presentation_id,
            typeof sequence === 'number' ? sequence : undefined,
          )
        }
        return
      }

      if (
        event.event_type === 'invocation_started' ||
        event.event_type === 'invocation_completed' ||
        event.event_type === 'presentation_updated' ||
        event.event_type === 'process_started' ||
        event.event_type === 'process_ended'
      ) {
        scheduleDetailRefresh(event)
      }
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
    const source = api.openEventSource(eventStreamUrl(visibleAgentIds()))
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
    if (sameIds(unique, visibleAgentIds())) return
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

  return {
    runtimeId,
    agents,
    connectionState,
    connectionError,
    visibleAgentIds,
    setVisibleAgentIds,
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
      if (agentRefreshFrame !== undefined) cancelAnimationFrame(agentRefreshFrame)
      for (const controller of controllers.values()) controller.dispose()
      controllers.clear()
    },
  }
}
