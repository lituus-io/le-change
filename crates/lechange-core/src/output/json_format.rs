//! JSON/matrix/escape formatting utilities

/// Escape a string for JSON output
pub fn escape_json_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

/// Escape for GitHub Actions safe output (percent-encoding special chars)
pub fn safe_output_escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Format a list of values as a JSON array string
pub fn format_json_array(values: &[&str]) -> String {
    let escaped: Vec<String> = values.iter().map(|v| format!("\"{}\"", escape_json_value(v))).collect();
    format!("[{}]", escaped.join(","))
}

/// Format as a GitHub Actions matrix value
pub fn format_matrix(values: &[&str]) -> String {
    // Matrix format: {"include":[{"value":"a"},{"value":"b"}]}
    if values.is_empty() {
        return r#"{"include":[]}"#.to_string();
    }

    let entries: Vec<String> = values
        .iter()
        .map(|v| format!(r#"{{"value":"{}"}}"#, escape_json_value(v)))
        .collect();

    format!(r#"{{"include":[{}]}}"#, entries.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_json_value() {
        assert_eq!(escape_json_value("hello"), "hello");
        assert_eq!(escape_json_value("he\"llo"), "he\\\"llo");
        assert_eq!(escape_json_value("path\\to\\file"), "path\\\\to\\\\file");
        assert_eq!(escape_json_value("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn test_safe_output_escape() {
        assert_eq!(safe_output_escape("hello"), "hello");
        assert_eq!(safe_output_escape("a%b"), "a%25b");
        assert_eq!(safe_output_escape("a\nb"), "a%0Ab");
    }

    #[test]
    fn test_format_json_array() {
        assert_eq!(format_json_array(&[]), "[]");
        assert_eq!(format_json_array(&["a", "b"]), r#"["a","b"]"#);
    }

    #[test]
    fn test_format_matrix() {
        assert_eq!(format_matrix(&[]), r#"{"include":[]}"#);
        assert_eq!(
            format_matrix(&["a"]),
            r#"{"include":[{"value":"a"}]}"#
        );
    }
}
