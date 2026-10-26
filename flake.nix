{
  description = "STAR/MUX Linux demo development shell";
  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        native = with pkgs; [ wayland libxkbcommon libdrm libgbm libGL pixman udev vulkan-loader ];
      in {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [ rustc cargo rustfmt clippy pkg-config python3 tmux openssh xwayland ];
          buildInputs = native;
          shellHook = ''
            export VK_DRIVER_FILES="$(python3 -c 'import glob; print(":".join(glob.glob("${pkgs.mesa}/share/vulkan/icd.d/*.json")))')"
          '';
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath native;
          LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
          __EGL_VENDOR_LIBRARY_FILENAMES = "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
        };
      });
}
