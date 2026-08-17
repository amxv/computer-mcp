import type { LiveboardPreferences, ThemePreference } from '../api/client'
import type { RuntimeConnectionState } from '../streams/runtime'
import { AgentsIcon, MoonIcon, SunIcon, SystemThemeIcon } from './icons'

interface ToolbarProps {
  preferences: LiveboardPreferences
  currentAgentCount: number
  activeProcessCount: number
  saving: boolean
  error?: string
  connectionState?: RuntimeConnectionState
  connectionError?: string
  onOpenAgents: () => void
  onMaximumChange: (maximum: number) => void
  onThemeChange: (theme: ThemePreference) => void
  onCommandExpansionChange: (expanded: boolean) => void
  onDiffExpansionChange: (expanded: boolean) => void
}

export function Toolbar(props: ToolbarProps) {
  const nextTheme = (): ThemePreference => {
    switch (props.preferences.theme) {
      case 'light':
        return 'system'
      case 'system':
        return 'dark'
      case 'dark':
        return 'light'
    }
  }
  const themeLabel = () =>
    props.preferences.theme === 'system'
      ? 'System'
      : props.preferences.theme === 'light'
        ? 'Light'
        : 'Dark'
  const connectionState = () => props.connectionState ?? 'connected'
  const connectionLabel = () => {
    switch (connectionState()) {
      case 'connected':
        return 'Connected'
      case 'connecting':
        return 'Connecting'
      case 'recovering':
        return 'Recovering'
      case 'disconnected':
        return 'Disconnected'
      case 'incompatible':
        return 'Version mismatch'
    }
  }
  return (
    <header class="toolbar">
      <div class="toolbar-brand-group">
        <strong class="brand">zodex</strong>
      </div>
      <div class="toolbar-controls">
        <span class="preference-state" role="status" aria-live="polite">
          {props.error ? (
            <span class="preference-error" title={props.error} aria-label="Save failed">
              !
            </span>
          ) : props.saving ? (
            <span class="preference-spinner" aria-label="Saving preferences" />
          ) : null}
        </span>
        <span
          class="connection-detail"
          aria-label={`Local observer ${connectionLabel().toLowerCase()}`}
          title={props.connectionError}
        >
          <span
            class={`connection-dot connection-${connectionState()}`}
            aria-hidden="true"
          />
          {connectionState() === 'connected'
            ? `${props.currentAgentCount} ${props.currentAgentCount === 1 ? 'Agent' : 'Agents'} · ${props.activeProcessCount} ${props.activeProcessCount === 1 ? 'process' : 'processes'} running`
            : connectionLabel()}
        </span>
        <button type="button" class="toolbar-button" onClick={props.onOpenAgents}>
          <AgentsIcon />
          <span>All Agents</span>
        </button>
        <label class="toolbar-field">
          <span>Columns</span>
          <select
            aria-label="Maximum visible Agents"
            value={props.preferences.max_visible_agents}
            onChange={(event) =>
              props.onMaximumChange(Number(event.currentTarget.value))
            }
          >
            {[1, 2, 3, 4, 5, 6, 7, 8].map((value) => (
              <option value={value}>{value}</option>
            ))}
          </select>
        </label>
        <button
          type="button"
          class="toolbar-button compact-control"
          aria-pressed={props.preferences.command_outputs_expanded}
          onClick={() =>
            props.onCommandExpansionChange(
              !props.preferences.command_outputs_expanded,
            )
          }
          title="Toggle all command outputs"
        >
          Cmd {props.preferences.command_outputs_expanded ? 'open' : 'closed'}
        </button>
        <button
          type="button"
          class="toolbar-button compact-control"
          aria-pressed={props.preferences.diffs_expanded}
          onClick={() => props.onDiffExpansionChange(!props.preferences.diffs_expanded)}
          title="Toggle all diffs"
        >
          Diff {props.preferences.diffs_expanded ? 'open' : 'closed'}
        </button>
        <button
          type="button"
          class="toolbar-button theme-toggle"
          aria-label={`Theme: ${themeLabel()}. Switch to ${nextTheme()}`}
          title={`Theme: ${themeLabel()}`}
          onClick={() => props.onThemeChange(nextTheme())}
        >
          {props.preferences.theme === 'light' ? (
            <SunIcon />
          ) : props.preferences.theme === 'system' ? (
            <SystemThemeIcon />
          ) : (
            <MoonIcon />
          )}
        </button>
      </div>
    </header>
  )
}
