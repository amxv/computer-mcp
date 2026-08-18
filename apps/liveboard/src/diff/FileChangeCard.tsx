import {
  For,
  Show,
  createEffect,
  createSignal,
  onCleanup,
} from 'solid-js'

import type {
  PresentationDiffLine,
  PresentationFileChange,
  PresentationRecord,
} from '../api/client'
import { openFileInEditor } from '../api/client'
import type { AgentStreamController } from '../streams/AgentStreamController'
import { CheckIcon, CopyIcon } from '../ui/icons'
import type { DiffHighlighter } from './HighlightWorkerClient'
import { resolveDiffSyntaxLanguage } from './language'

type FileChangesRecord = Extract<PresentationRecord, { kind: 'file_changes' }>

function operationLabel(operation: PresentationFileChange['operation']) {
  switch (operation) {
    case 'created':
      return 'Added'
    case 'deleted':
      return 'Deleted'
    case 'renamed':
      return 'Renamed'
    case 'edited':
    default:
      return 'Edited'
  }
}

function shortPath(path: string) {
  return path.split('/').filter(Boolean).pop() ?? path
}

function diffCopyPrefix(kind: string) {
  if (kind === 'add') return '+'
  if (kind === 'remove') return '-'
  return ' '
}

function copyText(change: PresentationFileChange) {
  return change.lines.map((line) => `${diffCopyPrefix(line.kind)}${line.text}`).join('\n')
}

function hashPart(hash: number, value: string) {
  let next = hash >>> 0
  for (let index = 0; index < value.length; index += 1) {
    next ^= value.charCodeAt(index)
    next = Math.imul(next, 16_777_619) >>> 0
  }
  return next
}

export function fileChangeRevision(change: PresentationFileChange) {
  let hash = 2_166_136_261
  hash = hashPart(hash, change.operation)
  hash = hashPart(hash, change.path)
  hash = hashPart(hash, change.old_path ?? '')
  hash = hashPart(hash, change.write_mode ?? '')
  hash = hashPart(hash, String(change.added))
  hash = hashPart(hash, String(change.removed))
  hash = hashPart(hash, change.diff_truncated ? '1' : '0')
  for (const line of change.lines) {
    hash = hashPart(hash, line.kind)
    hash = hashPart(hash, String(line.old_line ?? ''))
    hash = hashPart(hash, String(line.new_line ?? ''))
    hash = hashPart(hash, line.text)
  }
  return `${change.lines.length}-${hash.toString(16).padStart(8, '0')}`
}

function DiffSource(props: { line: PresentationDiffLine; html?: string | null }) {
  return (
    <code class="diff-source-code">
      <Show
        when={props.html}
        fallback={<span class="diff-source-plain">{props.line.text || ' '}</span>}
      >
        {(html) => <span class="diff-source-highlight" innerHTML={html()} />}
      </Show>
    </code>
  )
}

function DiffBody(props: {
  change: PresentationFileChange
  subjectKey: string
  highlighter: DiffHighlighter
}) {
  const [highlighted, setHighlighted] = createSignal<ReadonlyMap<number, string | null>>(
    new Map(),
  )
  let generation = 0

  createEffect(() => {
    const change = props.change
    const language = resolveDiffSyntaxLanguage(change.path)
    const revision = fileChangeRevision(change)
    const currentGeneration = ++generation
    setHighlighted(new Map())
    if (!language || change.lines.length === 0) return

    void props.highlighter
      .highlight({
        subjectKey: props.subjectKey,
        revision,
        language,
        rows: change.lines.map((line, index) => ({ index, text: line.text })),
      })
      .then((result) => {
        if (
          currentGeneration !== generation ||
          result.subjectKey !== props.subjectKey ||
          result.revision !== revision
        ) {
          return
        }
        setHighlighted(new Map(result.rows.map((row) => [row.index, row.html])))
      })
      .catch(() => {
        // Highlight enrichment is optional. Canonical plain source remains the
        // immediate and correct fallback for worker/language failures.
      })
  })

  onCleanup(() => {
    generation += 1
  })

  return (
    <div class="diff-body" aria-label={`Unified diff for ${props.change.path}`}>
      <For each={props.change.lines}>
        {(line, index) => (
          <div
            class="diff-row"
            classList={{
              'diff-row-add': line.kind === 'add',
              'diff-row-remove': line.kind === 'remove',
              'diff-row-context': line.kind === 'context',
            }}
            data-diff-row-kind={line.kind}
          >
            <span class="diff-gutter diff-gutter-old" aria-label="Old line">
              {line.old_line ?? ''}
            </span>
            <span class="diff-gutter diff-gutter-new" aria-label="New line">
              {line.new_line ?? ''}
            </span>
            <DiffSource line={line} html={highlighted().get(index())} />
          </div>
        )}
      </For>
      <Show when={props.change.diff_truncated}>
        <div class="diff-warning" role="status">
          Diff preview truncated at the server display bound.
        </div>
      </Show>
    </div>
  )
}

