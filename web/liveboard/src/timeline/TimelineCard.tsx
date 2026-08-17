import { For, Match, Show, Switch } from 'solid-js'

import type { PresentationRecord } from '../api/client'

export function TimelineCard(props: { record: PresentationRecord }) {
  return (
    <article
      class="timeline-card"
      classList={{ 'timeline-card-degraded': props.record.evidence.degraded }}
      data-presentation-id={props.record.presentation_id}
    >
      <Switch>
        <Match when={props.record.kind === 'command'}>
          {(() => {
            const record = props.record.kind === 'command' ? props.record : undefined
            if (!record) return null
            return (
              <>
                <div class="timeline-card-heading">
                  <span class={`card-status card-status-${record.status}`} aria-hidden="true" />
                  <code class="command-preview">$ {record.command}</code>
                </div>
                <div class="timeline-card-meta">
                  <span>{record.status}</span>
                  <Show when={record.duration_ms !== null}>
                    <span>{Math.max(0, Math.round((record.duration_ms ?? 0) / 1000))}s</span>
                  </Show>
                  <Show when={record.polls && record.polls.count > 0}>
                    <span>Polled {record.polls?.count}×</span>
                  </Show>
                </div>
              </>
            )
          })()}
        </Match>
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
              <div class="timeline-card-heading">
                <span class="card-kind">stdin</span>
                <code class="interaction-preview">{record.chars || '(empty input)'}</code>
              </div>
            )
          })()}
        </Match>
        <Match when={props.record.kind === 'kill'}>
          <div class="timeline-card-heading">
            <span class="card-kind card-kind-danger">kill</span>
            <span>Process termination requested</span>
          </div>
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
