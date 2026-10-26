.PHONY: bindings check check-pin check-chain dev ui-check ui-build

export CARGO_TARGET_DIR ?= $(CURDIR)/target

bindings:
	cargo test --locked -p hellas-gate --lib export_bindings

check-pin:
	python3 scripts/check-hellas-pin.py

check: check-pin bindings ui-build
	cargo fmt --all -- --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo test --locked --workspace

check-chain:
	cargo test --locked --features chain-tests --lib paid_preview_provision_and_http_work_use_real_validators_and_delegated_coins

ui-check:
	tsc --noEmit -p ui/tsconfig.json

dev:
	cargo tauri dev

ui-build: ui-check
	node ui/build.mjs
	node --test ui/startup.test.mjs
