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
# No compile. Needs find, sed, tr and grep.

set -u

root=${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)} || exit 2
src="$root/crates/cw-mcp/src"
[ -d "$src" ] || { printf 'mcp private runtime: cannot run: no %s\n' "$src" >&2; exit 2; }

status=0
fail() {
  printf 'mcp private runtime: FAIL: %s\n' "$1" >&2
  status=1
}

# statements <file>: the Rust source with line comments removed, whitespace
# collapsed, and one statement (up to `;`, `{` or `}`) per line.
statements() {
  sed 's://.*$::' "$1" | tr '\n\t' '  ' | tr -s ' ' | tr ';{}' '\n\n\n'
}

# A crate path through rmcp or tokio, or a `use` of either.
named='(^|[^A-Za-z0-9_:])(::)?(rmcp|tokio)( ?::|[^A-Za-z0-9_]*$)'
# `pub` not followed by `(`: visible outside the crate.
public='(^|[^A-Za-z0-9_])pub( [^(]|$)'

files=$(find "$src" -name '*.rs' -print | LC_ALL=C sort)
for file in $files; do
  relative=${file#"$root"/}
  case "$file" in
    "$src"/wire/*)
      statements "$file" | grep -E "$public" | sed 's/^ *//' | cut -c1-160 |
        while IFS= read -r statement; do
          printf 'mcp private runtime: FAIL: %s is public in wire/: %s\n' "$relative" "$statement" >&2
        done
      statements "$file" | grep -qE "$public" && status=1
      ;;
    *)
      statements "$file" | grep -E "$named" | sed 's/^ *//' | cut -c1-160 |
        while IFS= read -r statement; do
          printf 'mcp private runtime: FAIL: %s names rmcp or tokio outside wire/: %s\n' "$relative" "$statement" >&2
        done
      statements "$file" | grep -qE "$named" && status=1
      ;;
  esac
done
[ "$status" -eq 0 ] || fail "keep rmcp and tokio inside crates/cw-mcp/src/wire/, and nothing there plain pub"
exit "$status"
