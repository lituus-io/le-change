# Contributing to LeChange

Thank you for your interest in contributing to LeChange! This document provides guidelines and instructions for contributing.

## Code of Conduct

This project adheres to a Code of Conduct that all contributors are expected to follow. Please read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing.

## Getting Started

### Prerequisites

- Rust 1.70+ (nightly toolchain for GATs)
- Python 3.8+
- Git 2.18+
- Maturin for building Python bindings

### Setting Up Development Environment

```bash
# Clone the repository
git clone https://github.com/lituus-io/lechange.git
cd lechange

# Install Rust nightly
rustup toolchain install nightly
rustup default nightly

# Install development dependencies
pip install maturin pytest pytest-asyncio pytest-cov

# Build the project
cargo build --workspace

# Build Python bindings
maturin develop

# Run tests to verify setup
cargo test --workspace
pytest python/tests
```

## Development Workflow

### 1. Create a Branch

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b fix/your-bug-fix
```

Use descriptive branch names:
- `feature/` for new features
- `fix/` for bug fixes
- `docs/` for documentation changes
- `refactor/` for code refactoring
- `test/` for test additions/modifications
- `perf/` for performance improvements

### 2. Make Your Changes

- Write clear, concise code following the project's style
- Add tests for new functionality
- Update documentation as needed
- Ensure all tests pass before committing

### 3. Testing Your Changes

```bash
# Run all Rust tests
cargo test --workspace

# Run specific test
cargo test test_name

# Run property-based tests
cargo test --test property_based

# Run Python tests
pytest python/tests -v

# Run benchmarks
cargo bench

# Check code formatting
cargo fmt --check

# Run clippy lints
cargo clippy --all-targets --all-features -- -D warnings
```

### 4. Commit Your Changes

Follow conventional commit format:

```bash
git commit -m "feat: add new pattern matching feature"
git commit -m "fix: correct diff parsing for renamed files"
git commit -m "docs: update API reference"
git commit -m "test: add property tests for interner"
git commit -m "perf: optimize string interning"
```

Commit message format:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Test additions or modifications
- `chore`: Build process or auxiliary tool changes

### 5. Push and Create Pull Request

```bash
git push origin feature/your-feature-name
```

Then create a pull request on GitHub with:
- Clear title describing the change
- Detailed description of what was changed and why
- Reference to any related issues
- Screenshots or examples if applicable

## Code Style Guidelines

### Rust Code

- Follow the official [Rust Style Guide](https://doc.rust-lang.org/1.0.0/style/)
- Use `cargo fmt` to format code
- Address all `cargo clippy` warnings
- Write documentation comments for public APIs
- Keep functions focused and concise
- Prefer explicit error handling over `.unwrap()` or `.expect()`

Example:

```rust
/// Parses a single diff line into a ChangedFile.
///
/// # Arguments
///
/// * `line` - The diff line as raw bytes
///
/// # Returns
///
/// Returns `Some(ChangedFile)` if the line is valid, `None` otherwise.
///
/// # Examples
///
/// ```
/// let line = b"M\tsrc/main.rs";
/// let file = parser.parse_diff_line(line);
/// assert!(file.is_some());
/// ```
pub fn parse_diff_line(&self, line: &[u8]) -> Option<ChangedFile> {
    // Implementation
}
```

### Python Code

- Follow [PEP 8](https://pep8.org/) style guide
- Use type hints for function signatures
- Write docstrings for classes and functions
- Keep functions concise and focused

Example:

```python
def get_changed_files(self, config: Config) -> ChangedFiles:
    """
    Detect changed files in the repository.

    Args:
        config: Configuration specifying which changes to detect

    Returns:
        ChangedFiles object containing all detected changes

    Raises:
        GitError: If git operations fail
        PathError: If repository path is invalid
    """
    # Implementation
```

## Testing Guidelines

### Unit Tests

- Write tests for all new functionality
- Test edge cases and error conditions
- Use descriptive test names
- Keep tests focused on a single behavior

```rust
#[test]
fn test_parse_added_file() {
    let interner = StringInterner::new();
    let parser = DiffParser::new(&interner);
    let line = b"A\tsrc/new_file.rs";

    let result = parser.parse_diff_line(line);

    assert!(result.is_some());
    let file = result.unwrap();
    assert_eq!(file.change_type, ChangeType::Added);
}
```

### Property-Based Tests

- Use proptest for testing properties that should hold for all inputs
- Test invariants and mathematical properties

```rust
proptest! {
    #[test]
    fn test_interner_idempotent(s in "[a-z]{1,100}") {
        let interner = StringInterner::new();
        let id1 = interner.intern(&s);
        let id2 = interner.intern(&s);
        prop_assert_eq!(id1, id2);
    }
}
```

### Integration Tests

- Test end-to-end workflows
- Verify integration between components
- Test with realistic data

### Benchmarks

- Add benchmarks for performance-critical code
- Compare before and after performance
- Document performance improvements

```rust
fn bench_pattern_matching(c: &mut Criterion) {
    let matcher = PatternMatcher::new(&["**/*.rs"], &[], false).unwrap();

    c.bench_function("pattern_matching", |b| {
        b.iter(|| matcher.matches_sync(black_box("src/main.rs")))
    });
}
```

## Documentation

### Code Documentation

- Document all public APIs with doc comments
- Include examples in documentation
- Explain complex algorithms or non-obvious code
- Update documentation when changing behavior

### README and Guides

- Keep README.md up to date with new features
- Update API reference when adding new methods
- Add examples for new functionality
- Update migration guides if breaking changes are made

## Performance Considerations

- Profile code before optimizing
- Document performance characteristics
- Add benchmarks for performance-critical code
- Consider memory usage and allocations
- Use zero-copy techniques where possible

## Security

- Never commit secrets or credentials
- Validate all external input
- Use safe Rust practices (avoid `unsafe` unless necessary)
- Run security audits with `cargo audit`
- Report security issues privately to <spicyzhug@gmail.com>

## Pull Request Process

1. **Self-Review**: Review your own changes before submitting
2. **Tests**: Ensure all tests pass
3. **Documentation**: Update documentation as needed
4. **Description**: Write a clear PR description
5. **CI**: Wait for CI checks to pass
6. **Review**: Address review feedback promptly
7. **Merge**: Maintainer will merge once approved

### PR Checklist

- [ ] Code follows project style guidelines
- [ ] Tests added for new functionality
- [ ] All tests pass locally
- [ ] Documentation updated
- [ ] Commit messages follow conventional format
- [ ] No merge conflicts
- [ ] CI checks pass

## Release Process

Releases are managed by maintainers:

1. Update version in `Cargo.toml` files
2. Update `CHANGELOG.md`
3. Create git tag: `git tag -a v0.1.0 -m "Release v0.1.0"`
4. Push tag: `git push origin v0.1.0`
5. GitHub Actions will automatically:
   - Build wheels for all platforms
   - Publish to PyPI
   - Publish to crates.io
   - Create GitHub release

## Getting Help

- **Questions**: Open a [GitHub Discussion](https://github.com/lituus-io/lechange/discussions)
- **Bugs**: Open a [GitHub Issue](https://github.com/lituus-io/lechange/issues)
- **Email**: <spicyzhug@gmail.com>

## Recognition

Contributors will be acknowledged in:
- Release notes
- README contributors section
- Git commit history

Thank you for contributing to LeChange!

---

Copyright (c) 2024-2026 lituus-io

Licensed under the MIT License.
