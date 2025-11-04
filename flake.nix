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

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        pname = "globalprotect-openconnect";
        version = cargoToml.workspace.package.version;

        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        naersk' = pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };

        cpu = pkgs.stdenv.hostPlatform.parsed.cpu.name;

        gpgui = pkgs.fetchzip {
          url = "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/v${version}/gpgui_${cpu}.bin.tar.xz";
          hash = "sha256-dy8McFjcOAvRGEc8Al9PA7LKfxJNZycSEppE4FmqT1Q=";
        };
      in
      {
        # For `nix build`
        packages.default = naersk'.buildPackage {
          inherit pname version;

          # Must be set to true to avoid issues with the Tauri build process
          singleStep = true;

          src = ./.;

          buildInputs =
            with pkgs;
            [
              openconnect
            ]
            ++ lib.optionals stdenv.isLinux [
              glib
              gtk3
              libsoup_3
              webkitgtk_4_1
            ];

          nativeBuildInputs =
            with pkgs;
            [ ]
            ++ lib.optionals stdenv.isLinux [
              autoPatchelfHook
            ];

          runtimeDependencies =
            with pkgs;
            [ ]
            ++ lib.optionals stdenv.isLinux [
              libappindicator-gtk3
            ];

          overrideMain =
            { ... }:
            {
              postPatch = ''
                substituteInPlace crates/openconnect/src/vpn_utils.rs \
                  --replace-fail /etc/vpnc/vpnc-script ${pkgs.vpnc-scripts}/bin/vpnc-script \
                  --replace-fail /usr/lib/openconnect/hipreport.sh ${pkgs.openconnect}/libexec/openconnect/hipreport.sh

                substituteInPlace crates/common/src/constants.rs \
                  --replace-fail /usr/bin/gpclient $out/bin/gpclient \
                  --replace-fail /usr/bin/gpservice $out/bin/gpservice \
                  --replace-fail /usr/bin/gpgui-helper $out/bin/gpgui-helper \
                  --replace-fail /usr/bin/gpgui $out/bin/gpgui \
                  --replace-fail /usr/bin/gpauth $out/bin/gpauth
              '';
            };

          postInstall = ''
            # Copy the prebuilt gpgui binary to the output bin directory
            cp ${gpgui}/gpgui $out/bin/gpgui
            chmod +x $out/bin/gpgui

            cp -r packaging/files/usr/share $out/share
            cp -r packaging/files/usr/lib $out/lib

            # Change the `/usr/bin/gpclient` path in the desktop file
            substituteInPlace $out/share/applications/gpgui.desktop \
              --replace-fail /usr/bin/gpclient $out/bin/gpclient

            substituteInPlace $out/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down \
              --replace-fail /usr/bin/gpclient $out/bin/gpclient

            # Change the `/usr/bin/gpservice` path in the polkit policy file
            substituteInPlace $out/share/polkit-1/actions/com.yuezk.gpgui.policy \
              --replace-fail /usr/bin/gpservice $out/bin/gpservice
          '';
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
