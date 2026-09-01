import type {
  ApiOutputMetadataDocument,
  ApiOutputPage,
} from '../api/client'
import { LiveOutputBuffer } from './LiveOutputBuffer'

const RECENT_OUTPUT_CHUNK_LIMIT = 64

export interface CommandOutputLoader {
  loadMetadata: (invocationId: number) => Promise<ApiOutputMetadataDocument>
  loadDisplayPage: (
    invocationId: number,
    cursor: number,
    limit: number,
  ) => Promise<ApiOutputPage>
}

export class CommandOutputState {
  readonly buffer: LiveOutputBuffer
  private readonly subscribers = new Set<() => void>()
  private hydration: Promise<void> | undefined
  private recoveryNeeded = true
  private hydrated = false
  private final = false
  private complete = false
  private loading = false
  private displayState: string | undefined
  private displayReason: string | undefined
  private recoveryError: string | undefined

  constructor(
    readonly presentationId: string,
    readonly invocationId: number,
    private readonly loader: CommandOutputLoader,
    private readonly onUnusedFinal: () => void,
    maximumBytes = 96 * 1024,
  ) {
    this.buffer = new LiveOutputBuffer(maximumBytes)
  }

  subscribe(callback: () => void) {
    this.subscribers.add(callback)
    return () => {
      this.subscribers.delete(callback)
      if (this.final && this.subscribers.size === 0) this.onUnusedFinal()
    }
  }

  appendLive(
    sequence: number,
    text: string,
    displayState?: string,
    displayReason?: string,
  ) {
    return this.appendLiveBatch([{ sequence, text }], displayState, displayReason)
  }

  appendLiveBatch(
    chunks: ReadonlyArray<{ sequence: number; text: string }>,
    displayState?: string,
    displayReason?: string,
  ) {
    let added = false
    let duplicate = chunks.length > 0
    let sequenceGap = false
    for (const chunk of chunks) {
      const result = this.buffer.append(chunk.sequence, chunk.text)
      added ||= result.added
      duplicate &&= result.duplicate
      sequenceGap ||= result.sequenceGap
    }
    if (sequenceGap) this.recoveryNeeded = true
    this.updateDisplayState(displayState, displayReason)
    if (added || displayState === 'unavailable') this.notify()
    if (this.recoveryNeeded && this.subscribers.size > 0) {
      this.requestRecentTail()
    }
    return { added, duplicate, sequenceGap }
  }

  markComplete(displayState?: string, displayReason?: string) {
    this.complete = true
    // EOF is the authoritative point at which the durable output tail is
    // complete. Reconcile once even when no later live chunk exists to expose
    // a dropped tail on the independent ephemeral output channel.
    this.recoveryNeeded = true
    this.updateDisplayState(displayState, displayReason)
    this.notify()
    if (this.subscribers.size > 0) this.requestRecentTail()
  }

  markRecoveryNeeded() {
    this.recoveryNeeded = true
    if (this.subscribers.size > 0) this.requestRecentTail()
  }

  markFinal() {
    this.final = true
    if (this.subscribers.size === 0) this.onUnusedFinal()
  }

  isFinal() {
    return this.final
  }

  isComplete() {
    return this.complete
  }

  needsRecovery() {
    return this.recoveryNeeded || !this.hydrated
  }

  isDisplayUnavailable() {
    return this.displayState === 'unavailable'
  }

  displayUnavailableReason() {
    return this.displayReason
  }

  recoveryErrorMessage() {
    return this.recoveryError
  }

  isLoading() {
    return this.loading
  }

  hasDroppedPrefix() {
    return this.buffer.hasDroppedPrefix()
  }

  materialize() {
    return this.buffer.materialize()
  }

  subscriberCount() {
    return this.subscribers.size
  }

  async ensureRecentTail() {
    if (!this.needsRecovery()) return
    if (this.hydration) return this.hydration
    this.recoveryError = undefined
    this.loading = true
    this.notify()
    this.hydration = this.hydrateRecentTail().finally(() => {
      this.loading = false
      this.hydration = undefined
      this.notify()
    })
    return this.hydration
  }

  requestRecentTail() {
    void this.ensureRecentTail().catch((error: unknown) => {
      this.recoveryError = error instanceof Error ? error.message : String(error)
      this.notify()
    })
  }

  private async hydrateRecentTail() {
    const metadataDocument = await this.loader.loadMetadata(this.invocationId)
    const metadata = metadataDocument.output
    if (!metadata.available || metadata.last_cursor === null) {
      this.hydrated = true
      this.recoveryNeeded = false
      this.notify()
      return
    }
    const startCursor = Math.max(
      metadata.first_cursor ?? 0,
      metadata.last_cursor - (RECENT_OUTPUT_CHUNK_LIMIT - 1),
    )
    if (metadata.first_cursor !== null && startCursor > metadata.first_cursor) {
      this.buffer.markDroppedPrefix()
    }
    const page = await this.loader.loadDisplayPage(
      this.invocationId,
      startCursor,
      RECENT_OUTPUT_CHUNK_LIMIT,
    )
    this.updateDisplayState(page.display_state, page.display_reason)
    if (page.display_state === 'unavailable') {
      this.hydrated = true
      this.recoveryNeeded = false
      this.notify()
      return
    }
    this.buffer.mergeDurable(page.chunks, {
      dropBeforeSequence: startCursor,
    })
    this.hydrated = true
    this.recoveryNeeded = this.buffer.hasInternalSequenceGap()
    this.notify()
  }

  private updateDisplayState(state?: string, reason?: string) {
    if (state !== undefined) this.displayState = state
    if (reason !== undefined) this.displayReason = reason
  }

  private notify() {
    for (const subscriber of this.subscribers) subscriber()
  }
}
