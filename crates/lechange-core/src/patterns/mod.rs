//! Pattern matching module

pub mod loader;
pub mod matcher;

pub use loader::{PatternGroup, PatternLoader};
pub use matcher::PatternMatcher;
