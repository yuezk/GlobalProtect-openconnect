{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      flake-utils,
      naersk,
      nixpkgs,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        inherit (pkgs) lib;

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        pname = "globalprotect-openconnect";
        version = cargoToml.workspace.package.version;
        releaseTag = "v2.6.3";

        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        naersk' = pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };

        src = pkgs.fetchzip {
          url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/${releaseTag}/globalprotect-openconnect-${version}.tar.gz";
          hash = "sha256-cp7KRUwVJsyg8m7O70k/uo6WwLqCPbFCnWtXzHn1GgU=";
        };

        cpu = pkgs.stdenv.hostPlatform.parsed.cpu.name;

        gpguiHashes = {
          x86_64 = "sha256-HbZYZOr0Vei/wBNQUxZNOSEfHlJV3PP8CzhL34Ixm9I=";
          aarch64 = "sha256-aHou275EwrWqUVsGmW8f6zfVIKuhqW0m+ZdGD7hq7jM=";
        };

        gpgui = pkgs.fetchzip {
          url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/${releaseTag}/gpgui_${cpu}.bin.tar.xz";
          hash = gpguiHashes.${cpu};
        };

        binaryHashes = {
          x86_64 = "sha256-UA3GdjS69P3oYGQ9alE7T0Ro7+ZzzyY6VpIg2kgR+9Y=";
          aarch64 = "sha256-1DCpSffjm0HA42aWnpPCa0WQP7XfgKYCBzikXW5hQwA=";
        };

        binaryPackage = pkgs.fetchzip {
          url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/${releaseTag}/globalprotect-openconnect_${version}_${cpu}.bin.tar.xz";
          hash = binaryHashes.${cpu};
        };

        linuxRuntimeDependencies = with pkgs; [
          glib-networking
          libayatana-appindicator
        ];

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

        rewriteSourceInstallPaths = ''
          substituteInPlace $out/share/applications/gpgui.desktop \
            --replace-fail /usr/bin/gpclient $out/bin/gpclient

          substituteInPlace $out/libexec/gpclient/hipreport.sh \
            --replace-fail /usr/bin/gpclient $out/bin/gpclient

          substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
            --replace-fail /usr/bin/gpservice $out/bin/gpservice

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
          inherit pname version src;

          # Must be set to true to avoid issues with the Tauri build process
          singleStep = true;

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

          overrideMain =
            { ... }:
            {
              postPatch = ''
                substituteInPlace crates/openconnect/src/vpn_utils.rs \
                  --replace-fail /usr/libexec/gpclient/vpnc-script $out/libexec/gpclient/vpnc-script \
                  --replace-fail /usr/libexec/gpclient/hipreport.sh $out/libexec/gpclient/hipreport.sh

                substituteInPlace crates/common/src/constants.rs \
                  --replace-fail /usr/bin/gpclient $out/bin/gpclient \
                  --replace-fail /usr/bin/gpservice $out/bin/gpservice \
                  --replace-fail /usr/bin/gpgui-helper $out/bin/gpgui-helper \
                  --replace-fail /usr/bin/gpgui $out/bin/gpgui \
                  --replace-fail /usr/bin/gpauth $out/bin/gpauth \
                  --replace-fail /opt/homebrew/ $out/
              '';
            };

          postInstall = ''
            # Copy the prebuilt gpgui binary to the output bin directory
            cp ${gpgui}/gpgui $out/bin/gpgui
            chmod +x $out/bin/gpgui

            cp -r packaging/files/usr/share $out/share
            cp -r packaging/files/usr/lib $out/lib
            cp -r packaging/files/usr/libexec $out/libexec

            ${rewriteSourceInstallPaths}
            ${installNixosPolkitRule}
          '';
        };

        prebuiltFiles = pkgs.stdenv.mkDerivation {
          inherit pname version;

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

            runHook postInstall
          '';
        };

        prebuiltCommand =
          binaryName:
          pkgs.buildFHSEnv {
            name = binaryName;
            targetPkgs =
              pkgs:
              [ prebuiltFiles ]
              ++ linuxBuildInputs
              ++ linuxRuntimeDependencies;
            runScript = "/usr/bin/${binaryName}";
            profile = ''
              export PATH=/run/wrappers/bin:$PATH
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
          gpclient = prebuiltCommand "gpclient";
          gpservice = prebuiltCommand "gpservice";
          gpauth = prebuiltCommand "gpauth";
          gpgui = prebuiltCommand "gpgui";
          gpgui-helper = prebuiltCommand "gpgui-helper";
        };

        prebuilt = pkgs.stdenv.mkDerivation {
          inherit pname version;

          dontUnpack = true;

          installPhase = ''
            runHook preInstall

            mkdir -p $out/bin
            cat > $out/bin/gpclient <<'EOF'
            #!${pkgs.runtimeShell}
            set -eu

            gpclient_fhs='${prebuiltCommands.gpclient}/bin/gpclient'
            gpservice_public='@gpservice_public@'

            if [ "''${1:-}" = "launch-gui" ]; then
              shift

              auth_data=
              minimized=
              for arg in "$@"; do
                case "$arg" in
                  --minimized)
                    minimized=--minimized
                    ;;
                  --*)
                    ;;
                  *)
                    auth_data=$arg
                    ;;
                esac
              done

              if [ -z "$auth_data" ]; then
                if [ -n "''${XDG_DATA_HOME:-}" ]; then
                  data_home=$XDG_DATA_HOME
                elif [ -n "''${HOME:-}" ]; then
                  data_home=$HOME/.local/share
                else
                  data_home=/tmp
                fi

                log_dir="$data_home/gpclient"
                mkdir -p "$log_dir"
                log_file="$log_dir/gpclient.log"
                env_file=$(mktemp)

                env > "$env_file"
                printf 'GP_LOG_FILE=%s\n' "$log_file" >> "$env_file"

                pkexec_bin=/run/wrappers/bin/pkexec
                if [ ! -x "$pkexec_bin" ]; then
                  pkexec_bin=pkexec
                fi

                set +e
                if [ -n "$minimized" ]; then
                  "$pkexec_bin" --user root "$gpservice_public" --minimized --env-file "$env_file" 2>"$log_file"
                else
                  "$pkexec_bin" --user root "$gpservice_public" --env-file "$env_file" 2>"$log_file"
                fi
                status=$?
                set -e
                rm -f "$env_file"
                exit "$status"
              fi

              set -- launch-gui "$@"
            fi

            exec "$gpclient_fhs" "$@"
            EOF
            substituteInPlace $out/bin/gpclient \
              --replace-fail '@gpservice_public@' "$out/bin/gpservice"
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
        # For `nix build`
        packages =
          {
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
    );
}
