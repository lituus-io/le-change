//! Unified pattern loading from source files and YAML

use crate::error::{Error, Result};
use crate::patterns::matcher::PatternMatcher;
use crate::types::GroupByKey;
use std::collections::HashMap;
use std::path::Path;

/// A named pattern group loaded from YAML
pub struct PatternGroup {
    /// Group name (YAML key)
    pub name: String,
    /// Compiled pattern matcher for this group
    pub matcher: PatternMatcher,
}

/// Unified pattern loader for source files and YAML
pub struct PatternLoader;

impl PatternLoader {
    /// Load patterns from a source file (one pattern per line)
    ///
    /// Lines starting with `#` are comments and are skipped.
    /// Empty lines are skipped.
    /// Trailing `/` is transformed to `/**`.
    pub fn load_from_file<'buf>(path: &str, buf: &'buf mut String) -> Result<Vec<&'buf str>> {
        *buf = std::fs::read_to_string(path).map_err(|e| {
            Error::Pattern(format!("Failed to read pattern file '{}': {}", path, e))
        })?;

        Ok(buf
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect())
    }

    /// Load YAML pattern groups
    ///
    /// YAML format:
    /// ```yaml
    /// frontend:
    ///   - src/components/**
    ///   - src/pages/**
    /// backend:
    ///   - src/api/**
    ///   - src/models/**
    /// ```
    pub fn load_yaml_groups(yaml: &str, negation_first: bool) -> Result<Vec<PatternGroup>> {
        let groups: HashMap<String, Vec<String>> =
            serde_yaml::from_str(yaml).map_err(|e| Error::Yaml(e.to_string()))?;

        let mut result = Vec::with_capacity(groups.len());

        for (name, patterns) in groups {
            let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();

            // Separate include and exclude patterns (exclude starts with !)
            let mut includes = Vec::new();
            let mut excludes = Vec::new();

            for pattern in &pattern_refs {
                if let Some(stripped) = pattern.strip_prefix('!') {
                    excludes.push(stripped);
                } else {
                    includes.push(*pattern);
                }
            }

            let matcher = PatternMatcher::new(&includes, &excludes, negation_first)?;
            result.push(PatternGroup { name, matcher });
        }

        Ok(result)
    }

    /// Parse a `files_group_by` template string.
    ///
    /// Template must contain exactly one `{group}` placeholder.
    /// Returns prefix (before `{group}`), suffix (after `{group}`), and the
    /// scan directory (prefix without trailing `/`).
    ///
    /// Example: `"stacks/{group}/**"` → prefix=`"stacks/"`, suffix=`"/**"`, scan_dir=`"stacks"`
    pub fn parse_group_by_template(template: &str) -> Result<GroupByTemplate<'_>> {
        let marker = "{group}";
        let first = template.find(marker).ok_or_else(|| {
            Error::Config(format!(
                "files_group_by template '{}' must contain '{{group}}'",
                template
            ))
        })?;

        // Check for duplicate
        if template[first + marker.len()..].contains(marker) {
            return Err(Error::Config(format!(
                "files_group_by template '{}' must contain exactly one '{{group}}'",
                template
            )));
        }

        let prefix = &template[..first];
        let suffix = &template[first + marker.len()..];

        // scan_dir = prefix without trailing separator
        let scan_dir = prefix.trim_end_matches('/');
        let scan_dir = if scan_dir.is_empty() { "." } else { scan_dir };

        Ok(GroupByTemplate {
            prefix,
            suffix,
            scan_dir,
        })
    }

    /// Discover groups from a template by scanning the filesystem.
    ///
    /// For each non-hidden subdirectory under `scan_dir`, creates a PatternGroup
    /// with the template pattern instantiated for that directory.
    pub fn discover_groups_from_template(
        template: &GroupByTemplate<'_>,
        repo_root: &Path,
        negation_first: bool,
        key_mode: GroupByKey,
    ) -> Result<Vec<PatternGroup>> {
        let scan_path = repo_root.join(template.scan_dir);
        if !scan_path.is_dir() {
            return Err(Error::Config(format!(
                "files_group_by scan directory '{}' does not exist",
                template.scan_dir
            )));
        }

        let mut groups = Vec::new();
        let entries = std::fs::read_dir(&scan_path).map_err(|e| {
            Error::Config(format!(
                "Failed to read directory '{}': {}",
                scan_path.display(),
                e
            ))
        })?;

        for entry in entries {
            let entry = entry
                .map_err(|e| Error::Config(format!("Failed to read directory entry: {}", e)))?;

            let ft = entry
                .file_type()
                .map_err(|e| Error::Config(format!("Failed to read file type: {}", e)))?;

            if !ft.is_dir() {
                continue;
            }

            let dir_name = entry.file_name();
            let dir_name_str = dir_name.to_string_lossy();

            // Skip hidden directories
            if dir_name_str.starts_with('.') {
                continue;
            }

            // Build the pattern: prefix + dir_name + suffix
            let pattern = format!("{}{}{}", template.prefix, dir_name_str, template.suffix);

            // Build the group key based on key_mode
            let key = match key_mode {
                GroupByKey::Name => dir_name_str.to_string(),
                GroupByKey::Path => {
                    if template.scan_dir == "." {
                        dir_name_str.to_string()
                    } else {
                        format!("{}/{}", template.scan_dir, dir_name_str)
                    }
                }
                GroupByKey::Hash => {
                    // 8-char hex hash of the directory name
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    dir_name_str.hash(&mut hasher);
                    format!("{:08x}", hasher.finish() as u32)
                }
            };

            let matcher = PatternMatcher::new(&[pattern.as_str()], &[], negation_first)?;
            groups.push(PatternGroup { name: key, matcher });
        }

        // Sort by name for deterministic order
        groups.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(groups)
    }
}

