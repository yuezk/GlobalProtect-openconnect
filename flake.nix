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
        pname = cargoToml.workspace.package.name;
        version = cargoToml.workspace.package.version;

        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        naersk' = pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };

      in
      {
        # For `nix build`
        packages.default = naersk'.buildPackage {
          inherit pname version;

          src = ./.;

          buildInputs = with pkgs; [
            openconnect
          ];

          # Must be set to true to avoid issues with the Tauri build process
          singleStep = true;

          overrideMain =
            { ... }:
            {
              postPatch = ''
                substituteInPlace crates/common/src/vpn_utils.rs \
                  --replace-fail /etc/vpnc/vpnc-script ${pkgs.vpnc-scripts}/bin/vpnc-script \
                  --replace-fail /usr/lib/openconnect/hipreport.sh ${pkgs.openconnect}/libexec/openconnect/hipreport.sh

                substituteInPlace crates/gpapi/src/lib.rs \
                  --replace-fail /usr/bin/gpclient $out/bin/gpclient \
                  --replace-fail /usr/bin/gpservice $out/bin/gpservice \
                  --replace-fail /usr/bin/gpgui-helper $out/bin/gpgui-helper \
                  --replace-fail /usr/bin/gpgui $out/bin/gpgui \
                  --replace-fail /usr/bin/gpauth $out/bin/gpauth
              '';
            };
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
