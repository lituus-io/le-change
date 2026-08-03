// Copyright (c) 2024-2026 Lituus-io. All rights reserved.
//! Git operations module

pub mod recovery;
pub mod repository;
pub mod sha;
pub mod submodule;
pub mod vanished;

pub use recovery::FileRecovery;
pub use repository::GitRepository;
pub use sha::ShaResolver;
pub use submodule::{SubmoduleInfo, SubmoduleProcessor};
pub use vanished::{pathspec_prefixes, VanishedDetector, VanishedScan};
