use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

const TIMELINE_CURSOR_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryTimelineCursor {
    History {
        started_at_ms: i64,
        root_id: i64,
        before_ms: Option<i64>,
    },
    Recovery {
        changed_at_ms: i64,
        root_id: i64,
        since_ms: i64,
    },
    Checkpoints {
        started_at_ms: i64,
        invocation_id: i64,
        presentation_root_id: i64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct CursorPayload {
    version: u8,
    kind: String,
    timestamp_ms: i64,
    id: i64,
    scope_ms: Option<i64>,
    scope_id: Option<i64>,
}

impl HistoryTimelineCursor {
    pub(crate) fn decode(value: &str) -> Result<Self> {
        if value.is_empty() || value.len() > 512 {
            bail!("timeline cursor is empty or too long");
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .context("timeline cursor is not valid URL-safe base64")?;
        let payload: CursorPayload =
            serde_json::from_slice(&bytes).context("timeline cursor payload is not valid JSON")?;
        if payload.version != TIMELINE_CURSOR_VERSION {
            bail!(
                "unsupported timeline cursor version {}; expected {}",
                payload.version,
                TIMELINE_CURSOR_VERSION
            );
        }
        if payload.id <= 0 {
            bail!("timeline cursor contains an invalid identifier");
        }
        match payload.kind.as_str() {
            "history" => Ok(Self::History {
                started_at_ms: payload.timestamp_ms,
                root_id: payload.id,
                before_ms: payload.scope_ms,
            }),
            "recovery" => Ok(Self::Recovery {
                changed_at_ms: payload.timestamp_ms,
                root_id: payload.id,
                since_ms: payload
                    .scope_ms
                    .context("recovery cursor is missing its watermark")?,
            }),
            "checkpoints" => Ok(Self::Checkpoints {
                started_at_ms: payload.timestamp_ms,
                invocation_id: payload.id,
                presentation_root_id: payload
                    .scope_id
                    .context("checkpoint cursor is missing its presentation root")?,
            }),
            other => bail!("unsupported timeline cursor kind `{other}`"),
        }
    }

    pub(crate) fn encode(&self) -> String {
        let payload = match *self {
            Self::History {
                started_at_ms,
                root_id,
                before_ms,
            } => CursorPayload {
                version: TIMELINE_CURSOR_VERSION,
                kind: "history".to_string(),
                timestamp_ms: started_at_ms,
                id: root_id,
                scope_ms: before_ms,
                scope_id: None,
            },
            Self::Recovery {
                changed_at_ms,
                root_id,
                since_ms,
            } => CursorPayload {
                version: TIMELINE_CURSOR_VERSION,
                kind: "recovery".to_string(),
                timestamp_ms: changed_at_ms,
                id: root_id,
                scope_ms: Some(since_ms),
                scope_id: None,
            },
            Self::Checkpoints {
                started_at_ms,
                invocation_id,
                presentation_root_id,
            } => CursorPayload {
                version: TIMELINE_CURSOR_VERSION,
                kind: "checkpoints".to_string(),
                timestamp_ms: started_at_ms,
                id: invocation_id,
                scope_ms: None,
                scope_id: Some(presentation_root_id),
            },
        };
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&payload).expect("timeline cursor payload must always serialize"),
        )
    }

    pub(crate) fn matches_history(&self, before_ms: Option<i64>) -> bool {
        matches!(self, Self::History { before_ms: cursor_before, .. } if *cursor_before == before_ms)
    }

    pub(crate) fn matches_recovery(&self, since_ms: i64) -> bool {
        matches!(self, Self::Recovery { since_ms: cursor_since, .. } if *cursor_since == since_ms)
    }

    pub(crate) fn matches_checkpoints(&self, presentation_root_id: i64) -> bool {
        matches!(
            self,
            Self::Checkpoints {
                presentation_root_id: cursor_root,
                ..
            } if *cursor_root == presentation_root_id
        )
    }
}
