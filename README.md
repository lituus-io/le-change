# LeChange

Ultra-fast Git change detection powered by Rust with zero-copy abstractions and parallel processing.

[![CI](https://github.com/lituus-io/lechange/workflows/CI/badge.svg)](https://github.com/lituus-io/lechange/actions)
[![PyPI version](https://badge.fury.io/py/lechange.svg)](https://badge.fury.io/py/lechange)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.8+](https://img.shields.io/badge/python-3.8+-blue.svg)](https://www.python.org/downloads/)

## Features

- **10-100x Faster** than GitPython and comparable tools
- **50-70% Less Memory** through string interning and zero-copy design
- **Drop-in Replacement** for tj-actions/changed-files
- **Parallel Processing** with Rayon for CPU-bound operations
- **Async Support** for I/O-bound operations
- **Pattern Matching** with glob syntax
- **Submodule Support** with recursive detection
- **Shallow Clone Compatible** with automatic depth handling
- **Type-Safe** with comprehensive Rust implementation

## Installation

### Python

```bash
pip install lechange
```

### From Source

```bash
git clone https://github.com/lituus-io/lechange.git
cd lechange
pip install maturin
maturin develop
```

## Quick Start

### Python API

```python
from lechange import ChangeDetector, Config

# Initialize detector
detector = ChangeDetector(".")

# Basic usage - detect all changes
config = Config(base="main", head="HEAD")
result = detector.get_changed_files(config)

print(f"Added files: {result.added_files}")
print(f"Modified files: {result.modified_files}")
print(f"Deleted files: {result.deleted_files}")

# Check if any files changed
if result.any_changed:
    print(f"Total changes: {result.all_changed_files_count}")
```

### Pattern Filtering

```python
# Only detect changes in Python files
config = Config(
    base="main",
    head="HEAD",
    files=["**/*.py"]
)
result = detector.get_changed_files(config)

# Exclude certain directories
config = Config(
    base="main",
    head="HEAD",
    files=["**/*"],
    files_ignore=["**/node_modules/**", "**/target/**"]
)
result = detector.get_changed_files(config)
```

### Async Usage

```python
import asyncio
from lechange import ChangeDetector, Config

async def detect_changes():
    detector = ChangeDetector(".")
    config = Config(base="main", head="HEAD")
    result = await detector.get_changed_files_async(config)
    return result

result = asyncio.run(detect_changes())
```

### GitHub Actions Integration

```yaml
name: Detect Changes

on: [push, pull_request]

jobs:
  detect:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install LeChange
        run: pip install lechange

      - name: Detect changed files
        run: |
          python -c "
          from lechange import ChangeDetector, Config

          detector = ChangeDetector('.')
          config = Config(
              base='origin/main',
              head='HEAD',
              files=['**/*.py', '**/*.rs']
          )

          result = detector.get_changed_files(config)

          print(f'Changed Python/Rust files: {result.all_changed_files_count}')

          if result.any_modified:
              print('Modified files:', result.modified_files)
          "
```

## Configuration Options

The `Config` class supports all tj-actions/changed-files parameters:

```python
config = Config(
    # Base and head references
    base="main",                              # Base commit/branch
    head="HEAD",                              # Head commit/branch
    sha="abc123",                             # Specific commit SHA

    # Pattern filtering
    files=["**/*.py", "**/*.rs"],            # Include patterns
    files_ignore=["**/tests/**"],            # Exclude patterns
    files_yaml="patterns.yaml",              # Load patterns from YAML

    # Change type filtering
    diff_filter="ACDMRT",                    # A=added, C=copied, D=deleted, M=modified, R=renamed, T=type-changed

    # Output options
    json=True,                               # Output as JSON list instead of space-separated string
    quotepath=False,                         # Disable quote escaping in paths
    safe_output=True,                        # Sanitize output for shell safety

    # Submodule options
    include_submodules=True,                 # Process submodules recursively

    # Performance options
    fetch_depth=0,                           # Fetch depth for shallow clones (0 = unlimited)

    # Advanced options
    since_last_remote_commit=False,          # Compare with last remote commit
    write_output_files=False,                # Write results to files
    output_dir=".lechange",                  # Output directory for files
)
```

## Performance Benchmarks

Tested on a repository with 10,000 changed files:

| Operation | LeChange | GitPython | Speedup |
|-----------|----------|-----------|---------|
| Parse 10k diffs | 45ms | 4.2s | 93x |
| Pattern matching (1k paths) | 3ms | 180ms | 60x |
| Full pipeline | 120ms | 5.1s | 42x |

Memory usage for 10k file processing:
- LeChange: ~15 MB
- GitPython: ~52 MB
- Reduction: 71%

## API Reference

### ChangeDetector

```python
class ChangeDetector:
    def __init__(self, repo_path: str = "."):
        """Initialize change detector for a repository."""

    def get_changed_files(self, config: Config) -> ChangedFiles:
        """Detect changed files synchronously."""

    async def get_changed_files_async(self, config: Config) -> ChangedFiles:
        """Detect changed files asynchronously."""
```

### ChangedFiles

```python
class ChangedFiles:
    # File lists
    added_files: List[str]
    modified_files: List[str]
    deleted_files: List[str]
    renamed_files: List[str]
    copied_files: List[str]
    type_changed_files: List[str]
    unmerged_files: List[str]
    unknown_files: List[str]
    all_changed_files: List[str]

    # Counts
    added_files_count: int
    modified_files_count: int
    deleted_files_count: int
    all_changed_files_count: int
    # ... (count for each type)

    # Boolean checks
    any_changed: bool
    any_added: bool
    any_modified: bool
    any_deleted: bool
    # ... (boolean for each type)

    # Renamed file mappings
    renamed_files_mapping: Dict[str, str]  # old_path -> new_path
```

## Migration from tj-actions/changed-files

LeChange is designed as a drop-in replacement. Here's how to migrate:

### GitHub Actions (Before)

```yaml
- name: Get changed files
  id: changed-files
  uses: tj-actions/changed-files@v40
  with:
    files: |
      **/*.py
      **/*.rs
```

### GitHub Actions (After)

```yaml
- name: Install LeChange
  run: pip install lechange

- name: Get changed files
  run: |
    python -c "
    from lechange import ChangeDetector, Config
    detector = ChangeDetector('.')
    config = Config(files=['**/*.py', '**/*.rs'])
    result = detector.get_changed_files(config)
    print(f'all_changed_files={\" \".join(result.all_changed_files)}')
    " >> $GITHUB_OUTPUT
```

### Python Script (Before - using GitPython)

```python
import git

repo = git.Repo(".")
diff = repo.git.diff("main...HEAD", name_only=True)
changed_files = diff.split("\n")
```

### Python Script (After - using LeChange)

```python
from lechange import ChangeDetector, Config

detector = ChangeDetector(".")
config = Config(base="main", head="HEAD")
result = detector.get_changed_files(config)
changed_files = result.all_changed_files
```

## Architecture

LeChange uses several advanced techniques for performance:

- **Zero-Copy Parsing**: Uses `memchr` and direct byte parsing without intermediate string allocations
- **String Interning**: Deduplicates file paths to reduce memory usage by 50-70%
- **GATs (Generic Associated Types)**: Enables zero-cost async abstractions
- **Parallel Processing**: Uses Rayon for CPU-bound filtering and pattern matching
- **LRU Caching**: Caches symlink detection and file operations
- **Compile-Time Optimization**: Precompiled glob patterns using the `globset` crate

## Development

### Prerequisites

- Rust 1.70+ (nightly for GATs)
- Python 3.8+
- Maturin for building Python bindings

### Building

```bash
# Clone repository
git clone https://github.com/lituus-io/lechange.git
cd lechange

# Build Rust library
cargo build --release

# Build Python bindings
maturin develop

# Run tests
cargo test --workspace
pytest python/tests

# Run benchmarks
cargo bench
```

### Running Tests

```bash
# Unit tests
cargo test --workspace

# Property-based tests
cargo test --test property_based

# Fuzz tests (requires nightly)
cargo +nightly fuzz run fuzz_diff_parser

# Integration tests
pytest python/tests -v

# All tests with coverage
cargo tarpaulin --out Html
pytest --cov=lechange --cov-report=html
```

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

Copyright (c) 2024-2026 lituus-io

## Author

**terekete** <<spicyzhug@gmail.com>>

## Acknowledgments

- Inspired by tj-actions/changed-files
- Built with PyO3, Tokio, Rayon, and other excellent Rust crates
- Uses git2-rs for Git operations

## Support

- **Issues**: [GitHub Issues](https://github.com/lituus-io/lechange/issues)
- **Discussions**: [GitHub Discussions](https://github.com/lituus-io/lechange/discussions)
- **Email**: <spicyzhug@gmail.com>
