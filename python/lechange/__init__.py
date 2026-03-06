"""
LeChange - Ultra-fast Git change detection powered by Rust.

Copyright (c) 2024-2026 Lituus-io. All rights reserved.
Author: terekete <spicyzhug@gmail.com>
License: AGPL-3.0-or-later (commercial license available)
"""

from lechange._lechange import (
    ChangeDetector,
    Config,
    ChangedFiles,
    PatternMatcher,
    PathUtil,
    FileRecovery,
    OutputWriter,
    escape_json,
    safe_output_escape,
    format_json_array,
    format_matrix,
    load_yaml_patterns,
    LeChangeError,
    GitError,
    ConfigError,
    PathError,
    RuntimeError as LeChangeRuntimeError,
    RecoveryError,
    YamlError,
    ShallowCloneError,
)

__version__ = "0.1.0"
__author__ = "terekete"
__email__ = "spicyzhug@gmail.com"
__license__ = "AGPL-3.0-or-later"

__all__ = [
    "ChangeDetector",
    "Config",
    "ChangedFiles",
    "PatternMatcher",
    "PathUtil",
    "FileRecovery",
    "OutputWriter",
    "escape_json",
    "safe_output_escape",
    "format_json_array",
    "format_matrix",
    "load_yaml_patterns",
    "LeChangeError",
    "GitError",
    "ConfigError",
    "PathError",
    "LeChangeRuntimeError",
    "RecoveryError",
    "YamlError",
    "ShallowCloneError",
    "__version__",
]
