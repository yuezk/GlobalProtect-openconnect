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
          x86_64 = "sha256-TdrF6c/FgNbUcVVVjflRl039o/xPVkarEI79+i8B8nE=";
          aarch64 = "sha256-vUkJbQohAnHS21lBRzIZmSmiHfn1zAaHUEBANRNddeA=";
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
          x86_64 = "sha256-zoxuLijRc9kZ1/lSXW48f51ycq6nH2GazPnlAqrd1ys=";
          aarch64 = "sha256-IMckXd6hFYF/pFLgPtJGF95qtprdpY6Z+V4/Z5AymOo=";
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
            --replace-fail /usr/libexec/gpclient/gp-vpnc-script-installer $out/bin/gp-vpnc-script-installer

          if [ -f $out/libexec/gpclient/gp-hip-script-installer ]; then
            substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
              --replace-fail /usr/libexec/gpclient/gp-hip-script-installer $out/libexec/gpclient/gp-hip-script-installer
          fi

          if [ -f $out/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down ]; then
            substituteInPlace $out/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down \
              --replace-fail /usr/bin/gpclient $out/bin/gpclient
          fi
        '';

        rewriteHostInstallPaths = ''
          substituteInPlace $out/share/applications/gpgui.desktop \
            --replace-fail /usr/bin/gpclient $out/bin/gpclient

          substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
            --replace-fail /usr/bin/gpservice $out/bin/gpservice

          for installer in gp-vpnc-script-installer gp-hip-script-installer; do
            if [ -x "${prebuiltFiles}/libexec/gpclient/$installer" ]; then
              substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
                --replace-fail "/usr/libexec/gpclient/$installer" "${prebuiltFiles}/libexec/gpclient/$installer"
            fi
          done

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

        fromSource = naersk'.buildPackage {
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

          preFixup = lib.optionalString pkgs.stdenv.isLinux ''
            gappsWrapperArgs+=(--prefix PATH : ${lib.makeBinPath [ pkgs.xdg-utils ]})
          '';

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
            if [ -f $out/bin/gp-hip-script-installer ]; then
              install -Dm755 $out/bin/gp-hip-script-installer $out/libexec/gpclient/gp-hip-script-installer
            fi

            # Copy the prebuilt gpgui binary to the output bin directory
            cp ${gpgui}/gpgui $out/bin/gpgui
            chmod +x $out/bin/gpgui

            cp -r packaging/files/usr/share $out/share
            cp -r packaging/files/usr/lib $out/lib

            ${installNixosPolkitRule}
          ''
          + ''
            ${rewriteVpncScriptToolPaths}
            ${rewriteSourceInstallPaths}
          '';
        };

        prebuiltFiles = pkgs.stdenv.mkDerivation {
          inherit pname;
          version = releaseVersion;

          src = binaryPackage;
          dontBuild = true;

          nativeBuildInputs = with pkgs; [
            autoPatchelfHook
            wrapGAppsHook4
          ];

          buildInputs = linuxBuildInputs;
          runtimeDependencies = linuxRuntimeDependencies;

          installPhase = ''
            runHook preInstall

            mkdir -p $out
            cp -r artifacts/usr/bin $out/bin
            cp -r artifacts/usr/libexec $out/libexec
            cp -r artifacts/usr/share $out/share

            if [ -d artifacts/usr/lib ]; then
              cp -r artifacts/usr/lib $out/lib
            fi

            install -Dm755 ${gpgui}/gpgui $out/bin/gpgui

            ${rewriteVpncScriptToolPaths}

            runHook postInstall
          '';
        };

        hostGuiLauncher = pkgs.writeShellScript "gpgui-host-launcher" ''
          set -eu

          if [ "''${1:-}" = "--version" ]; then
            exec ${prebuiltFiles}/bin/gpgui "$@"
          fi

          systemd_run_args=(
            --user
            --pipe
            --wait
            --quiet
            --collect
            --service-type=exec
          )

          while IFS= read -r -d "" env_entry; do
            env_name="''${env_entry%%=*}"
            case "$env_name" in
              *[!A-Za-z0-9_]* | [0-9]* | PATH | GP_VPNC_SCRIPT_INSTALLER_BINARY | GP_HIP_SCRIPT_INSTALLER_BINARY | INVOCATION_ID | JOURNAL_STREAM | LISTEN_* | NOTIFY_SOCKET | SYSTEMD_EXEC_PID)
                continue
                ;;
            esac
            systemd_run_args+=("--setenv=$env_name")
          done < <(${pkgs.coreutils}/bin/env --null)

          gui_path="/run/wrappers/bin:''${PATH:-}"
          systemd_run_args+=("--setenv=PATH=$gui_path")
          for helper in \
            GP_VPNC_SCRIPT_INSTALLER_BINARY=gp-vpnc-script-installer \
            GP_HIP_SCRIPT_INSTALLER_BINARY=gp-hip-script-installer; do
            helper_path="${prebuiltFiles}/libexec/gpclient/''${helper#*=}"
            if [ -x "$helper_path" ]; then
              systemd_run_args+=("--setenv=''${helper%%=*}=$helper_path")
            fi
          done

          exec ${pkgs.systemd}/bin/systemd-run \
            "''${systemd_run_args[@]}" \
            ${prebuiltFiles}/bin/gpgui \
            "$@"
        '';

        prebuiltCommand =
          {
            binaryName,
            extraProfile ? "",
          }:
          pkgs.buildFHSEnv {
            name = binaryName;
            targetPkgs = pkgs: [ prebuiltFiles ] ++ linuxBuildInputs ++ linuxRuntimeDependencies;
            runScript = "/usr/bin/${binaryName}";
            profile = ''
              export PATH=/run/wrappers/bin:$PATH
              for helper in \
                GP_VPNC_SCRIPT_INSTALLER_BINARY=gp-vpnc-script-installer \
                GP_HIP_SCRIPT_INSTALLER_BINARY=gp-hip-script-installer; do
                helper_path="${prebuiltFiles}/libexec/gpclient/''${helper#*=}"
                if [ -x "$helper_path" ]; then
                  export "''${helper%%=*}=$helper_path"
                fi
              done
              ${extraProfile}
            '';
            extraBwrapArgs = [
              "--bind-try"
              "/run/wrappers"
              "/run/wrappers"
              "--ro-bind-try"
              "/etc/gpgui"
              "/etc/gpgui"
            ];
          };

        prebuiltCommands = {
          gpclient = prebuiltCommand { binaryName = "gpclient"; };
          gpservice = prebuiltCommand {
            binaryName = "gpservice";
            extraProfile = ''
              export GP_GUI_BINARY='${hostGuiLauncher}'
            '';
          };
          gpauth = prebuiltCommand { binaryName = "gpauth"; };
          gpgui = prebuiltCommand { binaryName = "gpgui"; };
          gpgui-helper = prebuiltCommand { binaryName = "gpgui-helper"; };
        };

        prebuilt = pkgs.stdenv.mkDerivation {
          inherit pname;
          version = releaseVersion;

          dontUnpack = true;

          installPhase = ''
            runHook preInstall

            mkdir -p $out/bin
            cat > $out/bin/gpclient <<'EOF'
            #!${pkgs.runtimeShell}
            set -eu

            export GP_SERVICE_BINARY='@gpservice_public@'
            export GP_AUTH_BINARY='@gpauth_public@'
            export GP_GUI_BINARY='${hostGuiLauncher}'
            if [ "''${1:-}" = "launch-gui" ]; then
              # Authorization must run before entering Bubblewrap, which disables setuid elevation.
              export PATH=/run/wrappers/bin:$PATH
              exec '${prebuiltFiles}/bin/gpclient' "$@"
            fi
            exec '${prebuiltCommands.gpclient}/bin/gpclient' "$@"
            EOF
            substituteInPlace $out/bin/gpclient \
              --replace-fail '@gpservice_public@' "$out/bin/gpservice" \
              --replace-fail '@gpauth_public@' "$out/bin/gpauth"
            chmod +x $out/bin/gpclient

            cat > $out/bin/gpservice <<'EOF'
            #!${pkgs.runtimeShell}
            set -eu
            exec '@gpservice_fhs@' "$@"
            EOF
            substituteInPlace $out/bin/gpservice \
              --replace-fail '@gpservice_fhs@' '${prebuiltCommands.gpservice}/bin/gpservice'
            chmod +x $out/bin/gpservice

            ln -s ${prebuiltCommands.gpauth}/bin/gpauth $out/bin/gpauth
            ln -s ${prebuiltCommands.gpgui}/bin/gpgui $out/bin/gpgui
            ln -s ${prebuiltCommands."gpgui-helper"}/bin/gpgui-helper $out/bin/gpgui-helper

            cp -r ${prebuiltFiles}/libexec $out/libexec
            cp -r ${prebuiltFiles}/share $out/share

            if [ -d ${prebuiltFiles}/lib ]; then
              cp -r ${prebuiltFiles}/lib $out/lib
            fi

            ${rewriteHostInstallPaths}
            ${installNixosPolkitRule}

            runHook postInstall
          '';
        };
      in
      {
        inherit fromSource prebuilt;
      };

      nixosModule =
        {
          config,
          lib,
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
