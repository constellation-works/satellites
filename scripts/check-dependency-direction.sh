#!/bin/sh
# check-dependency-direction.sh — the kit crates depend on each other only as
# the table below allows, and a crate names no API its row forbids.
#
# Usage: sh scripts/check-dependency-direction.sh [repository-root]
# Exit status: 0 = every edge allowed, 1 = a violation, 2 = cannot run.
#
# A kit crate is a directory crates/cw-*/. Every one needs a row in ALLOWED;
# a new crate adds its row in the same change. Every dependency section of its
# Cargo.toml counts ([dependencies], [dev-dependencies], [build-dependencies]
# and their [target.*] forms), so no edge hides in a test or a platform.
# Needs only POSIX sh, sed, grep and awk.

set -u

# crate: the kit crates it may depend on (space-separated; empty for none).
ALLOWED='
cw-base:
cw-fs: cw-base
'

# crate: a regular expression of paths its sources must not name. Comment
# lines are not searched, so documentation may still name what is forbidden.
# cw-base does no I/O and reads no environment.
FORBIDDEN='
cw-base: std::(fs|env|process)([^A-Za-z0-9_]|$)|std::\{([^}]*[ ,])?(fs|env|process)([^A-Za-z0-9_]|$)
'

root=${1:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)} || exit 2
[ -d "$root/crates" ] || { printf 'dependency direction: cannot run: no %s/crates\n' "$root" >&2; exit 2; }

status=0
fail() {
  printf 'dependency direction: FAIL: %s\n' "$1" >&2
  status=1
}

# row <table> <crate>: the text after "<crate>:" in <table>, or nothing.
row() {
  printf '%s\n' "$1" | sed -n "s/^$2:[[:space:]]*//p"
}

# kit_deps <Cargo.toml>: every cw-* dependency named in a dependency section,
# whether as a key (cw-base = ..., cw-base.workspace = true) or a table header
# ([dependencies.cw-base]).
kit_deps() {
  awk '
    /^[[:space:]]*\[/ {
      header = $0
      in_deps = (header ~ /dependencies[[:space:]]*\]/)
      if (match(header, /dependencies\.cw-[a-z0-9_-]+/)) {
        name = substr(header, RSTART + 13, RLENGTH - 13)
        print name
      }
      next
    }
    in_deps && match($0, /^[[:space:]]*cw-[a-z0-9_-]+/) {
      name = substr($0, RSTART, RLENGTH)
      sub(/^[[:space:]]*/, "", name)
      print name
    }
  ' "$1" | sort -u
}

found=0
for manifest in "$root"/crates/cw-*/Cargo.toml; do
  [ -f "$manifest" ] || continue
  found=1
  crate=$(basename "$(dirname "$manifest")")
  if ! printf '%s\n' "$ALLOWED" | grep -q "^$crate:"; then
    fail "$crate has no row in ALLOWED in scripts/check-dependency-direction.sh; add one"
    continue
  fi
  allowed=$(row "$ALLOWED" "$crate")
  for dep in $(kit_deps "$manifest"); do
    case " $allowed " in
      *" $dep "*) ;;
      *) fail "$crate depends on $dep; its row allows: ${allowed:-no kit crate}" ;;
    esac
  done
  forbidden=$(row "$FORBIDDEN" "$crate")
  if [ -n "$forbidden" ] && [ -d "$root/crates/$crate/src" ]; then
    hits=$(find "$root/crates/$crate/src" -name '*.rs' -exec grep -nHE "$forbidden" {} + 2>/dev/null |
      grep -vE '^[^:]+:[0-9]+:[[:space:]]*//')
    if [ -n "$hits" ]; then
      fail "$crate names an API its row forbids ($forbidden):"
      printf '%s\n' "$hits" | sed "s|^$root/|    |" >&2
    fi
  fi
done

[ "$found" = 1 ] || printf 'dependency direction: no kit crates yet\n' >&2
exit "$status"
