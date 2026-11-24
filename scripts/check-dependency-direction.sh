#!/bin/sh
# check-dependency-direction.sh — the kit crates depend on each other only as
# the table below allows, and a crate names no module its row forbids.
#
# Usage: sh scripts/check-dependency-direction.sh [repository-root]
# Exit status: 0 = every edge allowed, 1 = a violation, 2 = cannot run.
#
# A kit crate is a workspace package named cw-*. Every one needs a row in
# ALLOWED; a new crate adds its row in the same change. The graph comes from
# `cargo metadata --no-deps`, which names each dependency by its package, so
# a renamed dependency (`files = { package = "cw-fs" }`) or a quoted key is
# seen for what it is. Every kind counts (normal, dev, build, any target), so
# no edge hides in a test or a platform. No compile. Needs cargo, jq, awk,
# sed and grep.

set -u

# crate: the kit crates it may depend on (space-separated; empty for none).
ALLOWED='
cw-base:
cw-fs: cw-base
cw-testkit:
'

# crate: the modules its sources must not name (space-separated). Found in a
# path (`std::fs::read`), a `use` of the module, or a grouped `use` on one
# line or many (`use std::{io, fs};`). Line comments are not searched, so
# documentation may still name what is forbidden.
# cw-base does no I/O and reads no environment.
FORBIDDEN='
cw-base: std::fs std::env std::process
'

root=${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)} || exit 2
[ -f "$root/Cargo.toml" ] || { printf 'dependency direction: cannot run: no %s/Cargo.toml\n' "$root" >&2; exit 2; }
for tool in cargo jq; do
  command -v "$tool" >/dev/null 2>&1 ||
    { printf 'dependency direction: cannot run: %s is not installed\n' "$tool" >&2; exit 2; }
done

status=0
fail() {
  printf 'dependency direction: FAIL: %s\n' "$1" >&2
  status=1
}

# row <table> <crate>: the text after "<crate>:" in <table>, or nothing.
row() {
  printf '%s\n' "$1" | sed -n "s/^$2:[[:space:]]*//p"
}

# Cargo's own status decides whether there is a graph at all: a manifest it
# cannot read must not look like a workspace with no kit crates.
metadata=$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml") ||
  { printf 'dependency direction: cannot run: cargo metadata failed\n' >&2; exit 2; }

# One line per kit crate, tab-separated so a path may hold spaces:
# "<name>\t<manifest dir>\t<kit dependencies, space-separated>".
tab=$(printf '\t')
graph=$(printf '%s\n' "$metadata" |
  jq -er '[.packages[] | select(.name | startswith("cw-"))
    | [.name, (.manifest_path | sub("/Cargo.toml$"; "")),
       ([.dependencies[].name | select(startswith("cw-"))] | unique | join(" "))]
    | join("\t")] | join("\n")') ||
  { printf 'dependency direction: cannot run: cargo metadata gave no package list\n' >&2; exit 2; }

# statements <file>: the Rust source with line comments removed, whitespace
# collapsed, and one statement (up to `;`) per line.
statements() {
  sed 's://.*$::' "$1" | tr '\n\t' '  ' | tr -s ' ' | tr ';' '\n'
}

# A character that cannot continue an identifier.
edge='([^A-Za-z0-9_]|$)'

[ -n "$graph" ] || printf 'dependency direction: no kit crates yet\n' >&2
# The loop runs in a subshell (it reads a pipe), so it hands its status out.
printf '%s\n' "$graph" | {
while IFS=$tab read -r crate dir deps; do
  [ -n "$crate" ] || continue
  if ! printf '%s\n' "$ALLOWED" | grep -q "^$crate:"; then
    fail "$crate has no row in ALLOWED in scripts/check-dependency-direction.sh; add one"
    continue
  fi
  allowed=$(row "$ALLOWED" "$crate")
  for dep in $deps; do
    case " $allowed " in
      *" $dep "*) ;;
      *) fail "$crate depends on $dep; its row allows: ${allowed:-no kit crate}" ;;
    esac
  done
  for module in $(row "$FORBIDDEN" "$crate"); do
    parent=${module%%::*}
    leaf=${module##*::}
    # A path through the module, or a grouped `use` that lists it.
    direct="(^|[^A-Za-z0-9_:])(::)?$parent ?:: ?$leaf$edge"
    grouped="(^|[^A-Za-z0-9_:])(::)?$parent ?:: ?\{.*[{,] ?$leaf$edge"
    # One line per offending statement, "<file>: <statement>"; a file name
    # may hold spaces, so it is read whole from each line of `find`.
    hits=$(find "$dir/src" -name '*.rs' 2>/dev/null | LC_ALL=C sort |
      while IFS= read -r file; do
        statements "$file" | grep -E "$direct|$grouped" | sed 's/^ *//' |
          while IFS= read -r statement; do
            printf '%s: %s\n' "${file#"$dir"/}" "$statement"
          done
      done)
    [ -n "$hits" ] || continue
    fail "$crate names $module, which its row forbids:"
    # A statement can be long; its start is enough to find it.
    printf '%s\n' "$hits" | cut -c1-160 | sed 's/^/    /' >&2
  done
done
exit "$status"
} || status=1

exit "$status"
