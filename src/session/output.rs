use std::io::Read as _;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::Notify;

use crate::invocation::InvocationContext;

use super::{SessionOutputChunk, SessionOutputObserver};

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
            loop {
                let read = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let chunk = String::from_utf8_lossy(&buf[..read]);
                observer.observe_output(SessionOutputChunk {
                    internal_session_id,
                    session_handle: session_handle.clone(),
                    invocation: invocation.clone(),
                    text: chunk.to_string(),
                });
                output.append(&chunk);
            }
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
