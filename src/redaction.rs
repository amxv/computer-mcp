pub const REDACTED_QUERY_VALUE: &str = "<redacted>";

pub fn redact_api_key_query_params(text: &str) -> String {
    redact_query_param(text, "key", REDACTED_QUERY_VALUE)
}

fn redact_query_param(text: &str, param: &str, replacement: &str) -> String {
    let token = format!("{param}=");
    let mut redacted = String::with_capacity(text.len());
    let mut cursor = 0;

    while let Some(relative_start) = text[cursor..].find(&token) {
        let start = cursor + relative_start;
        if !is_param_start(text, start) {
            let keep_until = start + token.len();
            redacted.push_str(&text[cursor..keep_until]);
            cursor = keep_until;
            continue;
        }

        redacted.push_str(&text[cursor..start]);
        redacted.push_str(&token);
        redacted.push_str(replacement);

        let mut value_end = start + token.len();
        while value_end < text.len() {
            let ch = text.as_bytes()[value_end] as char;
            if is_query_delimiter(ch) {
                break;
            }
            value_end += 1;
        }
        cursor = value_end;
    }

    redacted.push_str(&text[cursor..]);
    redacted
}

fn is_param_start(text: &str, start: usize) -> bool {
    if start == 0 {
        return true;
    }

    let prev = text.as_bytes()[start - 1] as char;
    !is_param_name_char(prev)
}

fn is_param_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn is_query_delimiter(ch: char) -> bool {
    matches!(
        ch,
        '&' | '#' | ' ' | '\t' | '\r' | '\n' | '"' | '\'' | ',' | ')' | ']' | '}' | '>'
    )
}

#[cfg(test)]
mod tests {
    use super::redact_api_key_query_params;

    #[test]
    fn redacts_simple_mcp_url() {
        let input = "https://203.0.113.5/mcp?key=supersecret";
        let got = redact_api_key_query_params(input);
        assert_eq!(got, "https://203.0.113.5/mcp?key=<redacted>");
    }

    #[test]
    fn redacts_when_key_is_not_first_query_param() {
        let input = "https://host/mcp?foo=1&key=abc123&bar=2";
        let got = redact_api_key_query_params(input);
        assert_eq!(got, "https://host/mcp?foo=1&key=<redacted>&bar=2");
    }

    #[test]
    fn redacts_multiple_occurrences_in_log_text() {
        let input = "first key=alpha second https://h/mcp?key=beta&x=1";
        let got = redact_api_key_query_params(input);
        assert_eq!(
            got,
            "first key=<redacted> second https://h/mcp?key=<redacted>&x=1"
        );
    }

    #[test]
    fn does_not_redact_non_key_fields() {
        let input = "apikey=value&monkey=banana";
        let got = redact_api_key_query_params(input);
        assert_eq!(got, input);
    }
}
