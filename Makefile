SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

.PHONY: help setup setup-wsl portable-check release-package build check ui-install ui-browser-install ui-dev ui-build ui-check ui-test \
        bindings bindings-check core-check app-install app-build app-run app-smoke app-check engine-smoke
.NOTPARALLEL: build check

help: ## Show supported development commands
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

setup: ## Install repo-local build tools and locked UI dependencies
	python3 scripts/dev.py setup --browser

setup-wsl: setup ## Compatibility alias for setup

build: app-build ## Build the Windows application

check: app-check ## Run production Rust and UI checks without opening Windows UI

ui-install: app-install ## Install the production UI's locked dependencies

ui-browser-install: ## Install the UI tests' Chromium browser on this host
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

core-check: ## Run host core formatting, Clippy, tests and bindings check
	python3 scripts/dev.py core-check

portable-check: ## Check portable core and UI without a Windows toolchain
	python3 scripts/dev.py check --portable

app-install: ## Install locked production frontend dependencies on this host
	npm --prefix ui ci --no-audit --no-fund

app-build: ## Build the real Windows application from Linux/WSL
	bash scripts/build-app.sh

app-run: ## Run the built app with local configuration; starts disconnected
	bash scripts/run-app.sh

app-smoke: app-build ## Build and run native UI/IPC/tray checks, without connecting voice
	bash scripts/run-app.sh --smoke

app-check: ## Check core, production UI and Windows Rust integration
	python3 scripts/dev.py check

release-package: ## Package an existing executable, notices and sources
	python3 scripts/package-third-party.py
	python3 scripts/dev.py package

engine-smoke: app-build ## Verify gateway/voice workers reject invalid private setup and exit
	bash scripts/build-native-tool.sh engine-probe
	app_target_dir="$$(realpath -m -- "$${ECHOCORD_APP_TARGET_DIR:-$${CARGO_TARGET_DIR:-$$PWD/target}}")"; \
	  "$$app_target_dir/x86_64-pc-windows-msvc/release/engine-probe.exe" "$$(wslpath -w "$$app_target_dir/x86_64-pc-windows-msvc/release/echocord.exe")" "$$(wslpath -w "$$PWD/artifacts/app/engine-probe")"
