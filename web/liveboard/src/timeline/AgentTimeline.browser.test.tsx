import { render } from 'solid-js/web'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ApiTimelinePage, PresentationRecord } from '../api/client'
import '../styles.css'
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
    presentation_version: 2,
    runtime_id: 'runtime-browser',
    records,
    has_more: false,
    next_cursor: null,
  }
}

const outputLoaders = {
  loadOutputMetadata: async () => {
    throw new Error('output metadata not expected in timeline geometry test')
  },
  loadDisplayOutputPage: async () => {
    throw new Error('output page not expected in timeline geometry test')
  },
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
    expect(container.textContent).toContain('↓ 1 new')

    const newButton = container.querySelector<HTMLButtonElement>('.new-activity-button')!
    newButton.click()
    await vi.waitFor(() => expect(controller.following()).toBe(true))
    await vi.waitFor(() => expect(distanceFromEnd(scroll)).toBeLessThanOrEqual(3))
    expect(controller.unseenCount()).toBe(0)

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
})
