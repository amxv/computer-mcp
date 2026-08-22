export type LiveboardView =
  | { kind: 'unified' }
  | { kind: 'focused'; agentId: string }

const AGENT_ID = /^[a-z0-9]{4}$/

export function parseLiveboardView(search: string): LiveboardView {
  const params = new URLSearchParams(search)
  const agents = params.getAll('agent')
  if (agents.length === 0) return { kind: 'unified' }
  if (agents.length !== 1 || !AGENT_ID.test(agents[0]!)) {
    throw new Error('Focused Liveboard link has an invalid Agent ID')
  }
  return { kind: 'focused', agentId: agents[0]! }
}

export function canonicalLiveboardUrl(
  view: LiveboardView,
  currentHref = window.location.href,
): string {
  const url = new URL(currentHref)
  url.search = ''
  url.hash = ''
  if (view.kind === 'focused') url.searchParams.set('agent', view.agentId)
  return url.toString()
}

export function unifiedLiveboardUrl(currentHref = window.location.href): string {
  return canonicalLiveboardUrl({ kind: 'unified' }, currentHref)
}

export function focusedLiveboardUrl(
  agentId: string,
  currentHref = window.location.href,
): string {
  if (!AGENT_ID.test(agentId)) throw new Error('Invalid Agent ID')
  return canonicalLiveboardUrl({ kind: 'focused', agentId }, currentHref)
}
