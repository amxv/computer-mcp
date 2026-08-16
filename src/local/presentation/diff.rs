use similar::{ChangeTag, TextDiff};

use super::model::PresentationDiffLine;
use super::sanitize::sanitize_preview;

const MAX_DIFF_LINES: usize = 500;
const MAX_DIFF_INPUT_LINES: usize = 2_000;
const MAX_DIFF_LINE_CHARS: usize = 2_048;

pub(super) struct BuiltDiff {
    pub(super) added: usize,
    pub(super) removed: usize,
    pub(super) truncated: bool,
    pub(super) lines: Vec<PresentationDiffLine>,
}

pub(super) fn build_text_diff(before: &str, after: &str) -> Option<BuiltDiff> {
    if exceeds_line_limit(before) || exceeds_line_limit(after) {
        return None;
    }
    let diff = TextDiff::from_lines(before, after);
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
    let mut truncated = false;
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
                    old_line: change.old_index().map(|index| index + 1),
                    new_line: change.new_index().map(|index| index + 1),
                    text,
                });
            }
        }
    }

    Some(BuiltDiff {
        added,
        removed,
        truncated,
        lines,
    })
}

fn exceeds_line_limit(value: &str) -> bool {
    value.lines().take(MAX_DIFF_INPUT_LINES + 1).count() > MAX_DIFF_INPUT_LINES
}
