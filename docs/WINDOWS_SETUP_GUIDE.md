# Windows Setup Guide for Gate

This guide provides step-by-step instructions for setting up the Gate development environment on Windows.

## Prerequisites

### Required Software

1. **Visual Studio Build Tools 2022** (or Visual Studio Community/Professional)
   - Download from: https://visualstudio.microsoft.com/downloads/
   - Required workloads:
     - C++ build tools
     - Windows 10/11 SDK (latest version)
     - MSVC v143 - VS 2022 C++ x64/x86 build tools

2. **Git for Windows**
   - Download from: https://git-scm.com/download/win
   - Ensure "Git Bash" is included in the installation

3. **vcpkg** (for OpenSSL dependencies)
   - Clone vcpkg: `git clone https://github.com/Microsoft/vcpkg.git`
   - Run: `.\vcpkg\bootstrap-vcpkg.bat`
   - Add vcpkg to your PATH or note its location

### Rust Installation

1. **Install Rust via rustup**
   ```powershell
   # Download and run rustup-init.exe from https://rustup.rs/
   # Or use PowerShell:
   Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "rustup-init.exe"
   .\rustup-init.exe
   ```

2. **Verify Rust installation**
   ```powershell
   rustc --version
   cargo --version
   ```

3. **Install the specific nightly toolchain**
   ```powershell
   rustup install nightly-2025-06-10
   rustup default nightly-2025-06-10
   ```

4. **Add required components and targets**
   ```powershell
   rustup component add rustfmt clippy rust-src rust-analyzer
   rustup target add wasm32-unknown-unknown
   rustup target add x86_64-pc-windows-msvc
   ```

### Build Tools

1. **Install Trunk (WASM build tool)**
   ```powershell
   cargo install trunk --locked
   ```

2. **Install Tauri CLI**
   ```powershell
   cargo install tauri-cli --locked
   ```

3. **Install TailwindCSS standalone**
   ```powershell
   # Create a directory for tools (e.g., C:\tools)
   mkdir C:\tools
   
   # Download TailwindCSS for Windows
   Invoke-WebRequest -Uri "https://github.com/tailwindlabs/tailwindcss/releases/download/v4.1.11/tailwindcss-windows-x64.exe" -OutFile "C:\tools\tailwindcss.exe"
   
   # Add C:\tools to your PATH environment variable
   ```

### OpenSSL Dependencies

Gate requires OpenSSL libraries for Windows. Install them using vcpkg:

```powershell
# Navigate to your vcpkg directory
cd C:\path\to\vcpkg

# Install OpenSSL for both static and dynamic linking
.\vcpkg install openssl:x64-windows-static
.\vcpkg install openssl:x64-windows-static-md

# Integrate vcpkg with your development environment
.\vcpkg integrate install
```

### Environment Variables

Set the following environment variables (add to System Properties → Environment Variables):

```
VCPKG_ROOT=C:\path\to\vcpkg
OPENSSL_DIR=%VCPKG_ROOT%\installed\x64-windows-static
PKG_CONFIG_PATH=%VCPKG_ROOT%\installed\x64-windows-static\lib\pkgconfig
```

## Project Setup

1. **Clone the repository**
   ```powershell
   git clone https://github.com/hellas-ai/gate.git
   cd gate
   ```

2. **Verify toolchain**
   ```powershell
   # This should show nightly-2025-06-10
   rustup show
   ```

3. **Build the project**
   ```powershell
   # Quick development build
   cargo build

   # Or full release build
   cargo build --release --all
   ```

## Build Commands

### Core Build Commands

```powershell
# Development build
cargo build --all

# Release build
cargo build --release --all

# Run tests
cargo test --all --all-features

# Format code
cargo fmt --all

# Lint with clippy
cargo clippy --all-features -- -D warnings
```

### Frontend Development

```powershell
# Build daemon frontend for development
cd crates\frontend-daemon
trunk serve --port 8081

# Build for production
trunk build --release
```

### Desktop GUI (Tauri)

```powershell
# Development mode
cd crates\gui
cargo tauri dev

# Build desktop application
cargo tauri build

# Debug build (faster compilation)
cargo tauri build --debug
```

### Using Make (Optional)

If you have `make` available (via Git Bash, WSL, or standalone installation):

```bash
# View all available commands
make help

# Development build
make dev

# Run tests
make test

# Build GUI application
make gui-build

# Frontend development server
make frontend-daemon-dev
```

## Troubleshooting

### Common Issues

1. **OpenSSL Link Errors**
   ```
   error: failed to run custom build command for `openssl-sys`
   ```
   **Solution**: Ensure vcpkg is properly installed and integrated. Verify `VCPKG_ROOT` environment variable.

2. **WASM Target Missing**
   ```
   error: target 'wasm32-unknown-unknown' not found
   ```
   **Solution**: Add the target: `rustup target add wasm32-unknown-unknown`

3. **Trunk Not Found**
   ```
   'trunk' is not recognized as an internal or external command
   ```
   **Solution**: Ensure Cargo's bin directory is in PATH: `%USERPROFILE%\.cargo\bin`

4. **TailwindCSS Not Found**
   **Solution**: Verify tailwindcss.exe is in your PATH or add the directory containing it.

### Build Performance Tips

1. **Use PowerShell 7** instead of Windows PowerShell for better performance
2. **Enable Windows Defender exclusions** for your development directory
3. **Use SSD storage** for the Rust toolchain and project directory
4. **Increase virtual memory** if you encounter out-of-memory errors during large builds

### WSL Alternative

For a Unix-like experience, you can use Windows Subsystem for Linux (WSL):

1. Install WSL2 with Ubuntu
2. Follow the standard Linux development setup within WSL
3. Access Windows files via `/mnt/c/`

## Development Workflow

1. **Start development server**
   ```powershell
   # Terminal 1: Backend
   cargo run --bin gate

   # Terminal 2: Frontend (if developing UI)
   cd crates\frontend-daemon
   trunk serve
   ```

2. **Build for production**
   ```powershell
   # Build everything
   cargo build --release --all

   # Build desktop application
   cd crates\gui
   cargo tauri build
   ```

3. **Run tests before committing**
   ```powershell
   cargo test --all --all-features
   cargo fmt --all -- --check
   cargo clippy --all-features -- -D warnings
   ```

## IDE Setup

### Visual Studio Code

Recommended extensions:
- `rust-analyzer` - Rust language server
- `CodeLLDB` - Debugging support
- `Tauri` - Tauri development support
- `Even Better TOML` - TOML file support

### CLion/IntelliJ IDEA

- Install the Rust plugin
- Configure the Rust toolchain path to point to your rustup installation

## Additional Resources

- [Rust Installation Guide](https://forge.rust-lang.org/infra/channel-layout.html)
- [Tauri Windows Development](https://tauri.app/v1/guides/getting-started/prerequisites/#windows)
- [vcpkg Documentation](https://vcpkg.io/en/getting-started.html)
- [Trunk Documentation](https://trunkrs.dev/)

## Support

If you encounter issues not covered in this guide:

1. Check the project's GitHub Issues: https://github.com/hellas-ai/gate/issues
2. Consult the main CLAUDE.md file for development guidelines
3. Review the CI workflow in `.github/workflows/ci.yml` for reference build steps