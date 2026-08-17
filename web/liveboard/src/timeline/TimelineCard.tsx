import { Match, Show, Switch } from 'solid-js'

import type { PresentationRecord } from '../api/client'
import { CommandCard } from '../command/CommandCard'
import { FileChangesCard } from '../diff/FileChangeCard'
import {
  PLAIN_DIFF_HIGHLIGHTER,
  type DiffHighlighter,
} from '../diff/HighlightWorkerClient'
import type { AgentStreamController } from '../streams/AgentStreamController'

export function TimelineCard(props: {
  record: PresentationRecord
  controller: AgentStreamController
  runtimeId: string
  nowMs: number
  showRawButton?: boolean
  diffHighlighter?: DiffHighlighter
}) {
  return (
    <Switch fallback={null}>
      <Match when={props.record.kind === 'command' ? props.record : undefined}>
        {(record) => (
          <CommandCard
            record={record()}
            controller={props.controller}
            runtimeId={props.runtimeId}
            nowMs={props.nowMs}
            showRawButton={props.showRawButton ?? false}
          />
        )}
      </Match>
      <Match when={props.record.kind === 'file_changes' ? props.record : undefined}>
        {(record) => (
          <FileChangesCard
            record={record()}
            controller={props.controller}
            highlighter={props.diffHighlighter ?? PLAIN_DIFF_HIGHLIGHTER}
          />
        )}
      </Match>
      <Match when={props.record.kind === 'stdin' ? props.record : undefined}>
        {(record) => (
          <article
            class="timeline-card"
            classList={{ 'timeline-card-degraded': record().evidence.degraded }}
            data-presentation-id={record().presentation_id}
          >
            <div class="timeline-card-heading">
              <span class="card-kind">stdin</span>
              <code class="interaction-preview">{record().chars || '(empty input)'}</code>
            </div>
            <div class="timeline-card-meta">
              <span>{record().result_status ?? 'sent'}</span>
              <Show when={record().cross_agent && record().creator_agent_id}>
                <span>targets Agent {record().creator_agent_id}</span>
              </Show>
            </div>
            <EvidenceWarning record={record()} />
          </article>
        )}
      </Match>
      <Match when={props.record.kind === 'kill' ? props.record : undefined}>
        {(record) => (
          <article
            class="timeline-card"
            classList={{ 'timeline-card-degraded': record().evidence.degraded }}
            data-presentation-id={record().presentation_id}
          >
            <div class="timeline-card-heading">
              <span class="card-kind card-kind-danger">kill</span>
              <span>Process termination requested</span>
            </div>
            <div class="timeline-card-meta">
              <span>{record().result_status ?? 'requested'}</span>
              <Show when={record().cross_agent && record().creator_agent_id}>
                <span>targets Agent {record().creator_agent_id}</span>
              </Show>
            </div>
            <EvidenceWarning record={record()} />
          </article>
        )}
      </Match>
      <Match when={props.record.kind === 'poll_aggregate' ? props.record : undefined}>
        {(record) => (
          <article
            class="timeline-card"
            classList={{ 'timeline-card-degraded': record().evidence.degraded }}
            data-presentation-id={record().presentation_id}
          >
            <div class="timeline-card-heading">
              <span class="card-kind">polls</span>
              <span>
                {record().count} retained orphan poll{record().count === 1 ? '' : 's'}
              </span>
            </div>
            <EvidenceWarning record={record()} />
          </article>
        )}
      </Match>
      <Match when={props.record.kind === 'generic' ? props.record : undefined}>
        {(record) => (
          <article
            class="timeline-card"
            classList={{ 'timeline-card-degraded': record().evidence.degraded }}
            data-presentation-id={record().presentation_id}
          >
            <div class="timeline-card-heading">
              <span class={`card-status card-status-${record().status}`} aria-hidden="true" />
              <span class="card-kind">{record().tool_name}</span>
              <span>{record().status}</span>
            </div>
            <Show when={record().summary}>
              <p class="generic-summary">{record().summary}</p>
            </Show>
            <EvidenceWarning record={record()} />
          </article>
        )}
      </Match>
    </Switch>
  )
}

function EvidenceWarning(props: { record: PresentationRecord }) {
  return (
    <Show when={props.record.evidence.degraded}>
      <p class="card-evidence-warning">
        Evidence incomplete
        {props.record.evidence.reason ? ` · ${props.record.evidence.reason}` : ''}
      </p>
    </Show>
  )
}
