/// <reference lib="webworker" />

import { DiffHighlightEngine } from './highlightEngine'
import type {
  HighlightErrorMessage,
  HighlightReadyMessage,
  HighlightResultMessage,
  HighlightWorkerRequest,
} from './protocol'

const engine = new DiffHighlightEngine()

const ready: HighlightReadyMessage = {
  type: 'ready',
  languages: engine.languages(),
}
self.postMessage(ready)

self.onmessage = (event: MessageEvent<HighlightWorkerRequest>) => {
  const message = event.data
  if (message.type === 'ready_probe') {
    self.postMessage(ready)
    return
  }
  if (message.type !== 'highlight') return
  try {
    const response: HighlightResultMessage = {
      type: 'highlight_result',
      requestId: message.requestId,
      subjectKey: message.subjectKey,
      revision: message.revision,
      language: message.language,
      rows: engine.highlightBatch(message.language, message.rows),
    }
    self.postMessage(response)
  } catch (error) {
    const response: HighlightErrorMessage = {
      type: 'highlight_error',
      requestId: message.requestId,
      message: error instanceof Error ? error.message : String(error),
    }
    self.postMessage(response)
  }
}
