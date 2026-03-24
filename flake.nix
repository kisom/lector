{
  description = "Lector - a read-only document viewer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        runtimeDeps = with pkgs; [
          # Tauri / WebKitGTK (GUI)
          webkitgtk_4_1
          gtk3
          cairo
          gdk-pixbuf
          glib
          dbus
          openssl
          librsvg
          libsoup_3
          zlib

          # Fonts
          fontconfig
          freetype
        ];

        libPath = pkgs.lib.makeLibraryPath runtimeDeps;

        lector-gui = pkgs.rustPlatform.buildRustPackage {
          pname = "lector";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "lector-gui" ];

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = runtimeDeps;

          postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            patchelf --set-rpath "${libPath}" $out/bin/lector
          '';

          meta.mainProgram = "lector";
        };

        lector-tui = pkgs.rustPlatform.buildRustPackage {
          pname = "clector";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "lector-tui" ];
          cargoTestFlags = [ "-p" "lector-tui" "-p" "lector-core" ];
          cargoCheckFlags = [ "-p" "lector-tui" ];

          nativeBuildInputs = with pkgs; [ pkg-config ];

          meta.mainProgram = "clector";
        };
      in
      {
        packages = {
          default = lector-gui;
          gui = lector-gui;
          tui = lector-tui;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = runtimeDeps ++ (with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            pkg-config
          ]);

          shellHook = ''
            export LD_LIBRARY_PATH="${libPath}:$LD_LIBRARY_PATH"
          '';
        };
      }
    );
}
