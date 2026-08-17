import type {
  ApiAgent,
  LiveboardAgentPreference,
  LiveboardPreferences,
} from '../api/client'

export const MIN_COLUMN_WEIGHT = 0.1

function preferenceFor(
  preferences: LiveboardPreferences,
  agentId: string,
): LiveboardAgentPreference {
  return preferences.agents[agentId] ?? {}
}

function sortByPreferenceThenFirstSeen(
  left: ApiAgent,
  right: ApiAgent,
  preferences: LiveboardPreferences,
): number {
  const leftOrder = preferenceFor(preferences, left.id).order
  const rightOrder = preferenceFor(preferences, right.id).order
  if (leftOrder !== undefined || rightOrder !== undefined) {
    if (leftOrder === undefined) return 1
    if (rightOrder === undefined) return -1
    if (leftOrder !== rightOrder) return leftOrder - rightOrder
  }
  if (left.first_seen_at_ms !== right.first_seen_at_ms) {
    return left.first_seen_at_ms - right.first_seen_at_ms
  }
  return left.id.localeCompare(right.id)
}

export function currentRuntimeAgents(agents: readonly ApiAgent[]): ApiAgent[] {
  return agents.filter((agent) => agent.seen_in_current_runtime)
}

export function initialVisibleAgentIds(
  agents: readonly ApiAgent[],
  preferences: LiveboardPreferences,
): string[] {
  const current = currentRuntimeAgents(agents)
  const explicitlyVisible = current
    .filter((agent) => preferenceFor(preferences, agent.id).visible === true)
    .sort((left, right) =>
      sortByPreferenceThenFirstSeen(left, right, preferences),
    )
  const automatic = current
    .filter(
      (agent) => preferenceFor(preferences, agent.id).visible === undefined,
    )
    .sort((left, right) =>
      sortByPreferenceThenFirstSeen(left, right, preferences),
    )
  return [...explicitlyVisible, ...automatic]
    .slice(0, preferences.max_visible_agents)
    .map((agent) => agent.id)
}

export function admitAgent(
  visibleIds: readonly string[],
  agent: ApiAgent,
  preferences: LiveboardPreferences,
): string[] {
  if (
    !agent.seen_in_current_runtime ||
    visibleIds.includes(agent.id) ||
    preferenceFor(preferences, agent.id).visible === false ||
    visibleIds.length >= preferences.max_visible_agents
  ) {
    return [...visibleIds]
  }
  return [...visibleIds, agent.id]
}

export function addAgentToBoard(
  visibleIds: readonly string[],
  agentId: string,
  maximum: number,
): string[] {
  if (visibleIds.includes(agentId) || visibleIds.length >= maximum) {
    return [...visibleIds]
  }
  return [...visibleIds, agentId]
}

export function removeAgentFromBoard(
  visibleIds: readonly string[],
  agentId: string,
): string[] {
  return visibleIds.filter((id) => id !== agentId)
}

export function moveAgent(
  visibleIds: readonly string[],
  agentId: string,
  targetIndex: number,
): string[] {
  const currentIndex = visibleIds.indexOf(agentId)
  if (currentIndex < 0) return [...visibleIds]
  const clampedTarget = Math.max(
    0,
    Math.min(visibleIds.length - 1, targetIndex),
  )
  if (currentIndex === clampedTarget) return [...visibleIds]
  const next = [...visibleIds]
  next.splice(currentIndex, 1)
  next.splice(clampedTarget, 0, agentId)
  return next
}

export function shrinkBoardToMaximum(
  visibleIds: readonly string[],
  maximum: number,
): { visible: string[]; hidden: string[] } {
  const safeMaximum = Math.max(0, maximum)
  return {
    visible: visibleIds.slice(0, safeMaximum),
    hidden: visibleIds.slice(safeMaximum),
  }
}

export function columnWeights(
  visibleIds: readonly string[],
  preferences: LiveboardPreferences,
): Record<string, number> {
  return Object.fromEntries(
    visibleIds.map((agentId) => [
      agentId,
      preferenceFor(preferences, agentId).width_weight ?? 1,
    ]),
  )
}

export function resizeAdjacentWeights(
  leftWeight: number,
  rightWeight: number,
  deltaPixels: number,
  combinedPixels: number,
): [number, number] {
  const totalWeight = leftWeight + rightWeight
  if (!Number.isFinite(combinedPixels) || combinedPixels <= 0) {
    return [leftWeight, rightWeight]
  }
  const deltaWeight = (deltaPixels / combinedPixels) * totalWeight
  const nextLeft = Math.min(
    totalWeight - MIN_COLUMN_WEIGHT,
    Math.max(MIN_COLUMN_WEIGHT, leftWeight + deltaWeight),
  )
  return [nextLeft, totalWeight - nextLeft]
}

export function orderPatch(
  visibleIds: readonly string[],
): Record<string, LiveboardAgentPreference> {
  return Object.fromEntries(
    visibleIds.map((agentId, index) => [agentId, { order: index }]),
  )
}
