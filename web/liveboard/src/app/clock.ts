import { createSignal, onCleanup, type Accessor } from 'solid-js'

export function createCoarseClock(intervalMs = 1_000): Accessor<number> {
  const [now, setNow] = createSignal(Date.now())
  let interval: ReturnType<typeof setInterval> | undefined

  const stop = () => {
    if (interval !== undefined) {
      clearInterval(interval)
      interval = undefined
    }
  }
  const syncVisibility = () => {
    stop()
    setNow(Date.now())
    if (!document.hidden) {
      interval = setInterval(() => setNow(Date.now()), intervalMs)
    }
  }

  document.addEventListener('visibilitychange', syncVisibility)
  syncVisibility()
  onCleanup(() => {
    stop()
    document.removeEventListener('visibilitychange', syncVisibility)
  })
  return now
}
