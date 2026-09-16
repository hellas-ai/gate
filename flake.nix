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
    flake-utils.lib.eachDefaultSystem (system:
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
            export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-/tmp/hellas-gate-target}"
          '' + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
            export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
            export SDKROOT="$(/usr/bin/xcrun --sdk macosx --show-sdk-path)"
            export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang
            export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/usr/bin/clang
          '';
        };
      });
}
