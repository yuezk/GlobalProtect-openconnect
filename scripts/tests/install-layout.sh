#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$root"
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT HUP INT TERM
mkdir -p "$temp/release"
for binary in gpclient gpauth gpservice gpgui gpgui-helper gp-vpnc-script-installer gp-hip-script-installer; do
    printf '#!/bin/sh\nexit 0\n' > "$temp/release/$binary"
    chmod 755 "$temp/release/$binary"
done
mkdir -p "$temp/client-release"
cp "$temp/release/gpclient" "$temp/client-release/"
make install-gpclient-unix PREFIX='/custom/gp install' DESTDIR="$temp/client-stage" \
    CARGO_RELEASE_DIR="$temp/client-release" > "$temp/client.log"
installed="$temp/client-stage/custom/gp install"
test -x "$installed/bin/gpclient"
test -x "$installed/libexec/gpclient/vpnc-script"
test "$(find "$installed" -type f | wc -l | tr -d ' ')" = 2
for platform in linux bsd; do
    stage="$temp/$platform-stage"
    make "install-$platform" PREFIX=/custom/gp DESTDIR="$stage" \
        CARGO_RELEASE_DIR="$temp/release" GPGUI_BINARY="$temp/release/gpgui" > "$temp/$platform.log"
    installed="$stage/custom/gp"
    test -x "$installed/bin/gpclient"
    test -x "$installed/libexec/gpclient/gp-vpnc-script-installer"
    grep -Fxq 'Exec=/custom/gp/bin/gpclient launch-gui %u' "$installed/share/applications/gpgui.desktop"
    grep -q '>/custom/gp/bin/gpservice<' "$installed/share/polkit-1/actions/com.yuezk.gpgui.policy"
    grep -q '>/custom/gp/libexec/gpclient/gp-hip-script-installer<' "$installed/share/polkit-1/actions/com.yuezk.gpgui.policy"
    ! grep -F "$stage" "$installed/share/applications/gpgui.desktop"
    if [ "$platform" = bsd ]; then
        test ! -d "$installed/lib/NetworkManager"
    else
        test -x "$installed/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down"
    fi
    runtime="$stage/var/lib/gpclient"
    if [ "$platform" = bsd ]; then
        runtime="$stage/var/db/gpclient"
        mkdir -p "$stage/usr/local/libexec/gpclient"
        cp "$temp/release/gpgui" "$stage/usr/local/libexec/gpclient/gpgui"
    else
        mkdir -p "$runtime"
        cp "$temp/release/gpgui" "$runtime/gpgui"
    fi
    mkdir -p "$runtime/scripts"
    touch "$runtime/scripts/vpnc-script" "$runtime/scripts/vpnc-script.metadata"
    find "$stage" -type f > "$temp/$platform-installed-files"
    touch "$installed/bin/unrelated" "$installed/libexec/gpclient/unrelated" "$runtime/scripts/unrelated"
    # Repeat removal to cover an already-uninstalled or partial installation.
    for attempt in 1 2; do
        make "uninstall-$platform" PREFIX=/custom/gp DESTDIR="$stage" > "$temp/$platform-uninstall.log"
    done
    while IFS= read -r file; do test ! -e "$file"; done < "$temp/$platform-installed-files"
    test -f "$installed/bin/unrelated"
    test -f "$installed/libexec/gpclient/unrelated"
    test -f "$runtime/scripts/unrelated"
done
mv "$temp/release/gpclient" "$temp/release/gpclient.saved"
if make install-linux PREFIX=/custom/gp DESTDIR="$temp/missing-client-stage" \
    CARGO_RELEASE_DIR="$temp/release" > "$temp/missing-client.log" 2>&1; then
    echo 'FAIL: installation succeeded without gpclient' >&2
    exit 1
fi
test ! -e "$temp/missing-client-stage/custom/gp/bin/gpauth"
mv "$temp/release/gpclient.saved" "$temp/release/gpclient"
make install-linux PREFIX='/custom/gp install' DESTDIR="$temp/spaces-stage" \
    CARGO_RELEASE_DIR="$temp/release" GPGUI_BINARY="$temp/release/gpgui" > "$temp/spaces-install.log"
