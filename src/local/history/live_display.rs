use std::collections::HashMap;

use crate::local::presentation::StreamingDisplaySanitizer;

const DISPLAY_AVAILABLE: &str = "available";
const DISPLAY_UNAVAILABLE: &str = "unavailable";

pub(super) struct LiveDisplayStreams {
    streams: HashMap<i64, LiveDisplayStream>,
}

struct LiveDisplayStream {
    sanitizer: StreamingDisplaySanitizer,
    next_sequence: u64,
    unavailable_reason: Option<String>,
}

pub(super) struct LiveDisplayDelta {
    pub(super) text: String,
    pub(super) state: &'static str,
    pub(super) reason: Option<String>,
}

pub(super) struct LiveDisplayStatus {
    pub(super) state: &'static str,
    pub(super) reason: Option<String>,
}

impl LiveDisplayStreams {
    pub(super) fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    pub(super) fn observe(
        &mut self,
        invocation_id: i64,
        sequence: u64,
        raw: &str,
        capture_text: bool,
    ) -> LiveDisplayDelta {
        let stream = self
            .streams
            .entry(invocation_id)
            .or_insert_with(|| LiveDisplayStream {
                sanitizer: StreamingDisplaySanitizer::new(),
                next_sequence: 0,
                unavailable_reason: None,
            });
        if stream.unavailable_reason.is_none() && sequence != stream.next_sequence {
            stream.unavailable_reason = Some(format!(
                "live PTY display sequence is incomplete at {} (next observed sequence is {sequence})",
                stream.next_sequence
            ));
        }
        stream.next_sequence = sequence.saturating_add(1);

        if let Some(reason) = stream.unavailable_reason.clone() {
            return LiveDisplayDelta {
                text: String::new(),
                state: DISPLAY_UNAVAILABLE,
                reason: Some(reason),
            };
        }

        let text = if capture_text {
            stream.sanitizer.push(raw)
        } else {
            stream.sanitizer.advance(raw);
            String::new()
        };
        LiveDisplayDelta {
            text,
            state: DISPLAY_AVAILABLE,
            reason: None,
        }
    }

    pub(super) fn complete(&mut self, invocation_id: i64) -> LiveDisplayStatus {
        let reason = self
            .streams
            .remove(&invocation_id)
            .and_then(|stream| stream.unavailable_reason);
        match reason {
            Some(reason) => LiveDisplayStatus {
                state: DISPLAY_UNAVAILABLE,
                reason: Some(reason),
            },
            None => LiveDisplayStatus {
                state: DISPLAY_AVAILABLE,
                reason: None,
            },
        }
    }
}

impl Default for LiveDisplayStreams {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::LiveDisplayStreams;

    #[test]
    fn parser_state_survives_unobserved_prefix_and_sequence_holes_fail_closed() {
        let mut streams = LiveDisplayStreams::new();
        let first = streams.observe(7, 0, "before \u{1b}[31", false);
        assert_eq!(first.state, "available");
        assert!(first.text.is_empty());

        let second = streams.observe(7, 1, "mred\u{1b}[0m after", true);
        assert_eq!(second.state, "available");
        assert_eq!(second.text, "red after");

        let missing = streams.observe(7, 3, "uncertain", true);
        assert_eq!(missing.state, "unavailable");
        assert!(missing.text.is_empty());
        assert!(missing.reason.as_deref().unwrap().contains("sequence"));

        let completed = streams.complete(7);
        assert_eq!(completed.state, "unavailable");
        assert!(completed.reason.is_some());
    }
}
