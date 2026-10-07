let
  source = builtins.getEnv "GP_SOURCE";
  system = builtins.getEnv "NIX_SYSTEM";
  nixpkgsRef = builtins.getEnv "NIXPKGS";
  gp = builtins.getFlake ("git+file://" + source + "?submodules=1");
  nixpkgs = if nixpkgsRef == "locked" then gp.inputs.nixpkgs
    else builtins.getFlake ("github:NixOS/nixpkgs/" + nixpkgsRef);
  pkgs = import nixpkgs { inherit system; };
  unpack = name: variable:
    let
      archive = builtins.path {
        path = builtins.toPath (builtins.getEnv variable);
        inherit name;
      };
    in pkgs.runCommand name { nativeBuildInputs = [ pkgs.libarchive ]; } ''
      mkdir -p "$out"
      bsdtar --extract --file ${archive} --directory "$out" --strip-components 1
    '';
  binaries = unpack "gp-ci-binaries" "GP_BINARY_ARCHIVE";
  gui = unpack "gp-ci-gui" "GP_GUI_ARCHIVE";
  sourcePackage = gp.packages.${system}.fromSource.override { inherit gui; };
  prebuiltPackage = gp.packages.${system}.prebuilt.override { inherit binaries gui; };
  hostSystem = nixpkgs.lib.nixosSystem {
    inherit system;
    modules = [
      gp.nixosModules.default
      ({ options, ... }: {
        programs.globalprotect-openconnect = {
          enable = true;
          package = options.programs.globalprotect-openconnect.package.default.override {
            inherit binaries gui;
          };
        };
        system.stateVersion = "25.11";
      })
    ];
  };
  modulePackage = hostSystem.config.programs.globalprotect-openconnect.package;
in {
  source = sourcePackage;
  prebuilt = prebuiltPackage;
  module = assert builtins.elem modulePackage hostSystem.config.environment.systemPackages;
    assert hostSystem.config.services.ayatana-indicators.enable;
    modulePackage;
  runtime-source = import ./tests/native-runtime.nix { inherit pkgs; package = sourcePackage; };
  runtime-prebuilt = import ./tests/native-runtime.nix { inherit pkgs; package = prebuiltPackage; };
}
