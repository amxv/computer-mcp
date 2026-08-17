import type { ThemePreference } from '../api/client'

export function connectionSummary(
  runtimeId: string,
  agentCount: number,
  activeProcessCount: number,
): string {
  const agentLabel = agentCount === 1 ? 'Agent' : 'Agents'
  const processLabel = activeProcessCount === 1 ? 'process' : 'processes'
  return `${runtimeId.slice(0, 8)} · ${agentCount} ${agentLabel} · ${activeProcessCount} active ${processLabel}`
}

export function applyTheme(theme: ThemePreference): void {
  document.documentElement.dataset.theme = theme
}
