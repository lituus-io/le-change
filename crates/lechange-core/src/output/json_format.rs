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
    let mut buf = String::with_capacity(values.len() * 16 + 2);
    buf.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            buf.push(',');
        }
        buf.push('"');
        escape_json_into(v, &mut buf);
        buf.push('"');
    }
    buf.push(']');
    buf
}

/// Format as a GitHub Actions matrix value
pub fn format_matrix(values: &[&str]) -> String {
    let mut buf = String::with_capacity(values.len() * 24 + 16);
    buf.push_str(r#"{"include":["#);
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            buf.push(',');
        }
        buf.push_str(r#"{"value":""#);
        escape_json_into(v, &mut buf);
        buf.push_str(r#""}"#);
    }
    buf.push_str("]}");
    buf
}

/// Write a JSON-escaped string directly into a buffer — zero intermediate allocation.
pub fn escape_json_into(s: &str, buf: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(buf, "\\u{:04x}", c as u32);
            }
            c => buf.push(c),
        }
    }
}

/// Format deploy decisions as a GitHub Actions matrix JSON
///
/// Filters to `Deploy` groups only, producing:
/// `{"include":[{"stack":"prod","files":"file1 file2","count":2}]}`
///
/// The `resolve` function converts `InternedString` to `&str`.
///
/// When `include_reason` is true, adds `"action":"deploy","reason":"new_change"` fields.
/// When `include_concurrency` is true, adds `"concurrency_blocked":bool,"concurrency_blocked_by":N` fields.
pub fn format_deploy_matrix<'a, F>(
    decisions: &[crate::types::GroupDeployDecision],
    resolve: F,
    separator: &str,
    include_reason: bool,
    include_concurrency: bool,
) -> String
where
    F: Fn(crate::types::InternedString) -> Option<&'a str>,
{
    use crate::types::{GroupDeployAction, GroupDeployReason};

    let mut buf = String::with_capacity(256);
    buf.push_str(r#"{"include":["#);

    let mut first = true;
    for d in decisions {
        if d.action != GroupDeployAction::Deploy && !include_reason {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

        buf.push_str(r#"{"stack":""#);
        let key_str = resolve(d.key).unwrap_or("");
        escape_json_into(key_str, &mut buf);
        buf.push('"');

        // Files
        buf.push_str(r#","files":""#);
        let mut file_first = true;
        let mut count = 0u32;
        for &s in &d.files_to_rebuild {
            if let Some(path) = resolve(s) {
                if !file_first {
                    escape_json_into(separator, &mut buf);
                }
                file_first = false;
                escape_json_into(path, &mut buf);
                count += 1;
            }
        }
        buf.push('"');

        // Count
        buf.push_str(r#","count":"#);
        buf.push_str(&count.to_string());

        // Reason fields
        if include_reason {
            let action_str = match d.action {
                GroupDeployAction::Deploy => "deploy",
                GroupDeployAction::Skip => "skip",
            };
            buf.push_str(r#","action":""#);
            buf.push_str(action_str);
            buf.push('"');

            let reason_str = match d.reason {
                Some(GroupDeployReason::NewChange) => "new_change",
                Some(GroupDeployReason::PreviousFailure) => "previous_failure",
                Some(GroupDeployReason::BothNewAndFailed) => "both_new_and_failed",
                None => "previously_succeeded",
            };
            buf.push_str(r#","reason":""#);
            buf.push_str(reason_str);
            buf.push('"');
        }

        // Concurrency fields
        if include_concurrency {
            buf.push_str(r#","concurrency_blocked":"#);
            buf.push_str(if d.concurrency_blocked {
                "true"
            } else {
                "false"
            });
            buf.push_str(r#","concurrency_blocked_by":"#);
            buf.push_str(&d.concurrency_blocked_by.to_string());
        }

        buf.push('}');
    }

    buf.push_str("]}");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interner::StringInterner;

    #[test]
    fn test_escape_json_value() {
        assert_eq!(escape_json_value("hello"), "hello");
        assert_eq!(escape_json_value("he\"llo"), "he\\\"llo");
        assert_eq!(escape_json_value("path\\to\\file"), "path\\\\to\\\\file");
        assert_eq!(escape_json_value("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn test_escape_json_into_buffer() {
        let mut buf = String::new();
        escape_json_into("hello\"world", &mut buf);
        assert_eq!(buf, "hello\\\"world");
    }

    #[test]
    fn test_escape_json_into_special_chars() {
        let mut buf = String::new();
        escape_json_into("a\nb\tc\\d", &mut buf);
        assert_eq!(buf, "a\\nb\\tc\\\\d");
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
        assert_eq!(format_matrix(&["a"]), r#"{"include":[{"value":"a"}]}"#);
    }

    #[test]
    fn test_format_deploy_matrix_basic() {
        use crate::types::*;

        let interner = StringInterner::new();
        let dev_key = interner.intern("dev");
        let staging_key = interner.intern("staging");
        let prod_key = interner.intern("prod");

        let file0 = interner.intern("stacks/dev/config.yaml");
        let file1 = interner.intern("stacks/staging/config.yaml");
        let file2 = interner.intern("stacks/prod/config.yaml");

        let decisions = vec![
            GroupDeployDecision {
                key: dev_key,
                action: GroupDeployAction::Deploy,
                reason: Some(GroupDeployReason::NewChange),
                files_to_rebuild: vec![file0],
                files_to_skip: vec![],
                total_files: 1,
                concurrency_blocked: false,
                concurrency_blocked_by: 0,
            },
            GroupDeployDecision {
                key: staging_key,
                action: GroupDeployAction::Skip,
                reason: None,
                files_to_rebuild: vec![],
                files_to_skip: vec![file1],
                total_files: 1,
                concurrency_blocked: false,
                concurrency_blocked_by: 0,
            },
            GroupDeployDecision {
                key: prod_key,
                action: GroupDeployAction::Deploy,
                reason: Some(GroupDeployReason::PreviousFailure),
                files_to_rebuild: vec![file2],
                files_to_skip: vec![],
                total_files: 1,
                concurrency_blocked: false,
                concurrency_blocked_by: 0,
            },
        ];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", false, false);
        assert_eq!(
            json,
            r#"{"include":[{"stack":"dev","files":"stacks/dev/config.yaml","count":1},{"stack":"prod","files":"stacks/prod/config.yaml","count":1}]}"#
        );
    }

    #[test]
    fn test_format_deploy_matrix_empty() {
        let decisions: Vec<crate::types::GroupDeployDecision> = vec![];
        let resolve = |_: crate::types::InternedString| -> Option<&str> { None };
        let json = format_deploy_matrix(&decisions, resolve, " ", false, false);
        assert_eq!(json, r#"{"include":[]}"#);
    }

    #[test]
    fn test_format_deploy_matrix_multiple_files() {
        use crate::types::*;

        let interner = StringInterner::new();
        let prod_key = interner.intern("prod");
        let file0 = interner.intern("stacks/prod/config.yaml");
        let file1 = interner.intern("stacks/prod/secrets.yaml");

        let decisions = vec![GroupDeployDecision {
            key: prod_key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::BothNewAndFailed),
            files_to_rebuild: vec![file0, file1],
            files_to_skip: vec![],
            total_files: 2,
            concurrency_blocked: false,
            concurrency_blocked_by: 0,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", false, false);
        assert_eq!(
            json,
            r#"{"include":[{"stack":"prod","files":"stacks/prod/config.yaml stacks/prod/secrets.yaml","count":2}]}"#
        );
    }

    #[test]
    fn test_format_deploy_matrix_escaping() {
        use crate::types::*;

        let interner = StringInterner::new();
        let key = interner.intern("stack\"special");
        let file = interner.intern("path/with\"quote.yaml");

        let decisions = vec![GroupDeployDecision {
            key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::NewChange),
            files_to_rebuild: vec![file],
            files_to_skip: vec![],
            total_files: 1,
            concurrency_blocked: false,
            concurrency_blocked_by: 0,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", false, false);
        assert!(json.contains(r#"stack\"special"#));
        assert!(json.contains(r#"path/with\"quote.yaml"#));
    }

    #[test]
    fn test_matrix_with_reason() {
        use crate::types::*;

        let interner = StringInterner::new();
        let key = interner.intern("dev");
        let file = interner.intern("f.yaml");

        let decisions = vec![GroupDeployDecision {
            key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::NewChange),
            files_to_rebuild: vec![file],
            files_to_skip: vec![],
            total_files: 1,
            concurrency_blocked: false,
            concurrency_blocked_by: 0,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", true, false);
        assert!(json.contains(r#""action":"deploy""#));
        assert!(json.contains(r#""reason":"new_change""#));
    }

    #[test]
    fn test_matrix_with_concurrency() {
        use crate::types::*;

        let interner = StringInterner::new();
        let key = interner.intern("prod");
        let file = interner.intern("f.yaml");

        let decisions = vec![GroupDeployDecision {
            key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::NewChange),
            files_to_rebuild: vec![file],
            files_to_skip: vec![],
            total_files: 1,
            concurrency_blocked: true,
            concurrency_blocked_by: 2,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", false, true);
        assert!(json.contains(r#""concurrency_blocked":true"#));
        assert!(json.contains(r#""concurrency_blocked_by":2"#));
    }

    #[test]
    fn test_matrix_all_fields() {
        use crate::types::*;

        let interner = StringInterner::new();
        let key = interner.intern("prod");
        let file = interner.intern("f.yaml");

        let decisions = vec![GroupDeployDecision {
            key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::PreviousFailure),
            files_to_rebuild: vec![file],
            files_to_skip: vec![],
            total_files: 1,
            concurrency_blocked: true,
            concurrency_blocked_by: 1,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", true, true);
        assert!(json.contains(r#""action":"deploy""#));
        assert!(json.contains(r#""reason":"previous_failure""#));
        assert!(json.contains(r#""concurrency_blocked":true"#));
        assert!(json.contains(r#""concurrency_blocked_by":1"#));
    }

    #[test]
    fn test_matrix_basic_no_extras() {
        use crate::types::*;

        let interner = StringInterner::new();
        let key = interner.intern("dev");
        let file = interner.intern("f.yaml");

        let decisions = vec![GroupDeployDecision {
            key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::NewChange),
            files_to_rebuild: vec![file],
            files_to_skip: vec![],
            total_files: 1,
            concurrency_blocked: false,
            concurrency_blocked_by: 0,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", false, false);
        // Should NOT contain action/reason/concurrency fields
        assert!(!json.contains("action"));
        assert!(!json.contains("reason"));
        assert!(!json.contains("concurrency"));
    }

    #[test]
    fn test_matrix_buffer_no_intermediate_vec() {
        use crate::types::*;

        let interner = StringInterner::new();
        let key = interner.intern("x");
        let file = interner.intern("a.yaml");

        let decisions = vec![GroupDeployDecision {
            key,
            action: GroupDeployAction::Deploy,
            reason: Some(GroupDeployReason::NewChange),
            files_to_rebuild: vec![file],
            files_to_skip: vec![],
            total_files: 1,
            concurrency_blocked: false,
            concurrency_blocked_by: 0,
        }];

        let json = format_deploy_matrix(&decisions, |s| interner.resolve(s), " ", true, true);
        // Validate it's valid JSON by checking basic structure
        assert!(json.starts_with(r#"{"include":["#));
        assert!(json.ends_with("]}"));
    }

    #[test]
    fn test_format_json_array_unicode() {
        // CJK, emoji, accented characters
        let result = format_json_array(&["\u{4e16}\u{754c}", "caf\u{e9}", "\u{1f600}"]);
        assert!(result.contains("\u{4e16}\u{754c}"));
        assert!(result.contains("caf\u{e9}"));
        assert!(result.contains("\u{1f600}"));
        // Should still be a valid bracket-delimited JSON array
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn test_format_json_array_control_chars() {
        // Control characters must be escaped (null, bell, form-feed, etc.)
        let result = format_json_array(&["a\x00b", "c\x07d", "e\x0cf"]);
        // Null \u0000, bell \u0007, form-feed \u000c
        assert!(
            result.contains("\\u0000"),
            "null byte should be escaped: {}",
            result
        );
        assert!(
            result.contains("\\u0007"),
            "bell should be escaped: {}",
            result
        );
        assert!(
            result.contains("\\u000c"),
            "form-feed should be escaped: {}",
            result
        );
        // Standard control chars \n \r \t have their own escapes
        let result2 = format_json_array(&["x\ny", "a\rb", "p\tq"]);
        assert!(result2.contains("\\n"));
        assert!(result2.contains("\\r"));
        assert!(result2.contains("\\t"));
    }

    #[test]
    fn test_format_matrix_unicode() {
        let result = format_matrix(&["\u{4e16}\u{754c}", "caf\u{e9}"]);
        assert!(result.contains("\u{4e16}\u{754c}"));
        assert!(result.contains("caf\u{e9}"));
        assert!(result.starts_with(r#"{"include":["#));
        assert!(result.ends_with("]}"));
    }

    #[test]
    fn test_json_array_is_valid_json() {
        // Various edge cases: empty, single, multiple, special chars
        let cases: Vec<Vec<&str>> = vec![
            vec![],
            vec!["simple"],
            vec!["a", "b", "c"],
            vec!["with\"quote", "back\\slash"],
            vec!["new\nline", "tab\there"],
            vec!["\u{4e16}\u{754c}", "\u{1f680}"],
        ];
        for values in &cases {
            let json_str = format_json_array(&values.to_vec());
            let parsed: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("Invalid JSON for {:?}: {} -> {}", values, json_str, e));
            assert!(parsed.is_array(), "Expected array, got: {}", json_str);
            assert_eq!(parsed.as_array().unwrap().len(), values.len());
        }
    }

    #[test]
    fn test_matrix_is_valid_json() {
        let cases: Vec<Vec<&str>> = vec![
            vec![],
            vec!["single"],
            vec!["a", "b"],
            vec!["with\"quote"],
            vec!["\u{4e16}\u{754c}"],
        ];
        for values in &cases {
            let json_str = format_matrix(&values.to_vec());
            let parsed: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("Invalid JSON for {:?}: {} -> {}", values, json_str, e));
            assert!(parsed.is_object(), "Expected object, got: {}", json_str);
            let include = parsed.get("include").expect("missing 'include' key");
            assert!(include.is_array());
            assert_eq!(include.as_array().unwrap().len(), values.len());
        }
    }
}
