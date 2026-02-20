//! File output writer for writing results to files

use crate::error::Result;
use std::path::Path;

/// Output file writer
pub struct OutputWriter;

impl OutputWriter {
    /// Write a list of values to a text file
    pub fn write_text(
        output_dir: &Path,
        name: &str,
        values: &[&str],
        separator: &str,
    ) -> Result<()> {
        let path = output_dir.join(format!("{}.txt", name));
        let content = values.join(separator);
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Write a JSON array to a file
    pub fn write_json(output_dir: &Path, name: &str, values: &[&str]) -> Result<()> {
        let path = output_dir.join(format!("{}.json", name));
        let content = super::json_format::format_json_array(values);
        std::fs::write(&path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_text() {
        let dir = TempDir::new().unwrap();
        OutputWriter::write_text(dir.path(), "files", &["a.rs", "b.rs", "c.rs"], "\n").unwrap();
        let content = std::fs::read_to_string(dir.path().join("files.txt")).unwrap();
        assert_eq!(content, "a.rs\nb.rs\nc.rs");
    }

    #[test]
    fn test_write_text_custom_separator() {
        let dir = TempDir::new().unwrap();
        OutputWriter::write_text(dir.path(), "files", &["a.rs", "b.rs"], ",").unwrap();
        let content = std::fs::read_to_string(dir.path().join("files.txt")).unwrap();
        assert_eq!(content, "a.rs,b.rs");
    }

    #[test]
    fn test_write_json() {
        let dir = TempDir::new().unwrap();
        OutputWriter::write_json(dir.path(), "files", &["a.rs", "b.rs"]).unwrap();
        let content = std::fs::read_to_string(dir.path().join("files.json")).unwrap();
        assert_eq!(content, r#"["a.rs","b.rs"]"#);
    }

    #[test]
    fn test_write_json_empty() {
        let dir = TempDir::new().unwrap();
        OutputWriter::write_json(dir.path(), "files", &[]).unwrap();
        let content = std::fs::read_to_string(dir.path().join("files.json")).unwrap();
        assert_eq!(content, "[]");
    }

    #[test]
    fn test_write_json_escaping() {
        let dir = TempDir::new().unwrap();
        OutputWriter::write_json(dir.path(), "files", &["path\"with\"quotes"]).unwrap();
        let content = std::fs::read_to_string(dir.path().join("files.json")).unwrap();
        assert!(content.contains(r#"\""#));
        // Verify it's valid JSON structure
        assert!(content.starts_with('['));
        assert!(content.ends_with(']'));
    }
}
