const PREVIEW_ELLIPSIS: &str = "…";
const PREVIEW_INPUT_SCAN_MULTIPLIER: usize = 8;
const PREVIEW_INPUT_SCAN_SLOP: usize = 256;

pub(crate) fn sanitize_display_text(input: &str) -> String {
    let stripped = strip_ansi_escapes::strip_str(input);
    let mut output = String::with_capacity(stripped.len());
    for ch in stripped.chars() {
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
