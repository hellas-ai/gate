{
  description = "Gate - Free and open source components";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        
        # Set up Crane with our custom toolchain
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        
        # Create a custom source that includes necessary files
        src = craneLib.cleanCargoSource (craneLib.path ./.);
        
        # Common arguments for all builds
        commonArgs = {
          inherit src;
          strictDeps = true;
          
          buildInputs = with pkgs; [
            openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # Linux-specific build dependencies
            libsoup_3 
            pango 
            gdk-pixbuf 
            atk 
            webkitgtk_4_1 
            cairo 
            gtk3
          ];
          
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };
        
        # Build dependencies only (for caching)
        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          # Fix for git dependencies
          doCheck = false;
        });
        
        # Function to build individual packages
        buildPackage = name: craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = name;
          version = "0.1.0";
          cargoExtraArgs = "--package ${name}";
          # Allow warnings during build
          RUSTFLAGS = "-A warnings";
        });
      in
      {
        formatter = pkgs.nixpkgs-fmt;

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain with wasm and native targets
            rustToolchain

            # Build tools
            pkg-config
            openssl
            protobuf
            clang

            # Development tools
            sqlx-cli
            cargo-watch
            cargo-nextest
            cargo-expand
            cargo-outdated
            cargo-edit
            cargo-machete
            cargo-udeps
            cargo-audit
            cargo-unused-features
            cargo-depgraph
            cargo-bloat

            # Wasm tools
            wasm-pack
            wasm-bindgen-cli
            trunk
            nodePackages.tailwindcss
            
            # Tauri tools
            cargo-tauri
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # Linux-specific GUI dependencies
            gtk3
            libsoup_3
            webkitgtk_4_1
            glib-networking
            libappindicator-gtk3
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            # macOS-specific dependencies
            pkgs.darwin.apple_sdk.frameworks.WebKit
            pkgs.darwin.apple_sdk.frameworks.AppKit
            pkgs.darwin.apple_sdk.frameworks.CoreServices
          ];

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          
          # Fix for dynamic library loading during build
          LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib:${pkgs.stdenv.cc.cc.lib}/lib";
        };

        # Package outputs
        packages = {
          gate-daemon = buildPackage "gate-daemon";
          
          gate-tlsforward = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            pname = "gate-tlsforward";
            version = "0.1.0";
            cargoExtraArgs = "--package gate-tlsforward --features server";
            RUSTFLAGS = "-A warnings";
          });
          
          default = self.packages.${system}.gate-daemon;
        };
        
        # Checks for CI
        checks = {
          # Format check
          fmt = craneLib.cargoFmt {
            inherit src;
          };
          
          # Clippy linting  
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-features -- -D warnings";
          });
          
          # Test suite
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
            cargoTestArgs = "--all-features";
          });
          
          # Build check
          build = self.packages.${system}.gate-daemon;
        };

        # Apps for nix run
        apps = {
          default = {
            type = "app";
            program = "${self.packages.${system}.gate-daemon}/bin/gate";
          };
        };
      });
}