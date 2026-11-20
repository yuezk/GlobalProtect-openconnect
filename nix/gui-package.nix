{ pkgs, src, binaryName, version }:
let
  nativeShell = binaryName == "gpgui-nsh";
in
assert builtins.elem binaryName [ "gpgui" "gpgui-nsh" ];
assert pkgs.lib.assertMsg (!nativeShell || pkgs.lib.versionAtLeast pkgs.gtk4.version "4.14")
  "Native Shell requires GTK 4.14 or later.";
pkgs.stdenv.mkDerivation {
  pname = binaryName;
  inherit src version;
  dontBuild = true;
  nativeBuildInputs = [ pkgs.autoPatchelfHook ] ++ (
    if nativeShell then [ pkgs.wrapGAppsHook4 ] else [ pkgs.wrapGAppsHook3 ]
  );
  buildInputs = with pkgs;
    [ gnutls ] ++ (if nativeShell then [ gtk4 dbus ] else [
      gtk3 webkitgtk_4_1 libsoup_3 glib-networking libsecret
    ]);
  runtimeDependencies = [ pkgs.libglvnd ] ++ pkgs.lib.optionals (!nativeShell) [
    pkgs.glib-networking
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
