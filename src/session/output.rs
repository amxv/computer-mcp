use std::io::Read as _;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::Notify;

use crate::invocation::InvocationContext;

use super::{SessionOutputChunk, SessionOutputCompletion, SessionOutputObserver};

#[derive(Debug, Default)]
struct StreamingUtf8Decoder {
    pending: Vec<u8>,
}

impl StreamingUtf8Decoder {
    fn push(&mut self, bytes: &[u8]) -> String {
        let mut joined = Vec::with_capacity(self.pending.len().saturating_add(bytes.len()));
        joined.append(&mut self.pending);
        joined.extend_from_slice(bytes);

        let mut output = String::with_capacity(joined.len());
        let mut remaining = joined.as_slice();
        while !remaining.is_empty() {
            match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    output.push_str(valid);
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        output.push_str(
                            std::str::from_utf8(&remaining[..valid_up_to])
                                .expect("UTF-8 validator reported an invalid valid prefix"),
                        );
                    }
                    match error.error_len() {
                        Some(invalid_len) => {
                            output.push('\u{fffd}');
                            remaining = &remaining[valid_up_to + invalid_len..];
                        }
                        None => {
                            self.pending.extend_from_slice(&remaining[valid_up_to..]);
                            debug_assert!(self.pending.len() <= 3);
                            break;
                        }
                    }
                }
            }
        }
        output
    }

    fn finish(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        self.pending.clear();
        "\u{fffd}".to_string()
    }
}

#[derive(Debug)]
struct OutputState {
    text: String,
    dropped_bytes: usize,
}

#[derive(Debug)]
pub(super) struct OutputBuffer {
    inner: StdMutex<OutputState>,
    max_chars: usize,
    reader_done: AtomicBool,
    reader_done_notify: Notify,
}

impl OutputBuffer {
    pub(super) fn new(max_chars: usize) -> Self {
        Self {
            inner: StdMutex::new(OutputState {
                text: String::new(),
                dropped_bytes: 0,
            }),
            max_chars,
            reader_done: AtomicBool::new(false),
            reader_done_notify: Notify::new(),
        }
    }

    pub(super) fn append(&self, chunk: &str) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.text.push_str(chunk);

        if state.text.len() <= self.max_chars {
            return;
        }

        let overflow = state.text.len() - self.max_chars;
        let cut = next_char_boundary(&state.text, overflow);
        state.text.drain(..cut);
        state.dropped_bytes += cut;
    }

    pub(super) fn snapshot(&self) -> String {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.dropped_bytes == 0 {
            return state.text.clone();
        }

        format!(
            "[... {} bytes truncated ...]\n{}",
            state.dropped_bytes, state.text
        )
    }

    pub(super) fn mark_reader_done(&self) {
        self.reader_done.store(true, Ordering::Release);
        self.reader_done_notify.notify_waiters();
    }

    pub(super) async fn wait_for_reader_done(&self, timeout: Duration) {
        if self.reader_done.load(Ordering::Acquire) {
            return;
        }

        let notified = self.reader_done_notify.notified();
        if self.reader_done.load(Ordering::Acquire) {
            return;
        }

        let _ = tokio::time::timeout(timeout, notified).await;
    }
}

pub(super) fn spawn_reader(
    mut reader: std::fs::File,
    output: Arc<OutputBuffer>,
    observer: Arc<dyn SessionOutputObserver>,
    internal_session_id: u64,
    session_handle: Arc<str>,
    invocation: InvocationContext,
) -> Result<()> {
    std::thread::Builder::new()
        .name("zodex-pty-reader".to_string())
        .spawn(move || {
            let mut buf = [0_u8; 8192];
            let mut sequence = 0_u64;
            let mut decoder = StreamingUtf8Decoder::default();
            loop {
                let read = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let chunk = decoder.push(&buf[..read]);
                observer.observe_output(SessionOutputChunk {
                    internal_session_id,
                    session_handle: session_handle.clone(),
                    invocation: invocation.clone(),
                    sequence,
                    text: chunk.clone(),
                });
                sequence = sequence.saturating_add(1);
                output.append(&chunk);
            }
            let final_chunk = decoder.finish();
            if !final_chunk.is_empty() {
                observer.observe_output(SessionOutputChunk {
                    internal_session_id,
                    session_handle: session_handle.clone(),
                    invocation: invocation.clone(),
                    sequence,
                    text: final_chunk.clone(),
                });
                output.append(&final_chunk);
            }
            observer.observe_output_complete(SessionOutputCompletion {
                internal_session_id,
                session_handle,
                invocation,
            });
            output.mark_reader_done();
        })
        .context("failed to start PTY output reader thread")?;
    Ok(())
}

pub(super) fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }

    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::StreamingUtf8Decoder;

    #[test]
    fn streaming_utf8_decoder_preserves_valid_scalars_across_every_byte_boundary() {
        let text = "ascii-é-漢-🙂-done";
        let bytes = text.as_bytes();
        for split in 0..=bytes.len() {
            let mut decoder = StreamingUtf8Decoder::default();
            let decoded = [
                decoder.push(&bytes[..split]),
                decoder.push(&bytes[split..]),
                decoder.finish(),
            ]
            .concat();
            assert_eq!(decoded, text, "split at byte {split}");
        }

        let mut decoder = StreamingUtf8Decoder::default();
        let mut decoded = String::new();
        for byte in bytes {
            decoded.push_str(&decoder.push(std::slice::from_ref(byte)));
        }
        decoded.push_str(&decoder.finish());
        assert_eq!(decoded, text);
    }

    #[test]
    fn streaming_utf8_decoder_keeps_lossy_invalid_and_incomplete_eof_behavior() {
        let mut invalid = StreamingUtf8Decoder::default();
        let decoded = [invalid.push(b"a\xffb"), invalid.finish()].concat();
        assert_eq!(decoded, "a\u{fffd}b");

        let mut incomplete = StreamingUtf8Decoder::default();
        assert_eq!(incomplete.push(b"a\xf0\x9f"), "a");
        assert_eq!(incomplete.finish(), "\u{fffd}");
    }
}