find "$temp/spaces-stage" -type f > "$temp/spaces-installed-files"
make uninstall-linux PREFIX='/custom/gp install' DESTDIR="$temp/spaces-stage" > "$temp/spaces-uninstall.log"
while IFS= read -r file; do test ! -e "$file"; done < "$temp/spaces-installed-files"
# Execute generated commands against a harmless CLI fixture at a spaced prefix.
integration="$temp/integration prefix"
mkdir -p "$integration/bin" "$integration/share/applications" "$integration/lib/NetworkManager/dispatcher.d/pre-down.d"
printf '#!/bin/sh\nprintf "%%s\\n" "$@"\n' > "$integration/bin/gpclient"
chmod 755 "$integration/bin/gpclient"
cp packaging/files/usr/share/applications/gpgui.desktop "$integration/share/applications/"
cp packaging/files/usr/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down "$integration/lib/NetworkManager/dispatcher.d/pre-down.d/"
sh scripts/relocate-install-paths.sh "$integration" /usr "$integration"
desktop_command=$(sed -n 's/^Exec=//p' "$integration/share/applications/gpgui.desktop")
test "$desktop_command" = "\"$integration/bin/gpclient\" launch-gui %u"
test "$(sh -c "$desktop_command")" = "$(printf 'launch-gui\n%%u')"
test "$(sh "$integration/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down")" = "$(printf 'disconnect\n--wait\n3')"
mkdir -p "$temp/tools"
for os in FreeBSD OpenBSD; do
    printf '#!/bin/sh\ncase "$1" in -s) echo %s ;; -m) echo amd64 ;; esac\n' "$os" > "$temp/tools/uname"
    chmod 755 "$temp/tools/uname"
    PATH="$temp/tools:$PATH" make -n install PREFIX=/custom/gp \
        CARGO_RELEASE_DIR="$temp/release" BUILD_GUI_HELPER=0 GPGUI_BINARY= > "$temp/$os.log"
    grep -q 'Installing BSD integration' "$temp/$os.log"
    ! grep -q 'Installing Linux integration' "$temp/$os.log"
    PATH="$temp/tools:$PATH" make -n uninstall > "$temp/$os-uninstall.log"
    grep -q '/usr/local/bin/' "$temp/$os-uninstall.log"
    grep -q '/var/db/gpclient/' "$temp/$os-uninstall.log"
    ! grep -q 'NetworkManager' "$temp/$os-uninstall.log"
done
for platform in Linux Darwin-arm64 Darwin-x86_64; do
    os=Darwin
    arch=${platform#Darwin-}
    prefix=/usr/local
    case "$platform" in
        Linux) os=Linux; arch=x86_64; prefix=/usr ;;
        Darwin-arm64) prefix=/opt/homebrew ;;
    esac
    printf '#!/bin/sh\ncase "$1" in -s) echo %s ;; -m) echo %s ;; esac\n' "$os" "$arch" > "$temp/tools/uname"
    PATH="$temp/tools:$PATH" make -n uninstall > "$temp/$platform-uninstall.log"
    grep -q "$prefix/bin/" "$temp/$platform-uninstall.log"
    if [ "$os" = Darwin ]; then
        grep -q "$prefix/lib/gpclient/" "$temp/$platform-uninstall.log"
        ! grep -q 'NetworkManager\|/var/lib/gpclient' "$temp/$platform-uninstall.log"
    else
        grep -q '/var/lib/gpclient/' "$temp/$platform-uninstall.log"
    fi
