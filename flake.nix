{
  description = "pith, a desktop for pi";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      # linux only, it drives gtk and webkitgtk directly
      systems = [ "x86_64-linux" "aarch64-linux" ];
      each = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      mkPith = pkgs: with pkgs; rustPlatform.buildRustPackage {
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

        nativeBuildInputs = [ cargo-tauri.hook nodejs importNpmLock.npmConfigHook pkg-config wrapGAppsHook3 ];
        buildInputs = [ glib-networking libsoup_3 webkitgtk_4_1 gst_all_1.gst-plugins-base gst_all_1.gst-plugins-good ];

        # the deb puts ext/ and prompts/ in lib/pith, which pith checks for. its own .desktop has the keywords and wm class
        postInstall = ''
          install -Dm644 packaging/pith.desktop $out/share/applications/pith.desktop
        '';

        # pith runs `pi` and git off PATH. suffixed, so a pi you installed yourself still wins
        preFixup = ''
          gappsWrapperArgs+=(--suffix PATH : ${lib.makeBinPath [ pi-coding-agent git ]})
        '';

        meta = {
          description = "A desktop for pi: sessions, agents, a browser it can drive, design previews, reviews and your usage";
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
