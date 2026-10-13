.DEFAULT_GOAL := help

.PHONY: help build release run test lint lint-md fmt fmt-check check audit install completions uninstall clean

CARGO_HOME ?= $(HOME)/.cargo
LYRICS_BIN ?= $(CARGO_HOME)/bin/lyrics
ZSH_COMPLETIONS_DIR ?= $(HOME)/.zfunc
BASH_COMPLETIONS_DIR ?= $(HOME)/.local/share/bash-completion/completions
FISH_COMPLETIONS_DIR ?= $(HOME)/.config/fish/completions

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

install: ## Install the lyrics binary via cargo (~/.cargo/bin) and shell completions
	cargo install --path . --force
	$(MAKE) completions

completions: ## Install completions for every shell found (zsh, bash, fish); needs the binary installed
	@test -x "$(LYRICS_BIN)" || { echo "$(LYRICS_BIN) not found; run make install"; exit 1; }
	@if command -v zsh >/dev/null 2>&1; then \
		mkdir -p "$(ZSH_COMPLETIONS_DIR)" && \
		"$(LYRICS_BIN)" completions zsh > "$(ZSH_COMPLETIONS_DIR)/_lyrics" && \
		echo "zsh   $(ZSH_COMPLETIONS_DIR)/_lyrics (add 'fpath+=$(ZSH_COMPLETIONS_DIR)' before compinit in ~/.zshrc)"; \
	fi
	@if command -v bash >/dev/null 2>&1; then \
		mkdir -p "$(BASH_COMPLETIONS_DIR)" && \
		"$(LYRICS_BIN)" completions bash > "$(BASH_COMPLETIONS_DIR)/lyrics" && \
		echo "bash  $(BASH_COMPLETIONS_DIR)/lyrics"; \
	fi
	@if command -v fish >/dev/null 2>&1; then \
		mkdir -p "$(FISH_COMPLETIONS_DIR)" && \
		"$(LYRICS_BIN)" completions fish > "$(FISH_COMPLETIONS_DIR)/lyrics.fish" && \
		echo "fish  $(FISH_COMPLETIONS_DIR)/lyrics.fish"; \
	fi

uninstall: ## Remove the installed lyrics binary and its shell completions
	cargo uninstall lyrics-sidecar
	rm -f "$(ZSH_COMPLETIONS_DIR)/_lyrics" "$(BASH_COMPLETIONS_DIR)/lyrics" "$(FISH_COMPLETIONS_DIR)/lyrics.fish"

clean: ## Remove build artifacts
	cargo clean
