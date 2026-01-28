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

#[derive(Debug)]
pub enum ErrorKind {
    Git(String),
    InvalidPath(String),
    Config(String),
    Io(std::io::Error),
    Runtime(String),
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Git(msg) => ErrorKind::Git(msg.clone()),
            Error::InvalidPath(path) => ErrorKind::InvalidPath(path.clone()),
            Error::Config(msg) => ErrorKind::Config(msg.clone()),
            Error::Io(err) => ErrorKind::Io(std::io::Error::new(err.kind(), err.to_string())),
            Error::Runtime(msg) => ErrorKind::Runtime(msg.clone()),
            Error::Pattern(msg) => ErrorKind::Config(msg.clone()),
            Error::Http(msg) => ErrorKind::Runtime(msg.clone()),
            Error::Other(msg) => ErrorKind::Runtime(msg.clone()),
        }
    }
}
