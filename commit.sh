#!/usr/bin/env bash
# commit-per-file.sh — one Conventional Commit per changed file.
# Bash 3.2 compatible (macOS system bash). No associative arrays,
# no mapfile, no ${var,,}, no globstar, no local -n.
#
# Usage:
#   ./commit-per-file.sh            # interactive; shows the plan, asks y/N
#   ./commit-per-file.sh --dry-run  # print the plan only, no commits
#   ./commit-per-file.sh --yes      # skip the confirmation prompt

DRY_RUN=0
ASSUME_YES=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --yes|-y)  ASSUME_YES=1 ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) printf 'unknown arg: %s\n' "$arg" >&2; exit 2 ;;
  esac
done

B=$'\033[1m'; D=$'\033[2m'; R=$'\033[0m'
G=$'\033[32m'; Y=$'\033[33m'; C=$'\033[36m'; X=$'\033[31m'

# ---------------------------------------------------------------- analysis
printf '%s== git status ==%s\n' "$B" "$R"
git status --short
printf '\n%s== tracked diff vs HEAD ==%s\n' "$B" "$R"
if git rev-parse --verify HEAD >/dev/null 2>&1; then
  git diff --stat HEAD
else
  printf '  (no HEAD yet — first commit)\n'
fi
printf '\n'

# ------------------------------------------------------------ collect list
MODIFIED=$(git diff --name-only)
STAGED=$(git diff --cached --name-only)
UNTRACKED=$(git ls-files --others --exclude-standard)
DELETED=$(git ls-files --deleted)

ALL=$(printf '%s\n%s\n%s\n%s\n' \
  "$MODIFIED" "$STAGED" "$UNTRACKED" "$DELETED" \
  | sort -u | grep -v '^$' || true)

if [ -z "$ALL" ]; then
  printf '%sNo changes to commit.%s\n' "$Y" "$R"
  exit 0
fi

