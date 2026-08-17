import { createVirtualizer } from '@tanstack/solid-virtual'
import { For, Show, createEffect, createSignal, onCleanup, onMount } from 'solid-js'

import type { AgentStreamController } from '../streams/AgentStreamController'
import {
  PLAIN_DIFF_HIGHLIGHTER,
  type DiffHighlighter,
} from '../diff/HighlightWorkerClient'
import { TimelineCard } from './TimelineCard'

const END_THRESHOLD_PX = 40
const HISTORY_TRIGGER_PX = 72

export function AgentTimeline(props: {
  controller: AgentStreamController
  runtimeId?: string
  nowMs?: number
  commandOutputsExpanded?: boolean
  diffsExpanded?: boolean
  showRawButton?: boolean
  diffHighlighter?: DiffHighlighter
}) {
  let scrollElement: HTMLDivElement | undefined
  let userScrollIntentUntil = 0
  let lastMeasuredTotalSize = 0
  let followResizeFrame: number | undefined
  let followSettleFrame: number | undefined
  let returnToLiveFrame: number | undefined
  const [atLiveEnd, setAtLiveEnd] = createSignal(true)

  const distanceFromLiveEnd = () => {
    if (!scrollElement) return 0
    return Math.max(
      0,
      scrollElement.scrollHeight - scrollElement.clientHeight - scrollElement.scrollTop,
    )
  }

  const syncLiveEndState = () => {
    const atEnd = distanceFromLiveEnd() <= END_THRESHOLD_PX
    setAtLiveEnd(atEnd)
    if (atEnd) {
      props.controller.setFollowing(true)
    } else if (hasUserScrollIntent()) {
      props.controller.setFollowing(false)
    }
    return atEnd
  }

  const noteUserScrollIntent = (pauseFollowing = false) => {
    if (followResizeFrame !== undefined) {
      cancelAnimationFrame(followResizeFrame)
      followResizeFrame = undefined
    }
    if (followSettleFrame !== undefined) {
      cancelAnimationFrame(followSettleFrame)
      followSettleFrame = undefined
    }
    if (returnToLiveFrame !== undefined) {
      cancelAnimationFrame(returnToLiveFrame)
      returnToLiveFrame = undefined
    }
    userScrollIntentUntil = performance.now() + 250
    if (pauseFollowing) props.controller.setFollowing(false)
    if (distanceFromLiveEnd() > END_THRESHOLD_PX) props.controller.setFollowing(false)
  }

  const hasUserScrollIntent = () => performance.now() <= userScrollIntentUntil

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    get count() {
      return props.controller.orderedIds().length
    },
    getScrollElement: () => scrollElement ?? null,
    estimateSize: () => 88,
    getItemKey: (index) => props.controller.orderedIds()[index] ?? `missing-${index}`,
    overscan: 3,
    gap: 8,
    paddingStart: 46,
    paddingEnd: 14,
    scrollPaddingEnd: 8,
    anchorTo: 'end',
    followOnAppend: false,
    scrollEndThreshold: END_THRESHOLD_PX,
    measureElement: (element) => element.offsetHeight,
    useAnimationFrameWithResizeObserver: true,
    onChange: (instance) => {
      const totalSize = instance.getTotalSize()
      const grew = totalSize > lastMeasuredTotalSize + 0.5
      const changed = Math.abs(totalSize - lastMeasuredTotalSize) > 0.5
      lastMeasuredTotalSize = totalSize
      if (!changed || followResizeFrame !== undefined) return
      followResizeFrame = requestAnimationFrame(() => {
        followResizeFrame = undefined
        if (!scrollElement?.isConnected) return
        if (grew && props.controller.following() && !hasUserScrollIntent()) {
          scrollElement.scrollTop = scrollElement.scrollHeight
          if (followSettleFrame !== undefined) cancelAnimationFrame(followSettleFrame)
          followSettleFrame = requestAnimationFrame(() => {
            followSettleFrame = undefined
            if (
              scrollElement?.isConnected &&
              props.controller.following() &&
              !hasUserScrollIntent()
            ) {
              scrollElement.scrollTop = scrollElement.scrollHeight
              syncLiveEndState()
            }
          })
        }
        syncLiveEndState()
      })
    },
  })
  let previousCount = 0

  onCleanup(() => {
    if (followResizeFrame !== undefined) cancelAnimationFrame(followResizeFrame)
    if (followSettleFrame !== undefined) cancelAnimationFrame(followSettleFrame)
    if (returnToLiveFrame !== undefined) cancelAnimationFrame(returnToLiveFrame)
  })

  onMount(() => queueMicrotask(syncLiveEndState))

  createEffect(() => {
    props.controller.setCommandExpansionDefault(props.commandOutputsExpanded ?? false)
  })

  createEffect(() => {
    props.controller.setDiffExpansionDefault(props.diffsExpanded ?? true)
  })

  const scrollToLiveEnd = (behavior: ScrollBehavior = 'auto') => {
    if (scrollElement) {
      if (behavior === 'smooth') {
        scrollElement.scrollTo({
          top: scrollElement.scrollHeight,
          behavior,
        })
      } else {
        scrollElement.scrollTop = scrollElement.scrollHeight
      }
    }
  }

  createEffect(() => {
    const count = props.controller.orderedIds().length
    const shouldFollowAppend = count > previousCount && props.controller.following()
    previousCount = count
    if (!shouldFollowAppend) return
    requestAnimationFrame(() => {
      if (props.controller.following()) {
        scrollToLiveEnd()
      }
    })
  })

  const loadEarlier = () => void props.controller.loadEarlier()

  const onScroll = () => {
    const userScroll = hasUserScrollIntent()
    syncLiveEndState()
    if (
      scrollElement &&
      scrollElement.scrollTop <= HISTORY_TRIGGER_PX &&
      userScroll &&
      props.controller.historyActivated() &&
      !props.controller.historyLoading() &&
      !props.controller.historyExhausted()
    ) {
      loadEarlier()
    }
  }

  const returnToLive = () => {
    if (!scrollElement) return
    if (returnToLiveFrame !== undefined) cancelAnimationFrame(returnToLiveFrame)
    const startTop = scrollElement.scrollTop
    const targetTop = Math.max(0, scrollElement.scrollHeight - scrollElement.clientHeight)
    const distance = targetTop - startTop
    if (
      Math.abs(distance) < 2 ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      scrollElement.scrollTop = targetTop
      syncLiveEndState()
      return
    }
    const startedAt = performance.now()
    const durationMs = 180
    const tick = (now: number) => {
      if (!scrollElement?.isConnected) {
        returnToLiveFrame = undefined
        return
      }
      const progress = Math.min(1, (now - startedAt) / durationMs)
      const eased = 1 - (1 - progress) ** 3
      scrollElement.scrollTop = startTop + distance * eased
      if (progress < 1) {
        returnToLiveFrame = requestAnimationFrame(tick)
      } else {
        returnToLiveFrame = undefined
        scrollElement.scrollTop = scrollElement.scrollHeight
        syncLiveEndState()
      }
    }
    returnToLiveFrame = requestAnimationFrame(tick)
  }

  return (
    <div class="agent-timeline-shell">
      <div
        ref={scrollElement}
        class="agent-timeline-scroll"
        data-agent-timeline={props.controller.agentId}
        aria-label={`Agent ${props.controller.agentId} timeline`}
        tabIndex={0}
        onWheel={(event) => noteUserScrollIntent(event.deltaY < 0)}
        onTouchStart={() => noteUserScrollIntent()}
        onPointerDown={() => noteUserScrollIntent()}
        onKeyDown={(event) => {
          if (
            event.key === 'ArrowUp' ||
            event.key === 'ArrowDown' ||
            event.key === 'PageUp' ||
            event.key === 'PageDown' ||
            event.key === 'Home' ||
            event.key === 'End' ||
            event.key === ' '
          ) {
            noteUserScrollIntent(
              event.key === 'ArrowUp' || event.key === 'PageUp' || event.key === 'Home',
            )
          }
        }}
        onScroll={onScroll}
      >
        <div
          class="agent-timeline-canvas"
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          <div class="history-control">
            <Show
              when={!props.controller.historyExhausted()}
              fallback={<span>Start of retained activity</span>}
            >
              <button
                type="button"
                class="history-button"
                disabled={props.controller.historyLoading()}
                onClick={loadEarlier}
              >
                {props.controller.historyLoading() ? 'Loading…' : 'Load earlier activity'}
              </button>
            </Show>
          </div>
          <For each={virtualizer.getVirtualItems()}>
            {(item) => {
              const presentationId = () => String(item.key)
              let rowElement: HTMLDivElement | undefined
              return (
                <div
                  ref={(element) => {
                    rowElement = element
                    element.setAttribute('data-index', String(item.index))
                    queueMicrotask(() => {
                      if (element.isConnected) virtualizer.measureElement(element)
                    })
                  }}
                  data-index={item.index}
                  data-virtual-key={presentationId()}
                  class="virtual-timeline-item"
                  style={{ transform: `translateY(${item.start}px)` }}
                  onClick={() => {
                    const element = rowElement
                    if (!element) return
                    queueMicrotask(() => {
                      if (element.isConnected) virtualizer.measureElement(element)
                    })
                  }}
                >
                  <Show when={presentationId()} keyed>
                    {(stablePresentationId) => {
                      const record = () => props.controller.record(stablePresentationId)
                      return (
                        <Show when={record()}>
                          {(value) => (
                            <TimelineCard
                              record={value()}
                              controller={props.controller}
                        runtimeId={props.runtimeId ?? 'runtime-test'}
                        nowMs={props.nowMs ?? Date.now()}
                        showRawButton={props.showRawButton ?? false}
                        diffHighlighter={props.diffHighlighter ?? PLAIN_DIFF_HIGHLIGHTER}
                      />
                          )}
                        </Show>
                      )
                    }}
                  </Show>
                </div>
              )
            }}
          </For>
        </div>
      </div>
      <Show when={!atLiveEnd() || props.controller.unseenCount() > 0}>
        <button type="button" class="new-activity-button" onClick={returnToLive}>
          {props.controller.unseenCount() > 0
            ? `↓ ${props.controller.unseenCount()} new`
            : 'Scroll to bottom'}
        </button>
      </Show>
      <Show when={props.controller.historyError()}>
        {(error) => <span class="timeline-error">{error()}</span>}
      </Show>
    </div>
  )
}
