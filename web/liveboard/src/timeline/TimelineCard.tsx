import { For, Match, Show, Switch } from 'solid-js'

import type { PresentationRecord } from '../api/client'
import { CommandCard } from '../command/CommandCard'
import type { AgentStreamController } from '../streams/AgentStreamController'

export function TimelineCard(props: {
  record: PresentationRecord
  controller: AgentStreamController
  runtimeId: string
  nowMs: number
}) {
  if (props.record.kind === 'command') {
    return (
      <CommandCard
        record={props.record}
        controller={props.controller}
        runtimeId={props.runtimeId}
        nowMs={props.nowMs}
      />
    )
  }
  return (
    <article
      class="timeline-card"
      classList={{ 'timeline-card-degraded': props.record.evidence.degraded }}
      data-presentation-id={props.record.presentation_id}
    >
      <Switch>
        <Match when={props.record.kind === 'file_changes'}>
          {(() => {
            const record = props.record.kind === 'file_changes' ? props.record : undefined
            if (!record) return null
            return (
              <>
                <div class="timeline-card-heading">
                  <span class="card-kind">Files</span>
                  <span>{record.changes.length} change{record.changes.length === 1 ? '' : 's'}</span>
                </div>
                <div class="file-change-preview">
                  <For each={record.changes.slice(0, 3)}>
                    {(change) => (
                      <span>
                        <strong>{change.operation}</strong> {change.path}
                        <em>+{change.added} −{change.removed}</em>
                      </span>
                    )}
                  </For>
                </div>
              </>
            )
          })()}
        </Match>
        <Match when={props.record.kind === 'stdin'}>
          {(() => {
            const record = props.record.kind === 'stdin' ? props.record : undefined
            if (!record) return null
            return (
              <>
                <div class="timeline-card-heading">
                  <span class="card-kind">stdin</span>
                  <code class="interaction-preview">{record.chars || '(empty input)'}</code>
                </div>
                <div class="timeline-card-meta">
                  <span>{record.result_status ?? 'sent'}</span>
                  <Show when={record.cross_agent && record.creator_agent_id}>
                    <span>targets Agent {record.creator_agent_id}</span>
                  </Show>
                </div>
              </>
            )
          })()}
        </Match>
        <Match when={props.record.kind === 'kill'}>
          {(() => {
            const record = props.record.kind === 'kill' ? props.record : undefined
            if (!record) return null
            return (
              <>
                <div class="timeline-card-heading">
                  <span class="card-kind card-kind-danger">kill</span>
                  <span>Process termination requested</span>
                </div>
                <div class="timeline-card-meta">
                  <span>{record.result_status ?? 'requested'}</span>
                  <Show when={record.cross_agent && record.creator_agent_id}>
                    <span>targets Agent {record.creator_agent_id}</span>
                  </Show>
                </div>
              </>
            )
          })()}
        </Match>
        <Match when={props.record.kind === 'poll_aggregate'}>
          {(() => {
            const record = props.record.kind === 'poll_aggregate' ? props.record : undefined
            if (!record) return null
            return (
              <div class="timeline-card-heading">
                <span class="card-kind">polls</span>
                <span>{record.count} retained orphan poll{record.count === 1 ? '' : 's'}</span>
              </div>
            )
          })()}
        </Match>
        <Match when={props.record.kind === 'generic'}>
          {(() => {
            const record = props.record.kind === 'generic' ? props.record : undefined
            if (!record) return null
            return (
              <>
                <div class="timeline-card-heading">
                  <span class="card-kind">{record.tool_name}</span>
                  <span>{record.status}</span>
                </div>
                <Show when={record.summary}>
                  <p class="generic-summary">{record.summary}</p>
                </Show>
              </>
            )
          })()}
        </Match>
      </Switch>
      <Show when={props.record.evidence.degraded}>
        <p class="card-evidence-warning">
          Evidence incomplete{props.record.evidence.reason ? ` · ${props.record.evidence.reason}` : ''}
        </p>
      </Show>
    </article>
  )
}
