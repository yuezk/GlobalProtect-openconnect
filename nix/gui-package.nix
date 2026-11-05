{ pkgs, archive, checksum, binaryName, version }:
let
  nativeShell = binaryName == "gpgui-nsh";
in
assert builtins.elem binaryName [ "gpgui" "gpgui-nsh" ];
pkgs.stdenv.mkDerivation {
  pname = binaryName;
  inherit version;
  src = builtins.path {
    path = builtins.toPath archive;
    name = "${binaryName}.tar.xz";
    recursive = false;
    sha256 = checksum;
  };
  dontBuild = true;
  nativeBuildInputs = [ pkgs.autoPatchelfHook ] ++ (
    if nativeShell then [ pkgs.wrapGAppsHook4 ] else [ pkgs.wrapGAppsHook3 ]
  );
  buildInputs = with pkgs;
    [ gnutls ] ++ (if nativeShell then [ gtk4 ] else [
      gtk3 webkitgtk_4_1 libsoup_3 glib-networking libsecret libayatana-appindicator
    ]);
  runtimeDependencies = [ pkgs.libglvnd ] ++ pkgs.lib.optionals (!nativeShell) [
    pkgs.glib-networking pkgs.libayatana-appindicator
  ];
  installPhase = ''
    runHook preInstall
    install -Dm755 ${binaryName} "$out/bin/${binaryName}"
    if [ -f ICU4X-LICENSE.txt ]; then
      install -Dm644 ICU4X-LICENSE.txt "$out/share/licenses/${binaryName}/ICU4X-LICENSE.txt"
    fi
    runHook postInstall
  '';
}
