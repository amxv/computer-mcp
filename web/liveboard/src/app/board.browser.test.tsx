import { render } from 'solid-js/web'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type {
  ApiAgent,
  LiveboardAgentPreference,
  LiveboardPreferences,
  LiveboardPreferencesPatch,
} from '../api/client'
import '../styles.css'
import { App } from './App'
import { applyTheme } from './bootstrap'

function agent(id: string, index: number): ApiAgent {
  return {
    id,
    first_seen_at_ms: 1_000 + index * 100,
    last_seen_at_ms: Date.now() - index * 10_000,
    seen_in_current_runtime: true,
    active_process_count: index === 0 ? 1 : 0,
    workdirs: [
      {
        normalized_workdir: `/workspace/repos/project-${index + 1}`,
        ordinal: 0,
        first_seen_at_ms: 1_000,
        last_seen_at_ms: 2_000 + index,
        first_invocation_id: index + 1,
        last_invocation_id: index + 1,
        retained_invocation_count: 1,
      },
    ],
  }
}

function mergePreferences(
  current: LiveboardPreferences,
  patch: LiveboardPreferencesPatch,
): LiveboardPreferences {
  const agents = { ...current.agents }
  for (const [id, update] of Object.entries(patch.agents ?? {})) {
    agents[id] = { ...(agents[id] ?? {}), ...update }
  }
  return {
    ...current,
    ...(patch.theme === undefined ? {} : { theme: patch.theme }),
    ...(patch.max_visible_agents === undefined
      ? {}
      : { max_visible_agents: patch.max_visible_agents }),
    ...(patch.command_outputs_expanded === undefined
      ? {}
      : { command_outputs_expanded: patch.command_outputs_expanded }),
    ...(patch.diffs_expanded === undefined
      ? {}
      : { diffs_expanded: patch.diffs_expanded }),
    agents,
  }
}

function element<T extends Element>(selector: string): T {
  const found = document.querySelector<T>(selector)
  if (!found) throw new Error(`missing browser-test element: ${selector}`)
  return found
}

function buttonWithText(text: string): HTMLButtonElement {
  const found = Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
    (button) => button.textContent?.includes(text),
  )
  if (!found) throw new Error(`missing browser-test button containing: ${text}`)
  return found
}

function dispatchChange(select: HTMLSelectElement, value: string) {
  select.value = value
  select.dispatchEvent(new Event('change', { bubbles: true }))
}

let disposeCurrent: (() => void) | undefined
let containerCurrent: HTMLDivElement | undefined

afterEach(() => {
  disposeCurrent?.()
  containerCurrent?.remove()
  disposeCurrent = undefined
  containerCurrent = undefined
  vi.unstubAllGlobals()
  applyTheme('system')
})

