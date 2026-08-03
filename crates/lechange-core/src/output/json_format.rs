// Copyright (c) 2024-2026 Lituus-io. All rights reserved.
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
///
/// Format vanished files as a JSON array of {path, last_seen_sha} objects.
/// Zero-serde, same escaping discipline as the rest of this module.
pub fn format_vanished_json<'a, F>(vanished: &[crate::types::VanishedFile], resolve: F) -> String
where
    F: Fn(crate::types::InternedString) -> Option<&'a str>,
{
    let mut buf = String::with_capacity(64);
    buf.push('[');
    let mut first = true;
    for v in vanished {
        let (Some(path), Some(sha)) = (resolve(v.path), resolve(v.last_seen_sha)) else {
            continue;
        };
        if !first {
            buf.push(',');
        }
        first = false;
        buf.push_str(r#"{"path":""#);
        escape_json_into(path, &mut buf);
        buf.push_str(r#"","last_seen_sha":""#);
        escape_json_into(sha, &mut buf);
        buf.push_str(r#""}"#);
    }
    buf.push(']');
    buf
}

/// Format the deploy matrix for GitHub Actions `strategy.matrix`.
///
/// Deploy entries are always emitted; Destroy entries are always emitted with
/// `action`/`reason`/`last_seen_sha`; Skip entries only with `include_reason`.
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
    use crate::types::GroupDeployAction;

    let mut buf = String::with_capacity(256);
    buf.push_str(r#"{"include":["#);

    let mut first = true;
    for d in decisions {
        // Deploy always emitted; Destroy always emitted (consumers need the
        // destroy matrix regardless of include_reason); Skip only with reasons.
        if d.action == GroupDeployAction::Skip && !include_reason {
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
        for v in &d.vanished_files {
            if let Some(path) = resolve(v.path) {
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

        // Reason fields: always present on Destroy entries so consumers can
        // route them without configuring include_reason.
        if include_reason || d.action == GroupDeployAction::Destroy {
            buf.push_str(r#","action":""#);
            buf.push_str(d.action.as_str());
            buf.push('"');

            let reason_str = match d.reason {
                Some(r) => r.as_str(),
                None => "previously_succeeded",
            };
            buf.push_str(r#","reason":""#);
            buf.push_str(reason_str);
            buf.push('"');
        }

        // Reconstruction commit (Destroy entries)
        if let Some(sha) = d.reconstruct_sha {
            if let Some(sha_str) = resolve(sha) {
                buf.push_str(r#","last_seen_sha":""#);
                escape_json_into(sha_str, &mut buf);
                buf.push('"');
            }
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

/// Derive the literal (prefix, suffix) of a single glob pattern: the run of
/// literal characters before the first glob metacharacter (`* ? [ {`) and after
/// the last one. Used to turn a matched path into a short deploy-unit label —
/// e.g. `stacks/**/Pulumi.yaml` yields (`stacks/`, `/Pulumi.yaml`), so
/// `stacks/buckets/churn/Pulumi.yaml` labels as `buckets/churn`.
pub fn glob_literal_affixes(pattern: &str) -> (&str, &str) {
    match pattern.find(['*', '?', '[', '{']) {
        Some(first) => {
            let last = pattern.rfind(['*', '?', '[', '{']).unwrap_or(first);
            (&pattern[..first], &pattern[last + 1..])
        }
        None => (pattern, ""),
    }
}

/// Build a per-PATH deploy matrix for GitHub Actions `strategy.matrix` — one
/// entry per changed / vanished Pulumi-program file, regardless of directory
/// nesting. Unlike [`format_deploy_matrix`] (which groups by directory subtree
/// and mis-handles nested or parent+child stacks), this treats each matched
/// file as its own deploy unit:
///
/// * added / modified filtered file -> `action: deploy`
/// * deleted filtered file -> `action: destroy`, `reason: deleted`,
///   `last_seen_sha` = `base_sha`
/// * vanished file -> `action: destroy`, `reason: vanished`,
///   `last_seen_sha` = its last-seen commit
///
/// Each entry is `{stack, container, action, reason[, last_seen_sha]}`, where
/// `stack` is `container` with `strip_prefix`/`strip_suffix` removed (the
/// literal affixes of the caller's file glob — see [`glob_literal_affixes`]).
pub fn format_file_matrix<'a, F>(
    result: &crate::types::ProcessedResult,
    base_sha: Option<&str>,
    strip_prefix: &str,
    strip_suffix: &str,
    resolve: F,
) -> String
where
    F: Fn(crate::types::InternedString) -> Option<&'a str>,
{
    use crate::types::ChangeType;

    let label = |path: &str| -> String {
        path.strip_prefix(strip_prefix)
            .unwrap_or(path)
            .strip_suffix(strip_suffix)
            .unwrap_or_else(|| path.strip_prefix(strip_prefix).unwrap_or(path))
            .to_string()
    };

    let mut buf = String::with_capacity(256);
    buf.push_str(r#"{"include":["#);
    let mut first = true;

    let entry = |buf: &mut String,
                 first: &mut bool,
                 container: &str,
                 action: &str,
                 reason: &str,
                 last_seen: Option<&str>| {
        if !*first {
            buf.push(',');
        }
        *first = false;
        buf.push_str(r#"{"stack":""#);
        escape_json_into(&label(container), buf);
        buf.push_str(r#"","container":""#);
        escape_json_into(container, buf);
        buf.push_str(r#"","action":""#);
        buf.push_str(action);
        buf.push_str(r#"","reason":""#);
        buf.push_str(reason);
        buf.push('"');
        if let Some(sha) = last_seen {
            buf.push_str(r#","last_seen_sha":""#);
            escape_json_into(sha, buf);
            buf.push('"');
        }
        buf.push('}');
    };

    // Changed files (respecting the pattern filter), classified by change type.
    for &idx in &result.filtered_indices {
        let file = &result.all_files[idx as usize];
        let Some(container) = resolve(file.path) else {
            continue;
        };
        match file.change_type {
            ChangeType::Added => entry(
                &mut buf,
                &mut first,
                container,
                "deploy",
                "new_change",
                None,
            ),
            ChangeType::Modified => {
                entry(&mut buf, &mut first, container, "deploy", "modified", None)
            }
            ChangeType::Deleted => entry(
                &mut buf, &mut first, container, "destroy", "deleted", base_sha,
            ),
            // Renames/copies/typechanges are not deploy events for this model.
            _ => {}
        }
    }

    // Vanished files (added then removed within the range) -> destroy.
    for v in &result.vanished_files {
        let (Some(container), Some(sha)) = (resolve(v.path), resolve(v.last_seen_sha)) else {
            continue;
        };
        entry(
            &mut buf,
            &mut first,
            container,
            "destroy",
            "vanished",
            Some(sha),
        );
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
                vanished_files: Vec::new(),
                reconstruct_sha: None,
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
                vanished_files: Vec::new(),
                reconstruct_sha: None,
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
                vanished_files: Vec::new(),
                reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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
            vanished_files: Vec::new(),
            reconstruct_sha: None,
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

#[cfg(test)]
mod vanished_format_tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::types::{GroupDeployAction, GroupDeployDecision, GroupDeployReason, VanishedFile};

    #[test]
    fn test_destroy_matrix_entry_shape() {
        let interner = StringInterner::new();
        let d = GroupDeployDecision {
            key: interner.intern("gone"),
            action: GroupDeployAction::Destroy,
            reason: Some(GroupDeployReason::Vanished),
            files_to_rebuild: Vec::new(),
            files_to_skip: Vec::new(),
            total_files: 1,
            concurrency_blocked: false,
            concurrency_blocked_by: 0,
            vanished_files: vec![VanishedFile {
                path: interner.intern("stacks/gone/Pulumi.yaml"),
                last_seen_sha: interner.intern("cafe1234"),
            }],
            reconstruct_sha: Some(interner.intern("cafe1234")),
        };

        // include_reason=false: Destroy entries must STILL be emitted with
        // action/reason/last_seen_sha
        let json = format_deploy_matrix(&[d], |s| interner.resolve(s), " ", false, false);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let entry = &parsed["include"][0];
        assert_eq!(entry["stack"], "gone");
        assert_eq!(entry["action"], "destroy");
        assert_eq!(entry["reason"], "vanished");
        assert_eq!(entry["last_seen_sha"], "cafe1234");
        assert_eq!(entry["files"], "stacks/gone/Pulumi.yaml");
        assert_eq!(entry["count"], 1);
    }

    #[test]
    fn test_format_vanished_json() {
        let interner = StringInterner::new();
        let v = vec![
            VanishedFile {
                path: interner.intern("stacks/a/Pulumi.yaml"),
                last_seen_sha: interner.intern("aaaa"),
            },
            VanishedFile {
                path: interner.intern(r#"stacks/we "ird/x.yaml"#),
                last_seen_sha: interner.intern("bbbb"),
            },
        ];
        let json = format_vanished_json(&v, |s| interner.resolve(s));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed[0]["path"], "stacks/a/Pulumi.yaml");
        assert_eq!(parsed[0]["last_seen_sha"], "aaaa");
        assert_eq!(parsed[1]["path"], r#"stacks/we "ird/x.yaml"#);
        assert_eq!(format_vanished_json(&[], |s| interner.resolve(s)), "[]");
    }
}

#[cfg(test)]
mod file_matrix_tests {
    use super::*;
    use crate::interner::StringInterner;
    use crate::types::{ChangeType, ChangedFile, FileOrigin, ProcessedResult, VanishedFile};

    fn file(interner: &StringInterner, path: &str, ct: ChangeType) -> ChangedFile {
        ChangedFile {
            path: interner.intern(path),
            change_type: ct,
            previous_path: None,
            is_symlink: false,
            submodule_depth: 0,
            origin: FileOrigin {
                in_current_changes: true,
                in_previous_failure: false,
                in_previous_success: false,
            },
        }
    }

    #[test]
    fn test_glob_literal_affixes() {
        assert_eq!(
            glob_literal_affixes("stacks/**/Pulumi.yaml"),
            ("stacks/", "/Pulumi.yaml")
        );
        assert_eq!(
            glob_literal_affixes("stacks/*/x.yaml"),
            ("stacks/", "/x.yaml")
        );
        assert_eq!(glob_literal_affixes("no/glob/here"), ("no/glob/here", ""));
        assert_eq!(glob_literal_affixes("**"), ("", ""));
    }

    #[test]
    fn test_file_matrix_all_change_types_and_nesting() {
        let interner = StringInterner::new();
        let mut result = ProcessedResult {
            base_sha: Some(interner.intern("base9999")),
            ..Default::default()
        };
        result.all_files = vec![
            file(&interner, "stacks/flat/Pulumi.yaml", ChangeType::Added),
            file(
                &interner,
                "stacks/buckets/churn/Pulumi.yaml",
                ChangeType::Modified,
            ),
            file(&interner, "stacks/old/Pulumi.yaml", ChangeType::Deleted),
            file(&interner, "stacks/renamed/Pulumi.yaml", ChangeType::Renamed),
        ];
        result.filtered_indices = vec![0, 1, 2, 3];
        result.vanished_files = vec![VanishedFile {
            path: interner.intern("stacks/gone/Pulumi.yaml"),
            last_seen_sha: interner.intern("cafe1234"),
        }];

        let json = format_file_matrix(
            &result,
            interner.resolve(result.base_sha.unwrap()),
            "stacks/",
            "/Pulumi.yaml",
            |s| interner.resolve(s),
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let inc = parsed["include"].as_array().unwrap();
        // added, modified, deleted, vanished = 4 entries (rename excluded)
        assert_eq!(inc.len(), 4);

        assert_eq!(inc[0]["stack"], "flat");
        assert_eq!(inc[0]["container"], "stacks/flat/Pulumi.yaml");
        assert_eq!(inc[0]["action"], "deploy");
        assert_eq!(inc[0]["reason"], "new_change");
        assert!(inc[0].get("last_seen_sha").is_none());

        // nested label strips the affixes
        assert_eq!(inc[1]["stack"], "buckets/churn");
        assert_eq!(inc[1]["action"], "deploy");
        assert_eq!(inc[1]["reason"], "modified");

        assert_eq!(inc[2]["stack"], "old");
        assert_eq!(inc[2]["action"], "destroy");
        assert_eq!(inc[2]["reason"], "deleted");
        assert_eq!(inc[2]["last_seen_sha"], "base9999");

        assert_eq!(inc[3]["stack"], "gone");
        assert_eq!(inc[3]["action"], "destroy");
        assert_eq!(inc[3]["reason"], "vanished");
        assert_eq!(inc[3]["last_seen_sha"], "cafe1234");
    }

    #[test]
    fn test_file_matrix_empty() {
        let interner = StringInterner::new();
        let result = ProcessedResult::default();
        let json = format_file_matrix(&result, None, "stacks/", "/Pulumi.yaml", |s| {
            interner.resolve(s)
        });
        assert_eq!(json, r#"{"include":[]}"#);
    }
}
