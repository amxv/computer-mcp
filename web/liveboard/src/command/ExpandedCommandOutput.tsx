import {
  Show,
  createEffect,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js'

import type { CommandOutputState } from '../output/CommandOutputState'

export function ExpandedCommandOutput(props: {
  state: CommandOutputState
  final: boolean
}) {
  const [revision, setRevision] = createSignal(0)
  let outputElement: HTMLPreElement | undefined
  let frame: number | undefined

  const flush = () => {
    frame = undefined
    setRevision((value) => value + 1)
  }
  const scheduleFlush = () => {
    if (frame === undefined) frame = requestAnimationFrame(flush)
  }

  onMount(() => {
    const unsubscribe = props.state.subscribe(scheduleFlush)
    scheduleFlush()
    props.state.requestRecentTail()
    onCleanup(unsubscribe)
  })
  onCleanup(() => {
    if (frame !== undefined) cancelAnimationFrame(frame)
  })

  createEffect(() => {
    if (props.final) props.state.markFinal()
  })

  createEffect(() => {
    revision()
    if (outputElement) outputElement.textContent = props.state.materialize()
  })

  const hasDroppedPrefix = () => {
    revision()
    return props.state.hasDroppedPrefix()
  }
  const displayUnavailable = () => {
    revision()
    return props.state.isDisplayUnavailable()
  }
  const displayUnavailableReason = () => {
    revision()
    return props.state.displayUnavailableReason()
  }
  const recoveryError = () => {
    revision()
    return props.state.recoveryErrorMessage()
  }

  return (
    <div class="command-output-region">
      <Show when={hasDroppedPrefix()}>
        <div class="output-notice">Recent output tail · earlier buffered output dropped</div>
      </Show>
      <Show when={displayUnavailable()}>
        <div class="output-notice output-notice-warning">
          Display-safe output unavailable
          {displayUnavailableReason()
            ? ` · ${displayUnavailableReason()}`
            : ''}
        </div>
      </Show>
      <Show when={recoveryError()}>
        {(message) => <div class="output-notice output-notice-warning">{message()}</div>}
      </Show>
      <pre
        ref={outputElement}
        class="command-output-text"
        aria-label="Command output"
      />
    </div>
  )
}
