{
  description = "Lumen desktop shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        rec {
          lumen = pkgs.callPackage ./packaging/nix/package.nix {
            src = self;
          };
          default = lumen;
        });

      checks = forAllSystems (system: {
        lumen = self.packages.${system}.lumen;
      });

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              rustPlatform.bindgenHook
            ];

            packages = with pkgs; [
              appstream
              cargo
              clang
              cmake
              desktop-file-utils
              fftw
              flatpak
              flatpak-builder
              gtk4
              gtk4-layer-shell
              gtksourceview5
              libpulseaudio
              libxkbcommon
              pipewire
              pkg-config
              podman
              rpm
              rustc
              rustfmt
              udev
            ];
          };
        });
    };
}
