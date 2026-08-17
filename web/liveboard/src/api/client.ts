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
  seen_in_current_runtime: boolean
  active_process_count: number
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
