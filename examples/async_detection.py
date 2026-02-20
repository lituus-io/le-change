#!/usr/bin/env python3
"""
Async detection example.

This example demonstrates using LeChange with asyncio for
non-blocking change detection in async applications.
"""

import asyncio
from lechange import ChangeDetector, Config


async def detect_multiple_ranges():
    """Detect changes across multiple commit ranges concurrently."""
    detector = ChangeDetector(".")

    # Define multiple ranges to check
    ranges = [
        ("HEAD~1", "HEAD", "Last commit"),
        ("HEAD~5", "HEAD", "Last 5 commits"),
        ("HEAD~10", "HEAD", "Last 10 commits"),
    ]

    # Create tasks for concurrent detection
    tasks = []
    for base, head, description in ranges:
        config = Config(base=base, head=head)
        task = asyncio.create_task(
            detector.get_changed_files_async(config)
        )
        tasks.append((task, description))

    # Wait for all detections to complete
    results = []
    for task, description in tasks:
        result = await task
        results.append((description, result))

    return results


async def main():
    print("=== Async Change Detection ===\n")

    # Run concurrent detections
    results = await detect_multiple_ranges()

    # Print results
    for description, result in results:
        print(f"{description}:")
        print(f"  Total changes: {result.all_changed_files_count}")
        print(f"  Added: {result.added_files_count}")
        print(f"  Modified: {result.modified_files_count}")
        print(f"  Deleted: {result.deleted_files_count}")
        print()

    print("=== Pattern-based Async Detection ===\n")

    # Async detection with patterns
    detector = ChangeDetector(".")
    config = Config(
        base="HEAD~5",
        head="HEAD",
        files=["**/*.py", "**/*.rs"]
    )

    result = await detector.get_changed_files_async(config)

    print(f"Changed Python/Rust files: {result.all_changed_files_count}")
    for file in result.all_changed_files[:10]:  # Show first 10
        print(f"  {file}")
    if result.all_changed_files_count > 10:
        print(f"  ... and {result.all_changed_files_count - 10} more")


if __name__ == "__main__":
    asyncio.run(main())
