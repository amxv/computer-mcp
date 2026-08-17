import {
  createSignal,
  type Accessor,
  type Setter,
} from 'solid-js'

import type {
  ApiOutputMetadataDocument,
  ApiOutputPage,
  ApiTimelinePage,
  PresentationRecord,
} from '../api/client'
import {
  CommandOutputState,
  type CommandOutputLoader,
} from '../output/CommandOutputState'

interface CardSlot {
  record: Accessor<PresentationRecord>
  setRecord: Setter<PresentationRecord>
}

interface CommandExpansionSlot {
  override: Accessor<boolean | undefined>
  setOverride: Setter<boolean | undefined>
}

interface AgentStreamControllerOptions {
  agentId: string
  attachWatermarkMs: number
  loadHistoryPage: (input: {
    agentId: string
    beforeMs: number
    cursor?: string
  }) => Promise<ApiTimelinePage>
  loadOutputMetadata: (invocationId: number) => Promise<ApiOutputMetadataDocument>
  loadDisplayOutputPage: (
    invocationId: number,
    cursor: number,
    limit: number,
  ) => Promise<ApiOutputPage>
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
  appendLiveOutput: (input: {
    presentationId: string
    invocationId: number
    sequence: number
    text: string
    displayState?: string
    displayReason?: string
  }) => void
  completeLiveOutput: (
    presentationId: string,
    displayState?: string,
    displayReason?: string,
  ) => void
  outputState: (presentationId: string, invocationId: number) => CommandOutputState
  markOutputRecoveryNeeded: () => void
  lastLiveOutputSequence: (presentationId: string) => number | undefined
  commandExpanded: (presentationId: string) => Accessor<boolean>
  toggleCommandExpansion: (presentationId: string) => void
  setCommandExpansionDefault: (expanded: boolean) => void
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
  const outputStates = new Map<string, CommandOutputState>()
  const commandExpansion = new Map<string, CommandExpansionSlot>()
  const unseenIds = new Set<string>()
  const [orderedIds, setOrderedIds] = createSignal<string[]>([])
  const [following, setFollowingSignal] = createSignal(true)
  const [unseenCount, setUnseenCount] = createSignal(0)
  const [historyActivated, setHistoryActivated] = createSignal(false)
  const [historyLoading, setHistoryLoading] = createSignal(false)
  const [historyExhausted, setHistoryExhausted] = createSignal(false)
  const [historyError, setHistoryError] = createSignal<string>()
  const [commandExpansionDefault, setCommandExpansionDefaultSignal] = createSignal(false)
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
      if (record.kind === 'command' && record.status !== 'running') {
        outputStates.get(record.presentation_id)?.markFinal()
      }
      if (activity) markActivity(record.presentation_id)
      return false
    }

    const [recordSignal, setRecord] = createSignal(record, { equals: false })
    cards.set(record.presentation_id, {
      record: recordSignal,
      setRecord,
    })
    if (record.kind === 'command' && record.status !== 'running') {
      outputStates.get(record.presentation_id)?.markFinal()
    }
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

  const outputLoader: CommandOutputLoader = {
    loadMetadata: options.loadOutputMetadata,
    loadDisplayPage: options.loadDisplayOutputPage,
  }

  const outputState = (presentationId: string, invocationId: number) => {
    const existing = outputStates.get(presentationId)
    if (existing) return existing
    let state: CommandOutputState
    state = new CommandOutputState(
      presentationId,
      invocationId,
      outputLoader,
      () => {
        if (outputStates.get(presentationId) === state && state.subscriberCount() === 0) {
          outputStates.delete(presentationId)
        }
      },
    )
    outputStates.set(presentationId, state)
    return state
  }

  const expansionSlot = (presentationId: string) => {
    const existing = commandExpansion.get(presentationId)
    if (existing) return existing
    const [override, setOverride] = createSignal<boolean | undefined>()
    const slot = { override, setOverride }
    commandExpansion.set(presentationId, slot)
    return slot
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
    appendLiveOutput: ({
      presentationId,
      invocationId,
      sequence,
      text,
      displayState,
      displayReason,
    }) => {
      if (disposed) return
      const record = cards.get(presentationId)?.record()
      const existingOutputState = outputStates.get(presentationId)
      if (
        record?.kind === 'command' &&
        record.status !== 'running' &&
        existingOutputState === undefined
      ) {
        // Process lifecycle can become final before the PTY reader drains its
        // last chunks. Do not recreate a released buffer for a collapsed final
        // card, but keep feeding an already-mounted/expanded buffer so those
        // trailing bytes remain visible until the subscriber detaches.
        markActivity(presentationId)
        return
      }
      const state = existingOutputState ?? outputState(presentationId, invocationId)
      state.appendLive(
        sequence,
        text,
        displayState,
        displayReason,
      )
      markActivity(presentationId)
    },
    completeLiveOutput: (presentationId, displayState, displayReason) => {
      outputStates.get(presentationId)?.markComplete(displayState, displayReason)
    },
    outputState,
    markOutputRecoveryNeeded: () => {
      for (const state of outputStates.values()) state.markRecoveryNeeded()
    },
    lastLiveOutputSequence: (presentationId) =>
      outputStates.get(presentationId)?.buffer.lastRetainedSequence(),
    commandExpanded: (presentationId) => {
      const slot = expansionSlot(presentationId)
      return () => slot.override() ?? commandExpansionDefault()
    },
    toggleCommandExpansion: (presentationId) => {
      const slot = expansionSlot(presentationId)
      slot.setOverride(!(slot.override() ?? commandExpansionDefault()))
    },
    setCommandExpansionDefault: (expanded) => {
      if (commandExpansionDefault() === expanded) return
      setCommandExpansionDefaultSignal(expanded)
      for (const slot of commandExpansion.values()) slot.setOverride(undefined)
    },
    setFollowing,
    returnToLive: () => setFollowing(true),
    loadEarlier,
    dispose: () => {
      disposed = true
      cards.clear()
      outputStates.clear()
      commandExpansion.clear()
      unseenIds.clear()
      setOrderedIds([])
      setUnseenCount(0)
    },
  }
}
