//! Git operations module

pub mod diff;
pub mod repository;
pub mod sha;
pub mod submodule;

pub use repository::GitRepository;
pub use sha::ShaResolver;
pub use submodule::{SubmoduleInfo, SubmoduleProcessor};
