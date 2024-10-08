# This flake was initially generated by fh, the CLI for FlakeHub (version 0.1.12)
{
  # Flake inputs
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    naersk.url = "github:nix-community/naersk";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  # Flake outputs that other flakes can use
  outputs = {
    self,
    nixpkgs,
    naersk,
    rust-overlay,
  }: let
    supportedSystems = ["x86_64-linux" "aarch64-linux"];
    forEachSupportedSystem = f:
      nixpkgs.lib.genAttrs supportedSystems (system:
        f rec {
          pkgs = import nixpkgs {
            inherit system;
            overlays = [rust-overlay.overlays.default];
          };
          naersk' = pkgs.callPackage naersk {};
        });
  in {
    packages = forEachSupportedSystem ({
      pkgs,
      naersk',
    }: {
      default = naersk'.buildPackage {
        src = ./.;
        nativeBuildInputs = with pkgs; [
          perl
          jq
          openconnect
          libsoup
          webkitgtk
          pkg-config
        ];

        overrideMain = {...}: {
          postPatch  = ''
            substituteInPlace crates/common/src/vpn_utils.rs \
              --replace-fail /etc/vpnc/vpnc-script ${pkgs.vpnc-scripts}/bin/vpnc-script
            substituteInPlace crates/gpapi/src/lib.rs \
              --replace-fail /usr/bin/gpclient $out/bin/gpclient \
              --replace-fail /usr/bin/gpservice $out/bin/gpservice \
              --replace-fail /usr/bin/gpgui-helper $out/bin/gpgui-helper \
              --replace-fail /usr/bin/gpgui $out/bin/gpgui \
              --replace-fail /usr/bin/gpauth $out/bin/gpauth
          '';
        };

      };
    });
  };
}
