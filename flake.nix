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
          # GPU / rendering (wgpu backend)
          vulkan-loader
          libxkbcommon

          # Wayland
          wayland
          wayland-protocols

          # X11 fallback
          libx11
          libxcursor
          libxi
          libxrandr
          libxcb

          # Fonts
          fontconfig
          freetype

          # System
          udev
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
