import { For, Show, createEffect } from 'solid-js'

import type { ApiAgent, LiveboardPreferences } from '../api/client'
import { AgentsIcon, CloseIcon } from '../ui/icons'
import {
  agentActivity,
  compactWorkdir,
  mostRecentWorkdir,
  relativeActivityLabel,
} from './model'

interface AgentDrawerProps {
  open: boolean
  agents: readonly ApiAgent[]
  visibleIds: readonly string[]
  preferences: LiveboardPreferences
  nowMs: number
  onClose: () => void
  onAdd: (agentId: string) => void
}

export function AgentDrawer(props: AgentDrawerProps) {
  let closeButton: HTMLButtonElement | undefined
  createEffect(() => {
    if (props.open) queueMicrotask(() => closeButton?.focus())
  })

  const isFull = () => props.visibleIds.length >= props.preferences.max_visible_agents

  return (
    <Show when={props.open}>
      <div
        class="drawer-backdrop"
        onPointerDown={(event) => {
          if (event.target === event.currentTarget) props.onClose()
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') props.onClose()
        }}
      >
        <aside class="agent-drawer" role="dialog" aria-modal="true" aria-labelledby="drawer-title">
          <header class="drawer-header">
            <div class="drawer-title-row">
              <AgentsIcon />
              <div>
                <h2 id="drawer-title">All Agents</h2>
                <p>Current Local runtime</p>
              </div>
            </div>
            <button
              ref={closeButton}
              type="button"
              class="icon-button"
              aria-label="Close All Agents"
              onClick={props.onClose}
            >
              <CloseIcon />
            </button>
          </header>
          <div class="drawer-list">
            <Show
              when={props.agents.length > 0}
              fallback={<p class="drawer-empty">No Agents have appeared in this runtime yet.</p>}
            >
              <For each={props.agents}>
                {(agent) => {
                  const onBoard = () => props.visibleIds.includes(agent.id)
                  const preference = () => props.preferences.agents[agent.id]
                  const workdir = () => mostRecentWorkdir(agent)
                  const activity = () => agentActivity(agent, props.nowMs)
                  const addDisabled = () => onBoard() || isFull()
                  return (
                    <button
                      type="button"
                      class="drawer-agent"
                      classList={{ 'drawer-agent-selected': onBoard() }}
                      aria-pressed={onBoard()}
                      disabled={addDisabled()}
                      title={
                        onBoard()
                          ? `${agent.id} is already visible`
                          : isFull()
                            ? 'Board is at its configured maximum'
                            : `Add Agent ${agent.id} to board`
                      }
                      onClick={() => props.onAdd(agent.id)}
                    >
                      <div class="drawer-agent-main">
                        <div class="drawer-agent-identity">
                          <Show when={preference()?.alias}>
                            <strong>{preference()?.alias}</strong>
                          </Show>
                          <code>{agent.id}</code>
                        </div>
                        <span class={`activity-dot activity-${activity()}`} aria-hidden="true" />
                      </div>
                      <div class="drawer-agent-meta">
                        <span>
                          {agent.active_process_count > 0
                            ? `${agent.active_process_count} active`
                            : relativeActivityLabel(agent.last_seen_at_ms, props.nowMs)}
                        </span>
                        <Show when={workdir()}>
                          {(value) => (
                            <span title={value().normalized_workdir}>
                              {compactWorkdir(value().normalized_workdir)}
                            </span>
                          )}
                        </Show>
                      </div>
                    </button>
                  )
                }}
              </For>
            </Show>
          </div>
        </aside>
      </div>
    </Show>
  )
}
