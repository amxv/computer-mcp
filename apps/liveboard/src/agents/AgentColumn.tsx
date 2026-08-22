import {
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
  type JSX,
} from 'solid-js'

import type { ApiAgent } from '../api/client'
import {
  ChevronUpIcon,
  CheckIcon,
  CloseIcon,
  EditIcon,
  FolderIcon,
  GripIcon,
} from '../ui/icons'
import {
  agentActivity,
  relativeActivityLabel,
} from './model'

interface AgentColumnProps {
  agent: ApiAgent
  alias?: string
  nowMs: number
  weight: number
  order: number
  onHide: () => void
  onAliasSave: (alias: string) => void
  onReorderPointerDown: (event: PointerEvent) => void
  onResizePointerDown?: (event: PointerEvent) => void
  focused?: boolean
  onFocus?: () => void
  children?: JSX.Element
}

export function AgentColumn(props: AgentColumnProps) {
  const [editingAlias, setEditingAlias] = createSignal(false)
  const [aliasDraft, setAliasDraft] = createSignal(props.alias ?? '')
  const [workdirsOverflow, setWorkdirsOverflow] = createSignal(false)
  const [workdirsExpanded, setWorkdirsExpanded] = createSignal(false)
  let aliasInput: HTMLInputElement | undefined
  let workdirsInline: HTMLSpanElement | undefined
  let workdirsResizeObserver: ResizeObserver | undefined

  createEffect(() => {
    if (!editingAlias()) setAliasDraft(props.alias ?? '')
  })
  createEffect(() => {
    if (editingAlias()) {
      queueMicrotask(() => {
        aliasInput?.focus()
        aliasInput?.select()
      })
    }
  })

  const workdirs = () => props.agent.workdirs.map((workdir) => workdir.normalized_workdir)
  const measureWorkdirs = () => {
    if (!workdirsInline) return
    setWorkdirsOverflow(workdirsInline.scrollWidth > workdirsInline.clientWidth + 1)
  }
  const activity = () => agentActivity(props.agent, props.nowMs)
  const saveAlias = () => {
    props.onAliasSave(aliasDraft().trim())
    setEditingAlias(false)
  }

  createEffect(() => {
    workdirs().join('\0')
    setWorkdirsExpanded(false)
    queueMicrotask(measureWorkdirs)
  })
  createEffect(() => {
    if (!workdirsOverflow()) setWorkdirsExpanded(false)
  })
  onMount(() => {
    if (typeof ResizeObserver === 'undefined') return
    workdirsResizeObserver = new ResizeObserver(measureWorkdirs)
    if (workdirsInline) workdirsResizeObserver.observe(workdirsInline)
  })
  onCleanup(() => workdirsResizeObserver?.disconnect())

  return (
    <section
      class="agent-column"
      data-agent-column
      data-agent-id={props.agent.id}
      style={{
        '--column-weight': String(props.weight),
        '--column-order': String(props.order),
      }}
      aria-label={`Agent ${props.agent.id}${props.alias ? `, ${props.alias}` : ''}`}
    >
      <header class="agent-header">
        <div class="agent-header-topline">
          <Show when={!props.focused}>
            <button
              type="button"
              class="icon-button drag-handle"
              aria-label={`Drag Agent ${props.agent.id} to reorder`}
              title="Drag to reorder"
              onPointerDown={(event) => props.onReorderPointerDown(event)}
            >
              <GripIcon />
            </button>
          </Show>
          <div class="agent-identity">
            <Show
              when={editingAlias()}
              fallback={
                <>
                  <Show when={props.alias}>
                    <strong class="agent-alias">{props.alias}</strong>
                  </Show>
                  <code class="agent-id">{props.agent.id}</code>
                  <button
                    type="button"
                    class="icon-button identity-edit"
                    aria-label={`Edit alias for Agent ${props.agent.id}`}
                    title="Edit alias"
                    onClick={() => setEditingAlias(true)}
                  >
                    <EditIcon />
                  </button>
                </>
              }
            >
              <input
                ref={aliasInput}
                class="alias-input"
                value={aliasDraft()}
                maxlength={80}
                aria-label={`Alias for Agent ${props.agent.id}`}
                onInput={(event) => setAliasDraft(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') saveAlias()
                  if (event.key === 'Escape') setEditingAlias(false)
                }}
              />
              <button
                type="button"
                class="icon-button alias-action"
                aria-label={`Save alias for Agent ${props.agent.id}`}
                title="Save alias"
                onClick={saveAlias}
              >
                <CheckIcon />
              </button>
              <button
                type="button"
                class="icon-button alias-action"
                aria-label={`Cancel alias edit for Agent ${props.agent.id}`}
                title="Cancel"
                onClick={() => setEditingAlias(false)}
              >
                <CloseIcon />
              </button>
            </Show>
          </div>
          <div class="agent-header-actions">
            <Show when={!props.focused && props.onFocus}>
              {(handler) => (
                <button
                  type="button"
                  class="agent-focus-button"
                  aria-label={`Focus Agent ${props.agent.id}`}
                  title="Open focused view"
                  onClick={() => handler()()}
                >
                  Focus
                </button>
              )}
            </Show>
            <Show when={workdirsOverflow()}>
              <button
                type="button"
                class="icon-button agent-header-control agent-workdirs-toggle"
                aria-label={`${workdirsExpanded() ? 'Collapse' : 'Expand'} workdirs for Agent ${props.agent.id}`}
                aria-expanded={workdirsExpanded()}
                title={workdirsExpanded() ? 'Collapse workdirs' : 'Show all workdirs'}
                onClick={() => setWorkdirsExpanded((expanded) => !expanded)}
              >
                {workdirsExpanded() ? <ChevronUpIcon /> : <FolderIcon />}
              </button>
            </Show>
            <Show when={!props.focused}>
              <button
                type="button"
                class="icon-button agent-header-control"
                aria-label={`Remove Agent ${props.agent.id} from board`}
                title="Remove from board"
                onClick={props.onHide}
              >
                <CloseIcon />
              </button>
            </Show>
          </div>
        </div>
        <div class="agent-context-row">
          <span class={`activity-dot activity-${activity()}`} aria-hidden="true" />
          <span class="activity-label">
            {activity() === 'running'
              ? `${props.agent.active_process_count} active ${props.agent.active_process_count === 1 ? 'process' : 'processes'}`
              : activity() === 'recent'
                ? 'Recent'
                : 'Idle'}
          </span>
          <span class="activity-time">
            {relativeActivityLabel(props.agent.last_seen_at_ms, props.nowMs)}
          </span>
          <Show when={workdirs().length > 0}>
            <span
              ref={workdirsInline}
              class="workdir-badges"
              classList={{ 'workdir-badges-overflow': workdirsOverflow() }}
              aria-label="Agent workdirs"
            >
              <For each={workdirs()}>
                {(workdir) => (
                  <span class="workdir-badge" title={workdir}>
                    {workdir}
                  </span>
                )}
              </For>
            </span>
          </Show>
        </div>
        <Show when={workdirsExpanded() && workdirsOverflow()}>
          <div class="agent-workdirs-expanded" aria-label="Expanded Agent workdirs">
            <For each={workdirs()}>
              {(workdir) => (
                <span class="workdir-badge" title={workdir}>
                  {workdir}
                </span>
              )}
            </For>
          </div>
        </Show>
      </header>
      <Show
        when={props.children}
        fallback={
          <div class="agent-timeline-placeholder">
            <span class="timeline-placeholder-line" />
            <p>Waiting for live activity.</p>
          </div>
        }
      >
        {props.children}
      </Show>
      <Show when={!props.focused && props.onResizePointerDown}>
        {(handler) => (
          <button
            type="button"
            class="resize-handle"
            aria-label={`Resize Agent ${props.agent.id} column`}
            title="Resize adjacent columns"
            onPointerDown={(event) => handler()(event)}
          />
        )}
      </Show>
    </section>
  )
}
