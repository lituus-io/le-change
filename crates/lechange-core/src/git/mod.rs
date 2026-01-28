//! Git operations module

pub mod repository;
pub mod diff;
pub mod sha;
pub mod submodule;

pub use repository::GitRepository;
pub use sha::ShaResolver;
pub use submodule::{SubmoduleProcessor, SubmoduleInfo};
