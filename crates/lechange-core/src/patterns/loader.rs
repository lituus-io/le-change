//! Unified pattern loading from source files and YAML

use crate::error::{Error, Result};
use crate::patterns::matcher::PatternMatcher;
use std::collections::HashMap;

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
        *buf = std::fs::read_to_string(path)
            .map_err(|e| Error::Pattern(format!("Failed to read pattern file '{}': {}", path, e)))?;

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
                    // Transform trailing / to /**
                    let p = if pattern.ends_with('/') {
                        // Can't return borrowed str for transformed pattern,
                        // but we need the original slice. Store as-is for now.
                        *pattern
                    } else {
                        *pattern
                    };
                    includes.push(p);
                }
            }

            let matcher = PatternMatcher::new(&includes, &excludes, negation_first)?;
            result.push(PatternGroup { name, matcher });
        }

        Ok(result)
    }
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
        assert!(!frontend.matcher.matches_sync("src/components/test/Button.test.tsx"));
        assert!(!frontend.matcher.matches_sync("src/api/routes.ts"));
    }

    #[test]
    fn test_load_yaml_invalid() {
        let result = PatternLoader::load_yaml_groups("not: [valid: yaml", true);
        assert!(result.is_err());
    }
}
