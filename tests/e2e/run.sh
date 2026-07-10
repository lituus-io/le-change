#!/usr/bin/env bash
# Golden end-to-end harness for the lechange CLI.
#
# Builds the release binary, replays fixture git repositories through it, and
# byte-compares each scenario's GITHUB_OUTPUT against tests/e2e/golden/.
# The only permitted nondeterminism is the per-process heredoc delimiter
# (LECHANGE_EOF_<pid>_<nanos>), normalized before comparison.
#
# Usage:
#   tests/e2e/run.sh            # compare against goldens (CI mode)
#   tests/e2e/run.sh --update   # regenerate goldens from the current binary
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GOLDEN="$ROOT/tests/e2e/golden"
BIN="$ROOT/target/release/lechange"
MODE="${1:-check}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
FAIL=0

cargo build --release -p lechange-cli --locked --manifest-path "$ROOT/Cargo.toml"

gitc() { git -C "$1" -c user.email=t@t -c user.name=t -c commit.gpgsign=false "${@:2}"; }
normalize() { sed -E 's/LECHANGE_EOF_[0-9]+_[0-9]+/LECHANGE_EOF_NORMALIZED/g' "$1"; }

new_repo() { mkdir -p "$1" && gitc "$1" init -q -b main; }

run_scenario() { # name repo base head args...
  local name=$1 repo=$2 base=$3 head=$4; shift 4
  local raw="$WORK/$name.raw"
  : > "$raw"
  ( cd "$repo" && \
    GITHUB_OUTPUT="$raw" GITHUB_ACTIONS=true GITHUB_EVENT_NAME= \
    "$BIN" detect --base-sha "$base" --sha "$head" "$@" >/dev/null )
  if [ "$MODE" = "--update" ]; then
    normalize "$raw" > "$GOLDEN/$name.txt"
    echo "updated: $name"
  else
    if diff -u "$GOLDEN/$name.txt" <(normalize "$raw"); then
      echo "ok: $name"
    else
      echo "FAIL: $name"
      FAIL=1
    fi
  fi
}

# ── fixture 1: add / modify / delete under stacks/** ───────────────────
R=$WORK/basic; new_repo "$R"
mkdir -p "$R/stacks/s1" "$R/stacks/s2" "$R/docs"
echo "name: s1" > "$R/stacks/s1/Pulumi.yaml"
echo "name: s2" > "$R/stacks/s2/Pulumi.yaml"
echo "readme" > "$R/docs/readme.md"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
mkdir -p "$R/stacks/s3"
echo "name: s3" > "$R/stacks/s3/Pulumi.yaml"
echo "name: s1 v2" > "$R/stacks/s1/Pulumi.yaml"
gitc "$R" rm -q stacks/s2/Pulumi.yaml
echo "changed" >> "$R/docs/readme.md"
gitc "$R" add -A && gitc "$R" commit -qm change
HEAD=$(gitc "$R" rev-parse HEAD)

run_scenario basic_files "$R" "$BASE" "$HEAD" --files 'stacks/**/Pulumi.yaml'
run_scenario basic_group_by "$R" "$BASE" "$HEAD" \
  --files-group-by 'stacks/{group}/**' --deploy-matrix-include-reason
run_scenario basic_ignore "$R" "$BASE" "$HEAD" \
  --files 'stacks/**' --files-ignore 'stacks/s3/**'

# ── fixture 2: rename ───────────────────────────────────────────────────
R=$WORK/rename; new_repo "$R"
mkdir -p "$R/stacks/old"
echo "name: old" > "$R/stacks/old/Pulumi.yaml"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
gitc "$R" mv stacks/old stacks/new
gitc "$R" commit -qm rename
HEAD=$(gitc "$R" rev-parse HEAD)

run_scenario rename_default "$R" "$BASE" "$HEAD" --files 'stacks/**/Pulumi.yaml'

# ── fixture 3: JSON escaping (filename with spaces + quotes) ────────────
R=$WORK/escape; new_repo "$R"
mkdir -p "$R/stacks/esc"
echo base > "$R/stacks/esc/plain.txt"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
touch "$R/stacks/esc/with space \"q\".txt"
gitc "$R" add -A && gitc "$R" commit -qm esc
HEAD=$(gitc "$R" rev-parse HEAD)

run_scenario escape "$R" "$BASE" "$HEAD" --files 'stacks/**'

if [ "$MODE" != "--update" ] && [ "$FAIL" -ne 0 ]; then
  echo "golden e2e FAILED"
  exit 1
fi
echo "golden e2e complete"
