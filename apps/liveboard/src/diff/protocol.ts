import type { DiffSyntaxLanguage } from './language'

export interface DiffHighlightRowInput {
  index: number
  text: string
}

export interface DiffHighlightRowResult {
  index: number
  html: string | null
}

export interface DiffHighlightInput {
  subjectKey: string
  revision: string
  language: DiffSyntaxLanguage
  rows: DiffHighlightRowInput[]
}

export interface DiffHighlightResult {
  subjectKey: string
  revision: string
  language: DiffSyntaxLanguage
  rows: DiffHighlightRowResult[]
}

export interface HighlightRequestMessage extends DiffHighlightInput {
  type: 'highlight'
  requestId: number
}

export interface HighlightReadyProbeMessage {
  type: 'ready_probe'
}

export type HighlightWorkerRequest = HighlightRequestMessage | HighlightReadyProbeMessage

export interface HighlightResultMessage extends DiffHighlightResult {
  type: 'highlight_result'
  requestId: number
}

export interface HighlightReadyMessage {
  type: 'ready'
  languages: readonly DiffSyntaxLanguage[]
}

export interface HighlightErrorMessage {
  type: 'highlight_error'
  requestId: number
  message: string
}

export type HighlightWorkerResponse =
  | HighlightReadyMessage
  | HighlightResultMessage
  | HighlightErrorMessage
