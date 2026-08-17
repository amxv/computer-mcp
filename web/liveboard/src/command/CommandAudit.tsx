import { For, Show, createSignal, onMount } from 'solid-js'

import {
  fetchInvocationDetail,
  fetchTimelineCheckpoints,
  type ApiInvocationDetail,
  type TimelineCheckpoint,
} from '../api/client'

function checkpointLabel(checkpoint: TimelineCheckpoint, index: number) {
  if (checkpoint.checkpoint_kind === 'initial') return 'Initial response'
  return `Poll ${index}`
}

function renderEvidence(detail: ApiInvocationDetail) {
  return JSON.stringify(
    {
      invocation_id: detail.invocation.id,
      tool: detail.invocation.tool_name,
      arguments: detail.invocation.arguments,
      result: detail.invocation.result,
      error: detail.invocation.error,
      outcome: detail.invocation.outcome_kind,
      evidence_state: detail.invocation.evidence_state,
      capture_state: detail.invocation.capture_state,
    },
    null,
    2,
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
    <section class="command-audit" aria-label="What ChatGPT received">
      <div class="audit-heading">
        <strong>What ChatGPT received</strong>
        <span>Exact logical tool evidence</span>
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
        {(detail) => <pre class="audit-evidence">{renderEvidence(detail())}</pre>}
      </Show>
      <Show when={error()}>
        {(message) => <p class="audit-error">{message()}</p>}
      </Show>
    </section>
  )
}
