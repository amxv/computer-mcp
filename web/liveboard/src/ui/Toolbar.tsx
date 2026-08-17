import type { LiveboardPreferences, ThemePreference } from '../api/client'
import type { RuntimeConnectionState } from '../streams/runtime'
import { AgentsIcon } from './icons'

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
        <strong class="brand">Zodex Liveboard</strong>
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
            ? `${props.currentAgentCount} ${props.currentAgentCount === 1 ? 'Agent' : 'Agents'}${props.activeProcessCount > 0 ? ` · ${props.activeProcessCount} active` : ''}`
            : connectionLabel()}
        </span>
      </div>
      <div class="toolbar-controls">
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
        <label class="toolbar-field theme-field">
          <span>Theme</span>
          <select
            aria-label="Liveboard theme"
            value={props.preferences.theme}
            onChange={(event) =>
              props.onThemeChange(event.currentTarget.value as ThemePreference)
            }
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
        <span class="preference-state" role="status" aria-live="polite">
          {props.error ? 'Save failed' : props.saving ? 'Saving…' : ''}
        </span>
      </div>
    </header>
  )
}
