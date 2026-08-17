export interface DisplayOutputChunk {
  sequence: number
  text: string
}

export interface AppendOutputResult {
  added: boolean
  duplicate: boolean
  sequenceGap: boolean
}

const encoder = new TextEncoder()
const decoder = new TextDecoder()

function utf8ByteLength(text: string) {
  return encoder.encode(text).byteLength
}

function utf8Suffix(text: string, maximumBytes: number): string {
  const encoded = encoder.encode(text)
  if (encoded.byteLength <= maximumBytes) return text
  let start = encoded.byteLength - maximumBytes
  while (start < encoded.byteLength && (encoded[start]! & 0xc0) === 0x80) {
    start += 1
  }
  return decoder.decode(encoded.subarray(start))
}

export class LiveOutputBuffer {
  readonly maximumBytes: number
  readonly maximumChunks: number
  private chunks: DisplayOutputChunk[] = []
  private bytes = 0
  private droppedPrefix = false
  private lastSequence: number | undefined

  constructor(maximumBytes = 96 * 1024, maximumChunks = 2_048) {
    this.maximumBytes = maximumBytes
    this.maximumChunks = Math.max(1, maximumChunks)
  }

  append(sequence: number, text: string): AppendOutputResult {
    const existing = this.chunks.find((chunk) => chunk.sequence === sequence)
    if (existing) {
      return { added: false, duplicate: true, sequenceGap: false }
    }
    const sequenceGap =
      this.lastSequence !== undefined && sequence > this.lastSequence + 1
    this.insertChunk({ sequence, text })
    this.lastSequence = Math.max(this.lastSequence ?? sequence, sequence)
    this.trim()
    return { added: true, duplicate: false, sequenceGap }
  }

  mergeDurable(
    chunks: readonly DisplayOutputChunk[],
    options: { dropBeforeSequence?: number } = {},
  ) {
    if (options.dropBeforeSequence !== undefined) {
      const remaining = this.chunks.filter(
        (chunk) => chunk.sequence >= options.dropBeforeSequence!,
      )
      if (remaining.length !== this.chunks.length) this.droppedPrefix = true
      this.chunks = remaining
      this.recount()
    }
    for (const chunk of chunks) {
      if (!this.chunks.some((existing) => existing.sequence === chunk.sequence)) {
        this.insertChunk(chunk)
      }
    }
    this.lastSequence = this.chunks.at(-1)?.sequence
    this.trim()
  }

  materialize() {
    return this.chunks.map((chunk) => chunk.text).join('')
  }

  clear() {
    this.chunks = []
    this.bytes = 0
    this.lastSequence = undefined
    this.droppedPrefix = false
  }

  hasDroppedPrefix() {
    return this.droppedPrefix
  }

  markDroppedPrefix() {
    this.droppedPrefix = true
  }

  firstRetainedSequence() {
    return this.chunks[0]?.sequence
  }

  lastRetainedSequence() {
    return this.lastSequence
  }

  retainedBytes() {
    return this.bytes
  }

  retainedChunkCount() {
    return this.chunks.length
  }

  hasInternalSequenceGap() {
    for (let index = 1; index < this.chunks.length; index += 1) {
      if (this.chunks[index]!.sequence !== this.chunks[index - 1]!.sequence + 1) {
        return true
      }
    }
    return false
  }

  private insertChunk(chunk: DisplayOutputChunk) {
    const normalized = {
      sequence: chunk.sequence,
      text: utf8Suffix(chunk.text, this.maximumBytes),
    }
    const index = this.chunks.findIndex(
      (existing) => existing.sequence > normalized.sequence,
    )
    if (index < 0) this.chunks.push(normalized)
    else this.chunks.splice(index, 0, normalized)
    this.bytes += utf8ByteLength(normalized.text)
  }

  private trim() {
    while (
      this.chunks.length > 1 &&
      (this.bytes > this.maximumBytes || this.chunks.length > this.maximumChunks)
    ) {
      const removed = this.chunks.shift()!
      this.bytes -= utf8ByteLength(removed.text)
      this.droppedPrefix = true
    }
    if (this.chunks.length === 1 && this.bytes > this.maximumBytes) {
      const only = this.chunks[0]!
      only.text = utf8Suffix(only.text, this.maximumBytes)
      this.bytes = utf8ByteLength(only.text)
      this.droppedPrefix = true
    }
    this.lastSequence = this.chunks.at(-1)?.sequence
  }

  private recount() {
    this.bytes = this.chunks.reduce(
      (total, chunk) => total + utf8ByteLength(chunk.text),
      0,
    )
    this.lastSequence = this.chunks.at(-1)?.sequence
  }
}
