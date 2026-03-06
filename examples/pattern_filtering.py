#!/usr/bin/env python3
"""
Pattern filtering example.

This example shows how to filter changed files by patterns,
useful for detecting changes in specific file types or directories.
"""

from lechange import ChangeDetector, Config


def main():
    detector = ChangeDetector(".")

    # Example 1: Only Python files
    print("=== Example 1: Only Python files ===")
    config = Config(
        base="HEAD~5",
        head="HEAD",
        files=["**/*.py"]
    )
    result = detector.get_changed_files(config)
    print(f"Changed Python files: {result.all_changed_files_count}")
    for file in result.all_changed_files:
        print(f"  {file}")
    print()

    # Example 2: Multiple file types
    print("=== Example 2: Rust and TOML files ===")
    config = Config(
        base="HEAD~5",
        head="HEAD",
        files=["**/*.rs", "**/*.toml"]
    )
    result = detector.get_changed_files(config)
    print(f"Changed Rust/TOML files: {result.all_changed_files_count}")
    for file in result.all_changed_files:
        print(f"  {file}")
    print()

    # Example 3: Specific directory
    print("=== Example 3: Files in src/ directory ===")
    config = Config(
        base="HEAD~5",
        head="HEAD",
        files=["src/**"]
    )
    result = detector.get_changed_files(config)
    print(f"Changed files in src/: {result.all_changed_files_count}")
    for file in result.all_changed_files:
        print(f"  {file}")
    print()

    # Example 4: Exclude patterns
    print("=== Example 4: All files except tests ===")
    config = Config(
        base="HEAD~5",
        head="HEAD",
        files=["**/*"],
        files_ignore=["**/tests/**", "**/test_*.py"]
    )
    result = detector.get_changed_files(config)
    print(f"Changed files (excluding tests): {result.all_changed_files_count}")
    for file in result.all_changed_files:
        print(f"  {file}")
    print()


if __name__ == "__main__":
    main()
