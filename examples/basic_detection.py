#!/usr/bin/env python3
"""
Basic change detection example.

This example demonstrates the simplest usage of LeChange to detect
changed files between two commits.
"""

from lechange import ChangeDetector, Config


def main():
    # Initialize detector for current repository
    detector = ChangeDetector(".")

    # Create configuration
    config = Config(
        base="HEAD^",  # Previous commit
        head="HEAD",   # Current commit
    )

    # Detect changes
    result = detector.get_changed_files(config)

    # Print results
    print("=== Changed Files ===")
    print(f"Total changes: {result.all_changed_files_count}")
    print()

    if result.any_added:
        print(f"Added files ({result.added_files_count}):")
        for file in result.added_files:
            print(f"  + {file}")
        print()

    if result.any_modified:
        print(f"Modified files ({result.modified_files_count}):")
        for file in result.modified_files:
            print(f"  M {file}")
        print()

    if result.any_deleted:
        print(f"Deleted files ({result.deleted_files_count}):")
        for file in result.deleted_files:
            print(f"  - {file}")
        print()

    if result.any_renamed:
        print(f"Renamed files ({result.renamed_files_count}):")
        for old_path, new_path in result.renamed_files_mapping.items():
            print(f"  R {old_path} -> {new_path}")
        print()


if __name__ == "__main__":
    main()
