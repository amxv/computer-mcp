import { For, Show, createSignal, onCleanup, onMount } from 'solid-js'

import {
  fetchInvocationDetail,
  fetchTimelineCheckpoints,
  type ApiInvocationDetail,
  type TimelineCheckpoint,
} from '../api/client'
import { CheckIcon, CopyIcon } from '../ui/icons'

function checkpointLabel(checkpoint: TimelineCheckpoint, index: number) {
  if (checkpoint.checkpoint_kind === 'initial') return 'Initial response'
  return `Poll ${index}`
}

function renderRawValue(value: unknown) {
  if (typeof value === 'string') return value
  if (value === undefined) return ''
  return JSON.stringify(value, null, 2)
}

function RawEvidenceBlock(props: { label: string; value: unknown }) {
  const [copied, setCopied] = createSignal(false)
  let copyFeedbackTimeout: ReturnType<typeof setTimeout> | undefined
  const text = () => renderRawValue(props.value)

  onCleanup(() => {
    if (copyFeedbackTimeout !== undefined) clearTimeout(copyFeedbackTimeout)
  })

  const copy = async () => {
    if (!navigator.clipboard) return
    await navigator.clipboard.writeText(text())
    setCopied(true)
    if (copyFeedbackTimeout !== undefined) clearTimeout(copyFeedbackTimeout)
    copyFeedbackTimeout = setTimeout(() => {
      setCopied(false)
      copyFeedbackTimeout = undefined
    }, 2_000)
  }

  return (
    <div class="raw-evidence-block">
      <div class="raw-evidence-heading">
        <strong>{props.label}</strong>
        <button
          type="button"
          class="raw-copy-button"
          aria-label={copied() ? `${props.label} copied` : `Copy ${props.label.toLowerCase()}`}
          title={copied() ? 'Copied' : 'Copy'}
          onClick={() => void copy()}
        >
          {copied() ? <CheckIcon /> : <CopyIcon />}
        </button>
      </div>
      <pre class="audit-evidence">{text()}</pre>
    </div>
  )
}

export function CommandAudit(props: {
  presentationId: string
  runtimeId: string
}) {
  const [checkpoints, setCheckpoints] = createSignal<TimelineCheckpoint[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [hasMore, setHasMore] = createSignal(false)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string>()
  const [selectedInvocationId, setSelectedInvocationId] = createSignal<number>()
  const [selectedDetail, setSelectedDetail] = createSignal<ApiInvocationDetail>()

  const loadCheckpointPage = async (cursor?: string) => {
    if (loading()) return
    setLoading(true)
    setError(undefined)
    try {
      const page = await fetchTimelineCheckpoints(
        props.presentationId,
        { cursor, limit: 50 },
        props.runtimeId,
      )
      setCheckpoints((current) => [...current, ...page.checkpoints])
      setNextCursor(page.next_cursor)
      setHasMore(page.has_more)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setLoading(false)
    }
  }

  const selectCheckpoint = async (checkpoint: TimelineCheckpoint) => {
    setSelectedInvocationId(checkpoint.invocation_id)
    setSelectedDetail(undefined)
    setError(undefined)
    try {
      setSelectedDetail(
        await fetchInvocationDetail(checkpoint.invocation_id, props.runtimeId),
      )
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  onMount(() => void loadCheckpointPage())

  return (
    <section class="command-audit" aria-label="Raw tool exchange">
      <div class="audit-heading">
        <strong>Raw tool exchange</strong>
      </div>
      <div class="checkpoint-strip" role="list" aria-label="Command response checkpoints">
        <For each={checkpoints()}>
          {(checkpoint, index) => (
            <button
              type="button"
              class="checkpoint-button"
              classList={{
                'checkpoint-selected':
                  selectedInvocationId() === checkpoint.invocation_id,
              }}
              onClick={() => void selectCheckpoint(checkpoint)}
            >
              {checkpointLabel(checkpoint, index())}
            </button>
          )}
        </For>
        <Show when={hasMore()}>
          <button
            type="button"
            class="checkpoint-button"
            disabled={loading()}
            onClick={() => void loadCheckpointPage(nextCursor() ?? undefined)}
          >
            {loading() ? 'Loading…' : 'More'}
          </button>
        </Show>
      </div>
      <Show when={loading() && checkpoints().length === 0}>
        <p class="audit-empty">Loading checkpoint metadata…</p>
      </Show>
      <Show when={!loading() && checkpoints().length === 0 && !error()}>
        <p class="audit-empty">No retained checkpoints.</p>
      </Show>
      <Show when={selectedDetail()}>
        {(detail) => (
          <div class="raw-evidence-grid">
            <RawEvidenceBlock label="Tool input" value={detail().invocation.arguments} />
            <RawEvidenceBlock
              label="Tool output"
              value={detail().invocation.error ?? detail().invocation.result}
            />
          </div>
        )}
      </Show>
      <Show when={error()}>
        {(message) => <p class="audit-error">{message()}</p>}
      </Show>
    </section>
  )
}
