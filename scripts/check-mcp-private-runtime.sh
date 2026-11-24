#!/bin/sh
# check-mcp-private-runtime.sh — cw-mcp's public API names no type of its
# MCP SDK (rmcp) or its async runtime (tokio), so a project depends on
# neither and either can change without a breaking release.
#
# Usage: sh scripts/check-mcp-private-runtime.sh [repository-root]
# Exit status: 0 = kept private, 1 = a violation, 2 = cannot run.
#
# Two rules, which together with the compiler close the boundary:
#   1. Only crates/cw-mcp/src/wire/ names `rmcp` or `tokio`.
#   2. Nothing in wire/ is plain `pub`: everything there is `pub(crate)` or
#      narrower. The compiler refuses a re-export of a `pub(crate)` item,
#      and the workspace's `-D warnings` refuses one in a public signature
#      (`private_interfaces`), so nothing from wire/ reaches the public API.
# Line comments are not searched, so documentation may name either crate.
# No compile. Needs find, sort, cat, sed, tr and grep.

set -u

root=${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)} || exit 2
src="$root/crates/cw-mcp/src"
[ -d "$src" ] || { printf 'mcp private runtime: cannot run: no %s\n' "$src" >&2; exit 2; }

status=0
fail() {
  printf 'mcp private runtime: FAIL: %s\n' "$1" >&2
  status=1
}

# statements <text>: Rust source with line comments removed, whitespace
# collapsed, and one statement (up to `;`, `{` or `}`) per line.
statements() {
  printf '%s\n' "$1" | sed 's://.*$::' | tr '\n\t' '  ' | tr -s ' ' | tr ';{}' '\n\n\n'
}

# A crate path through rmcp or tokio, or a `use` of either.
named='(^|[^A-Za-z0-9_:])(::)?(rmcp|tokio)( ?::|[^A-Za-z0-9_]*$)'
# `pub` not followed by `(`: visible outside the crate.
public='(^|[^A-Za-z0-9_])pub( [^(]|$)'

# One path per line, so a path with spaces is read whole. A listing that
# fails, or finds no source, is a failure: a check that read nothing must
# not pass.
files=$(find "$src" -name '*.rs' -print) ||
  { printf 'mcp private runtime: cannot run: cannot list %s\n' "$src" >&2; exit 2; }
[ -n "$files" ] ||
  { printf 'mcp private runtime: cannot run: no Rust source under %s\n' "$src" >&2; exit 2; }

# The loop runs in a subshell (it reads a pipe), so it hands its status out.
printf '%s\n' "$files" | LC_ALL=C sort | {
while IFS= read -r file; do
  relative=${file#"$root"/}
  source=$(cat -- "$file") || { fail "cannot read $relative"; continue; }
  case "$file" in
    "$src"/wire/*) pattern=$public; why='is public in wire/' ;;
    *) pattern=$named; why='names rmcp or tokio outside wire/' ;;
  esac
  hits=$(statements "$source" | grep -E "$pattern" | sed 's/^ *//' | cut -c1-160)
  [ -n "$hits" ] || continue
  printf '%s\n' "$hits" | while IFS= read -r statement; do
    printf 'mcp private runtime: FAIL: %s %s: %s\n' "$relative" "$why" "$statement" >&2
  done
  status=1
done
[ "$status" -eq 0 ] || fail "keep rmcp and tokio inside crates/cw-mcp/src/wire/, and nothing there plain pub"
exit "$status"
} || status=1

exit "$status"
