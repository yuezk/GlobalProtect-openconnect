#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
collector=$root/scripts/collect-diagnostics.sh
fixtures=$(mktemp -d "${TMPDIR:-/tmp}/gp-diagnostics-tests.XXXXXXXX")
trap 'rm -rf "$fixtures"' 0
trap 'exit 130' INT
trap 'exit 143' TERM HUP
mkdir "$fixtures/bin"
for tool in sh awk tr cat chmod mktemp rm ps sleep; do
    ln -s "$(command -v "$tool")" "$fixtures/bin/$tool"
done
cat > "$fixtures/bin/uname" <<'MOCK'
#!/bin/sh
case "$1" in
    -s) printf '%s\n' "$TEST_OS" ;;
    -m) printf '%s\n' fixture-architecture ;;
    -r)
        case "${TEST_PROBE:-}" in
            timeout)
                sh -c 'trap "" TERM; echo $$ > "$TEST_CHILD_PID"; while :; do sleep 1; done' &
                wait ;;
            flood) awk 'BEGIN {while (1) print "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"}' ;;
            *) printf '%s\n' fixture-kernel ;;
        esac ;;
esac
MOCK
cat > "$fixtures/bin/id" <<'MOCK'
#!/bin/sh
printf '%s\n' 1000
MOCK
cat > "$fixtures/bin/gpclient" <<'MOCK'
#!/bin/sh
[ "$1" = --version ] || exit 2
printf '%s\n' 'gpclient 2.6.5'
MOCK
cat > "$fixtures/bin/gpauth" <<'MOCK'
#!/bin/sh
[ "$1" = --version ] || exit 2
printf '%s\n' 'gpauth 2.6.5'
MOCK
cat > "$fixtures/bin/gpgui" <<'MOCK'
#!/bin/sh
printf '%s\n' 'GUI WAS LAUNCHED' > "$TEST_GUI_MARKER"
exit 1
MOCK
cat > "$fixtures/bin/lspci" <<'MOCK'
#!/bin/sh
cat <<'DATA'
00:00.0 Host bridge: OMIT-BRIDGE
00:01.0 VGA compatible controller: Fixture GPU
        Kernel driver in use: fixture-driver
00:02.0 Network controller: OMIT-NETWORK
        Kernel driver in use: OMIT-NETWORK-DRIVER
DATA
MOCK
cat > "$fixtures/bin/pciconf" <<'MOCK'
#!/bin/sh
cat <<'DATA'
vgapci0@pci0:0:1:0: class=0x030000
    device = 'BSD Fixture GPU'
nic0@pci0:0:2:0: class=0x020000
    device = 'OMIT-NETWORK'