describe('Liveboard board shell', () => {
  it('manages current Agents without eviction and persists UI-only board changes', async () => {
    const agents = [
      agent('a111', 0),
      agent('b222', 1),
      agent('c333', 2),
      agent('d444', 3),
      agent('e555', 4),
    ]
    let preferences: LiveboardPreferences = {
      schema_version: 1,
      theme: 'system',
      max_visible_agents: 4,
      command_outputs_expanded: false,
      diffs_expanded: true,
      agents: {} satisfies Record<string, LiveboardAgentPreference>,
    }
    const patches: LiveboardPreferencesPatch[] = []

    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const path =
          typeof input === 'string'
            ? input
            : input instanceof URL
              ? input.toString()
              : input.url
        if (path === 'api/status') {
          return Response.json({
            schema_version: 1,
            api_version: 1,
            presentation_version: 2,
            runtime_id: 'runtime-board-browser',
            current_runtime_agent_count: agents.length,
            active_process_count: 1,
          })
        }
        if (path === 'api/agents?runtime=current') {
          return Response.json({
            schema_version: 1,
            runtime_id: 'runtime-board-browser',
            agents,
          })
        }
        if (path === 'preferences' && (init?.method ?? 'GET') === 'GET') {
          return Response.json(preferences)
        }
        if (path === 'preferences' && init?.method === 'PATCH') {
          const patch = JSON.parse(String(init.body)) as LiveboardPreferencesPatch
          patches.push(patch)
          preferences = mergePreferences(preferences, patch)
          return Response.json(preferences)
        }
        return Response.json({ error: 'not found' }, { status: 404 })
      }),
    )

    const container = document.createElement('div')
    document.body.append(container)
    containerCurrent = container
    disposeCurrent = render(() => <App />, container)
    await vi.waitFor(() => expect(document.body.textContent).toContain('Zodex Liveboard'))

    const visibleColumns = () =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-agent-column]')).map(
        (column) => column.dataset.agentId,
      )
    expect(visibleColumns()).toEqual(['a111', 'b222', 'c333', 'd444'])
    expect(visibleColumns()).not.toContain('e555')

    buttonWithText('All Agents').click()
    await vi.waitFor(() => expect(document.body.textContent).toContain('Current Local runtime'))
    const fifthRow = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.drawer-agent'),
    ).find((button) => button.textContent?.includes('e555'))
    expect(fifthRow).toBeDefined()
    expect(fifthRow?.disabled).toBe(true)
    expect(document.querySelectorAll('.drawer-agent-selected')).toHaveLength(4)
    expect(document.body.textContent).not.toContain('On board')
    element<HTMLButtonElement>('button[aria-label="Close All Agents"]').click()

    element<HTMLButtonElement>('button[aria-label="Remove Agent d444 from board"]').click()
    expect(visibleColumns()).toEqual(['a111', 'b222', 'c333'])
    await vi.waitFor(() =>
      expect(
        patches.some((patch) => patch.agents?.d444?.visible === false),
      ).toBe(true),
    )

    buttonWithText('All Agents').click()
    const addFifth = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.drawer-agent'),
    ).find((button) => button.textContent?.includes('e555'))
    expect(addFifth?.disabled).toBe(false)
    addFifth?.click()
    expect(visibleColumns()).toEqual(['a111', 'b222', 'c333', 'e555'])
    element<HTMLButtonElement>('button[aria-label="Close All Agents"]').click()

    element<HTMLButtonElement>('button[aria-label="Edit alias for Agent e555"]').click()
    const aliasInput = element<HTMLInputElement>('input[aria-label="Alias for Agent e555"]')
    aliasInput.value = 'release checks'
    aliasInput.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'release checks' }))
    aliasInput.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Enter' }))
    await vi.waitFor(() =>
      expect(
        patches.some((patch) => patch.agents?.e555?.alias === 'release checks'),
      ).toBe(true),
    )
    await vi.waitFor(() => expect(document.body.textContent).toContain('release checks'))
    expect(
      element<HTMLElement>('[data-agent-id="e555"] .agent-id').textContent,
    ).toBe('e555')

    const patchCountBeforeReorderDrag = patches.length
    const reorderHandle = element<HTMLButtonElement>(
      'button[aria-label="Drag Agent c333 to reorder"]',
    )
    reorderHandle.setPointerCapture = () => undefined
    reorderHandle.dispatchEvent(
      new PointerEvent('pointerdown', { bubbles: true, pointerId: 5, clientX: 800 }),
    )
    reorderHandle.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, pointerId: 5, clientX: -100 }),
    )
    expect(patches).toHaveLength(patchCountBeforeReorderDrag)
    reorderHandle.dispatchEvent(
      new PointerEvent('pointerup', { bubbles: true, pointerId: 5, clientX: -100 }),
    )
    expect(visibleColumns()[0]).toBe('c333')
    await vi.waitFor(() =>
      expect(patches.length).toBe(patchCountBeforeReorderDrag + 1),
    )

    element<HTMLButtonElement>('button[aria-label="Move Agent c333 right"]').click()
    expect(visibleColumns().slice(0, 2)).toEqual(['a111', 'c333'])
    await vi.waitFor(() =>
      expect(patches.some((patch) => patch.agents?.a111?.order === 0)).toBe(true),
    )

    const patchCountBeforeResize = patches.length
    const resizeHandle = element<HTMLButtonElement>(
      'button[aria-label="Resize Agent a111 column"]',
    )
    resizeHandle.setPointerCapture = () => undefined
    resizeHandle.dispatchEvent(
      new PointerEvent('pointerdown', { bubbles: true, pointerId: 7, clientX: 300 }),
    )
    resizeHandle.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, pointerId: 7, clientX: 340 }),
    )
    expect(patches).toHaveLength(patchCountBeforeResize)
    resizeHandle.dispatchEvent(
      new PointerEvent('pointerup', { bubbles: true, pointerId: 7, clientX: 340 }),
    )
    await vi.waitFor(() => expect(patches.length).toBe(patchCountBeforeResize + 1))
    expect(patches.at(-1)?.agents?.a111?.width_weight).toBeTypeOf('number')

    dispatchChange(element<HTMLSelectElement>('select[aria-label="Maximum visible Agents"]'), '2')
    expect(visibleColumns()).toHaveLength(2)
    await vi.waitFor(() =>
      expect(patches.some((patch) => patch.max_visible_agents === 2)).toBe(true),
    )
    const maximumPatch = patches.find((patch) => patch.max_visible_agents === 2)
    expect(
      Object.values(maximumPatch?.agents ?? {}).filter(
        (preference) => preference.visible === false,
      ),
    ).toHaveLength(2)

    buttonWithText('Cmd closed').click()
    await vi.waitFor(() =>
      expect(
        patches.some((patch) => patch.command_outputs_expanded === true),
      ).toBe(true),
    )
    buttonWithText('Diff open').click()
    await vi.waitFor(() =>
      expect(patches.some((patch) => patch.diffs_expanded === false)).toBe(true),
    )

    dispatchChange(element<HTMLSelectElement>('select[aria-label="Liveboard theme"]'), 'dark')
    await vi.waitFor(() => expect(document.documentElement.dataset.theme).toBe('dark'))
    expect(getComputedStyle(document.documentElement).colorScheme).toBe('dark')
  })
})