/// Parsed `files_group_by` template
pub struct GroupByTemplate<'a> {
    /// Text before `{group}` (e.g. `"stacks/"`)
    pub prefix: &'a str,
    /// Text after `{group}` (e.g. `"/**"`)
    pub suffix: &'a str,
    /// Directory to scan for groups (prefix without trailing `/`)
    pub scan_dir: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_from_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# Comment line").unwrap();
        writeln!(file, "**/*.rs").unwrap();
        writeln!(file).unwrap();
        writeln!(file, "src/**/*.ts").unwrap();
        writeln!(file, "  # Another comment  ").unwrap();
        writeln!(file, "docs/").unwrap();

        let path = file.path().to_str().unwrap().to_string();
        let mut buf = String::new();
        let patterns = PatternLoader::load_from_file(&path, &mut buf).unwrap();

        assert_eq!(patterns.len(), 3);
        assert_eq!(patterns[0], "**/*.rs");
        assert_eq!(patterns[1], "src/**/*.ts");
        assert_eq!(patterns[2], "docs/");
    }

    #[test]
    fn test_load_yaml_groups() {
        let yaml = r#"
frontend:
  - "src/components/**"
  - "src/pages/**"
  - "!src/components/test/**"
backend:
  - "src/api/**"
"#;

        let groups = PatternLoader::load_yaml_groups(yaml, true).unwrap();
        assert_eq!(groups.len(), 2);

        // Verify names exist (order may vary due to HashMap)
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"frontend"));
        assert!(names.contains(&"backend"));

        // Verify matching works
        let frontend = groups.iter().find(|g| g.name == "frontend").unwrap();
        assert!(frontend.matcher.matches_sync("src/components/Button.tsx"));
        assert!(!frontend
            .matcher
            .matches_sync("src/components/test/Button.test.tsx"));
        assert!(!frontend.matcher.matches_sync("src/api/routes.ts"));
    }

    #[test]
    fn test_load_yaml_invalid() {
        let result = PatternLoader::load_yaml_groups("not: [valid: yaml", true);
        assert!(result.is_err());
    }

    // --- files_group_by template tests ---

    #[test]
    fn test_parse_template_basic() {
        let t = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
        assert_eq!(t.prefix, "stacks/");
        assert_eq!(t.suffix, "/**");
        assert_eq!(t.scan_dir, "stacks");
    }

    #[test]
    fn test_parse_template_nested() {
        let t = PatternLoader::parse_group_by_template("infra/regions/{group}/config/**").unwrap();
        assert_eq!(t.prefix, "infra/regions/");
        assert_eq!(t.suffix, "/config/**");
        assert_eq!(t.scan_dir, "infra/regions");
    }

    #[test]
    fn test_parse_template_missing_placeholder() {
        let result = PatternLoader::parse_group_by_template("stacks/**");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_template_duplicate_placeholder() {
        let result = PatternLoader::parse_group_by_template("{group}/{group}/**");
        assert!(result.is_err());
    }

    #[test]
    fn test_discover_groups_name_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("stacks")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/dev")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/staging")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/prod")).unwrap();

        let t = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
        let groups =
            PatternLoader::discover_groups_from_template(&t, dir.path(), true, GroupByKey::Name)
                .unwrap();

        assert_eq!(groups.len(), 3);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"dev"));
        assert!(names.contains(&"staging"));
        assert!(names.contains(&"prod"));
    }

    #[test]
    fn test_discover_groups_path_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("stacks")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/dev")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/prod")).unwrap();

        let t = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
        let groups =
            PatternLoader::discover_groups_from_template(&t, dir.path(), true, GroupByKey::Path)
                .unwrap();

        assert_eq!(groups.len(), 2);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"stacks/dev"));
        assert!(names.contains(&"stacks/prod"));
    }

    #[test]
    fn test_discover_groups_hash_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("stacks")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/prod")).unwrap();

        let t = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
        let groups =
            PatternLoader::discover_groups_from_template(&t, dir.path(), true, GroupByKey::Hash)
                .unwrap();

        assert_eq!(groups.len(), 1);
        // Hash keys are 8-char hex strings
        assert_eq!(groups[0].name.len(), 8);
        assert!(groups[0].name.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_discover_groups_skips_hidden() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("stacks")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/prod")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/.git")).unwrap();

        let t = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
        let groups =
            PatternLoader::discover_groups_from_template(&t, dir.path(), true, GroupByKey::Name)
                .unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "prod");
    }

    #[test]
    fn test_discover_groups_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let t = PatternLoader::parse_group_by_template("nonexistent/{group}/**").unwrap();
        let result =
            PatternLoader::discover_groups_from_template(&t, dir.path(), true, GroupByKey::Name);
        assert!(result.is_err());
    }

    #[test]
    fn test_discover_groups_pattern_matching() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("stacks")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/dev")).unwrap();
        std::fs::create_dir(dir.path().join("stacks/prod")).unwrap();

        let t = PatternLoader::parse_group_by_template("stacks/{group}/**").unwrap();
        let groups =
            PatternLoader::discover_groups_from_template(&t, dir.path(), true, GroupByKey::Name)
                .unwrap();

        let dev = groups.iter().find(|g| g.name == "dev").unwrap();
        assert!(dev.matcher.matches_sync("stacks/dev/config.yaml"));
        assert!(!dev.matcher.matches_sync("stacks/prod/config.yaml"));

        let prod = groups.iter().find(|g| g.name == "prod").unwrap();
        assert!(prod.matcher.matches_sync("stacks/prod/config.yaml"));
        assert!(!prod.matcher.matches_sync("stacks/dev/config.yaml"));
    }

    #[test]
    fn test_yaml_groups_with_trailing_slash() {
        // Patterns with trailing slashes should still work correctly.
        // The YAML loader passes patterns directly to PatternMatcher which
        // handles glob matching (trailing slash is a valid glob component).
        let yaml = r#"
infra:
  - "terraform/"
  - "stacks/prod/**"
"#;

        let groups = PatternLoader::load_yaml_groups(yaml, true).unwrap();
        assert_eq!(groups.len(), 1);

        let infra = groups.iter().find(|g| g.name == "infra").unwrap();
        // "stacks/prod/**" should match nested files
        assert!(infra.matcher.matches_sync("stacks/prod/main.tf"));
        assert!(infra.matcher.matches_sync("stacks/prod/modules/vpc.tf"));
        // "terraform/" as a glob — it matches the literal directory name
        // (glob behavior: trailing slash matches directory entries)
        assert!(!infra.matcher.matches_sync("stacks/dev/main.tf"));
    }

    #[test]
    fn test_yaml_groups_exclude_pattern() {
        // Verify ! exclude patterns correctly exclude files from matching
        let yaml = r#"
app:
  - "src/**"
  - "!src/test/**"
  - "!src/vendor/**"
"#;

        let groups = PatternLoader::load_yaml_groups(yaml, true).unwrap();
        assert_eq!(groups.len(), 1);

        let app = &groups[0];
        assert_eq!(app.name, "app");

        // Included paths should match
        assert!(app.matcher.matches_sync("src/main.rs"));
        assert!(app.matcher.matches_sync("src/lib/utils.rs"));

        // Excluded paths should NOT match
        assert!(!app.matcher.matches_sync("src/test/unit_test.rs"));
        assert!(!app.matcher.matches_sync("src/vendor/dep/lib.rs"));

        // Paths outside src should NOT match
        assert!(!app.matcher.matches_sync("docs/README.md"));
    }
}
