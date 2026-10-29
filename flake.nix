{
  description = "STAR/FLUX native terminal and app workspace";
  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        gst = pkgs.gst_all_1;
        plugins = [ gst.gst-plugins-base gst.gst-plugins-good gst.gst-plugins-bad gst.gst-plugins-ugly gst.gst-libav ];
        native = plugins ++ (with pkgs; [ wayland libxkbcommon libdrm libgbm libGL pixman udev vulkan-loader alsa-lib ffmpeg gst.gstreamer ]);
        alsaPluginDir = pkgs.runCommand "starflux-alsa-plugins" {} ''
          mkdir -p $out
          ln -s ${pkgs.pipewire}/lib/alsa-lib/*.so $out/
          ln -s ${pkgs.alsa-plugins}/lib/alsa-lib/*.so $out/
        '';
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        package = pkgs.rustPlatform.buildRustPackage {
          pname = "starflux";
          version = cargoToml.package.version;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            # Distribution recipes do not change the Nix program. Keep its
            # derivation stable while AppImage/Arch packaging is verified.
            filter = path: type:
              let relative = pkgs.lib.removePrefix "${toString ./.}/" path;
              in pkgs.lib.cleanSourceFilter path type
                && builtins.baseNameOf path != ".github"
                && !(builtins.elem relative [ "scripts/dist" "packaging/AppRun" "packaging/arch" "docs/releases" ]);
          };
          cargoLock.lockFile = ./Cargo.lock;
          cargoLock.allowBuiltinFetchGit = true;
          nativeBuildInputs = with pkgs; [ pkg-config clang makeWrapper ];
          nativeCheckInputs = with pkgs; [ python3 tmux openssh git ];
          buildInputs = native;
          GST_PLUGIN_SYSTEM_PATH_1_0 = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/gstreamer-1.0") plugins;
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.ffmpeg.dev}/include";
          cargoBuildFlags = [ "--workspace" ];
          cargoTestFlags = [ "--workspace" "--all-features" ];
          preCheck = ''
            export XDG_DATA_HOME="$TMPDIR/starflux-check-data"
            mkdir -p "$XDG_DATA_HOME"
          '';
          postBuild = ''
            $CC -std=c11 -O2 -Wall -Wextra -Werror native/pty-proxy.c -o starflux-pty-proxy
          '';
          postInstall = ''
            install -m755 starflux-pty-proxy $out/bin/starflux-pty-proxy
            install -Dm644 LICENSE $out/share/licenses/starflux/LICENSE
            cp -R LICENSES/. $out/share/licenses/starflux/
            install -Dm644 README.md $out/share/doc/starflux/README.md
            install -Dm644 ${./packaging/starflux.desktop} $out/share/applications/starflux.desktop
            install -Dm644 ${./packaging/starflux.svg} $out/share/icons/hicolor/scalable/apps/starflux.svg
            for program in starflux starflux-terminal-client; do
              wrapProgram $out/bin/$program \
                --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.python3 pkgs.tmux pkgs.openssh pkgs.xwayland pkgs.pulseaudio pkgs.pipewire pkgs.kitty ]} \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath native} \
                --set ALSA_PLUGIN_DIR ${alsaPluginDir} \
                --set GST_PLUGIN_SYSTEM_PATH_1_0 ${pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/gstreamer-1.0") plugins}
            done
          '';
          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = cargoToml.package.repository;
            license = licenses.mit;
            platforms = platforms.linux;
            mainProgram = "starflux";
          };
        };
      in {
        packages.default = package;
        packages.starflux = package;
        checks.default = package;
        checks.smoke = pkgs.runCommand "starflux-smoke" {} ''
          ${package}/bin/starflux --version
          ${package}/bin/starflux --themes > $out
          ${package}/bin/starflux-terminal-client --help
          ${package}/bin/starflux-pty-proxy --help
        '';
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [ rustc cargo rustfmt clippy cargo-deny pkg-config python3 tmux openssh xwayland pulseaudio pipewire ];
          buildInputs = native;
          ALSA_PLUGIN_DIR = alsaPluginDir;
          GST_PLUGIN_SYSTEM_PATH_1_0 = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/gstreamer-1.0") plugins;
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.ffmpeg.dev}/include";
          shellHook = ''
            export VK_DRIVER_FILES="$(python3 -c 'import glob; print(":".join(glob.glob("${pkgs.mesa}/share/vulkan/icd.d/*.json")))')"
          '';
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath native;
          LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
          __EGL_VENDOR_LIBRARY_FILENAMES = "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
        };
      });
}