# ------------------------------------------------------- classification ----
classify() {
  # $1 = path, $2 = status (A/M/D)
  # echoes "<type>(<scope>): <subject>"  (scope omitted when empty)
  local f="$1" status="$2" type scope subject base
  scope=""
  case "$f" in
    crates/tuvix-core/src/focus.rs)
      type=feat;  scope=core;    subject="add focus ring routing" ;;
    crates/tuvix-core/src/keymap.rs)
      type=feat;  scope=core;    subject="add keymap binding trie" ;;
    crates/tuvix-core/src/lib.rs)
      type=feat;  scope=core;    subject="export focus and keymap modules" ;;
    crates/tuvix-core/benches/interaction.rs)
      type=perf;  scope=core;    subject="add focus and keymap benchmarks" ;;
    crates/tuvix-core/Cargo.toml)
      type=build; scope=core;    subject="register interaction benchmark" ;;
    crates/tuvix-widgets/src/scrollable.rs)
      type=feat;  scope=widgets; subject="add scrollable viewport widget" ;;
    crates/tuvix-widgets/src/lib.rs)
      type=feat;  scope=widgets; subject="export scrollable module" ;;
    crates/tuvix-widgets/benches/scrollable.rs)
      type=perf;  scope=widgets; subject="add scrollable viewport benchmarks" ;;
    crates/tuvix-widgets/Cargo.toml)
      type=build; scope=widgets; subject="register scrollable benchmark" ;;
    crates/tuvix-widgets/tests/m11_interaction.rs)
      type=test;  scope=widgets; subject="add M11 viewport and keymap gates" ;;
    crates/tuvix/examples/interaction.rs)
      type=docs;  scope=examples; subject="add M11 interaction showcase" ;;
    crates/tuvix/src/lib.rs)
      type=feat;  scope=tuvix;   subject="re-export M11 interaction surface" ;;
    Cargo.toml)
      type=chore; scope=release; subject="bump workspace to 0.12.0" ;;
    Cargo.lock)
      type=chore; scope=release; subject="refresh lockfile for 0.12.0" ;;
    CHANGELOG.md)
      type=chore; scope=release; subject="add 0.12.0 release notes" ;;
    README.md)
      type=docs;  subject="document M11 interaction features" ;;
    docs/ARCHITECTURE.md)
      type=docs;  subject="record M11 design decisions" ;;
    docs/BENCHMARKS.md)
      type=docs;  subject="add M11 benchmark scorecard" ;;
    *)
      base="${f##*/}"
      case "$f" in
        *.md)                        type=docs ;;
        *Cargo.toml|*Cargo.lock)     type=chore ;;
        */tests/*|tests/*)           type=test ;;
        */benches/*|benches/*)       type=perf ;;
        */examples/*|examples/*)     type=docs ;;
        *.rs)                        type=feat ;;
        *)                           type=chore ;;
      esac
      case "$status" in
        D) subject="remove $base" ;;
        A) subject="add $base" ;;
        *) subject="update $base" ;;
      esac
      ;;
  esac
  if [ -n "$scope" ]; then
    printf '%s(%s): %s\n' "$type" "$scope" "$subject"
  else
    printf '%s: %s\n' "$type" "$subject"
  fi
}

# --------------------------------------------------------- build the plan --
PLAN=""
COUNT=0
while IFS= read -r f; do
  [ -z "$f" ] && continue

  status=M
  if git ls-files --error-unmatch -- "$f" >/dev/null 2>&1; then
    if git diff --name-only --diff-filter=D -- "$f" | grep -q . ; then
      status=D
    fi
  else
    status=A
  fi

  msg=$(classify "$f" "$status")
  PLAN="${PLAN}${f}|${status}|${msg}
"
  COUNT=$((COUNT + 1))
done <<< "$ALL"

# -------------------------------------------------------------- show plan --
printf '%s== Planned commits (%d) ==%s\n' "$B" "$COUNT" "$R"
while IFS='|' read -r f st msg; do
  [ -z "$f" ] && continue
  printf '  %s[%s]%s %-58s %s→%s %s%s\n' \
    "$D" "$st" "$R" "$f" "$Y" "$R" "$msg" "$R"
done <<< "$PLAN"
printf '\n'

if [ "$DRY_RUN" -eq 1 ]; then
  printf '%s(dry-run: no commits made)%s\n' "$Y" "$R"
  exit 0
fi

if [ "$ASSUME_YES" -ne 1 ]; then
  printf '%sCreate %d commit(s)? [y/N] %s' "$Y" "$COUNT" "$R"
  read -r reply
  case "$reply" in
    y|Y|yes|YES) ;;
    *) printf 'aborted.\n'; exit 1 ;;
  esac
fi

# ------------------------------------------------------------ commit loop --
# Unstage anything so we can stage file-by-file cleanly.
git reset --quiet HEAD -- >/dev/null 2>&1 || true

committed=0
skipped=0
n=0
while IFS='|' read -r f st msg; do
  [ -z "$f" ] && continue
  n=$((n + 1))
  printf '%s[%d/%d]%s %s\n' "$C" "$n" "$COUNT" "$R" "$f"

  if ! git add -- "$f"; then
    printf '%sFailed to stage %s%s\n' "$X" "$f" "$R"; exit 1
  fi

  if git diff --cached --quiet; then
    printf '  %s(skip: no content change — mode-only or already committed)%s\n' "$D" "$R"
    skipped=$((skipped + 1))
    continue
  fi

  if ! git commit --quiet -m "$msg"; then
    printf '%sFailed to commit %s%s\n' "$X" "$f" "$R"; exit 1
  fi
  printf '  %s%s%s\n' "$G" "$msg" "$R"
  committed=$((committed + 1))
done <<< "$PLAN"

# ------------------------------------------------------------------ result -
printf '\n%s== Summary ==%s\n' "$B" "$R"
printf '  committed: %s%d%s\n' "$G" "$committed" "$R"
printf '  skipped:   %s%d%s\n' "$Y" "$skipped" "$R"

if [ "$committed" -gt 0 ]; then
  printf '\nLast %d commit(s):\n' "$committed"
  git log --oneline -"$committed"
fi
printf '\n'
git status --short
