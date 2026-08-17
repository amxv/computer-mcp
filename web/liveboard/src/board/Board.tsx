import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  on,
  type JSX,
} from 'solid-js'

import type {
  ApiAgent,
  LiveboardPreferences,
  LiveboardPreferencesPatch,
} from '../api/client'
import type { RuntimeConnectionState } from '../streams/runtime'
import { AgentColumn } from '../agents/AgentColumn'
import { AgentDrawer } from '../agents/AgentDrawer'
import { Toolbar } from '../ui/Toolbar'
import {
  addAgentToBoard,
  admitAgent,
  columnWeights,
  currentRuntimeAgents,
  initialVisibleAgentIds,
  moveAgent,
  orderPatch,
  removeAgentFromBoard,
  resizeAdjacentWeights,
  shrinkBoardToMaximum,
} from './model'

interface BoardProps {
  agents: readonly ApiAgent[]
  preferences: LiveboardPreferences
  nowMs: number
  saving: boolean
  error?: string
  connectionState?: RuntimeConnectionState
  connectionError?: string
  onPatch: (patch: LiveboardPreferencesPatch) => void
  onVisibleAgentsChange?: (agentIds: readonly string[]) => void
  renderTimeline?: (agentId: string) => JSX.Element
}

export function Board(props: BoardProps) {
  const [drawerOpen, setDrawerOpen] = createSignal(false)
  const [locallyHiddenIds, setLocallyHiddenIds] = createSignal(
    new Set(
      Object.entries(props.preferences.agents)
        .filter(([, preference]) => preference.visible === false)
        .map(([agentId]) => agentId),
    ),
  )
  const [visibleIds, setVisibleIds] = createSignal(
    initialVisibleAgentIds(props.agents, props.preferences),
  )
  const [weights, setWeights] = createSignal(
    columnWeights(visibleIds(), props.preferences),
  )
  let boardElement: HTMLDivElement | undefined

  const agents = createMemo(() => currentRuntimeAgents(props.agents))
  const agentById = createMemo(
    () => new Map(agents().map((agent) => [agent.id, agent] as const)),
  )
  const activeProcessCount = createMemo(() =>
    agents().reduce((count, agent) => count + agent.active_process_count, 0),
  )

  createEffect(
    on(
      () => props.agents,
      (nextAgents) => {
        setVisibleIds((previous) => {
          let next = previous.filter((id) =>
            nextAgents.some(
              (agent) => agent.id === id && agent.seen_in_current_runtime,
            ),
          )
          for (const agent of nextAgents) {
            if (!locallyHiddenIds().has(agent.id)) {
              next = admitAgent(next, agent, props.preferences)
            }
          }
          return next
        })
      },
    ),
  )

  createEffect(
    on(visibleIds, (ids) => {
      setWeights((current) => {
        const next: Record<string, number> = {}
        for (const id of ids) {
          next[id] =
            current[id] ?? props.preferences.agents[id]?.width_weight ?? 1
        }
        return next
      })
      props.onVisibleAgentsChange?.(ids)
    }),
  )

  const persistOrder = (next: string[]) => {
    setVisibleIds(next)
    props.onPatch({ agents: orderPatch(next) })
  }

  const hideAgent = (agentId: string) => {
    setLocallyHiddenIds((current) => new Set(current).add(agentId))
    setVisibleIds((current) => removeAgentFromBoard(current, agentId))
    props.onPatch({ agents: { [agentId]: { visible: false } } })
  }

  const addAgent = (agentId: string) => {
    const current = visibleIds()
    const next = addAgentToBoard(
      current,
      agentId,
      props.preferences.max_visible_agents,
    )
    if (next.length === current.length) return
    setLocallyHiddenIds((hidden) => {
      const nextHidden = new Set(hidden)
      nextHidden.delete(agentId)
      return nextHidden
    })
    setVisibleIds(next)
    props.onPatch({
      agents: {
        [agentId]: { visible: true, order: next.length - 1 },
      },
    })
  }

  const setMaximum = (maximum: number) => {
    const result = shrinkBoardToMaximum(visibleIds(), maximum)
    if (result.hidden.length > 0) {
      setLocallyHiddenIds((current) => {
        const next = new Set(current)
        for (const agentId of result.hidden) next.add(agentId)
        return next
      })
    }
    setVisibleIds(result.visible)
    props.onPatch({
      max_visible_agents: maximum,
      agents: Object.fromEntries(
        result.hidden.map((agentId) => [agentId, { visible: false }]),
      ),
    })
  }

  const beginReorder = (agentId: string, event: PointerEvent) => {
    if (!boardElement) return
    const handle = event.currentTarget as HTMLElement
    const column = handle.closest<HTMLElement>('[data-agent-column]')
    if (!column) return
    event.preventDefault()
    const startX = event.clientX
    let latestX = startX
    let frame: number | undefined

    const render = () => {
      frame = undefined
      column.style.transform = `translate3d(${latestX - startX}px, 0, 0)`
    }
    const move = (pointerEvent: PointerEvent) => {
      latestX = pointerEvent.clientX
      if (frame === undefined) frame = requestAnimationFrame(render)
    }
    const cleanup = () => {
      if (frame !== undefined) cancelAnimationFrame(frame)
      column.style.transform = ''
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', cancel)
    }
    const cancel = () => cleanup()
    const end = (pointerEvent: PointerEvent) => {
      latestX = pointerEvent.clientX
      cleanup()
      const columns = Array.from(
        boardElement?.querySelectorAll<HTMLElement>('[data-agent-column]') ?? [],
      )
      if (columns.length === 0) return
      const targetIndex = columns.reduce(
        (best, candidate, index) => {
          const rect = candidate.getBoundingClientRect()
          const distance = Math.abs(latestX - (rect.left + rect.width / 2))
          return distance < best.distance ? { index, distance } : best
        },
        { index: 0, distance: Number.POSITIVE_INFINITY },
      ).index
      const next = moveAgent(visibleIds(), agentId, targetIndex)
      if (next.join('\0') !== visibleIds().join('\0')) persistOrder(next)
    }

    handle.setPointerCapture(event.pointerId)
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end, { once: true })
    handle.addEventListener('pointercancel', cancel, { once: true })
  }

  const beginResize = (
    leftAgentId: string,
    rightAgentId: string,
    event: PointerEvent,
  ) => {
    if (!boardElement) return
    const handle = event.currentTarget as HTMLElement
    const leftColumn = boardElement.querySelector<HTMLElement>(
      `[data-agent-id="${leftAgentId}"]`,
    )
    const rightColumn = boardElement.querySelector<HTMLElement>(
      `[data-agent-id="${rightAgentId}"]`,
    )
    if (!leftColumn || !rightColumn) return
    event.preventDefault()
    const startX = event.clientX
    let latestX = startX
    let frame: number | undefined
    const currentWeights = weights()
    const leftWeight = currentWeights[leftAgentId] ?? 1
    const rightWeight = currentWeights[rightAgentId] ?? 1
    const combinedPixels =
      leftColumn.getBoundingClientRect().width +
      rightColumn.getBoundingClientRect().width

    const pairAt = (clientX: number) =>
      resizeAdjacentWeights(
        leftWeight,
        rightWeight,
        clientX - startX,
        combinedPixels,
      )
    const render = () => {
      frame = undefined
      const [nextLeft, nextRight] = pairAt(latestX)
      leftColumn.style.setProperty('--column-weight', String(nextLeft))
      rightColumn.style.setProperty('--column-weight', String(nextRight))
    }
    const move = (pointerEvent: PointerEvent) => {
      latestX = pointerEvent.clientX
      if (frame === undefined) frame = requestAnimationFrame(render)
    }
    const cleanup = () => {
      if (frame !== undefined) cancelAnimationFrame(frame)
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', end)
      handle.removeEventListener('pointercancel', cancel)
    }
    const restore = () => {
      leftColumn.style.setProperty('--column-weight', String(leftWeight))
      rightColumn.style.setProperty('--column-weight', String(rightWeight))
    }
    const cancel = () => {
      cleanup()
      restore()
    }
    const end = (pointerEvent: PointerEvent) => {
      latestX = pointerEvent.clientX
      cleanup()
      const [nextLeft, nextRight] = pairAt(latestX)
      setWeights((current) => ({
        ...current,
        [leftAgentId]: nextLeft,
        [rightAgentId]: nextRight,
      }))
      props.onPatch({
        agents: {
          [leftAgentId]: { width_weight: nextLeft },
          [rightAgentId]: { width_weight: nextRight },
        },
      })
    }

    handle.setPointerCapture(event.pointerId)
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', end, { once: true })
    handle.addEventListener('pointercancel', cancel, { once: true })
  }

  return (
    <>
      <Toolbar
        preferences={props.preferences}
        currentAgentCount={agents().length}
        activeProcessCount={activeProcessCount()}
        saving={props.saving}
        error={props.error}
        connectionState={props.connectionState}
        connectionError={props.connectionError}
        onOpenAgents={() => setDrawerOpen(true)}
        onMaximumChange={setMaximum}
        onThemeChange={(theme) => props.onPatch({ theme })}
        onCommandExpansionChange={(command_outputs_expanded) =>
          props.onPatch({ command_outputs_expanded })
        }
        onDiffExpansionChange={(diffs_expanded) => props.onPatch({ diffs_expanded })}
      />
      <div class="board-wrap">
        <div ref={boardElement} class="agent-board" aria-label="Agent board">
          <Show
            when={visibleIds().length > 0}
            fallback={
              <section class="empty-board">
                <p>Waiting for the first Agent activity in this Local runtime.</p>
                <button type="button" class="text-button" onClick={() => setDrawerOpen(true)}>
                  Open All Agents
                </button>
              </section>
            }
          >
            <For each={visibleIds()}>
              {(agentId, index) => {
                const agent = () => agentById().get(agentId)
                const nextId = () => visibleIds()[index() + 1]
                return (
                  <Show when={agent()}>
                    {(value) => (
                      <AgentColumn
                        agent={value()}
                        alias={props.preferences.agents[agentId]?.alias}
                        nowMs={props.nowMs}
                        weight={weights()[agentId] ?? 1}
                        index={index()}
                        count={visibleIds().length}
                        onHide={() => hideAgent(agentId)}
                        onAliasSave={(alias) =>
                          props.onPatch({ agents: { [agentId]: { alias } } })
                        }
                        onMove={(direction) =>
                          persistOrder(
                            moveAgent(
                              visibleIds(),
                              agentId,
                              index() + direction,
                            ),
                          )
                        }
                        onReorderPointerDown={(event) => beginReorder(agentId, event)}
                        onResizePointerDown={
                          nextId()
                            ? (event) => beginResize(agentId, nextId()!, event)
                            : undefined
                        }
                      >
                        {props.renderTimeline?.(agentId)}
                      </AgentColumn>
                    )}
                  </Show>
                )
              }}
            </For>
          </Show>
        </div>
      </div>
      <AgentDrawer
        open={drawerOpen()}
        agents={agents()}
        visibleIds={visibleIds()}
        preferences={props.preferences}
        nowMs={props.nowMs}
        onClose={() => setDrawerOpen(false)}
        onAdd={addAgent}
      />
    </>
  )
}
