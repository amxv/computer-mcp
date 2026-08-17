use std::time::Duration;

use similar::{ChangeTag, TextDiff};

use super::model::PresentationDiffLine;
use super::sanitize::sanitize_preview;

const MAX_DIFF_LINES: usize = 500;
const MAX_DIFF_PREVIEW_CHANGED_LINES: usize = 1_000;
const MAX_DIFF_LINE_CHARS: usize = 2_048;
const DIFF_CONTEXT_LINES: usize = 3;
const DIFF_TIMEOUT_INPUT_LINES: usize = 1_000;
const DIFF_TIMEOUT: Duration = Duration::from_millis(50);

pub(super) struct BuiltDiff {
    pub(super) added: usize,
    pub(super) removed: usize,
    pub(super) truncated: bool,
    pub(super) lines: Vec<PresentationDiffLine>,
}

pub(super) fn build_text_diff(before: &str, after: &str) -> BuiltDiff {
    // Strip the unchanged outer file before diffing. This keeps large files
    // with tiny edits (lockfiles are common) cheap without making file size a
    // reason to abandon the structured file-change presentation.
    let before_lines = before.split_inclusive('\n').collect::<Vec<_>>();
    let after_lines = after.split_inclusive('\n').collect::<Vec<_>>();
    let prefix = common_prefix_len(&before_lines, &after_lines);
    let suffix = common_suffix_len(&before_lines[prefix..], &after_lines[prefix..]);
    let before_changed_end = before_lines.len().saturating_sub(suffix);
    let after_changed_end = after_lines.len().saturating_sub(suffix);
    let window_start = prefix.saturating_sub(DIFF_CONTEXT_LINES);
    let before_window_end = (before_changed_end + DIFF_CONTEXT_LINES).min(before_lines.len());
    let after_window_end = (after_changed_end + DIFF_CONTEXT_LINES).min(after_lines.len());
    let before_window = before_lines[window_start..before_window_end].concat();
    let after_window = after_lines[window_start..after_window_end].concat();

    let mut config = TextDiff::configure();
    if before_window_end - window_start + after_window_end - window_start > DIFF_TIMEOUT_INPUT_LINES
    {
        // `similar` degrades to an approximate edit script at the deadline.
        // The important invariant here is that pathological diffs remain a
        // structured file-change card instead of collapsing to generic.
        config.timeout(DIFF_TIMEOUT);
    }
    let diff = config.diff_lines(&before_window, &after_window);
    let mut added = 0;
    let mut removed = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }

    let mut lines = Vec::new();
    let mut truncated = added + removed > MAX_DIFF_PREVIEW_CHANGED_LINES;
    'groups: for group in diff.grouped_ops(3) {
        for operation in group {
            for change in diff.iter_changes(&operation) {
                if lines.len() >= MAX_DIFF_LINES {
                    truncated = true;
                    break 'groups;
                }
                let text = change.value().strip_suffix('\n').unwrap_or(change.value());
                let text = text.strip_suffix('\r').unwrap_or(text);
                let (text, line_truncated) = sanitize_preview(text, MAX_DIFF_LINE_CHARS);
                truncated |= line_truncated;
                lines.push(PresentationDiffLine {
                    kind: match change.tag() {
                        ChangeTag::Equal => "context",
                        ChangeTag::Delete => "remove",
                        ChangeTag::Insert => "add",
                    }
                    .to_string(),
                    old_line: change.old_index().map(|index| window_start + index + 1),
                    new_line: change.new_index().map(|index| window_start + index + 1),
                    text,
                });
            }
        }
    }

    BuiltDiff {
        added,
        removed,
        truncated,
        lines,
    }
}

fn common_prefix_len<T: PartialEq>(left: &[T], right: &[T]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_suffix_len<T: PartialEq>(left: &[T], right: &[T]) -> usize {
    left.iter()
        .rev()
        .zip(right.iter().rev())
        .take_while(|(left, right)| left == right)
        .count()
}
