import { render } from 'solid-js/web'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ApiTimelinePage, PresentationRecord } from '../api/client'
import '../diff.css'
import '../styles.css'
import type { DiffHighlighter } from '../diff/HighlightWorkerClient'
import type { DiffHighlightInput } from '../diff/protocol'
import { createAgentStreamController } from '../streams/AgentStreamController'
import { AgentTimeline } from './AgentTimeline'

function generic(
  id: number,
  agentId: string,
  summary = `activity ${id}`,
): PresentationRecord {
  return {
    presentation_id: `inv-${id}`,
    primary_invocation_id: id,
    raw_evidence_count: 1,
    raw_invocation_ids: [id],
    raw_invocation_ids_truncated: false,
    agent_id: agentId,
    declared_workdir: '/repo',
    normalized_workdir: '/repo',
    new_workdir: null,
    started_at_ms: id * 100,
    duration_ms: 10,
    evidence: {
      evidence_state: 'complete',
      capture_state: 'complete',
      degraded: false,
      reason: null,
    },
    kind: 'generic',
    tool_name: 'fixture',
    status: 'success',
    summary,
  }
}

function page(records: PresentationRecord[]): ApiTimelinePage {
  return {
    schema_version: 1,
    presentation_version: 3,
    runtime_id: 'runtime-browser',
    records,
    has_more: false,
    next_cursor: null,
  }
}

function fileChange(id: number, agentId: string): PresentationRecord {
  return {
    presentation_id: `file-${id}`,
    primary_invocation_id: id,
    raw_evidence_count: 1,
    raw_invocation_ids: [id],
    raw_invocation_ids_truncated: false,
    agent_id: agentId,
    declared_workdir: '/repo',
    normalized_workdir: '/repo',
    new_workdir: null,
    started_at_ms: id * 100,
    duration_ms: 10,
    evidence: {
      evidence_state: 'complete',
      capture_state: 'complete',
      degraded: false,
      reason: null,
    },
    kind: 'file_changes',
    source_tool: 'apply_patch',
    changes: [
      {
        operation: 'edited',
        path: `/repo/src/file_${id}.rs`,
        old_path: null,
        write_mode: null,
        added: 1,
        removed: 0,
        diff_truncated: false,
        diff_lines_included: true,
        lines: [
          {
            kind: 'add',
            old_line: null,
            new_line: id,
            text: `let value_${id}: usize = ${id};`,
          },
        ],
      },
    ],
  }
}

function command(id: number, agentId: string): PresentationRecord {
  return {
    presentation_id: `command-${id}`,
    primary_invocation_id: id,
    raw_evidence_count: 1,
    raw_invocation_ids: [id],
    raw_invocation_ids_truncated: false,
    agent_id: agentId,
    declared_workdir: '/repo',
    normalized_workdir: '/repo',
    new_workdir: null,
    started_at_ms: id * 100,
    duration_ms: null,
    evidence: {
      evidence_state: 'complete',
      capture_state: 'complete',
      degraded: false,
      reason: null,
    },
    kind: 'command',
    command: `cargo test -p fixture-${agentId}`,
    status: 'running',
    effective_cwd: '/repo',
    exit_code: null,
    termination_reason: null,
    output: null,
    output_truncated: false,
    polls: {
      count: 40,
      final_status: 'running',
      caller_agent_ids: [agentId],
      cross_agent: false,
    },
  }
}

class CountingHighlighter implements DiffHighlighter {
  readonly calls: DiffHighlightInput[] = []
  isReady = () => true
  eagerLanguages = () => ['rust'] as const
  highlight = async (input: DiffHighlightInput) => {
    this.calls.push(input)
    return {
      subjectKey: input.subjectKey,
      revision: input.revision,
      language: input.language,
      rows: input.rows.map((row) => ({ index: row.index, html: null })),
    }
  }
}

const outputLoaders = {
  loadOutputMetadata: async (invocationId: number) => ({
    schema_version: 1,
    runtime_id: 'runtime-browser',
    invocation_id: invocationId,
    output: {
      available: false,
      chunk_count: 0,
      size_bytes: 0,
      capture_state: 'complete',
      capture_reason: null,
      first_cursor: null,
      last_cursor: null,
    },
  }),
  loadDisplayOutputPage: async (invocationId: number) => ({
    schema_version: 1,
    runtime_id: 'runtime-browser',
    invocation_id: invocationId,
    view: 'display' as const,
    chunks: [],
    next_cursor: null,
    display_state: 'available' as const,
  }),
}

function distanceFromEnd(element: HTMLElement) {
  return element.scrollHeight - element.clientHeight - element.scrollTop
}

