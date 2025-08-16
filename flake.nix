{
  description = "Gate - Free and open source components";

  inputs = {
    nixpkgs.url = "github:RossComputerGuy/nixpkgs/fix/rustc-llvm";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay)];
      pkgs = import nixpkgs {
        inherit system overlays;
      };

      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      # Create a custom rustPlatform that uses our nightly toolchain
      customRustPlatform = pkgs.makeRustPlatform {
        cargo = rustToolchain;
        rustc = rustToolchain;
        stdenv =
          if pkgs.stdenv.isLinux
          then pkgs.stdenvAdapters.useMoldLinker pkgs.stdenv
          else pkgs.stdenv;
      };

      # Create filtered source for Rust builds
      filteredSource = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          (pkgs.lib.fileset.fileFilter (file: file.name == "Cargo.toml" || file.name == "build.rs") ./crates)
          (pkgs.lib.fileset.fileFilter (file: pkgs.lib.hasSuffix ".rs" file.name) ./.)
          ./Cargo.toml
          ./Cargo.lock
          ./rust-toolchain.toml
          ./crates/sqlx/migrations
        ];
      };

      # Helper function to create frontend source for a specific frontend crate
      mkFrontendSource = frontendName:
        pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            # Include all the base Rust files
            (pkgs.lib.fileset.fileFilter (file: pkgs.lib.hasSuffix ".rs" file.name) ./.)
            ./Cargo.toml
            ./Cargo.lock
            # Include crates directory structure with Cargo.toml files
            (pkgs.lib.fileset.fileFilter
              (file: file.name == "Cargo.toml" || file.name == "build.rs" || file.name == "Trunk.toml" || file.name == "index.html" || file.name == "tailwind.config.js")
              ./crates)
            ./rust-toolchain.toml
            # Include the entire frontend directory
            ./crates/${frontendName}
            # Include frontend-common
            ./crates/frontend-common
            # Include chat-ui
            ./crates/chat-ui
          ];
        };

      # Frontend sources for each variant
      frontendDaemonSource = mkFrontendSource "frontend-daemon";
      frontendTauriSource = mkFrontendSource "frontend-tauri";
      frontendRelaySource = mkFrontendSource "frontend-relay";

      # Filtered source for GUI (includes frontend assets and tauri config)
      guiSource = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          # Include all the base Rust files
          (pkgs.lib.fileset.fileFilter (file: pkgs.lib.hasSuffix ".rs" file.name) ./.)
          ./Cargo.toml
          ./Cargo.lock
          # Include crates directory structure with Cargo.toml files
          (pkgs.lib.fileset.fileFilter
            (file: file.name == "Cargo.toml" || file.name == "build.rs")
            ./crates)
          ./rust-toolchain.toml
          # Include frontend files for building
          ./crates/frontend-tauri/index.html
          ./crates/frontend-tauri/Trunk.toml
          ./crates/frontend-tauri/style.css
          (pkgs.lib.fileset.maybeMissing ./crates/frontend-tauri/assets)
          ./crates/frontend-tauri/tailwind.config.js
          ./crates/frontend-tauri/.gitignore
          # Include frontend-common
          ./crates/frontend-common
          # Include GUI-specific files
          ./crates/gui/tauri.conf.json
          ./crates/gui/icons
        ];
      };
    in {
      formatter = pkgs.nixpkgs-fmt;

      devShells.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          trunk
          pkg-config
          wasm-bindgen-cli
          nodePackages.tailwindcss
        ];

        buildInputs = with pkgs;
          [
            rustToolchain
            openssl
            protobuf
            clang
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            mold
            # libsoup_3
            pango
            gdk-pixbuf
            atk
            webkitgtk_4_1
            cairo
            gio-sharp
            gtk3
          ]
          ++ [
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
            cargo-sort
            cargo-unused-features
            cargo-depgraph
            cargo-bloat

            # Python for testing llm-streams and icon conversion
            (python3.withPackages (ps:
              with ps; [
                pytest
                vcrpy
                python-dotenv
                pytest-recording
                requests
                nest-asyncio
                pytest-asyncio
                httpx
                openai
                anthropic
                pydantic
                # Icon conversion tools
                cairosvg
                pillow
              ]))

            # Wasm tools
            wasm-pack
            wasm-bindgen-cli

            trunk
            nodePackages.tailwindcss

            # Tauri tools
            cargo-tauri

            # github
            act
            gh
            
            # Image conversion tools
            librsvg  # provides rsvg-convert
            imagemagick
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # Linux-specific GUI dependencies
            gtk3
            libsoup_3
            webkitgtk_4_1
            glib-networking
            libappindicator-gtk3
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            # macOS-specific dependencies
            # darwin.apple_sdk.
          ];

        RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

        # Fix for dynamic library loading during build
        LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib:${pkgs.stdenv.cc.cc.lib}/lib";
      };

      # Export rustToolchain and customRustPlatform for use by other flakes
      inherit rustToolchain customRustPlatform;

      # Package outputs
      packages = let
        outputHashes = {
          "catgrad-0.1.1" = "sha256-Im5fhCYqlX1LVtDLncWHIk1CPW+fnhncGYjbZofGRZQ=";
          "yew-0.21.0" = "sha256-yLPQ5+qKSxFDilvz9dquK+tUvwLvdTHnx1Udx8Suvms=";
        };
      in {
        # Frontend packages - built separately for better caching
        gate-frontend-daemon = customRustPlatform.buildRustPackage {
          pname = "gate-frontend-daemon";
          version = "0.1.0";
          src = frontendDaemonSource;
          cargoLock = {
            lockFile = ./Cargo.lock;
            inherit outputHashes;
          };

          nativeBuildInputs = with pkgs; [
            openssl
            trunk
            wasm-bindgen-cli
            nodePackages.tailwindcss
          ];

          # Skip normal cargo build
          buildPhase = ''
            runHook preBuild

            cd crates/frontend-daemon
            trunk build --release

            runHook postBuild
          '';

          # Skip normal cargo install
          installPhase = ''
            runHook preInstall

            mkdir -p $out
            cp -r dist/* $out/

            runHook postInstall
          '';

          # Don't run tests
          doCheck = false;
        };

        gate-frontend-tauri = customRustPlatform.buildRustPackage {
          pname = "gate-frontend-tauri";
          version = "0.1.0";
          src = frontendTauriSource;
          cargoLock = {
            lockFile = ./Cargo.lock;
            inherit outputHashes;
          };

          nativeBuildInputs = with pkgs; [
            openssl
            trunk
            wasm-bindgen-cli
            nodePackages.tailwindcss
          ];

          # Skip normal cargo build
          buildPhase = ''
            runHook preBuild

            cd crates/frontend-tauri
            trunk build --release

            runHook postBuild
          '';

          # Skip normal cargo install
          installPhase = ''
            runHook preInstall

            mkdir -p $out
            cp -r dist/* $out/

            runHook postInstall
          '';

          # Don't run tests
          doCheck = false;
        };

        gate-frontend-relay = customRustPlatform.buildRustPackage {
          pname = "gate-frontend-relay";
          version = "0.1.0";
          src = frontendRelaySource;
          cargoLock = {
            lockFile = ./Cargo.lock;
            inherit outputHashes;
          };

          nativeBuildInputs = with pkgs; [
            openssl
            trunk
            wasm-bindgen-cli
            nodePackages.tailwindcss
          ];

          # Skip normal cargo build
          buildPhase = ''
            runHook preBuild

            cd crates/frontend-relay
            trunk build --release

            runHook postBuild
          '';

          # Skip normal cargo install
          installPhase = ''
            runHook preInstall

            mkdir -p $out
            cp -r dist/* $out/

            runHook postInstall
          '';

          # Don't run tests
          doCheck = false;
        };

        # Daemon package - just builds the Rust binary
        gate-daemon = customRustPlatform.buildRustPackage {
          pname = "gate-daemon";
          version = "0.1.0";
          src = filteredSource;
          cargoLock = {
            lockFile = ./Cargo.lock;
            inherit outputHashes;
          };
          cargoBuildFlags = ["--package" "gate-daemon"];
          doCheck = false;
          nativeBuildInputs = with pkgs; [pkg-config openssl];
          buildInputs = with pkgs;
            [openssl]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              libsoup_3
              pango
              gdk-pixbuf
              atk
              webkitgtk_4_1
              cairo
              gio-sharp
              gtk3
            ];
          # Allow warnings during build
          RUSTFLAGS = "-A warnings";
        };

        # Combined package with launcher script
        gate = pkgs.symlinkJoin {
          name = "gate";
          paths = [self.packages.${system}.gate-daemon];
          nativeBuildInputs = [pkgs.makeWrapper];
          postBuild = ''
            # Create launcher wrapper
            makeWrapper $out/bin/gate $out/bin/gate-launcher \
              --set GATE_SERVER__STATIC_DIR "${self.packages.${system}.gate-frontend-daemon}" \
              --prefix PATH : "${pkgs.lib.makeBinPath [pkgs.curl]}" \
              --run 'echo "Starting Gate daemon..."' \
              --run 'trap "echo -e \"\\nShutting down Gate daemon...\"" EXIT'

            # Create a more sophisticated launcher script
            cat > $out/bin/gate-with-browser << 'EOF'
            #!${pkgs.bash}/bin/bash
            set -e

            # Configuration
            GATE_PORT="''${GATE_PORT:-3000}"
            GATE_HOST="''${GATE_HOST:-localhost}"
            GATE_URL="http://''${GATE_HOST}:''${GATE_PORT}"

            # Set static files directory
            export GATE_SERVER__STATIC_DIR="${self.packages.${system}.gate-frontend-daemon}"
            echo "DEBUG: Static files directory set to: $GATE_SERVER__STATIC_DIR"

            # Function to check if server is ready
            wait_for_server() {
              echo "Waiting for Gate daemon to start..."
              for i in {1..30}; do
                if ${pkgs.curl}/bin/curl -s -o /dev/null "''${GATE_URL}/health"; then
                  echo "Gate daemon is ready!"
                  return 0
                fi
                sleep 0.5
              done
              echo "Gate daemon failed to start"
              return 1
            }

            # Function to open browser
            open_browser() {
              if [ "''${GATE_NO_BROWSER:-}" != "1" ]; then
                echo "Opening browser at ''${GATE_URL}"
                if command -v xdg-open >/dev/null 2>&1; then
                  xdg-open "''${GATE_URL}" 2>/dev/null || true
                elif command -v open >/dev/null 2>&1; then
                  open "''${GATE_URL}" 2>/dev/null || true
                else
                  echo "Please open your browser and navigate to ''${GATE_URL}"
                fi
              fi
            }

            # Start the daemon
            echo "Starting Gate daemon..."
            ${self.packages.${system}.gate-daemon}/bin/gate &
            GATE_PID=$!

            # Set up signal handlers for clean shutdown
            cleanup() {
              echo -e "\nShutting down Gate daemon..."
              kill $GATE_PID 2>/dev/null || true
              wait $GATE_PID 2>/dev/null || true
              exit 0
            }
            trap cleanup INT TERM

            # Wait for server and open browser
            if wait_for_server; then
              open_browser
              echo "Gate is running at ''${GATE_URL}"
              echo "Press Ctrl+C to stop"
              # Wait for the daemon process
              wait $GATE_PID
            else
              kill $GATE_PID 2>/dev/null || true
              exit 1
            fi
            EOF

            chmod +x $out/bin/gate-with-browser
          '';
        };
        gate-tlsforward = customRustPlatform.buildRustPackage {
          pname = "gate-tlsforward";
          version = "0.1.0";
          src = filteredSource;
          cargoLock = {
            lockFile = ./Cargo.lock;
            inherit outputHashes;
          };
          buildFeatures = ["server"];
          cargoBuildFlags = ["--package" "gate-tlsforward"];
          doCheck = false;
          nativeBuildInputs = with pkgs; [pkg-config];
          buildInputs = with pkgs; [openssl];
          # Allow warnings during build
          RUSTFLAGS = "-A warnings";
        };

        # GUI package with Tauri
        gate-gui = customRustPlatform.buildRustPackage {
          pname = "gate-gui";
          version = "0.1.0";
          src = guiSource;

          # Don't set cargoRoot since we're using workspace Cargo.lock
          buildAndTestSubdir = "crates/gui";

          cargoLock = {
            lockFile = ./Cargo.lock;
            inherit outputHashes;
          };

          nativeBuildInputs = with pkgs;
            [
              pkg-config
              cargo-tauri.hook
              nodePackages.nodejs
              trunk
              wasm-bindgen-cli
              nodePackages.tailwindcss
              makeWrapper
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              darwin.apple_sdk.frameworks.WebKit
              darwin.apple_sdk.frameworks.AppKit
              darwin.apple_sdk.frameworks.CoreServices
              # Tools needed for DMG creation
              libicns
              imagemagick
              makeBinaryWrapper
              # Additional DMG tools
              darwin.cctools
              (pkgs.writeShellScriptBin "hdiutil" ''
                # Wrapper for hdiutil to ensure it can find necessary tools
                export PATH="${pkgs.coreutils}/bin:${pkgs.findutils}/bin:$PATH"
                exec /usr/bin/hdiutil "$@"
              '')
            ];

          buildInputs = with pkgs;
            [
              openssl
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              gtk3
              libsoup_3
              webkitgtk_4_1
              cairo
              gdk-pixbuf
              glib
              pango
              atk
              glib-networking
              libappindicator-gtk3
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              darwin.apple_sdk.frameworks.WebKit
              darwin.apple_sdk.frameworks.AppKit
              darwin.apple_sdk.frameworks.CoreServices
              darwin.apple_sdk.frameworks.Security
              darwin.apple_sdk.frameworks.CoreGraphics
              darwin.apple_sdk.frameworks.CoreFoundation
              darwin.apple_sdk.frameworks.Foundation
              darwin.apple_sdk.frameworks.ApplicationServices
            ];

          # Disable default cargo build phases since cargo-tauri.hook handles it
          dontCargoCheck = true;
          doCheck = false;

          # Set environment variables
          WEBKIT_DISABLE_DMABUF_RENDERER = "1"; # Fix for WebKit on some Linux systems

          # Tauri-specific configuration
          tauriBuildFlags =
            if pkgs.stdenv.isLinux
            then "--bundles deb,appimage"
            else if pkgs.stdenv.isDarwin
            then "--bundles app,dmg"
            else "";

          # cargo-tauri.hook will handle everything, including running
          # the beforeBuildCommand from tauri.conf.json

          # Handle platform-specific setup
          preBuild = pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
            # Ensure DMG creation script can find tools
            export PATH="${pkgs.coreutils}/bin:${pkgs.findutils}/bin:${pkgs.gnutar}/bin:$PATH"

            # Create wrapper for SetFile if needed
            mkdir -p $TMPDIR/bin
            cat > $TMPDIR/bin/SetFile << 'EOF'
            #!/bin/bash
            # Stub for SetFile - not critical for DMG creation
            exit 0
            EOF
            chmod +x $TMPDIR/bin/SetFile
            export PATH="$TMPDIR/bin:$PATH"
          '';

          postInstall = ''
            # Wrap the binary with library paths on Linux
            if [[ -f $out/bin/gate-gui ]] && [[ "$(uname)" = "Linux" ]]; then
              wrapProgram $out/bin/gate-gui \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath (with pkgs; [
              gtk3
              libsoup_3
              webkitgtk_4_1
              cairo
              gdk-pixbuf
              glib
              pango
              atk
              glib-networking
              libappindicator-gtk3
            ])}" \
                --prefix GIO_MODULE_DIR : "${pkgs.glib-networking}/lib/gio/modules" \
                --set WEBKIT_DISABLE_DMABUF_RENDERER "1"
            fi

            # Handle platform-specific bundle installation
            if [[ -d target/release/bundle ]]; then
              # Linux bundles
              if [[ -d target/release/bundle/deb ]]; then
                mkdir -p $out/share/applications

                # Install desktop file if it exists
                if [[ -f target/release/bundle/deb/gate-gui.desktop ]]; then
                  cp target/release/bundle/deb/gate-gui.desktop $out/share/applications/
                fi
              fi

              # Install AppImage if built
              if [[ -f target/release/bundle/appimage/gate-gui_*.AppImage ]]; then
                mkdir -p $out/bin
                cp target/release/bundle/appimage/gate-gui_*.AppImage $out/bin/gate-gui.AppImage
                chmod +x $out/bin/gate-gui.AppImage
              fi

              # macOS bundles
              if [[ -d target/release/bundle/macos ]]; then
                mkdir -p $out/Applications
                cp -r target/release/bundle/macos/*.app $out/Applications/ || true

                # Create a wrapper script for command-line usage
                mkdir -p $out/bin
                cat > $out/bin/gate-gui << 'EOF'
            #!/bin/bash
            exec "$out/Applications/Gate.app/Contents/MacOS/gate-gui" "$@"
            EOF
                chmod +x $out/bin/gate-gui
              fi

              # Install DMG if built (for distribution)
              if [[ -f target/release/bundle/dmg/*.dmg ]]; then
                mkdir -p $out/share
                cp target/release/bundle/dmg/*.dmg $out/share/
              fi
            fi
          '';

          # Allow warnings during build
          RUSTFLAGS = "-A warnings";
        };

        default = self.packages.${system}.gate;
      };

      # Apps for nix run
      apps = {
        default = {
          type = "app";
          program = "${self.packages.${system}.gate}/bin/gate-with-browser";
        };
        gate = self.apps.${system}.default;
        gate-gui = {
          type = "app";
          program = "${self.packages.${system}.gate-gui}/bin/gate-gui";
        };
      };
    })
    // {
      # Overlay for adding gate packages to nixpkgs
      overlays.default = import ./nix/overlay.nix self;

      # NixOS modules
      nixosModules = {
        default = import ./nix/modules/default.nix;
        tlsforward = import ./nix/modules/tlsforward.nix;
        tlsforward-colmena = import ./nix/modules/tlsforward-colmena.nix;
        daemon = import ./nix/modules/daemon.nix;
      };
    };
}
