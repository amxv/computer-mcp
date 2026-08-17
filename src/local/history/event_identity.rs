#[derive(Debug, Clone, Copy)]
pub(super) struct HistoryCompletionResult {
    pub(super) presentation_root_invocation_id: Option<i64>,
}

pub(super) fn presentation_root_invocation_id(
    invocation_id: i64,
    continuation_kind: Option<&str>,
    target_created_by_invocation_id: Option<i64>,
) -> Option<i64> {
    if continuation_kind == Some("poll") {
        target_created_by_invocation_id
    } else {
        Some(invocation_id)
    }
}
