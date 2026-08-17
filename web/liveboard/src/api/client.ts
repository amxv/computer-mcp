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

export interface PresentationEvidence {
  evidence_state: string
  capture_state: string
  degraded: boolean
  reason: string | null
}

export interface PresentationPollSummary {
  count: number
  final_status: string | null
  caller_agent_ids: string[]
  cross_agent: boolean
}

export interface PresentationDiffLine {
  kind: string
  old_line: number | null
  new_line: number | null
  text: string
}

export interface PresentationFileChange {
  operation: 'created' | 'edited' | 'deleted' | 'renamed'
  path: string
  old_path: string | null
  write_mode: 'overwrite' | 'append' | null
  added: number
  removed: number
  diff_truncated: boolean
  lines: PresentationDiffLine[]
}

interface PresentationRecordBase {
  presentation_id: string
  primary_invocation_id: number
  raw_evidence_count: number
  raw_invocation_ids: number[]
  raw_invocation_ids_truncated: boolean
  agent_id: string | null
  declared_workdir: string | null
  normalized_workdir: string | null
  new_workdir: string | null
  started_at_ms: number
  duration_ms: number | null
  evidence: PresentationEvidence
}

export type PresentationRecord = PresentationRecordBase &
  (
    | {
        kind: 'command'
        command: string
        status: string
        effective_cwd: string | null
        exit_code: number | null
        termination_reason: string | null
        output: string | null
        output_truncated: boolean
        polls: PresentationPollSummary | null
      }
    | {
        kind: 'file_changes'
        source_tool: string
        changes: PresentationFileChange[]
      }
    | {
        kind: 'stdin'
        target_session_handle: string
        chars: string
        chars_truncated: boolean
        creator_agent_id: string | null
        cross_agent: boolean
        result_status: string | null
      }
    | {
        kind: 'kill'
        target_session_handle: string
        creator_agent_id: string | null
        cross_agent: boolean
        result_status: string | null
      }
    | {
        kind: 'poll_aggregate'
        target_session_handle: string
        count: number
        final_status: string | null
        creator_agent_id: string | null
        caller_agent_ids: string[]
        cross_agent: boolean
      }
    | {
        kind: 'generic'
        tool_name: string
        status: string
        summary: string | null
      }
  )

export interface ApiTimelinePage {
  schema_version: number
  presentation_version: number
  runtime_id: string
  records: PresentationRecord[]
  has_more: boolean
  next_cursor: string | null
}

export interface ApiTimelineDetail {
  schema_version: number
  presentation_version: number
  runtime_id: string
  record: PresentationRecord
}

export interface HistoryLiveEvent {
  schema_version: number
  runtime_id: string
  sequence: number
  emitted_at_ms: number
  event_type: string
  agent_id: string | null
  invocation_id: number | null
  presentation_id: string | null
  presentation_revision: number | null
  payload: Record<string, unknown>
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

export const OBSERVER_API_VERSION = 1
export const PRESENTATION_VERSION = 2
export const LIVE_EVENT_VERSION = 2

export function validateStatus(status: ApiStatus): void {
  if (
    status.schema_version !== OBSERVER_API_VERSION ||
    status.api_version !== OBSERVER_API_VERSION ||
    status.presentation_version !== PRESENTATION_VERSION
  ) {
    throw new Error('Local observer version is incompatible with this Liveboard')
  }
}

export function validateAgentList(list: ApiAgentList, runtimeId: string): void {
  if (list.schema_version !== OBSERVER_API_VERSION || list.runtime_id !== runtimeId) {
    throw new Error('Local Agent list belongs to an incompatible or changed runtime')
  }
}

export function validateTimelinePage(page: ApiTimelinePage, runtimeId: string): void {
  if (
    page.schema_version !== OBSERVER_API_VERSION ||
    page.presentation_version !== PRESENTATION_VERSION ||
    page.runtime_id !== runtimeId
  ) {
    throw new Error('Local timeline belongs to an incompatible or changed runtime')
  }
}

export function validateTimelineDetail(
  detail: ApiTimelineDetail,
  runtimeId: string,
): void {
  if (
    detail.schema_version !== OBSERVER_API_VERSION ||
    detail.presentation_version !== PRESENTATION_VERSION ||
    detail.runtime_id !== runtimeId
  ) {
    throw new Error('Local timeline detail belongs to an incompatible or changed runtime')
  }
}

export function validateLiveEvent(event: HistoryLiveEvent): void {
  if (event.schema_version !== LIVE_EVENT_VERSION) {
    throw new Error('Local live-event version is incompatible with this Liveboard')
  }
}

export async function fetchStatus(): Promise<ApiStatus> {
  const status = await fetchJson<ApiStatus>('api/status')
  validateStatus(status)
  return status
}

export async function fetchCurrentAgents(runtimeId: string): Promise<ApiAgentList> {
  const list = await fetchJson<ApiAgentList>('api/agents?runtime=current')
  validateAgentList(list, runtimeId)
  return list
}

export async function fetchTimelineDetail(
  presentationId: string,
  runtimeId: string,
): Promise<ApiTimelineDetail> {
  const detail = await fetchJson<ApiTimelineDetail>(
    `api/timeline/${encodeURIComponent(presentationId)}`,
  )
  validateTimelineDetail(detail, runtimeId)
  return detail
}

export interface TimelineQuery {
  agentId?: string
  limit?: number
  cursor?: string
  beforeMs?: number
  recoverySinceMs?: number
}

export async function fetchTimeline(
  query: TimelineQuery,
  runtimeId: string,
): Promise<ApiTimelinePage> {
  const params = new URLSearchParams()
  if (query.agentId) params.set('agent_id', query.agentId)
  if (query.limit !== undefined) params.set('limit', String(query.limit))
  if (query.cursor) params.set('cursor', query.cursor)
  if (query.beforeMs !== undefined) params.set('before_ms', String(query.beforeMs))
  if (query.recoverySinceMs !== undefined) {
    params.set('recovery_since_ms', String(query.recoverySinceMs))
  }
  const page = await fetchJson<ApiTimelinePage>(`api/timeline?${params.toString()}`)
  validateTimelinePage(page, runtimeId)
  return page
}

export function eventStreamUrl(outputAgentIds: readonly string[]): string {
  const params = new URLSearchParams()
  params.set('output_agent_ids', outputAgentIds.join(','))
  return `api/events?${params.toString()}`
}

export async function loadBootstrap() {
  const [status, agents, preferences] = await Promise.all([
    fetchStatus(),
    fetchJson<ApiAgentList>('api/agents?runtime=current'),
    fetchJson<LiveboardPreferences>('preferences'),
  ])
  if (status.runtime_id !== agents.runtime_id) {
    throw new Error('Local runtime changed during Liveboard bootstrap')
  }
  validateAgentList(agents, status.runtime_id)
  return { status, agents, preferences }
}
