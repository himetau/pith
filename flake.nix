{
  description = "pith, a desktop for pi";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      each = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      mkPith = pkgs: with pkgs; let
        # pith runs `pi` and git off PATH. suffixed, so a pi you installed yourself still wins
        path = lib.makeBinPath [ pi-coding-agent git ];
      in rustPlatform.buildRustPackage {
        pname = "pith";
        inherit ((lib.importJSON ./src-tauri/tauri.conf.json)) version;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./package.json ./package-lock.json ./tsconfig.json ./vite.config.ts
            ./src ./ext ./prompts ./packaging/pith.desktop
            # a local `path:` build would drag a whole target/ in otherwise
            (lib.fileset.difference ./src-tauri (lib.fileset.maybeMissing ./src-tauri/target))
          ];
        };

        # both lockfiles are read as they are, so there are no vendor hashes to bump when deps change
        cargoLock.lockFile = ./src-tauri/Cargo.lock;
        cargoRoot = "src-tauri";
        buildAndTestSubdir = "src-tauri";
        npmDeps = importNpmLock { npmRoot = ./.; };

        nativeBuildInputs = [ cargo-tauri.hook nodejs importNpmLock.npmConfigHook ]
          ++ lib.optionals stdenv.hostPlatform.isLinux [ pkg-config wrapGAppsHook3 ]
          ++ lib.optionals stdenv.hostPlatform.isDarwin [ makeBinaryWrapper ];
        buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
          glib-networking libsoup_3 webkitgtk_4_1 gst_all_1.gst-plugins-base gst_all_1.gst-plugins-good
        ];

        # linux gets the deb's layout, ext/ and prompts/ in lib/pith, which pith checks for. its own .desktop has the
        # keywords and wm class. macos gets pith.app, with them in its Resources, and a bin/pith for nix run
        postInstall = lib.optionalString stdenv.hostPlatform.isLinux ''
          install -Dm644 packaging/pith.desktop $out/share/applications/pith.desktop
        '' + lib.optionalString stdenv.hostPlatform.isDarwin ''
          makeWrapper $out/Applications/pith.app/Contents/MacOS/pith $out/bin/pith --suffix PATH : ${path}
        '';

        preFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
          gappsWrapperArgs+=(--suffix PATH : ${path})
        '';

        meta = {
          description = "A desktop for pi: sessions, agents, a browser it can drive, design previews, reviews and your usage";
          homepage = "https://github.com/himetau/pith";
          platforms = systems;
          mainProgram = "pith";
        };
      };
      # the native build: gpui draws the ui, no webview. gpui dlopens vulkan, wayland and xkbcommon at runtime,
      # so they go on the rpath. on macos the metal shaders compile at startup, a nix build has no metal compiler
      mkNative = pkgs: with pkgs; let
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

        nativeBuildInputs = [ pkg-config makeBinaryWrapper ] ++ lib.optionals stdenv.hostPlatform.isLinux [ autoPatchelfHook ];
        buildInputs = lib.optionals stdenv.hostPlatform.isLinux ([ fontconfig freetype stdenv.cc.cc.lib ] ++ runtimeLibs)
          ++ lib.optionals stdenv.hostPlatform.isDarwin [ apple-sdk_15 ];
        runtimeDependencies = lib.optionals stdenv.hostPlatform.isLinux runtimeLibs;

        # ext/ and prompts/ go in lib/pith beside bin/, which pith checks for
        postInstall = ''
          mkdir -p $out/lib/pith
          cp -r ext prompts $out/lib/pith/
          wrapProgram $out/bin/pith --suffix PATH : ${path}
        '' + lib.optionalString stdenv.hostPlatform.isLinux ''
          install -Dm644 packaging/pith.desktop $out/share/applications/pith.desktop
          install -Dm644 packaging/icon.png $out/share/icons/hicolor/512x512/apps/pith.png
        '';

        meta = {
          description = "A desktop for pi: sessions, agents, changes, a terminal and your usage, drawn natively";
          homepage = "https://github.com/himetau/pith";
          platforms = systems;
          mainProgram = "pith";
        };
      };
    in {
      packages = each (pkgs: rec { pith = mkNative pkgs; pith-tauri = mkPith pkgs; default = pith; });
      devShells = each (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.pith ];
          packages = [ pkgs.pi-coding-agent ];
        };
      });
    };
}
