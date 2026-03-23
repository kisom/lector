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

          # Fonts
          fontconfig
          freetype
        ];

        libPath = pkgs.lib.makeLibraryPath runtimeDeps;
      in
      {
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
