{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      flake-utils,
      naersk,
      nixpkgs,
      rust-overlay,
    }:
    let
      mkPackageSet =
        pkgs:
      let
        inherit (pkgs) lib;

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        pname = "globalprotect-openconnect";
        version = cargoToml.workspace.package.version;
        releaseTag = "snapshot";
        releaseVersion = "2.6.5";

        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        naersk' = pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };

        unpackReleaseAsset =
          { name, archive }:
          pkgs.runCommand name { nativeBuildInputs = [ pkgs.libarchive ]; } ''
            mkdir -p "$out"
            bsdtar --extract --file ${archive} --directory "$out" --strip-components 1
          '';

        src = lib.cleanSourceWith {
          src = lib.cleanSource ./.;
          filter = path: type:
            !(builtins.elem (builtins.baseNameOf path) [ "target" ".build" "node_modules" ]);
        };

        cpu = pkgs.stdenv.hostPlatform.parsed.cpu.name;

        gpguiHashes = {
          x86_64 = "sha256-8SUXA1GSmgocpjRuIsQsfjrJwjz/xdNbogElwUcQ9oI=";
          aarch64 = "sha256-GEUZP8YCqPVs3daVbrgYxW1KF6oq3Tum4xO/RF/waKA=";
        };

        gpguiArchive = pkgs.fetchurl {
          url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/${releaseTag}/gpgui_${cpu}.bin.tar.xz";
          hash = gpguiHashes.${cpu};
        };

        gpgui = unpackReleaseAsset {
          name = "gpgui-${releaseVersion}-${cpu}";
          archive = gpguiArchive;
        };

        binaryHashes = {
          x86_64 = "sha256-Oa89LHss3bGSaCZyK6pJvSzVBAJVA6+qWOlrVkijh6Q=";
          aarch64 = "sha256-BaRPtWgK80xcgsOexDka96Po71tkaVJsHB81HyYO6UA=";
        };

        binaryArchive = pkgs.fetchurl {
          url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/${releaseTag}/globalprotect-openconnect_${releaseVersion}_${cpu}.bin.tar.xz";
          hash = binaryHashes.${cpu};
        };

        binaryPackage = unpackReleaseAsset {
          name = "globalprotect-openconnect-${releaseVersion}-${cpu}-binary";
          archive = binaryArchive;
        };

        linuxRuntimeDependencies = with pkgs; [
          glib-networking
          libayatana-appindicator
          xdg-utils
        ];

        runtimeTools = with pkgs; [ coreutils iproute2 systemd procps util-linux nettools xdg-utils ];

        nativeRuntimeWrapping = lib.optionalString pkgs.stdenv.isLinux ''
          gappsWrapperArgs+=(
            --prefix PATH : "/run/wrappers/bin:${lib.makeBinPath runtimeTools}"
            --set GP_COMMAND_PATH "${lib.makeBinPath runtimeTools}"
            --set-default GP_VPNC_SCRIPT "$out/libexec/gpclient/vpnc-script"
            --set-default GP_CLIENT_BINARY "$out/bin/gpclient"
            --set-default GP_SERVICE_BINARY "$out/bin/gpservice"
            --set-default GP_AUTH_BINARY "$out/bin/gpauth"
            --set-default GP_GUI_BINARY "$out/bin/gpgui"
            --set-default GP_GUI_HELPER_BINARY "$out/bin/gpgui-helper"
            --set-default GP_VPNC_SCRIPT_INSTALLER_BINARY "$out/libexec/gpclient/gp-vpnc-script-installer"
            --set-default GP_HIP_SCRIPT_INSTALLER_BINARY "$out/libexec/gpclient/gp-hip-script-installer"
          )
        '';

        rewriteVpncScriptToolPaths = lib.optionalString pkgs.stdenv.isLinux ''
          substituteInPlace $out/libexec/gpclient/vpnc-script \
            --replace-fail /usr/bin/resolvectl ${pkgs.systemd}/bin/resolvectl \
            --replace-fail /usr/bin/busctl ${pkgs.systemd}/bin/busctl
        '';

        linuxBuildInputs =
          with pkgs;
          [
            libxml2
            zlib
            lz4
            gnutls
            p11-kit
            nettle
            gmp
          ]
          ++ lib.optionals stdenv.isLinux [
            glib
            gtk3
            libsoup_3
            webkitgtk_4_1
            glib-networking
            openssl
            libsecret
            libayatana-appindicator
          ];

        rewriteSourceInstallPaths = lib.optionalString pkgs.stdenv.isLinux ''
          substituteInPlace $out/share/applications/gpgui.desktop \
            --replace-fail /usr/bin/gpclient $out/bin/gpclient

          substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
            --replace-fail /usr/bin/gpservice $out/bin/gpservice \
            --replace-fail /usr/libexec/gpclient/gp-vpnc-script-installer $out/libexec/gpclient/gp-vpnc-script-installer

          if [ -f $out/libexec/gpclient/gp-hip-script-installer ]; then
            substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
              --replace-fail /usr/libexec/gpclient/gp-hip-script-installer $out/libexec/gpclient/gp-hip-script-installer
          fi

          if [ -f $out/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down ]; then
            substituteInPlace $out/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down \
              --replace-fail /usr/bin/gpclient $out/bin/gpclient
          fi
        '';

        installNixosPolkitRule = ''
          chmod u+w $out/share $out/share/polkit-1 2>/dev/null || true
          install -d $out/share/polkit-1/rules.d
          cat > $out/share/polkit-1/rules.d/49-gpgui.rules <<EOF
          polkit.addRule(function(action, subject) {
            if (
              action.id == "org.freedesktop.policykit.exec" &&
              action.lookup("program") == "$out/bin/gpservice" &&
              subject.active
            ) {
              return polkit.Result.YES;
            }
          });
          EOF
        '';

        fromSource = lib.makeOverridable ({ gui ? gpgui }: naersk'.buildPackage {
          inherit pname version;
          src = assert lib.assertMsg
            (builtins.pathExists (src + "/crates/openconnect/deps/openconnect/configure.ac")
              && builtins.pathExists (src + "/crates/openconnect/deps/libxml2/configure.ac"))
            "Source builds require initialized Git submodules; use a git+ flake URL with submodules=1.";
            src;
          name = "globalprotect-openconnect";

          # Must be set to true to avoid issues with the Tauri build process
          singleStep = true;

          cargoBuildOptions =
            old:
            old
            ++ lib.optionals pkgs.stdenv.isDarwin [
              "-p"
              "gpclient"
              "-p"
              "gpauth"
            ];

          buildInputs = linuxBuildInputs;

          nativeBuildInputs =
            with pkgs;
            [
              autoconf
              automake
              libtool
              pkg-config
            ]
            ++ lib.optionals stdenv.isLinux [
              autoPatchelfHook
              wrapGAppsHook4
            ];

          runtimeDependencies = lib.optionals pkgs.stdenv.isLinux linuxRuntimeDependencies;

          preFixup = nativeRuntimeWrapping;

          overrideMain =
            { ... }:
            {
              postPatch = ''
                substituteInPlace crates/openconnect/src/vpn_utils.rs \
                  --replace-fail /usr/libexec/gpclient/vpnc-script $out/libexec/gpclient/vpnc-script

                substituteInPlace crates/common/src/constants.rs \
                  --replace-fail /usr/bin/gpclient $out/bin/gpclient \
                  --replace-fail /usr/bin/gpauth $out/bin/gpauth \
                  --replace-fail /opt/homebrew/ $out/
              ''
              + lib.optionalString pkgs.stdenv.isLinux ''
                substituteInPlace crates/common/src/constants.rs \
                  --replace-fail /usr/bin/gpservice $out/bin/gpservice \
                  --replace-fail /usr/bin/gpgui-helper $out/bin/gpgui-helper \
                  --replace-fail /usr/bin/gpgui $out/bin/gpgui

                substituteInPlace crates/common/src/constants.rs \
                  --replace-fail /usr/libexec/gpclient/gp-hip-script-installer $out/libexec/gpclient/gp-hip-script-installer
              '';
            };

          postInstall = ''
            cp -r packaging/files/usr/libexec $out/libexec
          ''
          + lib.optionalString pkgs.stdenv.isLinux ''
            for installer in gp-vpnc-script-installer gp-hip-script-installer; do
              install -Dm755 "$out/bin/$installer" "$out/libexec/gpclient/$installer"
            done

            # Copy the prebuilt gpgui binary to the output bin directory
            cp ${gui}/gpgui $out/bin/gpgui
            chmod +x $out/bin/gpgui

            cp -r packaging/files/usr/share $out/share
            cp -r packaging/files/usr/lib $out/lib

            ${installNixosPolkitRule}
          ''
          + ''
            ${rewriteVpncScriptToolPaths}
            ${rewriteSourceInstallPaths}
          '';
        }) {};

        prebuilt = lib.makeOverridable ({ binaries ? binaryPackage, gui ? gpgui }: pkgs.stdenv.mkDerivation {
          inherit pname;
          version = releaseVersion;

          src = binaries;
          dontBuild = true;

          nativeBuildInputs = with pkgs; [
            autoPatchelfHook
            wrapGAppsHook4
          ];

          buildInputs = linuxBuildInputs;
          runtimeDependencies = linuxRuntimeDependencies;
          preFixup = nativeRuntimeWrapping;

          installPhase = ''
            runHook preInstall

            mkdir -p $out
            cp -r artifacts/usr/bin $out/bin
            cp -r artifacts/usr/libexec $out/libexec
            cp -r artifacts/usr/share $out/share

            if [ -d artifacts/usr/lib ]; then
              cp -r artifacts/usr/lib $out/lib
            fi

            install -Dm755 ${gui}/gpgui $out/bin/gpgui

            ${rewriteVpncScriptToolPaths}
            ${rewriteSourceInstallPaths}
            ${installNixosPolkitRule}

            runHook postInstall
          '';
        }) {};

      in
      {
        inherit fromSource prebuilt;
      };

      nixosModule =
        {
          config,
          lib,
          options,
          pkgs,
          ...
        }:
        let
          cfg = config.programs.globalprotect-openconnect;
        in
        {
          options.programs.globalprotect-openconnect = {
            enable = lib.mkEnableOption "GlobalProtect-openconnect";

            package = lib.mkOption {
              type = lib.types.package;
              # Use the NixOS module's package set so WebKit and graphics
              # dependencies match the rest of the system.
              default = (mkPackageSet pkgs).prebuilt;
              description = "GlobalProtect-openconnect package to install.";
            };
          };

          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];
            services.ayatana-indicators.enable = lib.mkDefault true;
            security.polkit = {
              enable = true;
            } // lib.optionalAttrs (options.security.polkit ? enablePkexecWrapper) {
              enablePkexecWrapper = true;
            };
          };
        };
    in
    (flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        inherit (pkgs) lib;
        inherit (mkPackageSet pkgs) fromSource prebuilt;
      in
      {
        # For `nix build`
        packages = {
          fromSource = fromSource;
        }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          default = prebuilt;
          prebuilt = prebuilt;
        }
        // lib.optionalAttrs (!pkgs.stdenv.isLinux) {
          default = fromSource;
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/gpclient";
        };

        # For `nix develop`: not fully set up yet
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
          ];
        };
      }
    ))
    // {
      nixosModules.default = nixosModule;
    };
}
