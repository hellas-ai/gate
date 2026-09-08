.PHONY: bindings check dev ui-check

GATE_TARGET ?= /tmp/hellas-gate-target

bindings:
	CARGO_TARGET_DIR=$(GATE_TARGET) cargo test -p hellas-gate --lib export_bindings

check: bindings
	CARGO_TARGET_DIR=$(GATE_TARGET) cargo check -p hellas-gate
	tsc --noEmit -p ui/tsconfig.json

ui-check:
	tsc --noEmit -p ui/tsconfig.json

dev:
	CARGO_TARGET_DIR=$(GATE_TARGET) cargo tauri dev