DATA
MOCK
cat > "$fixtures/bin/pcidump" <<'MOCK'
#!/bin/sh
printf '%s\n' '0:1:0: VGA Fixture GPU' '0:2:0: OMIT-NETWORK'
MOCK
cat > "$fixtures/bin/system_profiler" <<'MOCK'
#!/bin/sh
printf '%s\n' '      Chipset Model: Fixture Mac GPU' '      Serial Number: OMIT-SERIAL'
MOCK
cat > "$fixtures/bin/sw_vers" <<'MOCK'
#!/bin/sh
printf '%s\n' fixture-macos-version
MOCK
cat > "$fixtures/bin/sysctl" <<'MOCK'
#!/bin/sh
printf '%s\n' kvm
MOCK
cat > "$fixtures/bin/systemd-detect-virt" <<'MOCK'
#!/bin/sh
if [ "${TEST_PROBE:-}" = baremetal ]; then printf '%s\n' none; exit 1; fi
printf '%s\n' kvm
MOCK
cat > "$fixtures/bin/pkg" <<'MOCK'
#!/bin/sh
[ "$1" = query ] && [ "$2" = -a ] || exit 2
printf '%s\n' 'webkit2-gtk_41 2.52.6' 'gtk3 3.24.52' 'mesa-dri 26.2.4' 'hyprland 0.56.2' 'plasma6-plasma-workspace 6.4.4' 'private-package 1.0'
MOCK
cat > "$fixtures/bin/pkg_info" <<'MOCK'
#!/bin/sh
[ "$1" = -a ] || exit 2
printf '%s\n' 'webkitgtk41-2.52.6 WebKitGTK browser engine' 'gtk+3-3.24.52 GTK3 toolkit' 'sway-1.11 Wayland compositor' 'private-package-1.0 Do not collect'
MOCK
chmod +x "$fixtures/bin/"*
mkdir "$fixtures/home[regex]"
ln -s "$fixtures/bin/gpclient" "$fixtures/home[regex]/gpclient"
run() {
    env PATH="$fixtures/bin" HOME="$fixtures/home[regex]" TMPDIR="$fixtures" \
        TEST_OS="$1" TEST_PROBE="${2:-}" TEST_CHILD_PID="$fixtures/child.pid" \
        TEST_GUI_MARKER="$fixtures/gui-launched" \
        PRIVATE_SECRET=OMIT-SECRET DISPLAY=OMIT-DISPLAY WAYLAND_DISPLAY=OMIT-SOCKET \
        DBUS_SESSION_BUS_ADDRESS=OMIT-DBUS SSH_CONNECTION=OMIT-SSH \
        XDG_CURRENT_DESKTOP='KDE' DESKTOP_SESSION="$fixtures/home[regex]/desktop" \
        sh "$collector" --client-version embedded-version --client-path "$fixtures/home[regex]/gpclient" > "$fixtures/report"
}
assert_contains() {
    awk -v text="$1" 'index($0,text) {found=1} END {exit !found}' "$fixtures/report" || { printf 'Missing: %s\n' "$1" >&2; cat "$fixtures/report" >&2; exit 1; }
}
for os in Linux FreeBSD OpenBSD Darwin; do
    run "$os"
    assert_contains "os: ok: $os"
    assert_contains 'gpclient_version: ok: embedded-version'
    assert_contains 'gpclient_path: ok: ~/gpclient'
    assert_contains 'env.DESKTOP_SESSION: ok: ~/desktop'
    assert_contains 'env.XDG_CURRENT_DESKTOP: ok: KDE'
    assert_contains 'ssh_session: ok: yes'
    awk 'index($0,"OMIT-") {exit 1} END {if (NR != 28) exit 1}' "$fixtures/report"
    [ ! -e "$fixtures/gui-launched" ]
    awk 'index($0,"private-package") {exit 1}' "$fixtures/report"
    case "$os" in
        Linux) assert_contains 'Fixture GPU'; assert_contains 'fixture-driver' ;;
        FreeBSD) assert_contains 'BSD Fixture GPU'; assert_contains 'graphics_packages: ok: webkit2-gtk_41 2.52.6'; assert_contains 'desktop_version: ok: hyprland 0.56.2 plasma6-plasma-workspace 6.4.4' ;;
        OpenBSD) assert_contains 'VGA Fixture GPU'; assert_contains 'graphics_packages: ok: webkitgtk41-2.52.6'; assert_contains 'desktop_version: ok: sway-1.11' ;;
        Darwin) assert_contains 'Fixture Mac GPU'; assert_contains 'display_protocol: not_applicable:' ;;
    esac
    printf 'PASS: %s fixture selection and privacy\n' "$os"
done
env PATH="$fixtures/bin" TEST_OS=Darwin TEST_GUI_MARKER="$fixtures/gui-launched" \
    sh "$collector" --client-version fixture --gui-path "$fixtures/bin/gpgui" > "$fixtures/report"
assert_contains "gpgui_path: ok: $fixtures/bin/gpgui"
assert_contains 'gpgui_version: unavailable:'
[ ! -e "$fixtures/gui-launched" ]
printf '%s\n' 'PASS: nonbundle GUI path never borrows another installed app version'
mkdir "$fixtures/not-a-file"
printf '#!/bin/sh\nexit 0\n' > "$fixtures/not-executable"
chmod 644 "$fixtures/not-executable"
ln -s "$fixtures/missing" "$fixtures/broken-link"
for os in Linux FreeBSD OpenBSD Darwin; do
    for invalid in missing not-a-file not-executable broken-link; do
        env PATH="$fixtures/bin" TEST_OS="$os" TEST_GUI_MARKER="$fixtures/gui-launched" \
            sh "$collector" --client-path "$fixtures/$invalid" \
            --auth-path "$fixtures/$invalid" --gui-path "$fixtures/$invalid" > "$fixtures/report"
        for component in gpclient gpauth gpgui; do
            assert_contains "${component}_path: unavailable:"
            assert_contains "${component}_version: unavailable:"
        done
    done
