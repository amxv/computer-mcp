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
  mutationVersion: number
}

interface ExpansionSlot {
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
  requestFullPresentation?: (presentationId: string) => void
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
  recoveryCheckpoint: () => number
  mergeRecovery: (
    records: readonly PresentationRecord[],
    checkpoint?: number,
  ) => number
  appendLiveOutput: (input: {
    presentationId: string
    invocationId: number
    sequence: number
    text: string
    displayState?: string
    displayReason?: string
  }) => void
  appendLiveOutputBatch: (input: {
    presentationId: string
    invocationId: number
    chunks: ReadonlyArray<{ sequence: number; text: string }>
    displayState?: string
    displayReason?: string
  }) => void
  completeLiveOutput: (
    presentationId: string,
    displayState?: string,
    displayReason?: string,
  ) => void
  outputState: (presentationId: string, invocationId: number) => CommandOutputState
  commandOutputAvailability: (presentationId: string) => Accessor<boolean | undefined>
  markOutputRecoveryNeeded: () => void
  lastLiveOutputSequence: (presentationId: string) => number | undefined
  commandExpanded: (presentationId: string) => Accessor<boolean>
  toggleCommandExpansion: (presentationId: string) => void
  setCommandExpansionDefault: (expanded: boolean) => void
  diffExpanded: (diffKey: string) => Accessor<boolean>
  toggleDiffExpansion: (diffKey: string) => void
  setDiffExpansionDefault: (expanded: boolean) => void
  requestFullPresentation: (presentationId: string) => void
  dropDiffBodies: () => void
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
  const [outputAvailability, setOutputAvailability] = createSignal(new Map<string, boolean>())
  const commandExpansion = new Map<string, ExpansionSlot>()
  const diffExpansion = new Map<string, ExpansionSlot>()
  const unseenIds = new Set<string>()
  const [orderedIds, setOrderedIds] = createSignal<string[]>([])
  const [following, setFollowingSignal] = createSignal(true)
  const [unseenCount, setUnseenCount] = createSignal(0)
  const [historyActivated, setHistoryActivated] = createSignal(false)
  const [historyLoading, setHistoryLoading] = createSignal(false)
  const [historyExhausted, setHistoryExhausted] = createSignal(false)
  const [historyError, setHistoryError] = createSignal<string>()
  const [commandExpansionDefault, setCommandExpansionDefaultSignal] = createSignal(false)
  const [diffExpansionDefault, setDiffExpansionDefaultSignal] = createSignal(true)
  let historyCursor: string | undefined
  let nextMutationVersion = 1
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

  const markOutputAvailability = (presentationId: string, available: boolean) => {
    setOutputAvailability((current) => {
      if (current.get(presentationId) === available) return current
      const next = new Map(current)
      next.set(presentationId, available)
      return next
    })
  }

