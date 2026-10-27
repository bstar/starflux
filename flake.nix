{
  description = "STAR/MUX native terminal and app workspace";
  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        gst = pkgs.gst_all_1;
        plugins = [ gst.gst-plugins-base gst.gst-plugins-good gst.gst-plugins-bad gst.gst-plugins-ugly gst.gst-libav ];
        native = plugins ++ (with pkgs; [ wayland libxkbcommon libdrm libgbm libGL pixman udev vulkan-loader alsa-lib ffmpeg gst.gstreamer ]);
        alsaPluginDir = pkgs.runCommand "starmux-alsa-plugins" {} ''
          mkdir -p $out
          ln -s ${pkgs.pipewire}/lib/alsa-lib/*.so $out/
          ln -s ${pkgs.alsa-plugins}/lib/alsa-lib/*.so $out/
        '';
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        package = pkgs.rustPlatform.buildRustPackage {
          pname = "starmux";
          version = cargoToml.package.version;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            # Workflow-only edits cannot change the shipped program.
            filter = path: type: pkgs.lib.cleanSourceFilter path type
              && builtins.baseNameOf path != ".github";
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
            export XDG_DATA_HOME="$TMPDIR/starmux-check-data"
            mkdir -p "$XDG_DATA_HOME"
          '';
          postBuild = ''
            $CC -std=c11 -O2 -Wall -Wextra -Werror native/pty-proxy.c -o starmux-pty-proxy
          '';
          postInstall = ''
            install -m755 starmux-pty-proxy $out/bin/starmux-pty-proxy
            install -Dm644 LICENSE $out/share/licenses/starmux/LICENSE
            install -Dm644 README.md $out/share/doc/starmux/README.md
            install -Dm644 ${./packaging/starmux.desktop} $out/share/applications/starmux.desktop
            install -Dm644 ${./packaging/starmux.svg} $out/share/icons/hicolor/scalable/apps/starmux.svg
            for program in starmux starmux-terminal-client; do
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
            mainProgram = "starmux";
          };
        };
      in {
        packages.default = package;
        packages.starmux = package;
        checks.default = package;
        checks.smoke = pkgs.runCommand "starmux-smoke" {} ''
          ${package}/bin/starmux --version
          ${package}/bin/starmux --themes > $out
          ${package}/bin/starmux-terminal-client --help
          ${package}/bin/starmux-pty-proxy --help
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
