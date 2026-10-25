.PHONY: help build test fmt fmt-check clippy deny standards-check ci-fast ci-lint ci clean

CARGO ?= cargo
# Every build uses the committed Cargo.lock as is; a stale lock fails instead
# of being rewritten (STD-05 R23).
LOCKED := --locked
# `STANDARDS_STRICT=1` makes a missing docs/standards/ a failure (CI sets it).
STANDARDS_STRICT ?=
# GitHub Actions sets CI=true. There a missing gate tool fails the gate;
# locally it warns and skips or falls back (STD-04 R10).
CI ?=
# Cargo refuses a virtual workspace with no members, so until the first crate
# lands every cargo target is a no-op that says so.
CRATES := $(wildcard crates/*/Cargo.toml)
ifeq ($(CRATES),)
$(info no crates yet: cargo steps are skipped)
CARGO := true
endif

# $(call missing_tool,<tool>,<what it provides>): fail under CI, else warn.
define missing_tool
	if [ "$(CI)" = "true" ]; then \
		echo "error: $(1) is not installed; CI requires it for $(2)." >&2; exit 1; \
	fi; \
	echo "warning: $(1) not installed; continuing without $(2) (CI requires it)." >&2; \
	echo "         install: cargo install $(1) --locked" >&2
endef

help:
	@echo "satellites make targets"
	@echo ""
	@echo "  make ci-fast          Pre-handoff gate, no compile: fmt, standards-check"
	@echo "  make ci-lint          clippy -D warnings + cargo-deny (skipped locally if not installed)"
	@echo "  make test             All tests (nextest when installed, else cargo test) + doctests"
	@echo "  make standards-check  Run the vendored standards check (docs/standards/check.sh)"
	@echo "  make ci               Everything above; what CI runs"
	@echo "  make build | fmt | clean"

build:
	$(CARGO) build --workspace $(LOCKED)

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets $(LOCKED) -- -D warnings

# Supply chain (STD-02 R23, STD-05 R23/R24): advisories, licenses, sources.
deny:
	@if $(CARGO) deny --version >/dev/null 2>&1; then \
		$(CARGO) deny check; \
	else \
		$(call missing_tool,cargo-deny,the supply-chain check); \
	fi

# nextest enforces per-test timeouts (STD-03 R21/R22); plain cargo test does
# not, so the fallback says so. nextest does not run doctests, so they are a
# second leg. Until the first crate lands the workspace has no tests, so the
# zero-test check is off; turn on `--no-tests=fail` with the first crate.
test:
	@if $(CARGO) nextest --version >/dev/null 2>&1; then \
		$(CARGO) nextest run --workspace --all-targets $(LOCKED) --no-tests=pass; \
	else \
		$(call missing_tool,cargo-nextest,per-test timeouts); \
		$(CARGO) test --workspace --all-targets $(LOCKED); \
	fi
	$(CARGO) test --workspace --doc $(LOCKED)

# The constellation standards this repo adopted are vendored under
# docs/standards/ by operations/scripts/sync-standards.sh, which also writes
# check.sh.
standards-check:
	@if [ -f docs/standards/check.sh ]; then \
		sh docs/standards/check.sh; \
	elif [ "$(STANDARDS_STRICT)" = "1" ]; then \
		echo "error: standards not vendored (no docs/standards/check.sh)." >&2; \
		exit 1; \
	else \
		echo "warning: standards not vendored (no docs/standards/check.sh); skipping." >&2; \
	fi

# Pre-handoff gate for agents: no compile (STD-04 R12).
ci-fast: fmt-check standards-check

ci-lint: clippy deny

# Full pass. Keep aligned with .github/workflows/ci.yml.
ci: ci-fast ci-lint test

clean:
	$(CARGO) clean
