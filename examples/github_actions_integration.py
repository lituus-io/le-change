#!/usr/bin/env python3
"""
GitHub Actions integration example.

This example shows how to use LeChange in GitHub Actions workflows
to detect changed files and conditionally run jobs.
"""

import os
import sys
from lechange import ChangeDetector, Config


def get_github_context():
    """Extract GitHub Actions context from environment variables."""
    return {
        "event_name": os.getenv("GITHUB_EVENT_NAME", "push"),
        "base_ref": os.getenv("GITHUB_BASE_REF", ""),
        "head_ref": os.getenv("GITHUB_HEAD_REF", ""),
        "sha": os.getenv("GITHUB_SHA", "HEAD"),
        "repository": os.getenv("GITHUB_REPOSITORY", ""),
    }


def determine_base_sha(context):
    """Determine base SHA based on GitHub event type."""
    if context["event_name"] == "pull_request":
        # For PRs, compare against base branch
        return f"origin/{context['base_ref']}"
    else:
        # For push events, compare against previous commit
        return "HEAD^"


def main():
    print("=== GitHub Actions Change Detection ===\n")

    context = get_github_context()
    print(f"Event: {context['event_name']}")
    print(f"Repository: {context['repository']}")
    print()

    detector = ChangeDetector(".")
    base_sha = determine_base_sha(context)

    # Example 1: Detect all changes
    print("--- All Changes ---")
    config = Config(base=base_sha, head="HEAD")
    result = detector.get_changed_files(config)

    print(f"Total changes: {result.all_changed_files_count}")
    print()

    # Example 2: Check if Python files changed (for Python tests)
    print("--- Python Files Changed ---")
    config = Config(
        base=base_sha,
        head="HEAD",
        files=["**/*.py"]
    )
    result = detector.get_changed_files(config)

    run_python_tests = result.any_changed
    print(f"Run Python tests: {run_python_tests}")
    print(f"Changed files: {result.all_changed_files_count}")

    # Set GitHub Actions output
    if os.getenv("GITHUB_OUTPUT"):
        with open(os.getenv("GITHUB_OUTPUT"), "a") as f:
            f.write(f"run_python_tests={str(run_python_tests).lower()}\n")
            f.write(f"python_files_count={result.all_changed_files_count}\n")
    print()

    # Example 3: Check if Rust files changed (for Rust tests)
    print("--- Rust Files Changed ---")
    config = Config(
        base=base_sha,
        head="HEAD",
        files=["**/*.rs", "**/Cargo.toml"]
    )
    result = detector.get_changed_files(config)

    run_rust_tests = result.any_changed
    print(f"Run Rust tests: {run_rust_tests}")
    print(f"Changed files: {result.all_changed_files_count}")

    # Set GitHub Actions output
    if os.getenv("GITHUB_OUTPUT"):
        with open(os.getenv("GITHUB_OUTPUT"), "a") as f:
            f.write(f"run_rust_tests={str(run_rust_tests).lower()}\n")
            f.write(f"rust_files_count={result.all_changed_files_count}\n")
    print()

    # Example 4: Check if documentation changed
    print("--- Documentation Changed ---")
    config = Config(
        base=base_sha,
        head="HEAD",
        files=["**/*.md", "docs/**"]
    )
    result = detector.get_changed_files(config)

    docs_changed = result.any_changed
    print(f"Documentation changed: {docs_changed}")

    # Set GitHub Actions output
    if os.getenv("GITHUB_OUTPUT"):
        with open(os.getenv("GITHUB_OUTPUT"), "a") as f:
            f.write(f"docs_changed={str(docs_changed).lower()}\n")
    print()

    # Example 5: Output changed files as JSON for further processing
    print("--- JSON Output ---")
    config = Config(
        base=base_sha,
        head="HEAD",
        json=True
    )
    result = detector.get_changed_files(config)

    # In GitHub Actions, you can use this JSON output in subsequent steps
    print(f"All changed files (JSON): {result.all_changed_files}")


if __name__ == "__main__":
    main()
