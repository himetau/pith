{
  description = "pith, a desktop for pi";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      each = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # the agent is pi_agent_rust, the native port of pi, from its release builds (scripts/fetch-pi.sh pins the same one)
      piVersion = "0.8.0";
      piAssets = {
        x86_64-linux = { name = "pi-linux-amd64"; hash = "sha256-wE5/hzkb7LF8sex1RND9dnIx37WoNhZE6ohwuj05BW0="; };
        aarch64-linux = { name = "pi-linux-arm64"; hash = "sha256-MnZQTRlVTXqoryJwNnQYa5iHxIz/wc0SOLe/UjEAq/k="; };
        aarch64-darwin = { name = "pi-darwin-arm64"; hash = "sha256-mZnuWwUoOnmydxYvc535/zay/w+Nymzx1sxOGq5dBdk="; };
      };
      mkPi = pkgs: let a = piAssets.${pkgs.stdenv.hostPlatform.system}; in pkgs.stdenvNoCC.mkDerivation {
        pname = "pi-agent-rust";
        version = piVersion;
        src = pkgs.fetchurl { url = "https://github.com/Dicklesworthstone/pi_agent_rust/releases/download/v${piVersion}/${a.name}.tar.xz"; inherit (a) hash; };
        sourceRoot = ".";
        nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.autoPatchelfHook ];
        installPhase = "install -Dm755 pi $out/bin/pi";
      };

      # gpui draws the ui, no webview. gpui dlopens vulkan, wayland and xkbcommon at runtime,
      # so they go on the rpath. on macos the metal shaders compile at startup, a nix build has no metal compiler
      mkPith = pkgs: with pkgs; let
        pi = mkPi pkgs;
        path = lib.makeBinPath [ git ];
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

        # ext/, prompts/ and pi go in lib/pith beside bin/, where pith looks for them
        postInstall = ''
          mkdir -p $out/lib/pith
          cp -r ext prompts $out/lib/pith/
          ln -s ${pi}/bin/pi $out/lib/pith/pi
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
      packages = each (pkgs: rec { pith = mkPith pkgs; pi = mkPi pkgs; default = pith; });
      devShells = each (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.pith ];
          packages = [ self.packages.${pkgs.stdenv.hostPlatform.system}.pi ];
        };
      });
    };
}
