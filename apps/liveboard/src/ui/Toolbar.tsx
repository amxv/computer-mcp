import type { RuntimeConnectionState } from '../streams/runtime'
import {
  AgentsIcon,
  CogIcon,
  TerminalIcon,
  UserIcon,
} from './icons'

interface ToolbarProps {
  currentAgentCount: number
  activeProcessCount: number
  saving: boolean
  error?: string
  connectionState?: RuntimeConnectionState
  connectionError?: string
  focusedAgentId?: string
  onOpenAgents: () => void
  onOpenSettings: () => void
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
        <strong class="brand">zodex</strong>
        {props.focusedAgentId ? (
          <span class="focused-agent-badge">Focused · {props.focusedAgentId}</span>
        ) : null}
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
        {connectionState() === 'connected' ? (
          <span
            class="runtime-summary"
            role="status"
            aria-label={`${props.currentAgentCount} ${props.currentAgentCount === 1 ? 'Agent' : 'Agents'}, ${props.activeProcessCount} ${props.activeProcessCount === 1 ? 'process' : 'processes'} running`}
          >
            <span class="connection-dot connection-connected" aria-hidden="true" />
            <span class="runtime-summary-item" title="Agents">
              <UserIcon />
              <span>{props.currentAgentCount}</span>
            </span>
            <span class="runtime-summary-divider" aria-hidden="true" />
            <span class="runtime-summary-item" title="Processes running">
              <TerminalIcon />
              <span>{props.activeProcessCount}</span>
            </span>
          </span>
        ) : (
          <span
            class="connection-detail"
            aria-label={`Local observer ${connectionLabel().toLowerCase()}`}
            title={props.connectionError}
          >
            <span
              class={`connection-dot connection-${connectionState()}`}
              aria-hidden="true"
            />
            {connectionLabel()}
          </span>
        )}
        <button
          type="button"
          class="toolbar-button"
          aria-label="All Agents"
          onClick={props.onOpenAgents}
        >
          <AgentsIcon />
          <span class="toolbar-button-label">All Agents</span>
        </button>
        <button
          type="button"
          class="toolbar-button"
          aria-label="Open Settings"
          onClick={props.onOpenSettings}
        >
          <CogIcon />
          <span class="toolbar-button-label">Settings</span>
        </button>
      </div>
    </header>
  )
}
