SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

.PHONY: help setup-wsl build check ui-install ui-browser-install ui-dev ui-build ui-check ui-test \
        bindings bindings-check core-check app-install app-build app-run app-smoke app-check engine-smoke
.NOTPARALLEL: build check

help: ## Show supported development commands
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

setup-wsl: ## Install pinned user-level WSL build tools (requires rustup and uv)
	rustup show active-toolchain
	uv tool install cargo-xwin==0.23.1
	uv tool install cmake==4.4.4
	uv tool install ninja==1.13.2
	python3 scripts/prepare-opus.py

build: app-build ## Build the Windows application

check: app-check ## Run production Rust and UI checks without opening Windows UI

ui-install: app-install ## Install the production UI's locked dependencies

ui-browser-install: ## Install the UI tests' Chromium browser in WSL
	cd ui && npx --no-install playwright install chromium

ui-dev: ## Start the mocked UI preview on localhost
	npm --prefix ui run dev

ui-build: ## Typecheck and build the production UI
	npm --prefix ui run build

ui-check: ## Run UI lint, typecheck, build, and behavior tests
	npm --prefix ui run check

ui-test: ## Run production UI tests with fixture adapters
	npm --prefix ui test

bindings: ## Regenerate TypeScript control types from Rust
	cargo run --locked -p echocord-core --bin export-bindings -- ui/src/generated.ts

bindings-check: ## Fail if generated frontend types differ from Rust
	cargo run --locked -p echocord-core --bin export-bindings -- --check ui/src/generated.ts

core-check: ## Run portable core formatting, Clippy, and tests
	python3 scripts/prepare-opus.py
	python3 scripts/test_prepare_opus.py
	cargo fmt --all --check
	cargo clippy --locked -p echocord-core --all-targets -- -D warnings
	cargo test --locked -p echocord-core
	$(MAKE) bindings-check

app-install: ## Install locked production frontend dependencies in WSL
	npm --prefix ui ci --no-audit --no-fund

app-build: ## Build the real Windows application from WSL
	bash scripts/build-app.sh

app-run: ## Run the built app with private local configuration; starts disconnected
	bash scripts/run-app.sh

app-smoke: app-build ## Build and run native UI/IPC/tray checks, without connecting voice
	bash scripts/run-app.sh --smoke

app-check: core-check ## Check production UI and Windows Rust integration
	npm --prefix ui run check
	bash scripts/build-app.sh --check

engine-smoke: app-build ## Verify actual GUI engine mode rejects empty credentials before capture
	bash scripts/build-native-tool.sh engine-probe
	target/x86_64-pc-windows-msvc/release/engine-probe.exe "$$(wslpath -w "$$PWD/target/x86_64-pc-windows-msvc/release/echocord.exe")" "$$(wslpath -w "$$PWD/.trash/app/engine-probe")"
