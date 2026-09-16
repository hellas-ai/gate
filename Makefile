.PHONY: bindings check dev ui-check ui-build

export CARGO_TARGET_DIR ?= /tmp/hellas-gate-target

bindings:
	cargo test --locked -p hellas-gate --lib export_bindings

check: bindings ui-build
	cargo fmt --all -- --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo test --locked --workspace

ui-check:
	tsc --noEmit -p ui/tsconfig.json

dev:
	cargo tauri dev

ui-build: ui-check
	node ui/build.mjs
