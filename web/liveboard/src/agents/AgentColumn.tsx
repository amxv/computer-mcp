import { Show, createEffect, createSignal, type JSX } from 'solid-js'

import type { ApiAgent } from '../api/client'
import {
  ChevronLeftIcon,
  ChevronRightIcon,
  CheckIcon,
  CloseIcon,
  EditIcon,
  GripIcon,
  HideIcon,
} from '../ui/icons'
import {
  agentActivity,
  compactWorkdir,
  mostRecentWorkdir,
  relativeActivityLabel,
} from './model'

interface AgentColumnProps {
  agent: ApiAgent
  alias?: string
  nowMs: number
  weight: number
  index: number
  count: number
  onHide: () => void
  onAliasSave: (alias: string) => void
  onMove: (direction: -1 | 1) => void
  onReorderPointerDown: (event: PointerEvent) => void
  onResizePointerDown?: (event: PointerEvent) => void
  children?: JSX.Element
}

export function AgentColumn(props: AgentColumnProps) {
  const [editingAlias, setEditingAlias] = createSignal(false)
  const [aliasDraft, setAliasDraft] = createSignal(props.alias ?? '')
  let aliasInput: HTMLInputElement | undefined

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

  const workdir = () => mostRecentWorkdir(props.agent)
  const activity = () => agentActivity(props.agent, props.nowMs)
  const saveAlias = () => {
    props.onAliasSave(aliasDraft().trim())
    setEditingAlias(false)
  }

  return (
    <section
      class="agent-column"
      data-agent-column
      data-agent-id={props.agent.id}
      style={{ '--column-weight': String(props.weight) }}
      aria-label={`Agent ${props.agent.id}${props.alias ? `, ${props.alias}` : ''}`}
    >
      <header class="agent-header">
        <div class="agent-header-topline">
          <button
            type="button"
            class="icon-button drag-handle"
            aria-label={`Drag Agent ${props.agent.id} to reorder`}
            title="Drag to reorder"
            onPointerDown={(event) => props.onReorderPointerDown(event)}
          >
            <GripIcon />
          </button>
          <div class="agent-identity">
            <Show
              when={editingAlias()}
              fallback={
                <>
                  <Show when={props.alias}>
                    <strong class="agent-alias">{props.alias}</strong>
                  </Show>
                  <code class="agent-id">{props.agent.id}</code>
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
            <button
              type="button"
              class="icon-button"
              aria-label={`Move Agent ${props.agent.id} left`}
              title="Move left"
              disabled={props.index === 0}
              onClick={() => props.onMove(-1)}
            >
              <ChevronLeftIcon />
            </button>
            <button
              type="button"
              class="icon-button"
              aria-label={`Move Agent ${props.agent.id} right`}
              title="Move right"
              disabled={props.index === props.count - 1}
              onClick={() => props.onMove(1)}
            >
              <ChevronRightIcon />
            </button>
            <button
              type="button"
              class="icon-button"
              aria-label={`Edit alias for Agent ${props.agent.id}`}
              title="Edit alias"
              onClick={() => setEditingAlias(true)}
            >
              <EditIcon />
            </button>
            <button
              type="button"
              class="icon-button"
              aria-label={`Remove Agent ${props.agent.id} from board`}
              title="Remove from board"
              onClick={props.onHide}
            >
              <HideIcon />
            </button>
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
          <Show when={workdir()}>
            {(value) => (
              <span class="workdir" title={value().normalized_workdir}>
                {compactWorkdir(value().normalized_workdir)}
              </span>
            )}
          </Show>
        </div>
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
      <Show when={props.onResizePointerDown}>
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
