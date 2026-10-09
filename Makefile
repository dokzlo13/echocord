SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

.PHONY: help setup-wsl build check check-audio check-desktop probe-audio probe-desktop \
        run-audio run-desktop smoke-windows ui-install ui-browser-install ui-dev ui-build ui-check ui-test \
        bindings bindings-check core-check app-install app-build app-run app-smoke app-check \
        check-probes build-probes prototype-install prototype-dev prototype-check engine-smoke
.NOTPARALLEL: build check smoke-windows build-probes check-probes

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

build-probes: probe-audio probe-desktop ## Build the isolated dependency probes

check-probes: check-audio check-desktop ## Check the isolated dependency probes

probe-audio: ## Cross-build the offline audio probe in WSL
	bash scripts/probe-audio.sh

probe-desktop: ## Cross-build the minimal Tauri probe in WSL
	bash scripts/probe-desktop.sh

check-audio: ## Check Rust formatting and Windows-target Clippy for the audio probe
	bash scripts/probe-audio.sh --lint

check-desktop: ## Check Rust formatting, Windows-target Clippy, and Tauri build
	bash scripts/probe-desktop.sh --check

run-audio: ## Build and run the offline audio probe on Windows (no recording or Discord)
	bash scripts/probe-audio.sh --run

run-desktop: ## Build and run the self-closing Windows Tauri lifecycle probe
	bash scripts/probe-desktop.sh --run

smoke-windows: run-audio run-desktop ## Run both native Windows probes explicitly

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

prototype-install: ## Install the historical draft's dependencies
	npm --prefix prototypes/control-ui ci --no-audit --no-fund

prototype-dev: ## Preview the historical draft (not the application UI)
	npm --prefix prototypes/control-ui run dev

prototype-check: ## Check the historical fixture prototype
	npm --prefix prototypes/control-ui run check

engine-smoke: app-build ## Verify actual GUI engine mode rejects empty credentials before capture
	bash scripts/build-native-tool.sh engine-probe
	target/x86_64-pc-windows-msvc/release/engine-probe.exe "$$(wslpath -w "$$PWD/target/x86_64-pc-windows-msvc/release/echocord.exe")" "$$(wslpath -w "$$PWD/.trash/app/engine-probe")"
