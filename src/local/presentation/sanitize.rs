use std::io::Write as _;
use std::sync::{Arc, Mutex};

const PREVIEW_ELLIPSIS: &str = "…";
const PREVIEW_INPUT_SCAN_MULTIPLIER: usize = 8;
const PREVIEW_INPUT_SCAN_SLOP: usize = 256;

pub(crate) fn sanitize_display_text(input: &str) -> String {
    let stripped = strip_ansi_escapes::strip_str(input);
    sanitize_stripped_text(&stripped)
}

fn sanitize_stripped_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\t' => output.push(ch),
            value if value.is_control() || is_bidi_control(value) => {
                output.push_str(&format!("\\u{{{:x}}}", value as u32));
            }
            value => output.push(value),
        }
    }
    output
}

#[derive(Default)]
struct SharedDisplayState {
    capture: bool,
    output: Vec<u8>,
}

#[derive(Clone, Default)]
struct SharedDisplayBuffer(Arc<Mutex<SharedDisplayState>>);

impl std::io::Write for SharedDisplayBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut state = self.0.lock().expect("display sanitizer buffer poisoned");
        if state.capture {
            state.output.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Stateful display sanitizer for a sequence of PTY chunks.
///
/// ANSI/OSC parser state is deliberately retained across `push` calls because
/// PTY chunk boundaries are arbitrary. Each returned string contains only the
/// display delta produced by that one raw chunk after the parser has consumed
/// all preceding chunks.
pub(crate) struct StreamingDisplaySanitizer {
    writer: strip_ansi_escapes::Writer<SharedDisplayBuffer>,
    state: Arc<Mutex<SharedDisplayState>>,
}

impl StreamingDisplaySanitizer {
    pub(crate) fn new() -> Self {
        let sink = SharedDisplayBuffer::default();
        let state = sink.0.clone();
        Self {
            writer: strip_ansi_escapes::Writer::new(sink),
            state,
        }
    }

    pub(crate) fn push(&mut self, raw: &str) -> String {
        self.set_capture(true);
        self.write_raw(raw);
        let bytes = {
            let mut state = self
                .state
                .lock()
                .expect("display sanitizer buffer poisoned");
            std::mem::take(&mut state.output)
        };
        let stripped = String::from_utf8(bytes)
            .expect("ANSI stripping valid UTF-8 PTY text must preserve UTF-8");
        sanitize_stripped_text(&stripped)
    }

    pub(crate) fn advance(&mut self, raw: &str) {
        self.set_capture(false);
        self.write_raw(raw);
        self.state
            .lock()
            .expect("display sanitizer buffer poisoned")
            .output
            .clear();
    }

    fn set_capture(&self, capture: bool) {
        let mut state = self
            .state
            .lock()
            .expect("display sanitizer buffer poisoned");
        state.capture = capture;
        state.output.clear();
    }

    fn write_raw(&mut self, raw: &str) {
        self.writer
            .write_all(raw.as_bytes())
            .expect("writing to the in-memory display sanitizer cannot fail");
        self.writer
            .flush()
            .expect("flushing the in-memory display sanitizer cannot fail");
    }
}

impl Default for StreamingDisplaySanitizer {
    fn default() -> Self {
        Self::new()
    }
}

fn is_bidi_control(value: char) -> bool {
    matches!(
        value,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

pub(crate) fn sanitize_preview(input: &str, max_chars: usize) -> (String, bool) {
    let scan_limit = max_chars
        .saturating_mul(PREVIEW_INPUT_SCAN_MULTIPLIER)
        .saturating_add(PREVIEW_INPUT_SCAN_SLOP);
    let mut input_chars = input.chars();
    let bounded_input = input_chars.by_ref().take(scan_limit).collect::<String>();
    let input_truncated = input_chars.next().is_some();
    let sanitized = sanitize_display_text(&bounded_input);
    let mut iter = sanitized.chars();
    let preview = iter.by_ref().take(max_chars).collect::<String>();
    if input_truncated || iter.next().is_some() {
        (format!("{preview}{PREVIEW_ELLIPSIS}"), true)
    } else {
        (preview, false)
    }
}

pub(crate) fn markdown_code_span(value: &str) -> String {
    let flattened = value.replace('\n', " ");
    let longest_run = flattened
        .split(|ch| ch != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest_run.saturating_add(1).max(1));
    if flattened.starts_with('`')
        || flattened.ends_with('`')
        || (flattened.starts_with(' ') && flattened.ends_with(' '))
    {
        format!("{fence} {flattened} {fence}")
    } else {
        format!("{fence}{flattened}{fence}")
    }
}
