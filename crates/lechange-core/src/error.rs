//! Error types for lechange-core

use std::fmt;

/// Result type alias for lechange operations
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for lechange operations
#[derive(Debug)]
pub enum Error {
    /// Git operation error
    Git(String),

    /// Invalid configuration
    Config(String),

    /// Invalid path
    InvalidPath(String),

    /// I/O error
    Io(std::io::Error),

    /// Runtime error (Tokio, threading, etc.)
    Runtime(String),

    /// Pattern matching error
    Pattern(String),

    /// HTTP/API error
    Http(String),

    /// Workflow API error
    Workflow(String),

    /// Workflow timeout error
    WorkflowTimeout(String),

    /// API rate limit exceeded
    RateLimitExceeded(String),

    /// File recovery error
    Recovery(String),

    /// YAML parsing error
    Yaml(String),

    /// GitHub event parsing error
    EventParse(String),

    /// Shallow clone depth exhausted
    ShallowExhausted(String),

    /// Other errors
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Git(msg) => write!(f, "Git error: {}", msg),
            Error::Config(msg) => write!(f, "Configuration error: {}", msg),
            Error::InvalidPath(path) => write!(f, "Invalid path: {}", path),
            Error::Io(err) => write!(f, "I/O error: {}", err),
            Error::Runtime(msg) => write!(f, "Runtime error: {}", msg),
            Error::Pattern(msg) => write!(f, "Pattern error: {}", msg),
            Error::Http(msg) => write!(f, "HTTP error: {}", msg),
            Error::Workflow(msg) => write!(f, "Workflow error: {}", msg),
            Error::WorkflowTimeout(msg) => write!(f, "Workflow timeout: {}", msg),
            Error::RateLimitExceeded(msg) => write!(f, "Rate limit exceeded: {}", msg),
            Error::Recovery(msg) => write!(f, "Recovery error: {}", msg),
            Error::Yaml(msg) => write!(f, "YAML error: {}", msg),
            Error::EventParse(msg) => write!(f, "Event parse error: {}", msg),
            Error::ShallowExhausted(msg) => write!(f, "Shallow clone exhausted: {}", msg),
            Error::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<git2::Error> for Error {
    fn from(err: git2::Error) -> Self {
        Error::Git(err.to_string())
    }
}

impl From<globset::Error> for Error {
    fn from(err: globset::Error) -> Self {
        Error::Pattern(err.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Http(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Other(format!("JSON error: {}", err))
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(err: serde_yaml::Error) -> Self {
        Error::Yaml(err.to_string())
    }
}

/// Fieldless error category for zero-cost pattern matching.
///
/// Single byte representation (`#[repr(u8)]`), `Copy`, no allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorKind {
    /// Git operation error
    Git,
    /// Invalid file path error
    InvalidPath,
    /// Configuration error
    Config,
    /// I/O operation error
    Io,
    /// Runtime error
    Runtime,
    /// Pattern matching error
    Pattern,
    /// HTTP/API error
    Http,
    /// Workflow API error
    Workflow,
    /// Workflow timeout error
    WorkflowTimeout,
    /// API rate limit exceeded
    RateLimitExceeded,
    /// File recovery error
    Recovery,
    /// YAML parsing error
    Yaml,
    /// GitHub event parsing error
    EventParse,
    /// Shallow clone depth exhausted
    ShallowExhausted,
    /// Other errors
    Other,
}

impl Error {
    /// Get the error kind — zero allocation, returns a Copy enum.
    #[inline]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Error::Git(_) => ErrorKind::Git,
            Error::InvalidPath(_) => ErrorKind::InvalidPath,
            Error::Config(_) => ErrorKind::Config,
            Error::Io(_) => ErrorKind::Io,
            Error::Runtime(_) => ErrorKind::Runtime,
            Error::Pattern(_) => ErrorKind::Pattern,
            Error::Http(_) => ErrorKind::Http,
            Error::Workflow(_) => ErrorKind::Workflow,
            Error::WorkflowTimeout(_) => ErrorKind::WorkflowTimeout,
            Error::RateLimitExceeded(_) => ErrorKind::RateLimitExceeded,
            Error::Recovery(_) => ErrorKind::Recovery,
            Error::Yaml(_) => ErrorKind::Yaml,
            Error::EventParse(_) => ErrorKind::EventParse,
            Error::ShallowExhausted(_) => ErrorKind::ShallowExhausted,
            Error::Other(_) => ErrorKind::Other,
        }
    }

