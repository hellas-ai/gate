{
  description = "Hellas Gate local desktop development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        formatter = pkgs.nixpkgs-fmt;
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            cargo-tauri
            clang
            esbuild
            nodejs
            pkg-config
            rust
            typescript
          ];
          buildInputs = with pkgs; lib.optionals stdenv.hostPlatform.isLinux [
            sqlite
            atk
            cairo
            gdk-pixbuf
            glib
            glib-networking
            gtk3
            libsoup_3
            libappindicator-gtk3
            pango
            webkitgtk_4_1
          ];
          shellHook = ''
            export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$PWD/target}"
          '' + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
            export DEVELOPER_DIR="''${GATE_DEVELOPER_DIR:-$(/usr/bin/env -u DEVELOPER_DIR /usr/bin/xcode-select -p)}"
            # Native archives must use the same LLVM as Xcode's linker.
            export CC=/usr/bin/clang
            export CXX=/usr/bin/clang++
            export AR=/usr/bin/ar
            export SDKROOT="$(/usr/bin/xcrun --sdk macosx --show-sdk-path)"
            export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang
            export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/usr/bin/clang
          '';
        };
      });
}
