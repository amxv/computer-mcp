import { Match, Show, Switch, createEffect, createResource } from 'solid-js'

import { loadBootstrap } from '../api/client'
import { applyTheme, connectionSummary } from './bootstrap'

export function App() {
  const [bootstrap] = createResource(loadBootstrap)
  createEffect(() => {
    const value = bootstrap()
    if (value) {
      applyTheme(value.preferences.theme)
    }
  })

  return (
    <div class="app-shell">
      <header class="toolbar">
        <div>
          <strong class="brand">Zodex Liveboard</strong>
          <Show when={bootstrap()}>
            {(value) => (
              <span class="connection-detail">
                {connectionSummary(
                  value().status.runtime_id,
                  value().status.current_runtime_agent_count,
                  value().status.active_process_count,
                )}
              </span>
            )}
          </Show>
        </div>
        <span class="phase-label">Local observer</span>
      </header>
      <main class="bootstrap-surface">
        <Switch>
          <Match when={bootstrap.loading}>
            <p>Connecting to Zodex Local…</p>
          </Match>
          <Match when={bootstrap.error}>
            <div role="status" class="connection-error">
              <strong>Local observer disconnected</strong>
              <span>{String(bootstrap.error)}</span>
            </div>
          </Match>
          <Match when={bootstrap()}>
            {(value) => (
              <section aria-label="Liveboard bootstrap" class="empty-board">
                <p>
                  {value().agents.agents.length === 0
                    ? 'Waiting for the first Agent activity in this Local runtime.'
                    : `${value().agents.agents.length} current-runtime Agent${value().agents.agents.length === 1 ? '' : 's'} ready for the board.`}
                </p>
              </section>
            )}
          </Match>
        </Switch>
      </main>
    </div>
  )
}
