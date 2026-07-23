# Makefile for the py-faust-rs crate (PyO3/maturin bindings, managed with uv).
#
# Python tooling runs through uv; the Rust extension is built via the maturin
# backend. Bootstrap with `make sync`, then `make test` is the primary check.
# Run `make help` for the full target list.

UV ?= uv
PYTEST_ARGS ?=

.DEFAULT_GOAL := help

# Self-documenting help: lists every target with a `## ` comment.
.PHONY: help
help:
	@echo "py-faust-rs targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  %-14s %s\n", $$1, $$2}'

# --- environment -----------------------------------------------------------

.PHONY: sync
sync: ## Create .venv, build + install the extension (release), add dev tools
	$(UV) sync

.PHONY: lock
lock: ## Refresh uv.lock from pyproject.toml
	$(UV) lock

.PHONY: develop
develop: ## Fast in-place (debug) rebuild of the extension into the venv
	$(UV) run maturin develop --uv

.PHONY: build
build: ## Build a release wheel into target/wheels
	$(UV) run maturin build --release

# --- checks ----------------------------------------------------------------

# Rebuild the extension first so the suite runs against current sources rather
# than skipping (the suite skips when `faust_rs` is not importable).
.PHONY: test
test: develop ## Build the extension, then run the pytest suite
	$(UV) run pytest $(PYTEST_ARGS)

.PHONY: fmt
fmt: ## Format the Rust sources
	cargo fmt

.PHONY: fmt-check
fmt-check: ## Check Rust formatting without writing changes
	cargo fmt --check

.PHONY: clippy
clippy: ## Lint the Rust sources with clippy (warnings are errors)
	cargo clippy --all-targets -- -D warnings

.PHONY: lint
lint: fmt-check clippy ## Run formatting and clippy checks

.PHONY: check
check: ## Type-check the crate without producing artifacts
	cargo check

# --- housekeeping ----------------------------------------------------------

.PHONY: repl
repl: ## Open a Python REPL in the project venv (extension importable)
	$(UV) run python

.PHONY: clean
clean: ## Remove the venv, Rust target dir, wheels, and Python caches
	cargo clean
	rm -rf .venv
	rm -rf target/wheels
	find . -type d -name __pycache__ -prune -exec rm -rf {} +
	rm -rf .pytest_cache
