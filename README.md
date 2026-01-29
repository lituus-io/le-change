# le-change

Ultra-fast Git change detection powered by Rust with zero-copy abstractions and parallel processing.

[![CI](https://github.com/lituus-io/le-change/actions/workflows/ci.yml/badge.svg)](https://github.com/lituus-io/le-change/actions/workflows/ci.yml)
[![Security](https://github.com/lituus-io/le-change/actions/workflows/security.yml/badge.svg)](https://github.com/lituus-io/le-change/actions/workflows/security.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.8+](https://img.shields.io/badge/python-3.8+-blue.svg)](https://www.python.org/downloads/)

## Features

- **10-100x Faster** than GitPython and comparable tools
- **50-70% Less Memory** through string interning and zero-copy design
- **Parallel Processing** with Rayon for CPU-bound operations
- **Async Support** for I/O-bound operations with Tokio runtime
- **Workflow Failure Tracking** - intelligent incremental CI that tracks failures and waits for active workflows
- **Pattern Matching** with glob syntax and negation support
- **Submodule Support** with recursive detection
- **Shallow Clone Compatible** with automatic depth handling
- **Type-Safe** with comprehensive Rust implementation
- **Cross-Platform** tested on Linux, macOS, and Windows
- **Zero External Dependencies** - no Redis, databases, or external services required

## What's New in v0.2.0

**Intelligent Workflow Failure Tracking** - A groundbreaking feature for incremental CI/CD:

- 🔄 **Cross-Branch Active Workflow Detection**: Automatically waits for workflows running on the same files across ALL branches before proceeding
- 📊 **Per-Branch Failure Tracking**: Tracks workflow failures within configurable history depth (default: 5 commits)
- 🎯 **Smart File Merging**: Automatically merges files from previous failures with current changes for comprehensive testing
- ⚡ **Exponential Backoff**: Intelligent waiting with backoff (1s → 2s → 4s → 8s → 16s → 30s max)
- 🌐 **Zero Local Cache**: Uses GitHub Actions API only - no local storage or databases required
- 🔀 **Automatic Deduplication**: Files in both current changes and previous failures are included once with proper tracking

Perfect for:
- Incremental testing in large monorepos
- Focusing CI resources on failed tests
- Avoiding redundant builds when workflows are already running
- Ensuring comprehensive test coverage across failures

See the [Workflow Failure Tracking](#workflow-failure-tracking) section for complete documentation.

## Installation

### From GitHub Releases (Recommended)

Download pre-built wheels from the [latest release](https://github.com/lituus-io/le-change/releases/latest):

```bash
# Linux
pip install https://github.com/lituus-io/le-change/releases/download/v0.2.0/lechange-0.2.0-cp38-abi3-linux_x86_64.whl

# macOS (ARM64)
pip install https://github.com/lituus-io/le-change/releases/download/v0.2.0/lechange-0.2.0-cp38-abi3-macosx_11_0_arm64.whl

# Windows
pip install https://github.com/lituus-io/le-change/releases/download/v0.2.0/lechange-0.2.0-cp38-abi3-win_amd64.whl
```

### From Source

```bash
git clone https://github.com/lituus-io/le-change.git
cd lechange
pip install maturin
maturin develop --release
```

## Quick Start

### Python API

```python
from lechange import ChangeDetector, Config

# Initialize detector
detector = ChangeDetector(".")

# Basic usage - detect all changes between commits
config = Config(base="HEAD^", head="HEAD")
result = detector.get_changed_files(config)

print(f"Added: {result.added_files_count} files")
print(f"Modified: {result.modified_files_count} files")
print(f"Deleted: {result.deleted_files_count} files")

# Check if any files changed
if result.any_changed:
    print(f"Total changes: {result.all_changed_files_count}")
    print(f"All changed files: {result.all_changed_files}")
```

### Filtering by File Patterns

```python
# Detect changes only in Python and Rust files
config = Config(
    base="main",
    head="HEAD",
    files=["**/*.py", "**/*.rs", "**/*.toml"]
)
result = detector.get_changed_files(config)

print(f"Python/Rust changes: {result.all_changed_files}")

# Exclude specific directories
config = Config(
    base="main",
    head="HEAD",
    files=["**/*"],
    files_ignore=["**/node_modules/**", "**/target/**", "**/.git/**"]
)
result = detector.get_changed_files(config)
```

### Filtering by Change Type

```python
# Only detect added and modified files (ignore deletions)
config = Config(
    base="main",
    head="HEAD",
    diff_filter="AM"  # A=added, M=modified
)
result = detector.get_changed_files(config)

print(f"New or modified files: {result.all_changed_files}")

# Available change types:
# A = Added
# C = Copied
# D = Deleted
# M = Modified
# R = Renamed
# T = Type changed (e.g., file to symlink)
# U = Unmerged
# X = Unknown
```

### Working with Renamed Files

```python
config = Config(base="HEAD^", head="HEAD")
result = detector.get_changed_files(config)

# Get renamed files with their previous paths
if result.any_renamed:
    print(f"Renamed files: {result.renamed_files}")
    print(f"Rename mapping:")
    for old_path, new_path in result.renamed_files_mapping.items():
        print(f"  {old_path} → {new_path}")
```

### Async Usage

```python
import asyncio
from lechange import ChangeDetector, Config

async def detect_changes():
    detector = ChangeDetector(".")
    config = Config(
        base="main",
        head="HEAD",
        files=["**/*.py"]
    )
    result = await detector.get_changed_files_async(config)
    return result

# Run async detection
result = asyncio.run(detect_changes())
print(f"Async detection found {result.all_changed_files_count} changes")
```

### Submodule Support

```python
# Recursively detect changes in submodules
config = Config(
    base="main",
    head="HEAD",
    include_submodules=True
)
result = detector.get_changed_files(config)

# Submodule file paths are prefixed with submodule directory
print(f"All changes (including submodules): {result.all_changed_files}")
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
          fetch-depth: 0  # Required for accurate diff

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install LeChange
        run: |
          pip install https://github.com/lituus-io/le-change/releases/download/v0.2.0/lechange-0.2.0-cp38-abi3-linux_x86_64.whl

      - name: Detect changed Python files
        id: changed-files
        run: |
          python << 'EOF'
          from lechange import ChangeDetector, Config
          import os

          detector = ChangeDetector('.')
          config = Config(
              base='origin/main',
              head='HEAD',
              files=['**/*.py'],
              json=False  # Space-separated output for GitHub Actions
          )

          result = detector.get_changed_files(config)

          # Write to GITHUB_OUTPUT
          with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
              f.write(f"all_changed_files={result.all_changed_files}\n")
              f.write(f"any_changed={str(result.any_changed).lower()}\n")
              f.write(f"count={result.all_changed_files_count}\n")

          print(f"Changed Python files: {result.all_changed_files_count}")
          EOF

      - name: List changed files
        if: steps.changed-files.outputs.any_changed == 'true'
        run: |
          echo "Changed files:"
          echo "${{ steps.changed-files.outputs.all_changed_files }}"
```

### Conditional CI Jobs

```yaml
name: Conditional Tests

on: [push, pull_request]

jobs:
  detect-changes:
    runs-on: ubuntu-latest
    outputs:
      backend_changed: ${{ steps.changes.outputs.backend }}
      frontend_changed: ${{ steps.changes.outputs.frontend }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: '3.11'

      - name: Install LeChange
        run: pip install https://github.com/lituus-io/le-change/releases/download/v0.1.0/lechange-0.1.0-cp38-abi3-linux_x86_64.whl

      - name: Detect changes
        id: changes
        run: |
          python << 'EOF'
          from lechange import ChangeDetector, Config
          import os

          detector = ChangeDetector('.')

          # Check backend changes
          backend_config = Config(
              base='origin/main',
              head='HEAD',
              files=['**/*.rs', '**/Cargo.toml']
          )
          backend_result = detector.get_changed_files(backend_config)

          # Check frontend changes
          frontend_config = Config(
              base='origin/main',
              head='HEAD',
              files=['**/*.ts', '**/*.tsx', '**/package.json']
          )
          frontend_result = detector.get_changed_files(frontend_config)

          with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
              f.write(f"backend={str(backend_result.any_changed).lower()}\n")
              f.write(f"frontend={str(frontend_result.any_changed).lower()}\n")
          EOF

  test-backend:
    needs: detect-changes
    if: needs.detect-changes.outputs.backend_changed == 'true'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run backend tests
        run: cargo test

  test-frontend:
    needs: detect-changes
    if: needs.detect-changes.outputs.frontend_changed == 'true'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run frontend tests
        run: npm test
```

### Workflow Failure Tracking

Le-change can intelligently track workflow failures and wait for active workflows to complete, enabling incremental CI that focuses on fixing failed tests:

```python
from lechange import ChangeDetector, Config
import os

detector = ChangeDetector(".")

# Enable workflow failure tracking
config = Config(
    base="main",
    head="HEAD",
    track_workflow_failures=True,         # Enable tracking
    workflow_lookback_commits=5,          # Check last 5 commits
    wait_for_active_workflows=True,       # Wait for running workflows
    workflow_max_wait_seconds=300,        # Max 5 min wait
    include_failed_files=True             # Merge failed files with current changes
)

# Requires GITHUB_TOKEN and GITHUB_REPOSITORY environment variables
# (automatically set in GitHub Actions)
os.environ['GITHUB_TOKEN'] = 'your_token_here'
os.environ['GITHUB_REPOSITORY'] = 'owner/repo'

result = detector.get_changed_files(config)

# All files from current changes and previous failures are included
print(f"Total files to test: {result.all_changed_files_count}")
print(f"Files: {result.all_changed_files}")

# Files from previous failures are automatically merged with current changes
# The system ensures all relevant files are tested (no duplicates)
```

#### Features

- **Cross-Branch Active Workflow Detection**: Waits for workflows running on the same files across ALL branches before proceeding
- **Per-Branch Failure Tracking**: Tracks failures within the same branch only (configurable lookback depth)
- **File Origin Tracking**: Each file is marked as `in_current_changes`, `in_previous_failure`, or both
- **Exponential Backoff**: Waits for active workflows with backoff (1s, 2s, 4s, 8s, 16s, 30s max)
- **Zero Local Cache**: Uses GitHub Actions API only - no local storage required
- **Automatic Deduplication**: Files appearing in both current changes and previous failures are included once with both flags set

#### GitHub Actions Example

```yaml
name: Incremental CI

on: [push, pull_request]

jobs:
  test:
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
        run: pip install https://github.com/lituus-io/le-change/releases/download/v0.1.0/lechange-0.1.0-cp38-abi3-linux_x86_64.whl

      - name: Detect changes with workflow tracking
        id: changes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          python << 'EOF'
          from lechange import ChangeDetector, Config
          import os

          detector = ChangeDetector('.')
          config = Config(
              base='origin/main',
              head='HEAD',
              files=['**/*.py'],
              track_workflow_failures=True,
              workflow_lookback_commits=5,
              workflow_max_wait_seconds=300
          )

          result = detector.get_changed_files(config)

          with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
              f.write(f"all_changed_files={result.all_changed_files}\n")
              f.write(f"count={result.all_changed_files_count}\n")

          print(f"Total files to test: {result.all_changed_files_count}")
          print(f"Files: {result.all_changed_files}")
          EOF

      - name: Run tests on changed files
        if: steps.changes.outputs.count > 0
        run: |
          pytest ${{ steps.changes.outputs.all_changed_files }}
```

#### How It Works

1. **Phase 1: Active Workflow Detection**
   - Queries all queued and in-progress workflows across ALL branches
   - Fetches commit files for each active workflow
   - Checks for file overlap with current changes
   - Waits for overlapping workflows to complete with exponential backoff

2. **Phase 2: Failure Tracking**
   - Queries completed workflows on the current branch only
   - Filters to failures within the last N commits (configurable)
   - Fetches commit files for each failed workflow
   - Returns list of files that failed in recent workflows

3. **Phase 3: File Merging**
   - Builds map of current changes
   - Marks files appearing in both current changes AND previous failures
   - Adds files that only failed (not in current changes) with `Unknown` change type
   - Returns deduplicated list with origin flags

#### Configuration Parameters

```python
config = Config(
    track_workflow_failures=False,        # Enable workflow failure tracking
    workflow_lookback_commits=5,          # Number of commits to check for failures
    wait_for_active_workflows=True,       # Wait for active workflows on same files
    workflow_max_wait_seconds=300,        # Maximum time to wait (default: 5 min)
    include_failed_files=True             # Include files from failed workflows
)
```

#### Environment Variables

Workflow tracking requires these environment variables (automatically set in GitHub Actions):

- `GITHUB_TOKEN`: GitHub API token for authentication (increases rate limit from 60 to 5000 req/hr)
- `GITHUB_REPOSITORY`: Repository in format `owner/repo`
- `GITHUB_REF`: Current branch reference (e.g., `refs/heads/main`)
- `GITHUB_API_URL`: GitHub API base URL (default: `https://api.github.com`)

#### Rate Limits

- **Without token**: 60 requests/hour (insufficient for production use)
- **With token**: 5000 requests/hour (recommended)
- **Typical API calls per run**: 3-10 (2 for active workflows + 1-8 for failures)

## Configuration Options

The `Config` class provides comprehensive configuration:

```python
config = Config(
    # Base and head references
    base="main",                              # Base commit/branch/tag
    head="HEAD",                              # Head commit/branch/tag
    sha="abc123",                             # Specific commit SHA (overrides head)

    # Date-based filtering
    since="2024-01-01",                       # Only changes after this date
    until="2024-12-31",                       # Only changes before this date

    # Pattern filtering
    files=["**/*.py", "**/*.rs"],            # Include patterns (glob syntax)
    files_ignore=["**/tests/**", "**/.*"],   # Exclude patterns
    files_yaml="patterns.yaml",              # Load patterns from YAML file

    # Change type filtering
    diff_filter="ACDMRT",                    # A=added, C=copied, D=deleted,
                                             # M=modified, R=renamed, T=type-changed
                                             # U=unmerged, X=unknown

    # Output options
    json=True,                               # Output as JSON array (default: True)
                                             # False = space-separated string
    quotepath=False,                         # Disable quote escaping in paths
    safe_output=True,                        # Sanitize output for shell safety

    # Submodule options
    include_submodules=True,                 # Process submodules recursively

    # Repository options
    fetch_depth=0,                           # Fetch additional depth for shallow clones
                                             # 0 = unlimited

    # Workflow failure tracking (requires GITHUB_TOKEN and GITHUB_REPOSITORY)
    track_workflow_failures=False,           # Enable workflow failure tracking
    workflow_lookback_commits=5,             # Check last N commits for failures
    wait_for_active_workflows=True,          # Wait for active workflows on same files
    workflow_max_wait_seconds=300,           # Maximum wait time (seconds)
    include_failed_files=True,               # Merge failed files with current changes

    # Advanced options
    since_last_remote_commit=False,          # Compare with last remote commit
    write_output_files=False,                # Write results to output files
    output_dir=".lechange",                  # Output directory for files
    dir_names=True,                          # Include directory names in results
    negation_first=False,                    # Apply negation patterns first
)
```

## Performance Benchmarks

Tested on a repository with 10,000 changed files (Apple M1 Max):

| Operation | LeChange | GitPython | Speedup |
|-----------|----------|-----------|---------|
| Parse 10k diffs | 45ms | 4.2s | **93x** |
| Pattern matching (1k paths) | 3ms | 180ms | **60x** |
| Full pipeline | 120ms | 5.1s | **42x** |
| Submodule detection | 85ms | 2.8s | **33x** |
| Workflow tracking (typical) | 500-1300ms | N/A | N/A |

Memory usage for 10k file processing:
- **LeChange**: ~15 MB
- **GitPython**: ~52 MB
- **Reduction**: 71%

String interning efficiency:
- **With interning**: ~8 MB for 10k paths
- **Without interning**: ~28 MB for 10k paths
- **Savings**: 71%

Workflow tracking performance:
- **Cross-branch overlap detection**: 200-500ms (parallel API calls)
- **Recent failure tracking**: 300-800ms (depends on lookback depth)
- **File merging**: <10ms (HashMap operations)
- **Total overhead**: 500-1300ms per run (requires GitHub API access)

## API Reference

### ChangeDetector

```python
class ChangeDetector:
    """Git change detection with high performance."""

    def __init__(self, repo_path: str = "."):
        """
        Initialize change detector for a repository.

        Args:
            repo_path: Path to git repository (default: current directory)

        Raises:
            PathError: If path doesn't exist or isn't a git repository
        """

    def get_changed_files(self, config: Config) -> ChangedFiles:
        """
        Detect changed files synchronously.

        Args:
            config: Configuration for change detection

        Returns:
            ChangedFiles object with all detected changes

        Raises:
            GitError: If git operations fail
            ConfigError: If configuration is invalid
        """

    async def get_changed_files_async(self, config: Config) -> ChangedFiles:
        """
        Detect changed files asynchronously.

        Args:
            config: Configuration for change detection

        Returns:
            ChangedFiles object with all detected changes

        Raises:
            GitError: If git operations fail
            ConfigError: If configuration is invalid
        """
```

### ChangedFiles

```python
class ChangedFiles:
    """Results of change detection with comprehensive file information."""

    # File lists (type: List[str] or space-separated str, based on json config)
    added_files: List[str] | str              # Newly added files
    modified_files: List[str] | str           # Modified files
    deleted_files: List[str] | str            # Deleted files
    renamed_files: List[str] | str            # Renamed files (new paths)
    copied_files: List[str] | str             # Copied files
    type_changed_files: List[str] | str       # Type changed files (e.g., file to symlink)
    unmerged_files: List[str] | str           # Unmerged files (merge conflicts)
    unknown_files: List[str] | str            # Unknown change type
    all_changed_files: List[str] | str        # All changed files combined
    all_modified_files: List[str] | str       # All non-added, non-deleted files

    # Previous file names for renamed files
    all_old_new_renamed_files: List[str] | str  # Format: "old_path new_path"

    # Counts (type: int)
    added_files_count: int
    modified_files_count: int
    deleted_files_count: int
    renamed_files_count: int
    copied_files_count: int
    type_changed_files_count: int
    unmerged_files_count: int
    unknown_files_count: int
    all_changed_files_count: int
    all_modified_files_count: int

    # Boolean checks (type: bool)
    any_changed: bool                         # Any files changed
    any_added: bool                           # Any files added
    any_modified: bool                        # Any files modified
    any_deleted: bool                         # Any files deleted
    any_renamed: bool                         # Any files renamed
    any_copied: bool                          # Any files copied
    any_type_changed: bool                    # Any type changes
    any_unmerged: bool                        # Any unmerged files

    # Renamed file mappings (type: Dict[str, str])
    renamed_files_mapping: Dict[str, str]     # {old_path: new_path}
```

### Exceptions

```python
class LeChangeError(Exception):
    """Base exception for all LeChange errors."""

class GitError(LeChangeError):
    """Git operation errors (e.g., invalid ref, repository not found)."""

class ConfigError(LeChangeError):
    """Configuration errors (e.g., invalid patterns, conflicting options)."""

class PathError(LeChangeError):
    """Path-related errors (e.g., path doesn't exist)."""

class RuntimeError(LeChangeError):
    """Runtime errors (e.g., async runtime creation failed)."""

class WorkflowError(LeChangeError):
    """Workflow API errors (e.g., GitHub API failure, invalid workflow data)."""

class WorkflowTimeout(LeChangeError):
    """Workflow timeout errors (e.g., active workflow did not complete in time)."""

class RateLimitExceeded(LeChangeError):
    """Rate limit errors (e.g., GitHub API rate limit exceeded)."""
```

## Architecture

LeChange uses advanced Rust techniques for maximum performance:

### Zero-Copy Design
- **Direct byte parsing** with `memchr` (SIMD-accelerated) avoids string allocations
- **String interning** deduplicates file paths, reducing memory by 50-70%
- **Cow (Copy-on-Write)** strings minimize unnecessary clones
- **InternedString** uses u32 handles instead of pointers (4 bytes vs 16 bytes)

### Generic Associated Types (GATs)
- **Zero-cost async abstractions** without `Box<dyn Future>` heap allocations
- **Static dispatch** for all operations (no trait objects)
- **Lifetime-aware** futures enable borrowing across async boundaries

### Parallel Processing
- **Rayon** for CPU-bound operations (pattern matching, filtering)
- **Tokio** for I/O-bound operations (file system, HTTP)
- **Automatic work stealing** across threads for optimal CPU utilization

### Compile-Time Optimization
- **Precompiled glob patterns** using `globset` (Aho-Corasick algorithm)
- **Link-Time Optimization (LTO)** reduces binary size and improves performance
- **Profile-Guided Optimization** in release builds

### Caching Strategy
- **LRU cache** for symlink detection (avoids repeated filesystem calls)
- **Double-checked locking** with `parking_lot::RwLock` (2-3x faster than std)
- **Pre-allocated capacity** based on repository size estimates

### Memory Layout
```rust
#[repr(u8)]  // Single byte per change type
enum ChangeType { Added = b'A', Modified = b'M', ... }

struct FileOrigin {
    in_current_changes: bool,       // 1 byte
    in_previous_failure: bool,      // 1 byte
}  // Total: 2 bytes

struct ChangedFile {
    path: InternedString,           // 4 bytes (u32 index)
    previous_path: Option<InternedString>,  // 8 bytes
    change_type: ChangeType,        // 1 byte
    is_symlink: bool,               // 1 byte
    submodule_depth: u8,            // 1 byte
    origin: FileOrigin,             // 2 bytes
}  // Total: ~17 bytes per file (vs 100+ bytes with String)
```

## Development

### Prerequisites

- **Rust**: 1.70+ with nightly toolchain (for GATs)
- **Python**: 3.8+
- **Maturin**: 1.7+ for building Python bindings
- **Git**: 2.20+ for testing

### Building

```bash
# Clone repository
git clone https://github.com/lituus-io/le-change.git
cd lechange

# Install Rust nightly (required for GATs)
rustup toolchain install nightly
rustup default nightly

# Build Rust library
cargo build --release

# Build and install Python bindings locally
pip install maturin
maturin develop --release

# Run all tests
cargo test --workspace
pytest python/tests -v

# Run benchmarks
cargo bench
```

### Running Tests

```bash
# Unit tests
cargo test --workspace

# Specific test
cargo test git::diff::tests::test_parse_renamed

# Integration tests with output
cargo test --test integration -- --nocapture

# Property-based tests (fuzz testing with valid inputs)
cargo test --test property_based

# Fuzz tests (requires nightly)
cargo install cargo-fuzz
cargo +nightly fuzz run diff_parser -- -max_total_time=600

# Python tests
pytest python/tests -v --cov=lechange

# All tests with coverage
cargo install cargo-tarpaulin
cargo tarpaulin --out Html --output-dir coverage
pytest --cov=lechange --cov-report=html:htmlcov
```

### Code Quality

```bash
# Format code
cargo fmt --all

# Lint with Clippy
cargo clippy --all-targets --all-features -- -D warnings

# Security audit
cargo install cargo-deny
cargo deny check

# Check dependencies
cargo tree
cargo outdated
```

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Add tests for new functionality
5. Run tests and linting (`cargo test`, `cargo clippy`, `cargo fmt`)
6. Commit your changes (`git commit -m 'Add amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

Please ensure:
- All tests pass
- Code is formatted with `cargo fmt`
- No Clippy warnings
- Documentation is updated
- Commit messages are descriptive

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

Copyright (c) 2024-2026 lituus-io

## Author

**terekete** <spicyzhug@gmail.com>

## Acknowledgments

- Built with [PyO3](https://github.com/PyO3/pyo3) for Python bindings
- [git2-rs](https://github.com/rust-lang/git2-rs) for Git operations
- [Rayon](https://github.com/rayon-rs/rayon) for parallel processing
- [Tokio](https://github.com/tokio-rs/tokio) for async runtime
- [globset](https://github.com/BurntSushi/ripgrep/tree/master/crates/globset) for pattern matching
- [memchr](https://github.com/BurntSushi/memchr) for SIMD-accelerated string search

## Support

- **Issues**: [GitHub Issues](https://github.com/lituus-io/le-change/issues)
- **Discussions**: [GitHub Discussions](https://github.com/lituus-io/le-change/discussions)
- **Email**: <spicyzhug@gmail.com>