    /// Borrow the error message — zero allocation.
    #[inline]
    pub fn message(&self) -> &str {
        match self {
            Error::Git(msg)
            | Error::Config(msg)
            | Error::InvalidPath(msg)
            | Error::Runtime(msg)
            | Error::Pattern(msg)
            | Error::Http(msg)
            | Error::Workflow(msg)
            | Error::WorkflowTimeout(msg)
            | Error::RateLimitExceeded(msg)
            | Error::Recovery(msg)
            | Error::Yaml(msg)
            | Error::EventParse(msg)
            | Error::ShallowExhausted(msg)
            | Error::Other(msg) => msg,
            Error::Io(_) => "I/O error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_kind_is_copy() {
        let err = Error::Git("test".to_string());
        let k = err.kind();
        let k2 = k; // Copy — no move
        assert_eq!(k, k2);
    }

    #[test]
    fn test_error_kind_zero_alloc() {
        // ErrorKind is a fieldless enum — no String data
        assert_eq!(std::mem::size_of::<ErrorKind>(), 1);
    }

    #[test]
    fn test_error_message_borrows() {
        let err = Error::Config("bad config".to_string());
        let msg: &str = err.message();
        assert_eq!(msg, "bad config");
        // msg borrows from err — no allocation
    }

    #[test]
    fn test_all_error_variants_have_kind() {
        let cases: Vec<(Error, ErrorKind)> = vec![
            (Error::Git("g".into()), ErrorKind::Git),
            (Error::InvalidPath("p".into()), ErrorKind::InvalidPath),
            (Error::Config("c".into()), ErrorKind::Config),
            (Error::Io(std::io::Error::other("io")), ErrorKind::Io),
            (Error::Runtime("r".into()), ErrorKind::Runtime),
            (Error::Pattern("pat".into()), ErrorKind::Pattern),
            (Error::Http("h".into()), ErrorKind::Http),
            (Error::Workflow("w".into()), ErrorKind::Workflow),
            (
                Error::WorkflowTimeout("wt".into()),
                ErrorKind::WorkflowTimeout,
            ),
            (
                Error::RateLimitExceeded("rl".into()),
                ErrorKind::RateLimitExceeded,
            ),
            (Error::Recovery("rec".into()), ErrorKind::Recovery),
            (Error::Yaml("y".into()), ErrorKind::Yaml),
            (Error::EventParse("ep".into()), ErrorKind::EventParse),
            (
                Error::ShallowExhausted("se".into()),
                ErrorKind::ShallowExhausted,
            ),
            (Error::Other("o".into()), ErrorKind::Other),
        ];

        for (err, expected_kind) in cases {
            assert_eq!(err.kind(), expected_kind, "Mismatch for {:?}", err);
        }
    }

    #[test]
    fn test_error_kind_repr_u8() {
        assert_eq!(std::mem::size_of::<ErrorKind>(), 1);
    }

    #[test]
    fn test_error_messages_never_contain_token_patterns() {
        // Verify that all error variant messages don't accidentally include
        // GitHub token patterns (ghp_, gho_, ghs_, github_pat_)
        let token_patterns = ["ghp_", "gho_", "ghs_", "github_pat_", "Bearer "];
        let errors: Vec<Error> = vec![
            Error::Git("git error".into()),
            Error::Config("config error".into()),
            Error::Http("http error".into()),
            Error::Workflow("workflow error".into()),
            Error::Runtime("runtime error".into()),
            Error::RateLimitExceeded("rate limit exceeded".into()),
        ];

        for err in &errors {
            let msg = err.message();
            let display = format!("{}", err);
            let debug = format!("{:?}", err);
            for pattern in &token_patterns {
                assert!(
                    !msg.contains(pattern),
                    "Error message contains token pattern '{}': {}",
                    pattern,
                    msg
                );
                assert!(
                    !display.contains(pattern),
                    "Error Display contains token pattern '{}': {}",
                    pattern,
                    display
                );
                assert!(
                    !debug.contains(pattern),
                    "Error Debug contains token pattern '{}': {}",
                    pattern,
                    debug
                );
            }
        }
    }
}
