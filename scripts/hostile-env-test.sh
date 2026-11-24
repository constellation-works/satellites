#!/usr/bin/env bash
# Run a project's test suite under a hostile environment, so a test that
# reaches for the host's state finds decoys instead, and check that it left
# no trace (STD-03 R20).
#
#   usage: scripts/hostile-env-test.sh <command> [args...]
#   e.g.:  scripts/hostile-env-test.sh cargo test --workspace --locked
#
# The command runs once with:
#
#   HOME             a read-only home full of decoys: a shell profile, a git
#                    config whose hooksPath points at decoy hooks that write a
#                    marker whenever a git that read that config runs one,
#                    and a config, data, state and cache tree;
#   XDG_*_HOME, XDG_RUNTIME_DIR, XDG_CONFIG_DIRS, XDG_DATA_DIRS
#                    decoy trees inside that home, read-only too, so neither
#                    the per-user directories nor the system search paths
#                    reach the host's;
#   TMPDIR           a fresh directory whose path has a space and a non-ASCII
#                    letter, so a fixture that splits or re-encodes paths
#                    breaks.
#
# Afterwards it checks that the suite passed, no git hook marker exists, the
# home tree is byte for byte as it was, and TMPDIR holds nothing (every temp
# root was removed). Exits 0 only if all of that held.
#
# A plain read is not observed: a test that reads the decoy home, or a git
# that reads the decoy config without running a hook, still passes here. What the run shows is that such a read sees decoys, not the
# developer's files, and that a write there fails.
#
# GIT_CONFIG_GLOBAL and GIT_CONFIG_SYSTEM are not redirected: if the caller
# exports one, it still points git at the caller's configuration. Run the
# script from an environment without them for the git decoys to apply.
#
# CARGO_HOME and RUSTUP_HOME are pinned to their current values before HOME
# moves, so cargo and rustup still find the toolchain and the registry.
set -euo pipefail

if [[ $# -eq 0 ]]; then
    echo "usage: $0 <command> [args...]" >&2
    exit 2
fi

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"

scratch_parent="${TMPDIR:-/tmp}"
scratch="$(mktemp -d "${scratch_parent%/}/hostile-env.XXXXXX")"
failed=0
cleanup() {
    chmod -R u+w "$scratch" 2>/dev/null || true
    if [[ "$failed" -eq 0 ]]; then
        rm -rf "$scratch"
    else
        echo "hostile-env: kept $scratch for inspection" >&2
    fi
}
trap cleanup EXIT

fail() {
    echo "hostile-env: FAIL: $*" >&2
    failed=1
}

home="$scratch/home"
marker="$scratch/hook-ran"
hooks="$scratch/hooks"
tmp="$scratch/tmp dir é"

mkdir -p "$home/.config/git" "$home/.local/share" "$home/.local/state" \
    "$home/.cache" "$home/.run" "$home/.config-dirs" "$home/.data-dirs" "$hooks" "$tmp"

# Decoys: what a test would find if it looked at the home.
printf 'echo "hostile-env: a test sourced the home profile" >&2; exit 97\n' >"$home/.profile"
cp "$home/.profile" "$home/.bashrc"
cp "$home/.profile" "$home/.zshrc"
for dir in .config .local/share .local/state .cache .run .config-dirs .data-dirs; do
    printf 'decoy: a test read the host %s\n' "$dir" >"$home/$dir/DECOY"
done
for hook in pre-commit prepare-commit-msg commit-msg post-commit post-checkout \
    post-merge pre-push reference-transaction; do
    printf '#!/bin/sh\necho "%s $PWD" >>"%s"\n' "$hook" "$marker" >"$hooks/$hook"
    chmod +x "$hooks/$hook"
done
for config in "$home/.gitconfig" "$home/.config/git/config"; do
    printf '[core]\n\thooksPath = %s\n[user]\n\tname = hostile-env\n\temail = hostile-env@example.invalid\n' \
        "$hooks" >"$config"
done

# Every file with its checksum and mode, to show nothing changed.
snapshot() {
    (cd "$home" && find . -print | LC_ALL=C sort | while IFS= read -r entry; do
        if [[ -f "$entry" ]]; then
            printf '%s %s %s\n' "$entry" "$(ls -ld "$entry" | cut -c1-10)" \
                "$(cksum <"$entry")"
        else
            printf '%s %s\n' "$entry" "$(ls -ld "$entry" | cut -c1-10)"
        fi
    done)
}

chmod -R a-w "$home"
home_before="$(snapshot)"

status=0
env HOME="$home" \
    XDG_CONFIG_HOME="$home/.config" \
    XDG_DATA_HOME="$home/.local/share" \
    XDG_STATE_HOME="$home/.local/state" \
    XDG_CACHE_HOME="$home/.cache" \
    XDG_RUNTIME_DIR="$home/.run" \
    XDG_CONFIG_DIRS="$home/.config-dirs" \
    XDG_DATA_DIRS="$home/.data-dirs" \
    TMPDIR="$tmp" \
    "$@" || status=$?

[[ "$status" -eq 0 ]] || fail "the suite exited $status under the hostile environment"
if [[ -e "$marker" ]]; then
    fail "a test ran git with the home's hooks ($(wc -l <"$marker" | tr -d " ") runs):"
    LC_ALL=C sort "$marker" | uniq -c | sort -rn | awk 'NR <= 20' >&2
fi
[[ "$home_before" == "$(snapshot)" ]] || fail "the home tree changed during the run"
leftover="$(cd "$tmp" && find . -mindepth 1 -maxdepth 1 -print | LC_ALL=C sort)"
[[ -z "$leftover" ]] || fail "temporary files were left in TMPDIR:
$leftover"

if [[ "$failed" -ne 0 ]]; then
    exit 1
fi
echo "hostile-env: the suite passed; no decoy git hook ran, the home is unchanged and TMPDIR is empty"
