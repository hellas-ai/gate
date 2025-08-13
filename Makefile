# Gate Project Makefile
# Comprehensive build, test, and development commands

# Variables
CARGO = cargo
TRUNK = trunk
TAURI = cargo tauri
RUST_LOG ?= info
RUST_BACKTRACE ?= 1

# Detect OS
UNAME_S := $(shell uname -s)
ifeq ($(UNAME_S),Darwin)
    OPEN_CMD = open
else
    OPEN_CMD = xdg-open
endif

# Colors for output
RED = \033[0;31m
GREEN = \033[0;32m
YELLOW = \033[0;33m
BLUE = \033[0;34m
NC = \033[0m # No Color

# Default target
.DEFAULT_GOAL := help

# Phony targets
.PHONY: help build dev run clean test test-unit test-integration lint fmt fmt-check check audit \
        frontend-build-dev frontend-build frontend-clean frontend-daemon-dev frontend-daemon-build \
        frontend-tauri-dev frontend-tauri-build frontend-relay-dev frontend-relay-build \
        gui-dev gui-build gui-build-dev gui-build-dmg \
        docs docs-deps ci pre-commit db-migrate db-reset server p2p tlsforward all-services

## Help
help: ## Show this help message
	@echo "$(BLUE)Gate Project Makefile$(NC)"
	@echo "$(YELLOW)Usage:$(NC) make [target]"
	@echo ""
	@echo "$(GREEN)Available targets:$(NC)"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(BLUE)%-20s$(NC) %s\n", $$1, $$2}'
	@echo ""
	@echo "$(YELLOW)Examples:$(NC)"
	@echo "  make build          # Build all crates in release mode"
	@echo "  make test           # Run all tests"
	@echo "  make frontend-dev   # Start frontend development server"
	@echo "  make gui-build      # Build native desktop app"

## Building
build: ## Build all crates in release mode
	@echo "$(GREEN)Building all crates in release mode...$(NC)"
	$(CARGO) build --release --all

dev: ## Build all crates in debug mode
	@echo "$(GREEN)Building all crates in debug mode...$(NC)"
	$(CARGO) build --all

run: ## Run the Gate server
	@echo "$(GREEN)Starting Gate server...$(NC)"
	RUST_LOG=$(RUST_LOG) $(CARGO) run --bin gate

clean: ## Clean all build artifacts
	@echo "$(RED)Cleaning build artifacts...$(NC)"
	$(CARGO) clean
	rm -rf crates/frontend/dist
	rm -rf crates/frontend-daemon/dist
	rm -rf crates/frontend-tauri/dist
	rm -rf crates/frontend-relay/dist
	rm -f crates/frontend/assets/tailwind.output.css
	rm -f crates/frontend-daemon/assets/tailwind.output.css
	rm -f crates/frontend-tauri/assets/tailwind.output.css
	rm -f crates/frontend-relay/assets/tailwind.output.css

## Testing & Quality
test: ## Run all tests
	@echo "$(GREEN)Running all tests...$(NC)"
	$(CARGO) test --all --all-features

test-unit: ## Run unit tests only
	@echo "$(GREEN)Running unit tests...$(NC)"
	$(CARGO) test --lib --all

test-integration: ## Run integration tests only
	@echo "$(GREEN)Running integration tests...$(NC)"
	$(CARGO) test --test '*' --all

lint: ## Run clippy on all crates
	@echo "$(YELLOW)Running clippy...$(NC)"
	$(CARGO) clippy --all --all-targets --all-features -- -D warnings

fmt: ## Format all code with rustfmt
	@echo "$(GREEN)Formatting code...$(NC)"
	$(CARGO) fmt --all

fmt-check: ## Check formatting without making changes
	@echo "$(YELLOW)Checking code formatting...$(NC)"
	$(CARGO) fmt --all -- --check

check: ## Run cargo check on all crates
	@echo "$(YELLOW)Running cargo check...$(NC)"
	$(CARGO) check --all --all-features

audit: ## Security audit of dependencies
	@echo "$(YELLOW)Running security audit...$(NC)"
	$(CARGO) audit


frontend-build: ## Build frontend for production
	@echo "$(GREEN)Building frontend for production...$(NC)"
	cd crates/frontend && $(TRUNK) build --release

frontend-build-dev: ## Build frontend dev
	@echo "$(GREEN)Building frontend for dev...$(NC)"
	cd crates/frontend && $(TRUNK) build

frontend-clean: ## Clean frontend build artifacts
	@echo "$(RED)Cleaning frontend artifacts...$(NC)"
	rm -rf crates/frontend/dist
	rm -f crates/frontend/assets/tailwind.output.css

## Frontend - Daemon
frontend-daemon-dev: ## Run daemon frontend development server
	@echo "$(GREEN)Starting daemon frontend dev server on http://localhost:8081...$(NC)"
	cd crates/frontend-daemon && $(TRUNK) serve

frontend-daemon-build: ## Build daemon frontend for production
	@echo "$(GREEN)Building daemon frontend for production...$(NC)"
	cd crates/frontend-daemon && $(TRUNK) build --release

## Frontend - Tauri
frontend-tauri-dev: ## Run Tauri frontend development server
	@echo "$(GREEN)Starting Tauri frontend dev server on http://localhost:8081...$(NC)"
	cd crates/frontend-tauri && $(TRUNK) serve

frontend-tauri-build: ## Build Tauri frontend for production
	@echo "$(GREEN)Building Tauri frontend for production...$(NC)"
	cd crates/frontend-tauri && $(TRUNK) build --release

## Frontend - Relay
frontend-relay-dev: ## Run relay frontend development server
	@echo "$(GREEN)Starting relay frontend dev server on http://localhost:8081...$(NC)"
	cd crates/frontend-relay && $(TRUNK) serve

frontend-relay-build: ## Build relay frontend for production
	@echo "$(GREEN)Building relay frontend for production...$(NC)"
	cd crates/frontend-relay && $(TRUNK) build --release

## GUI/Tauri
gui-dev: ## Run Tauri in development mode
	@echo "$(GREEN)Starting Tauri development mode...$(NC)"
	cd crates/gui && $(TAURI) dev

gui-build: ## Build native desktop app
	@echo "$(GREEN)Building native desktop app...$(NC)"
	cd crates/gui && $(TAURI) build

gui-build-dev: ## Build native desktop app in debug mode (faster)
	@echo "$(GREEN)Building native desktop app (debug mode)...$(NC)"
	cd crates/gui && $(TAURI) build --debug

gui-build-dmg: ## Build macOS DMG installer (macOS only)
ifeq ($(UNAME_S),Darwin)
	@echo "$(GREEN)Building macOS DMG installer...$(NC)"
	cd crates/gui && $(TAURI) build --bundles dmg
else
	@echo "$(RED)DMG building is only available on macOS$(NC)"
	@exit 1
endif

## Documentation
docs: ## Generate and open documentation
	@echo "$(GREEN)Generating documentation...$(NC)"
	$(CARGO) doc --all --no-deps --open

docs-deps: ## Generate dependency graph
	@echo "$(GREEN)Generating dependency graph...$(NC)"
	$(CARGO) depgraph --all-deps | dot -Tpng > target/dependencies.png
	@echo "$(GREEN)Dependency graph saved to target/dependencies.png$(NC)"
	$(OPEN_CMD) target/dependencies.png

## CI/CD
ci: fmt-check lint test ## Run all CI checks
	@echo "$(GREEN)All CI checks passed!$(NC)"

pre-commit: fmt lint test-unit ## Run checks before committing
	@echo "$(GREEN)Pre-commit checks passed!$(NC)"

## Database
db-migrate: ## Run database migrations
	@echo "$(GREEN)Running database migrations...$(NC)"
	cd crates/sqlx && sqlx migrate run

db-reset: ## Reset database
	@echo "$(RED)Resetting database...$(NC)"
	rm -f gate.db gate.db-shm gate.db-wal
	@echo "$(GREEN)Database reset. Run 'make db-migrate' to recreate.$(NC)"

## Services
server: ## Run the main server
	@echo "$(GREEN)Starting Gate server...$(NC)"
	RUST_LOG=$(RUST_LOG) $(CARGO) run --bin gate

p2p: ## Run P2P service (if available)
	@echo "$(YELLOW)P2P service is a library, not a binary. Use it as a dependency.$(NC)"

tlsforward: ## Run TLS forward service
	@echo "$(GREEN)Starting TLS forward service...$(NC)"
	RUST_LOG=$(RUST_LOG) $(CARGO) run --bin gate-tlsforward

all-services: ## Run all services (requires multiple terminals)
	@echo "$(YELLOW)Starting all services...$(NC)"
	@echo "$(YELLOW)This requires multiple terminal windows.$(NC)"
	@echo ""
	@echo "Run these commands in separate terminals:"
	@echo "  1. make server"
	@echo "  2. make frontend-dev"
	@echo "  3. make tlsforward (if needed)"

## Workspace Management
workspace-check: ## Check workspace configuration
	@echo "$(YELLOW)Checking workspace configuration...$(NC)"
	@echo "Workspace members:"
	@grep -A20 "members = \[" Cargo.toml | grep -E '^\s*"' | sed 's/[",]//g' | sed 's/^/  /'
	@echo ""
	@echo "Default members:"
	@grep "default-members" Cargo.toml | sed 's/.*= //'

## Advanced
bench: ## Run benchmarks
	@echo "$(GREEN)Running benchmarks...$(NC)"
	$(CARGO) bench --all

coverage: ## Generate test coverage report
	@echo "$(GREEN)Generating test coverage...$(NC)"
	$(CARGO) tarpaulin --all --out Html
	$(OPEN_CMD) tarpaulin-report.html

update-deps: ## Update dependencies
	@echo "$(YELLOW)Updating dependencies...$(NC)"
	$(CARGO) update
	cd crates/frontend && npm update

check-updates: ## Check for outdated dependencies
	@echo "$(YELLOW)Checking for outdated dependencies...$(NC)"
	$(CARGO) outdated

## Quick Commands
quick: dev ## Quick build for development
	@echo "$(GREEN)Quick build complete!$(NC)"

watch: ## Watch for changes and rebuild
	@echo "$(GREEN)Watching for changes...$(NC)"
	cargo watch -x check -x test -x run

## Release
release-patch: ## Create a patch release
	@echo "$(YELLOW)Creating patch release...$(NC)"
	cargo release patch

release-minor: ## Create a minor release
	@echo "$(YELLOW)Creating minor release...$(NC)"
	cargo release minor

release-major: ## Create a major release
	@echo "$(YELLOW)Creating major release...$(NC)"
	cargo release major

docker-ci:
	act -j gui-build -P ubuntu-latest=catthehacker/ubuntu:act-latest --matrix os:ubuntu-latest