  const upsert = (record: PresentationRecord, activity = true): boolean => {
    if (disposed || record.agent_id !== options.agentId) return false
    if (record.kind === 'command' && (record.output?.length ?? 0) > 0) {
      markOutputAvailability(record.presentation_id, true)
    }
    const existing = cards.get(record.presentation_id)
    if (existing) {
      existing.setRecord(record)
      existing.mutationVersion = nextMutationVersion++
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
      mutationVersion: nextMutationVersion++,
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

  const mergeRecovery = (
    records: readonly PresentationRecord[],
    checkpoint?: number,
  ) => {
    // Recovery is an asynchronous API snapshot. Preserve any live SSE
    // mutation that reached this card after that recovery request began.
    let added = 0
    for (const record of records) {
      const existing = cards.get(record.presentation_id)
      if (
        checkpoint !== undefined &&
        existing !== undefined &&
        existing.mutationVersion > checkpoint
      ) {
        continue
      }
      if (upsert(record, false)) added += 1
    }
    return added
  }

  const setFollowing = (value: boolean) => {
    if (following() === value) {
      if (value && unseenCount() > 0) clearUnseen()
      return
    }
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

  const expansionSlot = (slots: Map<string, ExpansionSlot>, key: string) => {
    const existing = slots.get(key)
    if (existing) return existing
    const [override, setOverride] = createSignal<boolean | undefined>()
    const slot = { override, setOverride }
    slots.set(key, slot)
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

  const appendLiveOutputBatch = ({
    presentationId,
    invocationId,
    chunks,
    displayState,
    displayReason,
  }: {
    presentationId: string
    invocationId: number
    chunks: ReadonlyArray<{ sequence: number; text: string }>
    displayState?: string
    displayReason?: string
  }) => {
    if (disposed) return
    if (chunks.some((chunk) => chunk.text.length > 0) || displayState === 'unavailable') {
      markOutputAvailability(presentationId, true)
    }
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
    state.appendLiveBatch(chunks, displayState, displayReason)
    markActivity(presentationId)
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
    recoveryCheckpoint: () => nextMutationVersion - 1,
    mergeRecovery,
    appendLiveOutput: ({ presentationId, invocationId, sequence, text, displayState, displayReason }) =>
      appendLiveOutputBatch({
        presentationId,
        invocationId,
        chunks: [{ sequence, text }],
        displayState,
        displayReason,
      }),
    appendLiveOutputBatch,
    completeLiveOutput: (presentationId, displayState, displayReason) => {
      const state = outputStates.get(presentationId)
      if (displayState === 'unavailable') {
        markOutputAvailability(presentationId, true)
      } else if (outputAvailability().get(presentationId) !== true) {
        markOutputAvailability(presentationId, (state?.materialize().length ?? 0) > 0)
      }
      state?.markComplete(displayState, displayReason)
    },
    outputState,
    commandOutputAvailability: (presentationId) => () =>
      outputAvailability().get(presentationId),
    markOutputRecoveryNeeded: () => {
      for (const state of outputStates.values()) state.markRecoveryNeeded()
    },
    lastLiveOutputSequence: (presentationId) =>
      outputStates.get(presentationId)?.buffer.lastRetainedSequence(),
    commandExpanded: (presentationId) => {
      const slot = expansionSlot(commandExpansion, presentationId)
      return () => slot.override() ?? commandExpansionDefault()
    },
    toggleCommandExpansion: (presentationId) => {
      const slot = expansionSlot(commandExpansion, presentationId)
      slot.setOverride(!(slot.override() ?? commandExpansionDefault()))
    },
    setCommandExpansionDefault: (expanded) => {
      if (commandExpansionDefault() === expanded) return
      setCommandExpansionDefaultSignal(expanded)
      for (const slot of commandExpansion.values()) slot.setOverride(undefined)
    },
    diffExpanded: (diffKey) => {
      const slot = expansionSlot(diffExpansion, diffKey)
      return () => slot.override() ?? diffExpansionDefault()
    },
    toggleDiffExpansion: (diffKey) => {
      const slot = expansionSlot(diffExpansion, diffKey)
      slot.setOverride(!(slot.override() ?? diffExpansionDefault()))
    },
    setDiffExpansionDefault: (expanded) => {
      if (diffExpansionDefault() === expanded) return
      setDiffExpansionDefaultSignal(expanded)
      for (const slot of diffExpansion.values()) slot.setOverride(undefined)
    },
    requestFullPresentation: options.requestFullPresentation ?? (() => undefined),
    dropDiffBodies: () => {
      for (const slot of cards.values()) {
        const record = slot.record()
        if (record.kind !== 'file_changes') continue
        if (!record.changes.some((change) => change.diff_lines_included)) continue
        slot.setRecord({
          ...record,
          changes: record.changes.map((change) => ({
            ...change,
            diff_lines_included: false,
            lines: [],
          })),
        })
      }
    },
    setFollowing,
    returnToLive: () => setFollowing(true),
    loadEarlier,
    dispose: () => {
      disposed = true
      cards.clear()
      outputStates.clear()
      setOutputAvailability(new Map())
      commandExpansion.clear()
      diffExpansion.clear()
      unseenIds.clear()
      setOrderedIds([])
      setUnseenCount(0)
    },
  }
}
