.PHONY: help build test hostile-env-test fmt fmt-check clippy doc deny standards-check architecture-map dependency-direction mcp-private-runtime ci-fast ci-lint ci clean

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
	@echo "  make ci-fast          Pre-handoff gate, no compile: fmt, standards-check, architecture-map, dependency-direction, mcp-private-runtime"
	@echo "  make ci-lint          clippy -D warnings, rustdoc -D warnings, cargo-deny (skipped locally if not installed)"
	@echo "  make test             All tests (nextest when installed, else cargo test) + doctests"
	@echo "  make hostile-env-test The tests under a hostile HOME, TMPDIR and XDG_*"
	@echo "  make standards-check  Run the vendored standards check (docs/standards/check.sh)"
	@echo "  make architecture-map    Check ARCHITECTURE.md maps every crate and top-level module"
	@echo "  make dependency-direction  Check which kit crates each crate may depend on"
	@echo "  make mcp-private-runtime   Check that cw-mcp's public API names no rmcp or tokio type"
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

# Every public item is documented and its docs build without a warning.
doc:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps $(LOCKED)

# Supply chain (STD-02 R23, STD-05 R23/R24): advisories, licenses, sources.
deny:
	@if $(CARGO) deny --version >/dev/null 2>&1; then \
		$(CARGO) deny check; \
	else \
		$(call missing_tool,cargo-deny,the supply-chain check); \
	fi

# nextest enforces per-test timeouts (STD-03 R21/R22); plain cargo test does
# not, so the fallback says so. nextest does not run doctests, so they are a
# second leg. A run that finds no test fails, so a filter or a build change
# that drops every test cannot pass silently.
test:
	@if $(CARGO) nextest --version >/dev/null 2>&1; then \
		$(CARGO) nextest run --workspace --all-targets $(LOCKED) --no-tests=fail; \
	else \
		$(call missing_tool,cargo-nextest,per-test timeouts); \
		$(CARGO) test --workspace --all-targets $(LOCKED); \
	fi
	$(CARGO) test --workspace --doc $(LOCKED)

# The tests again under a read-only decoy HOME and XDG_* and an awkward
# TMPDIR, checking the host side afterwards (STD-03 R20). cargo test, not
# nextest, so the gate runs where nextest is not installed.
hostile-env-test:
	./scripts/hostile-env-test.sh $(CARGO) test --workspace --all-targets $(LOCKED)

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

# ARCHITECTURE.md maps every crate and top-level module, every path it
# names exists, and every unit has a doc comment (STD-02 R43). No compile.
# Invoked directly so its bash shebang runs (like the template's structure
# target): sh on some runners is dash, where the script's crate glob cannot
# run and the gate would silently skip.
architecture-map:
	./scripts/check-architecture-map.sh

# The kit crates depend on each other only as the script's table allows,
# cw-base names no file, environment or process API, and cw-cli asks about
# terminal state only in src/terminal/. No compile. The file, environment and
# process items the other crates may not use are clippy.toml's (ci-lint).
dependency-direction:
	sh scripts/check-dependency-direction.sh

# cw-mcp keeps its MCP SDK and its runtime out of every public signature.
# No compile.
mcp-private-runtime:
	sh scripts/check-mcp-private-runtime.sh

# Pre-handoff gate for agents: no compile (STD-04 R12).
ci-fast: fmt-check standards-check architecture-map dependency-direction mcp-private-runtime

ci-lint: clippy doc deny

# Full pass. Keep aligned with .github/workflows/ci.yml.
ci: ci-fast ci-lint test hostile-env-test

clean:
	$(CARGO) clean