function firstVisibleCard(scroll: HTMLElement) {
  const scrollTop = scroll.getBoundingClientRect().top
  return Array.from(
    scroll.querySelectorAll<HTMLElement>('[data-presentation-id]'),
  )
    .map((element) => ({
      element,
      offset: element.getBoundingClientRect().top - scrollTop,
    }))
    .filter(({ element }) => element.getBoundingClientRect().bottom > scrollTop + 5)
    .sort((left, right) => left.offset - right.offset)[0]
}

let disposers: Array<() => void> = []
let containers: HTMLElement[] = []

afterEach(() => {
  for (const dispose of disposers) dispose()
  for (const container of containers) container.remove()
  disposers = []
  containers = []
})

describe('independent virtualized Agent timeline', () => {
  it('reactively replaces an in-progress generic apply_patch card with its completed file diff', async () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 10_000,
      loadHistoryPage: async () => page([]),
      ...outputLoaders,
    })
    const initial = {
      ...(generic(10, 'a111', undefined) as Extract<PresentationRecord, { kind: 'generic' }>),
      tool_name: 'apply_patch',
      status: 'in_progress',
      summary: null,
    } satisfies PresentationRecord
    const completed = {
      ...(fileChange(10, 'a111') as Extract<PresentationRecord, { kind: 'file_changes' }>),
      presentation_id: initial.presentation_id,
    } satisfies PresentationRecord
    const container = document.createElement('div')
    container.style.width = '520px'
    container.style.height = '260px'
    document.body.append(container)
    containers.push(container)
    disposers.push(render(() => <AgentTimeline controller={controller} />, container))

    controller.upsert(initial, false)
    await vi.waitFor(() =>
      expect(
        container.querySelector('[data-presentation-id="inv-10"] .card-kind')?.textContent,
      ).toBe('apply_patch'),
    )

    controller.upsert(completed, false)
    await vi.waitFor(() =>
      expect(
        container.querySelector('[data-presentation-id="inv-10"].file-change-group'),
      ).not.toBeNull(),
    )
    expect(container.querySelector('[data-presentation-id="inv-10"].timeline-card')).toBeNull()
    expect(container.textContent).toContain('file_10.rs')
  })

  it('follows append/growth at the end, pauses one column, and preserves prepend anchor', async () => {
    const historyRecords = Array.from({ length: 8 }, (_, index) =>
      generic(index + 1, 'a111', `older ${index + 1}`),
    )
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 10_000,
      loadHistoryPage: async () => page(historyRecords),
      ...outputLoaders,
    })
    const container = document.createElement('div')
    container.style.width = '340px'
    container.style.height = '360px'
    document.body.append(container)
    containers.push(container)
    disposers.push(render(() => <AgentTimeline controller={controller} />, container))

    controller.mergeRecovery(
      Array.from({ length: 60 }, (_, index) => generic(index + 20, 'a111')),
    )
    const scroll = container.querySelector<HTMLElement>('[data-agent-timeline="a111"]')!
    await vi.waitFor(() => expect(scroll.scrollHeight).toBeGreaterThan(scroll.clientHeight))
    await vi.waitFor(() => expect(distanceFromEnd(scroll)).toBeLessThanOrEqual(2))
    expect(container.querySelectorAll('.virtual-timeline-item').length).toBeLessThan(30)

    controller.upsert(generic(100, 'a111', 'new live card'))
    await vi.waitFor(() => expect(distanceFromEnd(scroll)).toBeLessThanOrEqual(2))

    controller.upsert(
      generic(
        100,
        'a111',
        'streaming command equivalent growth '.repeat(32),
      ),
    )
    await vi.waitFor(() => expect(distanceFromEnd(scroll)).toBeLessThanOrEqual(3))

    scroll.scrollTop = Math.max(0, scroll.scrollHeight - scroll.clientHeight - 180)
    scroll.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -180 }))
    scroll.dispatchEvent(new Event('scroll', { bubbles: true }))
    await vi.waitFor(() => expect(controller.following()).toBe(false))
    await vi.waitFor(() => expect(container.textContent).toContain('Scroll to bottom'))
    const pausedAnchorBefore = firstVisibleCard(scroll)
    expect(pausedAnchorBefore).toBeDefined()
    const pausedAnchorId = pausedAnchorBefore!.element.dataset.presentationId
    const pausedAnchorOffset = pausedAnchorBefore!.offset

    controller.upsert(generic(101, 'a111', 'arrived while paused'))
    await new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)))
    expect(controller.unseenCount()).toBe(1)
    await vi.waitFor(() => {
      const pausedAnchorAfter = scroll.querySelector<HTMLElement>(
        `[data-presentation-id="${pausedAnchorId}"]`,
      )
      expect(pausedAnchorAfter).not.toBeNull()
      const offsetAfter =
        pausedAnchorAfter!.getBoundingClientRect().top -
        scroll.getBoundingClientRect().top
      expect(Math.abs(offsetAfter - pausedAnchorOffset)).toBeLessThan(4)
    })
    expect(container.textContent).toContain('1 new')
    expect(
      container.querySelector('.new-activity-button .lucide-arrow-down'),
    ).not.toBeNull()

    const newButton = container.querySelector<HTMLButtonElement>('.new-activity-button')!
    newButton.click()
    await vi.waitFor(() => expect(controller.following()).toBe(true))
    await vi.waitFor(() => expect(distanceFromEnd(scroll)).toBeLessThanOrEqual(3))
    expect(controller.unseenCount()).toBe(0)
    await vi.waitFor(() =>
      expect(container.querySelector('.new-activity-button')).toBeNull(),
    )

    scroll.scrollTop = Math.max(120, scroll.scrollHeight - scroll.clientHeight - 260)
    scroll.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -260 }))
    scroll.dispatchEvent(new Event('scroll', { bubbles: true }))
    await vi.waitFor(() => expect(controller.following()).toBe(false))
    const anchorBefore = firstVisibleCard(scroll)
    expect(anchorBefore).toBeDefined()
    const anchorId = anchorBefore!.element.dataset.presentationId
    const anchorOffset = anchorBefore!.offset

    await controller.loadEarlier()
    await vi.waitFor(() =>
      expect(
        scroll.querySelector<HTMLElement>(`[data-presentation-id="${anchorId}"]`),
      ).not.toBeNull(),
    )
    const anchorAfter = scroll.querySelector<HTMLElement>(
      `[data-presentation-id="${anchorId}"]`,
    )!
    const offsetAfter =
      anchorAfter.getBoundingClientRect().top - scroll.getBoundingClientRect().top
    expect(Math.abs(offsetAfter - anchorOffset)).toBeLessThan(4)
    expect(controller.orderedIds()[0]).toBe('inv-1')
  })

  it('keeps click ownership on the same tall command after history prepend and repeated resize', async () => {
    const historyRecords = Array.from({ length: 8 }, (_, index) =>
      generic(index + 1, 'a111', `older ${index + 1}`),
    )
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 10_000,
      loadHistoryPage: async () => page(historyRecords),
      ...outputLoaders,
    })
    const recent = Array.from({ length: 6 }, (_, index) => {
      const id = 20 + index
      return {
        ...(command(id, 'a111') as Extract<PresentationRecord, { kind: 'command' }>),
        command: Array.from(
          { length: 18 },
          (_, line) => `fixture command ${id} line ${line} with enough text to wrap across the card`,
        ).join('\n'),
      } satisfies PresentationRecord
    })
    controller.mergeRecovery(recent)
    for (const record of recent) {
      controller.appendLiveOutput({
        presentationId: record.presentation_id,
        invocationId: record.primary_invocation_id,
        sequence: 1,
        text: Array.from({ length: 20 }, (_, line) => `output ${record.primary_invocation_id}:${line}\n`).join(''),
        displayState: 'available',
      })
    }

    const container = document.createElement('div')
    container.style.width = '520px'
    container.style.height = '700px'
    document.body.append(container)
    containers.push(container)
    disposers.push(render(() => <AgentTimeline controller={controller} />, container))
    const scroll = container.querySelector<HTMLElement>('[data-agent-timeline="a111"]')!
    await vi.waitFor(() => expect(scroll.scrollHeight).toBeGreaterThan(scroll.clientHeight))

    await controller.loadEarlier()
    scroll.scrollTop = scroll.scrollHeight
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))

    const targetId = recent.at(-1)!.presentation_id
    for (let iteration = 0; iteration < 8; iteration += 1) {
      const target = scroll.querySelector<HTMLElement>(
        `[data-presentation-id="${targetId}"]`,
      )!
      expect(target).not.toBeNull()
      const row = target.closest<HTMLElement>('.virtual-timeline-item')!
      expect(row.dataset.virtualKey).toBe(targetId)
      const header = target.querySelector<HTMLElement>('.command-card-header')!
      const headerBox = header.getBoundingClientRect()
      const scrollBox = scroll.getBoundingClientRect()
      const x = headerBox.left + Math.min(180, headerBox.width * 0.45)
      const y = Math.max(
        scrollBox.top + 24,
        Math.min(scrollBox.bottom - 24, headerBox.top + 24),
      )
      const hit = document.elementFromPoint(x, y)
      expect(hit?.closest('[data-presentation-id]')?.getAttribute('data-presentation-id')).toBe(
        targetId,
      )

      const before = controller.commandExpanded(targetId)()
      ;(hit as HTMLElement).click()
      await vi.waitFor(() => expect(controller.commandExpanded(targetId)()).toBe(!before))
      expect(
        row.querySelector('[data-presentation-id]')?.getAttribute('data-presentation-id'),
      ).toBe(targetId)
    }
  })

  it('keeps auto-follow independent across two visible Agent columns', async () => {
    const controllerA = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 10_000,
      loadHistoryPage: async () => page([]),
      ...outputLoaders,
    })
    const controllerB = createAgentStreamController({
      agentId: 'b222',
      attachWatermarkMs: 10_000,
      loadHistoryPage: async () => page([]),
      ...outputLoaders,
    })
    const container = document.createElement('div')
    container.style.display = 'flex'
    container.style.width = '700px'
    container.style.height = '320px'
    document.body.append(container)
    containers.push(container)
    disposers.push(
      render(
        () => (
          <>
            <div style={{ width: '340px', height: '320px' }}>
              <AgentTimeline controller={controllerA} />
            </div>
            <div style={{ width: '340px', height: '320px' }}>
              <AgentTimeline controller={controllerB} />
            </div>
          </>
        ),
        container,
      ),
    )
    controllerA.mergeRecovery(
      Array.from({ length: 50 }, (_, index) => generic(index + 20, 'a111')),
    )
    controllerB.mergeRecovery(
      Array.from({ length: 50 }, (_, index) => generic(index + 20, 'b222')),
    )
    const scrollA = container.querySelector<HTMLElement>('[data-agent-timeline="a111"]')!
    const scrollB = container.querySelector<HTMLElement>('[data-agent-timeline="b222"]')!
    await vi.waitFor(() => expect(distanceFromEnd(scrollB)).toBeLessThanOrEqual(2))

    scrollA.scrollTop = Math.max(0, scrollA.scrollHeight - scrollA.clientHeight - 160)
    scrollA.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -160 }))
    scrollA.dispatchEvent(new Event('scroll', { bubbles: true }))
    await vi.waitFor(() => expect(controllerA.following()).toBe(false))
    expect(controllerB.following()).toBe(true)

    controllerA.upsert(generic(100, 'a111'))
    controllerB.upsert(generic(100, 'b222'))
    await vi.waitFor(() => expect(controllerA.unseenCount()).toBe(1))
    await vi.waitFor(() => expect(distanceFromEnd(scrollB)).toBeLessThanOrEqual(3))
    expect(controllerB.unseenCount()).toBe(0)
  })

  it('keeps four simultaneous collapsed PTY streams bounded without output DOM work', async () => {
    const agentIds = ['a111', 'b222', 'c333', 'd444'] as const
    const controllers = agentIds.map((agentId, index) => {
      const controller = createAgentStreamController({
        agentId,
        attachWatermarkMs: 10_000,
        loadHistoryPage: async () => page([]),
        ...outputLoaders,
      })
      controller.upsert(command(300 + index, agentId), false)
      return controller
    })
    const container = document.createElement('div')
    container.style.display = 'flex'
    container.style.width = '1360px'
    container.style.height = '360px'
    document.body.append(container)
    containers.push(container)
    disposers.push(
      render(
        () => (
          <>
            {controllers.map((controller) => (
              <div style={{ width: '340px', height: '360px' }}>
                <AgentTimeline controller={controller} />
              </div>
            ))}
          </>
        ),
        container,
      ),
    )

    await vi.waitFor(() =>
      expect(container.querySelectorAll('.command-card')).toHaveLength(4),
    )
    let timelineMutations = 0
    const observer = new MutationObserver((records) => {
      timelineMutations += records.length
    })
    observer.observe(container, { childList: true, characterData: true, subtree: true })

    for (let sequence = 1; sequence <= 2_000; sequence += 1) {
      for (let index = 0; index < controllers.length; index += 1) {
        controllers[index]!.appendLiveOutput({
          presentationId: `command-${300 + index}`,
          invocationId: 300 + index,
          sequence,
          text: `${sequence}\n`,
          displayState: 'available',
        })
      }
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))
    observer.disconnect()

    expect(container.querySelector('[aria-label="Command output"]')).toBeNull()
    expect(timelineMutations).toBe(0)
    for (let index = 0; index < controllers.length; index += 1) {
      const controller = controllers[index]!
      expect(controller.lastLiveOutputSequence(`command-${300 + index}`)).toBe(2_000)
      expect(
        controller.outputState(`command-${300 + index}`, 300 + index).materialize().length,
      ).toBeLessThan(96 * 1024)
    }
  })

  it('keeps one Agent PTY firehose isolated from three unrelated visible columns', async () => {
    const agentIds = ['a111', 'b222', 'c333', 'd444'] as const
    const controllers = agentIds.map((agentId, index) => {
      const controller = createAgentStreamController({
        agentId,
        attachWatermarkMs: 10_000,
        loadHistoryPage: async () => page([]),
        ...outputLoaders,
      })
      controller.upsert(command(200 + index, agentId), false)
      return controller
    })
    const activeController = controllers[0]!
    const container = document.createElement('div')
    container.style.display = 'flex'
    container.style.width = '1360px'
    container.style.height = '360px'
    document.body.append(container)
    containers.push(container)
    disposers.push(
      render(
        () => (
          <>
            {controllers.map((controller) => (
              <div style={{ width: '340px', height: '360px' }}>
                <AgentTimeline controller={controller} />
              </div>
            ))}
          </>
        ),
        container,
      ),
    )

    await vi.waitFor(() =>
      expect(container.querySelectorAll('.command-card')).toHaveLength(4),
    )
    activeController.toggleCommandExpansion('command-200')
    expect(
      container.querySelector('[data-agent-timeline="a111"] [aria-label="Command output"]'),
    ).toBeNull()
    activeController.appendLiveOutput({
      presentationId: 'command-200',
      invocationId: 200,
      sequence: 1,
      text: '1\n',
      displayState: 'available',
    })
    await vi.waitFor(() =>
      expect(
        container.querySelector('[data-agent-timeline="a111"] [aria-label="Command output"]'),
      ).not.toBeNull(),
    )

    const unrelatedMutationCounts = new Map<string, number>()
    const observers = agentIds.slice(1).map((agentId) => {
      const timeline = container.querySelector<HTMLElement>(
        `[data-agent-timeline="${agentId}"]`,
      )!
      unrelatedMutationCounts.set(agentId, 0)
      const observer = new MutationObserver((records) => {
        unrelatedMutationCounts.set(
          agentId,
          (unrelatedMutationCounts.get(agentId) ?? 0) + records.length,
        )
      })
      observer.observe(timeline, { childList: true, characterData: true, subtree: true })
      return observer
    })

    const output = container.querySelector<HTMLPreElement>(
      '[data-agent-timeline="a111"] [aria-label="Command output"]',
    )!
    let outputMutations = 0
    const outputObserver = new MutationObserver((records) => {
      outputMutations += records.length
    })
    outputObserver.observe(output, { childList: true, characterData: true, subtree: true })

    for (let sequence = 2; sequence <= 2_000; sequence += 1) {
      activeController.appendLiveOutput({
        presentationId: 'command-200',
        invocationId: 200,
        sequence,
        text: `${sequence}\n`,
        displayState: 'available',
      })
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))

    outputObserver.disconnect()
    for (const observer of observers) observer.disconnect()
    expect(output.textContent).toContain('2000\n')
    expect(outputMutations).toBeLessThanOrEqual(2)
    expect([...unrelatedMutationCounts.values()]).toEqual([0, 0, 0])
  })

  it('highlights only expanded file diffs that the Agent virtualizer actually mounts', async () => {
    const controller = createAgentStreamController({
      agentId: 'a111',
      attachWatermarkMs: 20_000,
      loadHistoryPage: async () => page([]),
      ...outputLoaders,
    })
    const highlighter = new CountingHighlighter()
    const container = document.createElement('div')
    container.style.width = '340px'
    container.style.height = '360px'
    document.body.append(container)
    containers.push(container)
    disposers.push(
      render(
        () => <AgentTimeline controller={controller} diffHighlighter={highlighter} />,
        container,
      ),
    )

    controller.mergeRecovery(
      Array.from({ length: 100 }, (_, index) => fileChange(index + 1, 'a111')),
    )
    const scroll = container.querySelector<HTMLElement>('[data-agent-timeline="a111"]')!
    await vi.waitFor(() => expect(highlighter.calls.length).toBeGreaterThan(0))
    await vi.waitFor(() => expect(scroll.scrollHeight).toBeGreaterThan(scroll.clientHeight))
    expect(container.querySelectorAll('.virtual-timeline-item').length).toBeLessThan(30)
    expect(highlighter.calls.length).toBeLessThan(30)
    expect(highlighter.calls.length).toBeLessThan(controller.orderedIds().length)
  })
})
