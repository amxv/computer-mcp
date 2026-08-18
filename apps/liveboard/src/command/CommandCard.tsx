import { Show, createSignal, onCleanup } from 'solid-js'

import type { PresentationRecord } from '../api/client'
import type { AgentStreamController } from '../streams/AgentStreamController'
import { CheckIcon, CopyIcon, TerminalIcon } from '../ui/icons'
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
  showRawButton?: boolean
}) {
  const expanded = props.controller.commandExpanded(props.record.presentation_id)
  const streamedOutputAvailability = props.controller.commandOutputAvailability(
    props.record.presentation_id,
  )
  const [auditOpen, setAuditOpen] = createSignal(false)
  const [commandCopied, setCommandCopied] = createSignal(false)
  let commandCopyFeedbackTimeout: ReturnType<typeof setTimeout> | undefined
  const outcome = () => commandOutcome(props.record)
  const durationMs = () =>
    props.record.duration_ms ??
    (props.record.status === 'running'
      ? Math.max(0, props.nowMs - props.record.started_at_ms)
      : 0)
  const final = () => props.record.status !== 'running'
  const showCompactOutcome = () =>
    outcome().compact !== 'Completed' &&
    outcome().compact !== 'Failed' &&
    outcome().compact !== 'Running'
  const hasOutput = () => {
    if ((props.record.output?.length ?? 0) > 0) return true
    const streamed = streamedOutputAvailability()
    if (streamed !== undefined) return streamed
    return props.record.started_at_ms < props.controller.attachWatermarkMs
  }

  onCleanup(() => {
    if (commandCopyFeedbackTimeout !== undefined) clearTimeout(commandCopyFeedbackTimeout)
  })

  const copyCommand = async () => {
    if (!navigator.clipboard) return
    await navigator.clipboard.writeText(props.record.command)
    setCommandCopied(true)
    if (commandCopyFeedbackTimeout !== undefined) clearTimeout(commandCopyFeedbackTimeout)
    commandCopyFeedbackTimeout = setTimeout(() => {
      setCommandCopied(false)
      commandCopyFeedbackTimeout = undefined
    }, 2_000)
  }

  const toggleOutput = () => {
    if (!hasOutput()) return
    props.controller.toggleCommandExpansion(props.record.presentation_id)
  }

  return (
    <article
      class="command-card"
      classList={{ 'timeline-card-degraded': props.record.evidence.degraded }}
      data-presentation-id={props.record.presentation_id}
    >
      <div
        class="command-card-header"
        classList={{ 'command-card-header-expandable': hasOutput() }}
        onClick={toggleOutput}
      >
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
        <span class="command-terminal-icon" aria-hidden="true">
          <TerminalIcon />
        </span>
        <code class="command-line">
          <span class="command-prompt">$ </span>
          {props.record.command}
        </code>
        <div class="command-header-actions">
          <Show when={props.showRawButton}>
            <button
              type="button"
              class="command-audit-button"
              aria-expanded={auditOpen()}
              onClick={(event) => {
                event.stopPropagation()
                setAuditOpen((value) => !value)
              }}
            >
              {auditOpen() ? 'Hide Raw' : 'Raw'}
            </button>
          </Show>
          <button
            type="button"
            class="command-copy-button"
            aria-label={commandCopied() ? 'Command copied' : 'Copy command'}
            title={commandCopied() ? 'Copied' : 'Copy command'}
            onClick={(event) => {
              event.stopPropagation()
              void copyCommand()
            }}
          >
            {commandCopied() ? <CheckIcon /> : <CopyIcon />}
          </button>
        </div>
      </div>
      <div class="command-meta">
        <Show when={showCompactOutcome()}>
          <span>{outcome().compact}</span>
        </Show>
        <span>duration {formatDuration(durationMs())}</span>
        <Show when={props.record.exit_code !== null && props.record.exit_code !== 0 && final()}>
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
      <Show when={hasOutput() && expanded()}>
        <ExpandedCommandOutput
          state={props.controller.outputState(
            props.record.presentation_id,
            props.record.primary_invocation_id,
          )}
          final={final()}
        />
      </Show>
      <Show when={props.showRawButton && auditOpen()}>
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
