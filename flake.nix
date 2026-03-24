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
          pango
          harfbuzz
          zlib

          # Fonts
          fontconfig
          freetype
        ];

        libPath = pkgs.lib.makeLibraryPath runtimeDeps;

        commonBuildArgs = {
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = with pkgs; [ pkg-config mold clang ];
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER = "clang";
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
        };

        # Single derivation builds both binaries to share the dependency compile
        lector-all = pkgs.rustPlatform.buildRustPackage (commonBuildArgs // {
          pname = "lector";
          version = "1.2.4";
          buildInputs = runtimeDeps;

          # Ensure shared libs are found during check phase
          LD_LIBRARY_PATH = libPath;

          postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            patchelf --set-rpath "${libPath}" $out/bin/lector
            patchelf --set-rpath "${libPath}" $out/bin/clector
          '';
        });

        lector-gui = lector-all;

        lector-tui = pkgs.rustPlatform.buildRustPackage (commonBuildArgs // {
          pname = "clector";
          version = "1.2.4";
          cargoBuildFlags = [ "-p" "lector-tui" ];
          cargoTestFlags = [ "-p" "lector-tui" "-p" "lector-core" ];
          cargoCheckFlags = [ "-p" "lector-tui" ];

          # Ensure shared libs are found during check phase
          LD_LIBRARY_PATH = libPath;

          meta.mainProgram = "clector";
        });
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
            mold
            clang
          ]);

          shellHook = ''
            export LD_LIBRARY_PATH="${libPath}:$LD_LIBRARY_PATH"
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"
          '';
        };
      }
    );
}