done
if [ "$(uname -s)" = Darwin ]; then
    mkdir -p "$temp/build-library" "$temp/unreferenced-library" "$temp/App/Contents/Helpers"
    printf 'int fixture(void) { return 0; }\n' > "$temp/lib.c"
    printf 'extern int fixture(void); int main(void) { return fixture(); }\n' > "$temp/main.c"
    library="$temp/build-library/libopenconnect.5.dylib"
    cc -dynamiclib "$temp/lib.c" -Wl,-install_name,@rpath/libopenconnect.5.dylib -o "$library"
    printf 'int main(void) { return 0; }\n' > "$temp/no-library.c"
    cc "$temp/no-library.c" -o "$temp/release/gpclient"
    if make install-macos PREFIX=/custom/gp DESTDIR="$temp/no-library-stage" \
        CARGO_RELEASE_DIR="$temp/release" > "$temp/no-library.log" 2>&1; then
        echo 'FAIL: macOS installation succeeded without an OpenConnect dependency' >&2
        exit 1
    fi
    test ! -e "$temp/no-library-stage/custom/gp/libexec/gpclient/vpnc-script"
    cc "$temp/main.c" "$library" -Wl,-rpath,@executable_path/../lib/gpclient \
        -Wl,-rpath,"$temp/build-library" -o "$temp/release/gpclient"
    printf 'stale library must never be selected\n' > "$temp/unreferenced-library/libopenconnect.5.dylib"
    test "$(sh scripts/macos-openconnect-library.sh "$temp/release/gpclient")" = "$library"
    cp "$temp/release/gpclient" "$temp/client-release/"
    make install-gpclient PREFIX='/custom/gp install' DESTDIR="$temp/macos-client-stage" \
        CARGO_RELEASE_DIR="$temp/client-release" > "$temp/macos-client.log"
    installed="$temp/macos-client-stage/custom/gp install"
    test -x "$installed/bin/gpclient"
    test -x "$installed/libexec/gpclient/vpnc-script"
    test -f "$installed/lib/gpclient/libopenconnect.5.dylib"
    test "$(find "$installed" -type f | wc -l | tr -d ' ')" = 3
    installed="$temp/macos-stage/custom/gp"
    mkdir -p "$installed/lib" "$temp/homebrew-cellar"
    printf 'Homebrew-owned library\n' > "$temp/homebrew-cellar/libopenconnect.5.dylib"
    ln -s "$temp/homebrew-cellar/libopenconnect.5.dylib" "$installed/lib/libopenconnect.5.dylib"
    make install-macos PREFIX=/custom/gp DESTDIR="$temp/macos-stage" \
        CARGO_RELEASE_DIR="$temp/release" > "$temp/macos.log"
    test -f "$installed/lib/gpclient/libopenconnect.5.dylib"
    test -L "$installed/lib/libopenconnect.5.dylib"
    test "$(cat "$temp/homebrew-cellar/libopenconnect.5.dylib")" = 'Homebrew-owned library'
    ! otool -l "$temp/macos-stage/custom/gp/bin/gpclient" | grep -F "$temp/build-library"
    cp "$temp/release/gpclient" "$temp/App/Contents/Helpers/gpclient"
    install_name_tool -delete_rpath "$temp/build-library" "$temp/App/Contents/Helpers/gpclient"
    dylibbundler --overwrite-dir --bundle-deps --search-path "$temp/build-library" \
        --install-path @executable_path/../Frameworks --dest-dir "$temp/App/Contents/Frameworks" \
        --fix-file "$temp/App/Contents/Helpers/gpclient" > "$temp/bundle.log"
    codesign --force --sign - "$temp/App/Contents/Frameworks/libopenconnect.5.dylib"
    codesign --force --sign - "$temp/App/Contents/Helpers/gpclient"
    mv "$temp/build-library" "$temp/removed-build-library"
    "$temp/macos-stage/custom/gp/bin/gpclient"
    "$temp/macos-client-stage/custom/gp install/bin/gpclient"
    "$temp/App/Contents/Helpers/gpclient"
    touch "$installed/lib/gpclient/unrelated" "$installed/libexec/gpclient/unrelated"
    for attempt in 1 2; do
        make uninstall PREFIX=/custom/gp DESTDIR="$temp/macos-stage" > "$temp/macos-uninstall.log"
    done
    test ! -e "$installed/bin/gpclient"
    test ! -e "$installed/bin/gpauth"
    test ! -e "$installed/lib/gpclient/libopenconnect.5.dylib"
    test ! -e "$installed/libexec/gpclient/vpnc-script"
    test -f "$installed/lib/gpclient/unrelated"
    test -f "$installed/libexec/gpclient/unrelated"
    test -L "$installed/lib/libopenconnect.5.dylib"
    test "$(cat "$temp/homebrew-cellar/libopenconnect.5.dylib")" = 'Homebrew-owned library'
fi
echo 'PASS: install/uninstall paths, unrelated-file preservation, and native macOS library relocation'
