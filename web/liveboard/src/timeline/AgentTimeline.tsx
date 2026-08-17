import { createVirtualizer } from '@tanstack/solid-virtual'
import { For, Show, createEffect, onCleanup } from 'solid-js'

import type { AgentStreamController } from '../streams/AgentStreamController'
import { TimelineCard } from './TimelineCard'

const END_THRESHOLD_PX = 40
const HISTORY_TRIGGER_PX = 72

export function AgentTimeline(props: {
  controller: AgentStreamController
  runtimeId?: string
  nowMs?: number
  commandOutputsExpanded?: boolean
}) {
  let scrollElement: HTMLDivElement | undefined
  let userScrollIntentUntil = 0
  let lastMeasuredTotalSize = 0
  let followResizeFrame: number | undefined

  const noteUserScrollIntent = () => {
    userScrollIntentUntil = performance.now() + 250
  }

  const hasUserScrollIntent = () => performance.now() <= userScrollIntentUntil

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    get count() {
      return props.controller.orderedIds().length
    },
    getScrollElement: () => scrollElement ?? null,
    estimateSize: () => 88,
    getItemKey: (index) => props.controller.orderedIds()[index] ?? `missing-${index}`,
    overscan: 5,
    gap: 8,
    paddingStart: 46,
    paddingEnd: 14,
    scrollPaddingEnd: 8,
    anchorTo: 'end',
    followOnAppend: false,
    scrollEndThreshold: END_THRESHOLD_PX,
    useAnimationFrameWithResizeObserver: true,
    onChange: (instance) => {
      const totalSize = instance.getTotalSize()
      const grew = totalSize > lastMeasuredTotalSize + 0.5
      lastMeasuredTotalSize = totalSize
      if (
        !grew ||
        !props.controller.following() ||
        hasUserScrollIntent() ||
        followResizeFrame !== undefined
      ) {
        return
      }
      followResizeFrame = requestAnimationFrame(() => {
        followResizeFrame = undefined
        if (
          scrollElement?.isConnected &&
          props.controller.following() &&
          !hasUserScrollIntent()
        ) {
          scrollElement.scrollTop = scrollElement.scrollHeight
        }
      })
    },
  })
  let previousCount = 0

  onCleanup(() => {
    if (followResizeFrame !== undefined) cancelAnimationFrame(followResizeFrame)
  })

  createEffect(() => {
    props.controller.setCommandExpansionDefault(props.commandOutputsExpanded ?? false)
  })

  const scrollToLiveEnd = () => {
    if (scrollElement) {
      scrollElement.scrollTop = scrollElement.scrollHeight
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
    const atEnd = virtualizer.isAtEnd(END_THRESHOLD_PX)
    const userScroll = hasUserScrollIntent()
    if (atEnd) {
      props.controller.setFollowing(true)
    } else if (userScroll) {
      props.controller.setFollowing(false)
    }
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
    props.controller.returnToLive()
    scrollToLiveEnd()
  }

  return (
    <div class="agent-timeline-shell">
      <div
        ref={scrollElement}
        class="agent-timeline-scroll"
        data-agent-timeline={props.controller.agentId}
        aria-label={`Agent ${props.controller.agentId} timeline`}
        tabIndex={0}
        onWheel={noteUserScrollIntent}
        onTouchStart={noteUserScrollIntent}
        onPointerDown={noteUserScrollIntent}
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
            noteUserScrollIntent()
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
              const presentationId = () => props.controller.orderedIds()[item.index]
              const record = () => {
                const id = presentationId()
                return id ? props.controller.record(id) : undefined
              }
              return (
                <Show when={record()}>
                  {(value) => (
                    <div
                      ref={(element) => {
                        element.setAttribute('data-index', String(item.index))
                        queueMicrotask(() => {
                          if (element.isConnected) virtualizer.measureElement(element)
                        })
                      }}
                      data-index={item.index}
                      class="virtual-timeline-item"
                      style={{ transform: `translateY(${item.start}px)` }}
                    >
                      <TimelineCard
                        record={value()}
                        controller={props.controller}
                        runtimeId={props.runtimeId ?? 'runtime-test'}
                        nowMs={props.nowMs ?? Date.now()}
                      />
                    </div>
                  )}
                </Show>
              )
            }}
          </For>
        </div>
      </div>
      <Show when={!props.controller.following() && props.controller.unseenCount() > 0}>
        <button type="button" class="new-activity-button" onClick={returnToLive}>
          ↓ {props.controller.unseenCount()} new
        </button>
      </Show>
      <Show when={props.controller.historyError()}>
        {(error) => <span class="timeline-error">{error()}</span>}
      </Show>
    </div>
  )
}
