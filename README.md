# Le Change

Fast Git change detection with deploy matrix generation.

Rust core library with a CLI binary, GitHub Action, and Python bindings.

## Features

- Diff two commits and list changed files by type (added, modified, deleted, renamed)
- Glob pattern filtering and exclusion
- Dynamic group discovery via `files_group_by` templates (e.g. `stacks/{group}/**`)
- Deploy matrix JSON output for GitHub Actions `strategy.matrix`
- Workflow failure tracking with per-run or per-job granularity
- Concurrent workflow detection with deadlock-safe priority ordering
- Ancestor directory file association for monorepo layouts
- Static musl binaries for Linux (zero runtime dependencies)

## GitHub Action

```yaml
jobs:
  detect:
    runs-on: ubuntu-latest
    outputs:
      matrix: ${{ steps.changes.outputs.matrix }}
      has_changes: ${{ steps.changes.outputs.has_changes }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: lituus-io/le-change@v1
        id: changes
        with:
          files: 'stacks/**/*.yaml'
          files_group_by: 'stacks/{group}/**'

  deploy:
    needs: detect
    if: needs.detect.outputs.has_changes == 'true'
    strategy:
      matrix: ${{ fromJson(needs.detect.outputs.matrix) }}
    runs-on: ubuntu-latest
    steps:
      - run: echo "Deploying ${{ matrix.stack }}"
```

### Action Inputs

| Input | Default | Description |
|-------|---------|-------------|
| `files` | | Glob patterns to include (comma-separated) |
| `files_ignore` | | Glob patterns to exclude |
| `files_group_by` | | Group discovery template (e.g. `stacks/{group}/**`) |
| `files_group_by_key` | `name` | Group key mode: `name`, `path`, or `hash` |
| `files_ancestor_lookup_depth` | `0` | Ancestor directory lookup depth (max 3) |
| `track_workflow_failures` | `false` | Enable workflow failure tracking |
| `failure_tracking_level` | `run` | Tracking granularity: `run` or `job` |
| `wait_for_active_workflows` | `false` | Wait for concurrent overlapping workflows |
| `workflow_max_wait_seconds` | `300` | Max wait time in seconds |
| `workflow_name_filter` | | Glob pattern to filter workflow names |
| `deploy_matrix_include_reason` | `false` | Add action/reason to matrix entries |
| `deploy_matrix_include_concurrency` | `false` | Add concurrency info to matrix entries |
| `token` | `github.token` | GitHub token for API access |
| `base_sha` | | Override base commit SHA |
| `sha` | | Override head commit SHA |

### Action Outputs

| Output | Description |
|--------|-------------|
| `matrix` | Deploy matrix JSON for `fromJson()` |
| `has_changes` | `true` if any deployable changes detected |
| `any_changed` | `true` if any files changed |
| `changed_files` | Space-separated changed file paths |
| `changed_files_count` | Number of changed files |
| `added_files` | Space-separated added file paths |
| `modified_files` | Space-separated modified file paths |
| `deleted_files` | Space-separated deleted file paths |
| `deploy_decisions` | JSON array of per-group deploy decisions |
| `files_to_rebuild` | Files needing rebuild |
| `files_to_skip` | Files safe to skip |
| `diagnostics` | JSON array of diagnostic messages |

## CLI

```bash
lechange detect \
  --files 'stacks/**/*.yaml' \
  --files-group-by 'stacks/{group}/**' \
  --base-sha abc123 \
  --sha def456 \
  --output-format json
```

All options accept environment variables with `LECHANGE_` prefix (e.g. `LECHANGE_FILES`).

Exit codes: `0` = changes detected, `1` = error, `2` = no changes.

## Python

```bash
pip install lechange
```

```python
from lechange import ChangeDetector, Config

detector = ChangeDetector(".")
config = Config(
    files=["src/**/*.py"],
    files_group_by="src/{group}/**",
    base_sha="abc123",
    sha="def456",
)
result = detector.get_changed_files(config)

print(result.all_changed_files)
print(result.deploy_matrix)
print(result.has_deployable_groups)
```

## Development

```bash
cargo test -p lechange-core    # Core library tests
cargo test -p lechange-cli     # CLI tests
cargo build --release -p lechange-cli  # Release binary
```

## License

Copyright (c) 2024-2026 Lituus-io. All rights reserved.

AGPL-3.0-or-later. Commercial license available — contact spicyzhug@gmail.com.
