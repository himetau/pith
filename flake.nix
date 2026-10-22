{
  description = "pith, a desktop for pi";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      each = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # gpui draws the ui, no webview. gpui dlopens vulkan, wayland and xkbcommon at runtime,
      # so they go on the rpath. on macos the metal shaders compile at startup, a nix build has no metal compiler
      mkPith = pkgs: with pkgs; let
        path = lib.makeBinPath [ pi-coding-agent git ];
        runtimeLibs = [ vulkan-loader wayland libxkbcommon xorg.libX11 xorg.libxcb xorg.libXcursor xorg.libXi libGL ];
      in rustPlatform.buildRustPackage {
        pname = "pith";
        version = (lib.importTOML ./native/Cargo.toml).package.version;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./ext ./prompts ./packaging/pith.desktop ./packaging/icon.png
            (lib.fileset.difference ./native (lib.fileset.maybeMissing ./native/target))
          ];
        };

        cargoLock.lockFile = ./native/Cargo.lock;
        cargoRoot = "native";
        buildAndTestSubdir = "native";
        buildFeatures = lib.optionals stdenv.hostPlatform.isDarwin [ "runtime-shaders" ];
        doCheck = false;

        nativeBuildInputs = [ pkg-config makeBinaryWrapper ] ++ lib.optionals stdenv.hostPlatform.isLinux [ autoPatchelfHook wrapGAppsHook3 ];
        # webkitgtk renders the browser and preview pages offscreen (core/web.rs), it isn't in the window
        buildInputs = lib.optionals stdenv.hostPlatform.isLinux ([ fontconfig freetype stdenv.cc.cc.lib webkitgtk_4_1 gtk3 libsoup_3 glib-networking ] ++ runtimeLibs)
          ++ lib.optionals stdenv.hostPlatform.isDarwin [ apple-sdk_15 ];
        runtimeDependencies = lib.optionals stdenv.hostPlatform.isLinux runtimeLibs;

        # ext/ and prompts/ go in lib/pith beside bin/, which pith checks for
        postInstall = ''
          mkdir -p $out/lib/pith
          cp -r ext prompts $out/lib/pith/
        '' + lib.optionalString stdenv.hostPlatform.isDarwin ''
          wrapProgram $out/bin/pith --suffix PATH : ${path}
        '' + lib.optionalString stdenv.hostPlatform.isLinux ''
          install -Dm644 packaging/pith.desktop $out/share/applications/pith.desktop
          install -Dm644 packaging/icon.png $out/share/icons/hicolor/512x512/apps/pith.png
        '';
        preFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
          gappsWrapperArgs+=(--suffix PATH : ${path})
        '';

        meta = {
          description = "A desktop for pi: sessions, agents, changes, a terminal and your usage, drawn natively";
          homepage = "https://github.com/himetau/pith";
          platforms = systems;
          mainProgram = "pith";
        };
      };
    in {
      packages = each (pkgs: rec { pith = mkPith pkgs; default = pith; });
      devShells = each (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.pith ];
          packages = [ pkgs.pi-coding-agent ];
        };
      });
    };
}
