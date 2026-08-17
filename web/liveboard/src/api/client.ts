export interface ApiStatus {
  schema_version: number
  api_version: number
  presentation_version: number
  runtime_id: string
  current_runtime_agent_count: number
  active_process_count: number
}

export interface ApiAgent {
  id: string
  first_seen_at_ms: number
  last_seen_at_ms: number
  seen_in_current_runtime: boolean
  active_process_count: number
  workdirs: ApiAgentWorkdir[]
}

export interface ApiAgentWorkdir {
  normalized_workdir: string
  ordinal: number
  first_seen_at_ms: number
  last_seen_at_ms: number
  first_invocation_id: number
  last_invocation_id: number
  retained_invocation_count: number
}

export interface ApiAgentList {
  schema_version: number
  runtime_id: string
  agents: ApiAgent[]
}

export type ThemePreference = 'system' | 'light' | 'dark'

export interface LiveboardPreferences {
  schema_version: number
  theme: ThemePreference
  max_visible_agents: number
  command_outputs_expanded: boolean
  diffs_expanded: boolean
  agents: Record<string, LiveboardAgentPreference>
}

export interface LiveboardAgentPreference {
  alias?: string
  visible?: boolean
  order?: number
  width_weight?: number
}

export interface LiveboardPreferencesPatch {
  schema_version?: number
  theme?: ThemePreference
  max_visible_agents?: number
  command_outputs_expanded?: boolean
  diffs_expanded?: boolean
  agents?: Record<string, LiveboardAgentPreference>
}

export async function fetchJson<T>(path: string): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) {
    throw new Error(`Liveboard request failed (${response.status})`)
  }
  return (await response.json()) as T
}

export async function patchPreferences(
  patch: LiveboardPreferencesPatch,
): Promise<LiveboardPreferences> {
  const response = await fetch('preferences', {
    method: 'PATCH',
    credentials: 'same-origin',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(patch),
  })
  if (!response.ok) {
    throw new Error(`Liveboard preference update failed (${response.status})`)
  }
  return (await response.json()) as LiveboardPreferences
}

export async function loadBootstrap() {
  const [status, agents, preferences] = await Promise.all([
    fetchJson<ApiStatus>('api/status'),
    fetchJson<ApiAgentList>('api/agents?runtime=current'),
    fetchJson<LiveboardPreferences>('preferences'),
  ])
  if (status.runtime_id !== agents.runtime_id) {
    throw new Error('Local runtime changed during Liveboard bootstrap')
  }
  return { status, agents, preferences }
}
