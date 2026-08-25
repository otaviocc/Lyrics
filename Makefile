.DEFAULT_GOAL := help

.PHONY: help build release run test lint lint-md fmt fmt-check check audit install uninstall clean

help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

build: ## Build the debug binary
	cargo build

release: ## Build the optimized release binary
	cargo build --release

run: ## Run the debug binary (use ARGS="scan ~/Music -v")
	cargo run -- $(ARGS)

test: ## Run the test suite
	cargo test

lint: ## Run clippy with warnings denied
	cargo clippy --all-targets -- -D warnings

lint-md: ## Lint markdown files (markdownlint-cli2, via npx)
	npx --yes markdownlint-cli2@0.23.2 "**/*.md"

fmt: ## Apply rustfmt
	cargo fmt

fmt-check: ## Check formatting without modifying files
	cargo fmt --check

check: fmt-check lint lint-md test ## Run fmt-check, lint, lint-md, and test: the full pre-commit gate

audit: ## Check dependencies for known security advisories (cargo-audit)
	cargo audit

install: ## Install the lyrics binary via cargo (~/.cargo/bin)
	cargo install --path . --force

uninstall: ## Remove the installed lyrics binary
	cargo uninstall lyrics-sidecar

clean: ## Remove build artifacts
	cargo clean