done
[ ! -e "$fixtures/gui-launched" ]
printf '%s\n' 'PASS: missing files, directories, nonexecutables and broken links are unavailable on all platforms'
# Successful probes must cancel their watchdog before doing any process-tree
# traversal. A slow snapshot used to let a finished probe become timed_out.
mv "$fixtures/bin/ps" "$fixtures/real-ps"
cat > "$fixtures/bin/ps" <<'MOCK'
#!/bin/sh
sleep 4
exit 1
MOCK
chmod +x "$fixtures/bin/ps"
run Linux
assert_contains 'os: ok: Linux'
assert_contains 'kernel: ok: fixture-kernel'
assert_contains 'graphics: ok:'
rm "$fixtures/bin/ps"
mv "$fixtures/real-ps" "$fixtures/bin/ps"
printf '%s\n' 'PASS: successful watchdog cancellation does not wait for process snapshots'
run Linux baremetal
assert_contains 'virtualization: ok: none'
printf '%s\n' 'PASS: bare-metal virtualization status'
long_value=$(awk 'BEGIN {for (i=0;i<700;i++) printf "x"; printf "\nsecond\tline"}')
env PATH="$fixtures/bin" TEST_OS=Linux XDG_CURRENT_DESKTOP="$long_value" \
    sh "$collector" --client-version fixture > "$fixtures/report"
awk 'index($0,"env.XDG_CURRENT_DESKTOP: ok: ")==1 {if (length($0)-length("env.XDG_CURRENT_DESKTOP: ok: ")>500) exit 1; found=1} END {if (NR!=28 || !found) exit 1}' "$fixtures/report"
printf '%s\n' 'PASS: control flattening and value cap'
# No internal version supplied: old installations use their safe --version switch.
env PATH="$fixtures/bin" TEST_OS=Linux sh "$collector" > "$fixtures/report"
assert_contains 'gpclient_version: ok: gpclient 2.6.5'
printf '%s\n' 'PASS: standalone version discovery'
run Linux timeout
assert_contains 'kernel: timed_out:'
child=$(cat "$fixtures/child.pid")
# A reaped-or-zombie descendant is no longer running; some init implementations
# do not promptly reap an orphan after its parent is killed.
state=$(ps -p "$child" -o stat= 2>/dev/null || :)
case "$state" in ''|Z*) ;; *) printf 'Descendant still running: %s\n' "$state" >&2; exit 1 ;; esac
printf '%s\n' 'PASS: timeout and descendant termination'
run Linux flood
assert_contains 'kernel: failed:'
[ "$(wc -c < "$fixtures/report")" -lt 65536 ]
printf '%s\n' 'PASS: output flood is bounded'
rm -f "$fixtures/child.pid"
env PATH="$fixtures/bin" TEST_OS=Linux TEST_PROBE=timeout TEST_CHILD_PID="$fixtures/child.pid" \
    sh "$collector" > "$fixtures/interrupted" 2>/dev/null &
collector_pid=$!
for attempt in 1 2 3 4 5; do [ ! -f "$fixtures/child.pid" ] || break; sleep 1; done
[ -f "$fixtures/child.pid" ]
kill -TERM "$collector_pid"
if wait "$collector_pid"; then exit 1; fi
child=$(cat "$fixtures/child.pid")
state=$(ps -p "$child" -o stat= 2>/dev/null || :)
case "$state" in ''|Z*) ;; *) printf 'Interrupted descendant still running: %s\n' "$state" >&2; exit 1 ;; esac
printf '%s\n' 'PASS: interruption terminates descendants'
mv "$fixtures/bin/gpauth" "$fixtures/gpauth"
run Linux
assert_contains 'gpauth_version: unavailable:'
mv "$fixtures/gpauth" "$fixtures/bin/gpauth"
printf '%s\n' 'PASS: missing optional helper'

env PATH="$fixtures/bin" TEST_OS=Linux sh "$collector" --output "$fixtures/saved" >/dev/null
cp "$fixtures/saved" "$fixtures/original"
if env PATH="$fixtures/bin" TEST_OS=Linux sh "$collector" --output "$fixtures/saved" >/dev/null 2>&1; then exit 1; fi
cmp "$fixtures/saved" "$fixtures/original"
sh "$collector" --help > "$fixtures/help"
[ -s "$fixtures/help" ]
if sh "$collector" --unknown >/dev/null 2>&1; then exit 1; fi
printf '%s\n' 'PASS: output refuses overwrite, help, argument validation'
