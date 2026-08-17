import { createSignal } from 'solid-js'
import { render } from 'solid-js/web'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ApiAgent, LiveboardPreferences } from '../api/client'
import '../styles.css'
import { Board } from './Board'

function agent(id: string, index: number): ApiAgent {
  return {
    id,
    first_seen_at_ms: index + 1,
    last_seen_at_ms: index + 1,
    seen_in_current_runtime: true,
    active_process_count: 0,
    workdirs: [],
  }
}

const preferences: LiveboardPreferences = {
  schema_version: 1,
  theme: 'system',
  max_visible_agents: 4,
  command_outputs_expanded: false,
  diffs_expanded: true,
  agents: {},
}

let disposeCurrent: (() => void) | undefined
let containerCurrent: HTMLDivElement | undefined

afterEach(() => {
  disposeCurrent?.()
  containerCurrent?.remove()
  disposeCurrent = undefined
  containerCurrent = undefined
})

describe('responsive Agent board layout', () => {
  it('handles 0/1/2/5 current Agents while respecting the configured four-column cap', async () => {
    const [agents, setAgents] = createSignal<ApiAgent[]>([])
    const container = document.createElement('div')
    container.style.width = '1280px'
    container.style.height = '720px'
    document.body.append(container)
    containerCurrent = container
    disposeCurrent = render(
      () => (
        <div class="app-shell">
          <Board
            agents={agents()}
            preferences={preferences}
            nowMs={10_000}
            saving={false}
            onPatch={() => undefined}
          />
        </div>
      ),
      container,
    )

    const columns = () =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-agent-column]'))
    expect(columns()).toHaveLength(0)
    expect(document.body.textContent).toContain('Waiting for the first Agent activity')

    setAgents([agent('a111', 0)])
    await vi.waitFor(() => expect(columns()).toHaveLength(1))
    const boardWidth = document.querySelector<HTMLElement>('.agent-board')!.getBoundingClientRect().width
    const oneColumnWidth = columns()[0]!.getBoundingClientRect().width
    expect(oneColumnWidth).toBeGreaterThan(boardWidth * 0.9)

    setAgents([agent('a111', 0), agent('b222', 1)])
    await vi.waitFor(() => expect(columns()).toHaveLength(2))
    expect(columns()[0]!.getBoundingClientRect().width).toBeGreaterThan(280)

    setAgents([
      agent('a111', 0),
      agent('b222', 1),
      agent('c333', 2),
      agent('d444', 3),
      agent('e555', 4),
    ])
    await vi.waitFor(() => expect(columns()).toHaveLength(4))
    expect(columns().map((column) => column.dataset.agentId)).toEqual([
      'a111',
      'b222',
      'c333',
      'd444',
    ])
  })
})
