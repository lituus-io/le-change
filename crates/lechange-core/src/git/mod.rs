//! Git operations module

pub mod diff;
pub mod recovery;
pub mod repository;
pub mod sha;
pub mod submodule;

pub use recovery::FileRecovery;
pub use repository::GitRepository;
pub use sha::ShaResolver;
pub use submodule::{SubmoduleInfo, SubmoduleProcessor};
