# Gate - Local LLM Gateway

Secure peer-to-peer LLM inference with end-to-end encryption and zero data exposure.

## Quick Start

### Development Setup

1. Enter the development environment:
```bash
nix develop  # or direnv allow
```

Otherwise use rustup - the `rust-toolchain.toml` file will configure the correct toolchain.

### Development Commands

```bash
cargo build                    # Build the project
cargo test                     # Run tests
cargo clippy                   # Check code quality
cargo fmt                      # Format code
cargo check --workspace       # Check all crates compile
```

## Project Status

Currently in documentation-only phase. See `docs/META.md` for development workflow and contribution guidelines.

## Documentation

- `docs/META.md` - **Start here** - Development workflow and contribution guidelines
- `docs/OVERVIEW.md` - Project overview and business context
- `docs/DESIGN.md` - Technical architecture
- `docs/PLAN.md` - Implementation roadmap with current tasks