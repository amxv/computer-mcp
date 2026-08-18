import { createEffect, createSignal, onCleanup, type Accessor } from 'solid-js'

const DRAWER_TRANSITION_MS = 130

export function createDrawerPresence(open: Accessor<boolean>) {
  const [present, setPresent] = createSignal(open())
  const [visible, setVisible] = createSignal(false)
  let openFrame: number | undefined
  let closeTimeout: ReturnType<typeof setTimeout> | undefined

  createEffect(() => {
    if (open()) {
      if (closeTimeout !== undefined) {
        clearTimeout(closeTimeout)
        closeTimeout = undefined
      }
      setPresent(true)
      if (openFrame !== undefined) cancelAnimationFrame(openFrame)
      openFrame = requestAnimationFrame(() => {
        openFrame = undefined
        if (open()) setVisible(true)
      })
      return
    }

    if (openFrame !== undefined) {
      cancelAnimationFrame(openFrame)
      openFrame = undefined
    }
    setVisible(false)
    if (present()) {
      if (closeTimeout !== undefined) clearTimeout(closeTimeout)
      closeTimeout = setTimeout(() => {
        closeTimeout = undefined
        if (!open()) setPresent(false)
      }, DRAWER_TRANSITION_MS + 20)
    }
  })

  onCleanup(() => {
    if (openFrame !== undefined) cancelAnimationFrame(openFrame)
    if (closeTimeout !== undefined) clearTimeout(closeTimeout)
  })

  return { present, visible }
}
