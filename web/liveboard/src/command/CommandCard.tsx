import { Show, createSignal } from 'solid-js'

import type { PresentationRecord } from '../api/client'
import type { AgentStreamController } from '../streams/AgentStreamController'
import { CommandAudit } from './CommandAudit'
import { ExpandedCommandOutput } from './ExpandedCommandOutput'

type CommandRecord = Extract<PresentationRecord, { kind: 'command' }>

function formatDuration(milliseconds: number) {
  const seconds = Math.max(0, milliseconds) / 1_000
  if (seconds < 60) return `${Math.floor(seconds)}s`
  const minutes = Math.floor(seconds / 60)
  const remainingSeconds = Math.floor(seconds % 60)
  if (minutes < 60) return `${minutes}m ${remainingSeconds}s`
  const hours = Math.floor(minutes / 60)
  return `${hours}h ${minutes % 60}m`
}

function commandOutcome(record: CommandRecord) {
  if (record.status === 'running') {
    return { className: 'running', label: 'Command running', compact: 'Running' }
  }
  if (record.status === 'incomplete') {
    return {
      className: 'incomplete',
      label: 'Command final state is incomplete',
      compact: 'Incomplete',
    }
  }
  if (
    record.exit_code === 0 &&
    record.termination_reason !== 'killed' &&
    record.termination_reason !== 'timeout'
  ) {
    return {
      className: 'success',
      label: 'Command exited successfully',
      compact: 'Completed',
    }
  }
  const compact =
    record.termination_reason === 'timeout'
      ? 'Timed out'
      : record.termination_reason === 'killed'
        ? 'Killed'
        : 'Failed'
  return {
    className: 'error',
    label: `${compact}${record.exit_code === null ? '' : ` with exit ${record.exit_code}`}`,
    compact,
  }
}

export function CommandCard(props: {
  record: CommandRecord
  controller: AgentStreamController
  runtimeId: string
  nowMs: number
}) {
  const expanded = props.controller.commandExpanded(props.record.presentation_id)
  const [auditOpen, setAuditOpen] = createSignal(false)
  const outcome = () => commandOutcome(props.record)
  const durationMs = () =>
    props.record.duration_ms ??
    (props.record.status === 'running'
      ? Math.max(0, props.nowMs - props.record.started_at_ms)
      : 0)
  const final = () => props.record.status !== 'running'

  return (
    <article
      class="command-card"
      classList={{ 'timeline-card-degraded': props.record.evidence.degraded }}
      data-presentation-id={props.record.presentation_id}
    >
      <div class="command-card-header">
        <span
          class={`command-status command-status-${outcome().className}`}
          aria-label={outcome().label}
          title={outcome().label}
        >
          <Show
            when={props.record.status === 'running'}
            fallback={<span class="command-status-dot" aria-hidden="true" />}
          >
            <span class="command-spinner" aria-hidden="true" />
          </Show>
        </span>
        <code class="command-line">
          <span class="command-prompt">$ </span>
          {props.record.command}
        </code>
        <button
          type="button"
          class="command-audit-button"
          aria-expanded={auditOpen()}
          onClick={() => setAuditOpen((value) => !value)}
        >
          Audit
        </button>
        <button
          type="button"
          class="command-chevron"
          aria-label={expanded() ? 'Collapse command output' : 'Expand command output'}
          aria-expanded={expanded()}
          onClick={() => props.controller.toggleCommandExpansion(props.record.presentation_id)}
        >
          <span aria-hidden="true">⌄</span>
        </button>
      </div>
      <div class="command-meta">
        <span>{outcome().compact}</span>
        <span>{formatDuration(durationMs())}</span>
        <Show when={props.record.exit_code !== null && final()}>
          <span>exit {props.record.exit_code}</span>
        </Show>
        <Show when={props.record.polls && props.record.polls.count > 0}>
          <span>Polled {props.record.polls?.count}×</span>
        </Show>
        <Show when={props.record.effective_cwd}>
          <span class="command-cwd" title={props.record.effective_cwd ?? undefined}>
            {props.record.effective_cwd}
          </span>
        </Show>
      </div>
      <Show when={expanded()}>
        <ExpandedCommandOutput
          state={props.controller.outputState(
            props.record.presentation_id,
            props.record.primary_invocation_id,
          )}
          final={final()}
        />
      </Show>
      <Show when={auditOpen()}>
        <CommandAudit
          presentationId={props.record.presentation_id}
          runtimeId={props.runtimeId}
        />
      </Show>
      <Show when={props.record.evidence.degraded}>
        <p class="card-evidence-warning">
          Evidence incomplete
          {props.record.evidence.reason ? ` · ${props.record.evidence.reason}` : ''}
        </p>
      </Show>
    </article>
  )
}
