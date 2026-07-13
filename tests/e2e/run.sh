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
normalize() {
  sed -E -e 's/LECHANGE_EOF_[0-9]+_[0-9]+/LECHANGE_EOF_NORMALIZED/g' \
         -e 's/[0-9a-f]{40}/GITSHA/g' "$1"
}

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

# ── fixture 4: vanished stack (added then removed within the range) ────
R=$WORK/vanished; new_repo "$R"
echo "readme" > "$R/README.md"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
mkdir -p "$R/stacks/gone"
echo "name: gone" > "$R/stacks/gone/Pulumi.yaml"
echo "[]" > "$R/stacks/gone/schema.json"
gitc "$R" add -A && gitc "$R" commit -qm add-stack
gitc "$R" rm -rq stacks/gone
gitc "$R" commit -qm remove-stack
HEAD=$(gitc "$R" rev-parse HEAD)

# NOTE: last_seen_sha is commit-dependent, so goldens normalize 40-hex SHAs.
run_scenario vanished_destroy "$R" "$BASE" "$HEAD" \
  --files-group-by 'stacks/{group}/**' --detect-vanished

# ── fixture 5: endpoint deletion routed to destroy ──────────────────────
R=$WORK/deltodestroy; new_repo "$R"
mkdir -p "$R/stacks/old"
echo "name: old" > "$R/stacks/old/Pulumi.yaml"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
gitc "$R" rm -rq stacks/old
gitc "$R" commit -qm rm
HEAD=$(gitc "$R" rev-parse HEAD)

run_scenario deleted_to_destroy "$R" "$BASE" "$HEAD" \
  --files-group-by 'stacks/{group}/**' --detect-vanished --deleted-to-destroy

# ── fixture 6: native per-path file_matrix (add + modify + delete + vanish) ─
# Exercises every file_matrix action/reason on a per-path (`--files`) run — the
# shape the bilayer staging/serving workflows consume without jq. The stack
# label is the Pulumi.yaml's parent dir (glob affixes stripped), including
# nested paths that dir-subtree grouping cannot express.
R=$WORK/filematrix; new_repo "$R"
mkdir -p "$R/stacks/keep" "$R/stacks/buckets/churn"
echo "name: keep" > "$R/stacks/keep/Pulumi.yaml"
echo "name: churn" > "$R/stacks/buckets/churn/Pulumi.yaml"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
mkdir -p "$R/stacks/gone"
echo "name: gone" > "$R/stacks/gone/Pulumi.yaml"   # added...
gitc "$R" add -A && gitc "$R" commit -qm add-gone
echo "name: churn v2" > "$R/stacks/buckets/churn/Pulumi.yaml"  # modify nested
gitc "$R" rm -rq stacks/gone                        # ...then removed (vanished)
gitc "$R" rm -q stacks/keep/Pulumi.yaml             # endpoint delete
gitc "$R" add -A && gitc "$R" commit -qm mutate
HEAD=$(gitc "$R" rev-parse HEAD)

# NOTE: last_seen_sha / base_sha are commit-dependent, normalized as GITSHA.
run_scenario file_matrix "$R" "$BASE" "$HEAD" \
  --files 'stacks/**/Pulumi.yaml' --detect-vanished

# ── fixture 7: deterministic vanish — add-then-remove with a near-identical add
# The removed path (stacks/old) and the added path (stacks/new) share almost all
# of their Pulumi.yaml. Detection is path-based, NOT content-similarity: the old
# path MUST still vanish (destroy) even though a lookalike was added elsewhere in
# the same range. Guards against rename/find_similar masking regressing.
R=$WORK/detvanish; new_repo "$R"
echo "readme" > "$R/README.md"
gitc "$R" add -A && gitc "$R" commit -qm base
BASE=$(gitc "$R" rev-parse HEAD)
mkdir -p "$R/stacks/old"
printf 'name: old\nruntime: yaml\nresources:\n  ds: {type: bq}\n' > "$R/stacks/old/Pulumi.yaml"
gitc "$R" add -A && gitc "$R" commit -qm add-old
gitc "$R" rm -rq stacks/old                          # removed...
mkdir -p "$R/stacks/new"
printf 'name: new\nruntime: yaml\nresources:\n  ds: {type: bq}\n' > "$R/stacks/new/Pulumi.yaml"  # ...near-identical add
gitc "$R" add -A && gitc "$R" commit -qm "rm old + add near-identical new"
HEAD=$(gitc "$R" rev-parse HEAD)

run_scenario deterministic_vanish "$R" "$BASE" "$HEAD" \
  --files 'stacks/**/Pulumi.yaml' --detect-vanished

if [ "$MODE" != "--update" ] && [ "$FAIL" -ne 0 ]; then
  echo "golden e2e FAILED"
  exit 1
fi
echo "golden e2e complete"
