import {
  createSignal,
  type Accessor,
  type Setter,
} from 'solid-js'

import type {
  ApiTimelinePage,
  PresentationRecord,
} from '../api/client'

interface CardSlot {
  record: Accessor<PresentationRecord>
  setRecord: Setter<PresentationRecord>
}

interface AgentStreamControllerOptions {
  agentId: string
  attachWatermarkMs: number
  loadHistoryPage: (input: {
    agentId: string
    beforeMs: number
    cursor?: string
  }) => Promise<ApiTimelinePage>
}

export interface AgentStreamController {
  agentId: string
  attachWatermarkMs: number
  orderedIds: Accessor<string[]>
  record: (presentationId: string) => PresentationRecord | undefined
  following: Accessor<boolean>
  unseenCount: Accessor<number>
  historyActivated: Accessor<boolean>
  historyLoading: Accessor<boolean>
  historyExhausted: Accessor<boolean>
  historyError: Accessor<string | undefined>
  upsert: (record: PresentationRecord, activity?: boolean) => boolean
  mergeRecovery: (records: readonly PresentationRecord[]) => number
  noteLiveOutput: (presentationId: string, sequence?: number) => void
  lastLiveOutputSequence: (presentationId: string) => number | undefined
  setFollowing: (following: boolean) => void
  returnToLive: () => void
  loadEarlier: () => Promise<number>
  dispose: () => void
}

function compareRecords(left: PresentationRecord, right: PresentationRecord) {
  if (left.started_at_ms !== right.started_at_ms) {
    return left.started_at_ms - right.started_at_ms
  }
  if (left.primary_invocation_id !== right.primary_invocation_id) {
    return left.primary_invocation_id - right.primary_invocation_id
  }
  return left.presentation_id.localeCompare(right.presentation_id)
}

export function createAgentStreamController(
  options: AgentStreamControllerOptions,
): AgentStreamController {
  const cards = new Map<string, CardSlot>()
  const outputSequences = new Map<string, number>()
  const unseenIds = new Set<string>()
  const [orderedIds, setOrderedIds] = createSignal<string[]>([])
  const [following, setFollowingSignal] = createSignal(true)
  const [unseenCount, setUnseenCount] = createSignal(0)
  const [historyActivated, setHistoryActivated] = createSignal(false)
  const [historyLoading, setHistoryLoading] = createSignal(false)
  const [historyExhausted, setHistoryExhausted] = createSignal(false)
  const [historyError, setHistoryError] = createSignal<string>()
  let historyCursor: string | undefined
  let disposed = false

  const clearUnseen = () => {
    unseenIds.clear()
    setUnseenCount(0)
  }

  const markActivity = (presentationId: string) => {
    if (following() || unseenIds.has(presentationId)) return
    unseenIds.add(presentationId)
    setUnseenCount(unseenIds.size)
  }

  const upsert = (record: PresentationRecord, activity = true): boolean => {
    if (disposed || record.agent_id !== options.agentId) return false
    const existing = cards.get(record.presentation_id)
    if (existing) {
      existing.setRecord(record)
      if (activity) markActivity(record.presentation_id)
      return false
    }

    const [recordSignal, setRecord] = createSignal(record, { equals: false })
    cards.set(record.presentation_id, {
      record: recordSignal,
      setRecord,
    })
    setOrderedIds((ids) => {
      const next = [...ids, record.presentation_id]
      next.sort((leftId, rightId) => {
        const left = cards.get(leftId)?.record()
        const right = cards.get(rightId)?.record()
        if (!left || !right) return leftId.localeCompare(rightId)
        return compareRecords(left, right)
      })
      return next
    })
    if (activity) markActivity(record.presentation_id)
    return true
  }

  const mergeRecovery = (records: readonly PresentationRecord[]) => {
    let added = 0
    for (const record of records) {
      if (upsert(record, false)) added += 1
    }
    return added
  }

  const setFollowing = (value: boolean) => {
    setFollowingSignal(value)
    if (value) clearUnseen()
  }

  const loadEarlier = async () => {
    if (disposed || historyLoading() || historyExhausted()) return 0
    setHistoryActivated(true)
    setHistoryLoading(true)
    setHistoryError(undefined)
    try {
      const page = await options.loadHistoryPage({
        agentId: options.agentId,
        beforeMs: options.attachWatermarkMs,
        cursor: historyCursor,
      })
      const added = mergeRecovery(page.records)
      historyCursor = page.next_cursor ?? undefined
      setHistoryExhausted(!page.has_more)
      return added
    } catch (error) {
      setHistoryError(error instanceof Error ? error.message : String(error))
      return 0
    } finally {
      setHistoryLoading(false)
    }
  }

  return {
    agentId: options.agentId,
    attachWatermarkMs: options.attachWatermarkMs,
    orderedIds,
    record: (presentationId) => cards.get(presentationId)?.record(),
    following,
    unseenCount,
    historyActivated,
    historyLoading,
    historyExhausted,
    historyError,
    upsert,
    mergeRecovery,
    noteLiveOutput: (presentationId, sequence) => {
      if (disposed) return
      if (sequence !== undefined) {
        const previous = outputSequences.get(presentationId)
        if (previous === undefined || sequence > previous) {
          outputSequences.set(presentationId, sequence)
        }
      }
      markActivity(presentationId)
    },
    lastLiveOutputSequence: (presentationId) => outputSequences.get(presentationId),
    setFollowing,
    returnToLive: () => setFollowing(true),
    loadEarlier,
    dispose: () => {
      disposed = true
      cards.clear()
      outputSequences.clear()
      unseenIds.clear()
      setOrderedIds([])
      setUnseenCount(0)
    },
  }
}
