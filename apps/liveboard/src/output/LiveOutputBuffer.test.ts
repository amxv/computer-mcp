import { describe, expect, it } from 'vitest'

import { LiveOutputBuffer } from './LiveOutputBuffer'

describe('LiveOutputBuffer', () => {
  it('dedupes sequences, detects gaps, and preserves ordered durable overlap', () => {
    const buffer = new LiveOutputBuffer(1024)
    expect(buffer.append(1, 'one\n')).toEqual({
      added: true,
      duplicate: false,
      sequenceGap: false,
    })
    expect(buffer.append(3, 'three\n').sequenceGap).toBe(true)
    expect(buffer.append(3, 'duplicate').duplicate).toBe(true)
    buffer.mergeDurable([
      { sequence: 2, text: 'two\n' },
      { sequence: 3, text: 'three\n' },
    ])
    expect(buffer.materialize()).toBe('one\ntwo\nthree\n')
    expect(buffer.hasInternalSequenceGap()).toBe(false)
    expect(buffer.lastRetainedSequence()).toBe(3)
  })

  it('keeps bounded UTF-8 tails without splitting a multi-byte scalar', () => {
    const buffer = new LiveOutputBuffer(48)
    for (let sequence = 1; sequence <= 20; sequence += 1) {
      buffer.append(sequence, `🙂-${sequence}-output\n`)
    }
    expect(buffer.retainedBytes()).toBeLessThanOrEqual(48)
    expect(buffer.hasDroppedPrefix()).toBe(true)
    expect(buffer.materialize()).not.toContain('�')

    const tiny = new LiveOutputBuffer(7)
    tiny.append(1, 'prefix🙂suffix')
    expect(tiny.retainedBytes()).toBeLessThanOrEqual(7)
    expect(tiny.materialize()).not.toContain('�')
  })

  it('keeps empty display chunks as sequence evidence', () => {
    const buffer = new LiveOutputBuffer()
    buffer.append(1, '')
    buffer.append(2, 'visible')
    expect(buffer.retainedChunkCount()).toBe(2)
    expect(buffer.materialize()).toBe('visible')
    expect(buffer.hasInternalSequenceGap()).toBe(false)
  })

  it('bounds zero-text control chunks by count as well as bytes', () => {
    const buffer = new LiveOutputBuffer(1024, 8)
    for (let sequence = 1; sequence <= 100; sequence += 1) {
      buffer.append(sequence, '')
    }
    expect(buffer.retainedBytes()).toBe(0)
    expect(buffer.retainedChunkCount()).toBe(8)
    expect(buffer.firstRetainedSequence()).toBe(93)
    expect(buffer.lastRetainedSequence()).toBe(100)
    expect(buffer.hasDroppedPrefix()).toBe(true)
    expect(buffer.hasInternalSequenceGap()).toBe(false)
  })
})
