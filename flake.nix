{
  description = "Hellas Gate local desktop development shell";

  inputs = {
    hellas = {
      url = "github:hellas-ai/hellas/5859c3900bdbb24e449953dba1b4e0de59f0050e";
      flake = false;
    };
    # Nix pins and flake helpers come from Hellas, separately from the SDK
    # revision above, so build tooling can be updated on its own:
    #   nix flake update hellas-build
    hellas-build.url = "github:hellas-ai/hellas";
    nixpkgs.follows = "hellas-build/nixpkgs";
    rust-overlay.follows = "hellas-build/rust-overlay";
  };

  outputs = { nixpkgs, rust-overlay, hellas, hellas-build, ... }:
    let
      perSystem = hellas-build.lib.forAllSystems (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        formatter = pkgs.nixpkgs-fmt;
        devShells = { default = pkgs.mkShell {
          GATE_HELLAS_REV = hellas.rev;
          nativeBuildInputs = with pkgs; [
            cargo-tauri
            clang
            esbuild
            nodejs
            pkg-config
            python3
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
        }; } // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          windows-cross = import ./nix/windows-cross.nix { inherit pkgs rust; };
        };
      });
    in {
      formatter = nixpkgs.lib.mapAttrs (_: config: config.formatter) perSystem;
      devShells = nixpkgs.lib.mapAttrs (_: config: config.devShells) perSystem;
    };
}
