use anyhow::{Context, Result};

use super::super::history::HistoryLiveEvent;

#[derive(Default)]
pub(super) struct SseDecoder {
    pending: Vec<u8>,
}

impl SseDecoder {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<HistoryLiveEvent>> {
        self.pending.extend_from_slice(chunk);
        let mut decoded = Vec::new();
        while let Some((end, delimiter_len)) = next_event_boundary(&self.pending) {
            let block = self.pending.drain(..end).collect::<Vec<_>>();
            self.pending.drain(..delimiter_len);
            let block =
                std::str::from_utf8(&block).context("Local SSE event was not valid UTF-8")?;
            if let Some(event) = decode_sse_block(block)? {
                decoded.push(event);
            }
        }
        Ok(decoded)
    }
}

fn next_event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    for index in 0..bytes.len() {
        if bytes.get(index..index + 2) == Some(b"\n\n") {
            return Some((index, 2));
        }
        if bytes.get(index..index + 4) == Some(b"\r\n\r\n") {
            return Some((index, 4));
        }
    }
    None
}

fn decode_sse_block(block: &str) -> Result<Option<HistoryLiveEvent>> {
    let mut data = String::new();
    for line in block.lines() {
        let Some(value) = line.strip_prefix("data:") else {
            continue;
        };
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value.strip_prefix(' ').unwrap_or(value));
    }
    if data.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&data)
        .context("failed to decode Local live event")
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::SseDecoder;

    #[test]
    fn sse_decoder_handles_split_chunks_and_crlf() {
        let json = r#"{"schema_version":2,"runtime_id":"runtime","sequence":7,"emitted_at_ms":1,"event_type":"output","agent_id":"k7m2","invocation_id":3,"presentation_id":"inv-3","presentation_revision":3,"payload":{"text":"ok"}}"#;
        let payload = format!("id: 7\r\nevent: output\r\ndata: {json}\r\n\r\n");
        let split = payload.len() / 2;
        let mut decoder = SseDecoder::default();
        assert!(
            decoder
                .push(&payload.as_bytes()[..split])
                .unwrap()
                .is_empty()
        );
        let decoded = decoder.push(&payload.as_bytes()[split..]).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].sequence, 7);
        assert_eq!(decoded[0].agent_id.as_deref(), Some("k7m2"));
        assert_eq!(decoded[0].presentation_id.as_deref(), Some("inv-3"));
    }
}
