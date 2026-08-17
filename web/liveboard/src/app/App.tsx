import {
  Match,
  Switch,
  createEffect,
  createResource,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js'

import {
  loadBootstrap,
  patchPreferences,
  type LiveboardPreferencesPatch,
} from '../api/client'
import { Board } from '../board/Board'
import { initialVisibleAgentIds } from '../board/model'
import { createRuntimeConnection } from '../streams/runtime'
import { AgentTimeline } from '../timeline/AgentTimeline'
import { applyTheme } from './bootstrap'
import { createCoarseClock } from './clock'

type Bootstrap = Awaited<ReturnType<typeof loadBootstrap>>

function LiveboardWorkspace(props: {
  bootstrap: Bootstrap
  viewerAttachWatermarkMs: number
}) {
  const [preferences, setPreferences] = createSignal(props.bootstrap.preferences)
  const [pendingSaves, setPendingSaves] = createSignal(0)
  const [saveError, setSaveError] = createSignal<string>()
  const now = createCoarseClock()
  const runtime = createRuntimeConnection({
    initialStatus: props.bootstrap.status,
    initialAgents: props.bootstrap.agents.agents,
    initialVisibleAgentIds: initialVisibleAgentIds(
      props.bootstrap.agents.agents,
      props.bootstrap.preferences,
    ),
    viewerAttachWatermarkMs: props.viewerAttachWatermarkMs,
  })
  let mutationQueue: Promise<void> = Promise.resolve()

  createEffect(() => applyTheme(preferences().theme))
  onMount(() => runtime.start())
  onCleanup(() => runtime.dispose())

  const persistPreferences = (patch: LiveboardPreferencesPatch) => {
    setPendingSaves((count) => count + 1)
    setSaveError(undefined)
    mutationQueue = mutationQueue
      .then(async () => {
        const updated = await patchPreferences(patch)
        setPreferences(updated)
      })
      .catch((error: unknown) => {
        setSaveError(error instanceof Error ? error.message : String(error))
      })
      .finally(() => setPendingSaves((count) => Math.max(0, count - 1)))
  }

  return (
    <div class="app-shell">
      <Board
        agents={runtime.agents()}
        preferences={preferences()}
        nowMs={now()}
        saving={pendingSaves() > 0}
        error={saveError()}
        connectionState={runtime.connectionState()}
        connectionError={runtime.connectionError()}
        onPatch={persistPreferences}
        onVisibleAgentsChange={runtime.setVisibleAgentIds}
        renderTimeline={(agentId) => (
          <AgentTimeline controller={runtime.controllerFor(agentId)} />
        )}
      />
    </div>
  )
}

export function App() {
  const viewerAttachWatermarkMs = Date.now()
  const [bootstrap] = createResource(loadBootstrap)

  return (
    <Switch>
      <Match when={bootstrap.loading}>
        <main class="bootstrap-surface">
          <p>Connecting to Zodex Local…</p>
        </main>
      </Match>
      <Match when={bootstrap.error}>
        <main class="bootstrap-surface">
          <div role="status" class="connection-error">
            <strong>Local observer disconnected</strong>
            <span>{String(bootstrap.error)}</span>
          </div>
        </main>
      </Match>
      <Match when={bootstrap()}>
        {(value) => (
          <LiveboardWorkspace
            bootstrap={value()}
            viewerAttachWatermarkMs={viewerAttachWatermarkMs}
          />
        )}
      </Match>
    </Switch>
  )
}