function FileChangeItem(props: {
  record: FileChangesRecord
  change: PresentationFileChange
  index: number
  controller: AgentStreamController
  highlighter: DiffHighlighter
}) {
  const diffKey = () => `${props.record.presentation_id}:file-change:${props.index}`
  const expanded = props.controller.diffExpanded(diffKey())
  const [copied, setCopied] = createSignal(false)
  const [openError, setOpenError] = createSignal<string>()
  let copyFeedbackTimeout: ReturnType<typeof setTimeout> | undefined

  onCleanup(() => {
    if (copyFeedbackTimeout !== undefined) clearTimeout(copyFeedbackTimeout)
  })

  const copy = async () => {
    const clipboard = navigator.clipboard
    if (!clipboard) return
    await clipboard.writeText(copyText(props.change))
    setCopied(true)
    if (copyFeedbackTimeout !== undefined) clearTimeout(copyFeedbackTimeout)
    copyFeedbackTimeout = setTimeout(() => {
      setCopied(false)
      copyFeedbackTimeout = undefined
    }, 2_000)
  }
  const canExpand = () =>
    !props.change.diff_lines_included ||
    props.change.lines.length > 0 ||
    props.change.diff_truncated
  const canOpenFile = () => props.change.operation !== 'deleted'
  const openFile = async () => {
    setOpenError(undefined)
    try {
      await openFileInEditor(props.change.path)
    } catch (error) {
      setOpenError(error instanceof Error ? error.message : String(error))
    }
  }
  const toggleExpanded = () => {
    if (!canExpand()) return
    props.controller.toggleDiffExpansion(diffKey())
  }

  createEffect(() => {
    if (expanded() && !props.change.diff_lines_included) {
      props.controller.requestFullPresentation(props.record.presentation_id)
    }
  })

  return (
    <section
      class="file-change-card"
      classList={{ 'file-change-card-degraded': props.record.evidence.degraded }}
      data-diff-key={diffKey()}
    >
      <div
        class="file-change-header"
        classList={{ 'file-change-header-expandable': canExpand() }}
        onClick={(event) => {
          if ((event.target as HTMLElement).closest('button')) return
          toggleExpanded()
        }}
      >
        <span class="diff-operation">{operationLabel(props.change.operation)}</span>
        <div class="diff-path" title={props.change.old_path ? `${props.change.old_path} → ${props.change.path}` : props.change.path}>
          <Show when={props.change.old_path}>
            {(oldPath) => (
              <>
                <span class="diff-old-path">{shortPath(oldPath())}</span>
                <span class="diff-path-arrow" aria-hidden="true">→</span>
              </>
            )}
          </Show>
          <Show
            when={canOpenFile()}
            fallback={<span class="diff-current-path">{shortPath(props.change.path)}</span>}
          >
            <button
              type="button"
              class="diff-current-path diff-current-path-button"
              aria-label={`Open ${props.change.path} in configured editor`}
              title={openError() ?? `Open ${props.change.path} in configured editor`}
              onClick={() => void openFile()}
            >
              {shortPath(props.change.path)}
            </button>
          </Show>
        </div>
        <div
          class="diff-counts"
          aria-label={`${props.change.added} lines added, ${props.change.removed} lines removed`}
        >
          <span class="diff-count-added">+{props.change.added}</span>
          <span class="diff-count-removed">-{props.change.removed}</span>
        </div>
        <button
          type="button"
          class="diff-copy-button"
          disabled={!props.change.diff_lines_included || props.change.lines.length === 0}
          aria-label={copied() ? `Diff copied for ${props.change.path}` : `Copy diff for ${props.change.path}`}
          title={copied() ? 'Copied' : 'Copy diff'}
          onClick={() => void copy()}
        >
          {copied() ? <CheckIcon /> : <CopyIcon />}
        </button>
      </div>
      <Show when={openError()}>
        {(message) => <div class="diff-open-error">{message()}</div>}
      </Show>
      <Show when={props.change.write_mode}>
        {(mode) => <div class="diff-write-mode">{mode()}</div>}
      </Show>
      <Show when={expanded()}>
        <Show
          when={props.change.diff_lines_included}
          fallback={<div class="diff-loading">Loading diff…</div>}
        >
          <DiffBody
            change={props.change}
            subjectKey={diffKey()}
            highlighter={props.highlighter}
          />
        </Show>
      </Show>
    </section>
  )
}

export function FileChangesCard(props: {
  record: FileChangesRecord
  controller: AgentStreamController
  highlighter: DiffHighlighter
}) {
  return (
    <article
      class="file-change-group"
      data-presentation-id={props.record.presentation_id}
      data-file-change-count={props.record.changes.length}
    >
      <For each={props.record.changes}>
        {(change, index) => (
          <FileChangeItem
            record={props.record}
            change={change}
            index={index()}
            controller={props.controller}
            highlighter={props.highlighter}
          />
        )}
      </For>
      <Show when={props.record.evidence.degraded}>
        <p class="card-evidence-warning file-change-evidence-warning">
          Evidence incomplete
          {props.record.evidence.reason ? ` · ${props.record.evidence.reason}` : ''}
        </p>
      </Show>
    </article>
  )
}
