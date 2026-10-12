#!/bin/sh
# Read-only support metadata. No network requests, logs or credential collection.
# Process-tree cleanup is best effort; deliberately daemonized probes can escape it.
umask 077
LC_ALL=C
export LC_ALL
GP_DIAGNOSTICS_HOME=$(printf '%s' "${HOME:-}" | tr '\000-\037\177' ' ')
export GP_DIAGNOSTICS_HOME
client_version= client_path= auth_path= gui_path= output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --help) printf '%s\n' 'Usage: sh collect-diagnostics.sh [--output PATH]' 'Collect local support metadata; review before sharing. No sudo required.'; exit 0 ;;
        --output|--client-version|--client-path|--auth-path|--gui-path)
            [ "$#" -ge 2 ] || { printf '%s\n' 'Missing option value' >&2; exit 2; }
            case "$1" in
                --output) output=$2 ;; --client-version) client_version=$2 ;;
                --client-path) client_path=$2 ;; --auth-path) auth_path=$2 ;; --gui-path) gui_path=$2 ;;
            esac
            shift 2 ;;
        *) printf '%s\n' 'Unknown option' >&2; exit 2 ;;
    esac
done
if [ -n "$output" ]; then
    set -C
    exec 3> "$output" || exit 1
    set +C
    exec 1>&3
fi
work=$(mktemp -d "${TMPDIR:-/tmp}/gp-diagnostics.XXXXXXXX") || exit 1
chmod 700 "$work" || exit 1
active= timer=
# Use a fixed PID/PPID snapshot, never full process arguments.
stop_tree() {
    tree_root=$1
    ps -axo pid=,ppid= > "$work/tree.$tree_root" 2>/dev/null || :
    tree_pids=$(awk -v root="$tree_root" '
        {parent[$1]=$2} END {
            marked[root]=1
            for (i=0;i<100;i++) {changed=0; for (pid in parent) if (marked[parent[pid]] && !marked[pid]) {marked[pid]=1; changed=1} if (!changed) break}
            for (pid in marked) if (marked[pid] && pid != root) print pid
        }' "$work/tree.$tree_root")
    # Stop descendants before the direct child: reaping the child must not race
    # cancellation of a watchdog which is still terminating descendants.
    for tree_pid in $tree_pids; do kill -TERM "$tree_pid" 2>/dev/null || :; done
    for tree_pid in $tree_pids; do kill -KILL "$tree_pid" 2>/dev/null || :; done
    kill -TERM "$tree_root" 2>/dev/null || :
    kill -KILL "$tree_root" 2>/dev/null || :
}
cleanup() {
    [ -z "$active" ] || { stop_tree "$active"; wait "$active" 2>/dev/null || :; }
    [ -z "$timer" ] || { stop_tree "$timer"; wait "$timer" 2>/dev/null || :; }
    rm -rf "$work"
}
trap cleanup 0
trap 'exit 130' INT
trap 'exit 143' TERM HUP
emit() {
    # HOME is matched literally, never used as a regular expression.
    [ "$2" = ok ] || { printf '%s: %s: \n' "$1" "$2"; return; }
    clean=$(printf '%s' "$3" | tr '\000-\037\177' ' ' | awk '
        BEGIN {home=ENVIRON["GP_DIAGNOSTICS_HOME"]}
        {text=$0; result=""; while (length(home)>0 && (pos=index(text,home))>0) {
            tail=substr(text,pos+length(home)); result=result substr(text,1,pos-1)
            if (tail=="" || substr(tail,1,1)=="/") result=result "~"; else result=result home
            text=tail
        } text=result text; printf "%s",substr(text,1,500)}')
    printf '%s: %s: %s\n' "$1" "$2" "$clean"
}
probe() {
    probe_status= probe_value=
    if ! command -v "$1" >/dev/null 2>&1; then probe_status=unavailable; return; fi
    rm -f "$work/timed"
    # ulimit -f is POSIX and limits stdout's regular file to at most 64 KiB.
    (trap - 0 INT TERM HUP; ulimit -f 64; exec "$@") < /dev/null > "$work/out" 2>/dev/null &
    active=$!
    (
        trap - 0 INT TERM HUP
        watchdog_sleep=
        trap '[ -z "$watchdog_sleep" ] || { kill "$watchdog_sleep" 2>/dev/null || :; wait "$watchdog_sleep" 2>/dev/null || :; }; exit' TERM HUP INT
        sleep 3 &
        watchdog_sleep=$!
        wait "$watchdog_sleep"
        : > "$work/timed"
        stop_tree "$active"
    ) 2>/dev/null &
    timer=$!
    if wait "$active" 2>/dev/null; then probe_status=ok; else probe_status=failed; fi
    active=
    # Cancel immediately; a process-tree snapshot here could itself cross the
    # timeout deadline after a successful probe. The watchdog reaps its sleep.
    kill -TERM "$timer" 2>/dev/null || :
    wait "$timer" 2>/dev/null || :
    timer=
    if [ "$probe_status" = failed ] && [ -f "$work/timed" ]; then probe_status=timed_out; fi
    if [ "$probe_status" != timed_out ]; then probe_value=$(cat "$work/out"); fi
}
field_probe() { field_key=$1; shift; probe "$@"; emit "$field_key" "$probe_status" "$probe_value"; }
emit schema_version ok 1
probe uname -s
os=$probe_value
emit os "$probe_status" "$os"
field_probe kernel uname -r
field_probe architecture uname -m
probe id -u
case "$probe_status:$probe_value" in ok:0) emit execution_user ok root ;; ok:*) emit execution_user ok normal_user ;; *) emit execution_user "$probe_status" '' ;; esac
if [ -n "${SSH_CONNECTION:-}${SSH_CLIENT:-}${SSH_TTY:-}" ]; then emit ssh_session ok yes; else emit ssh_session ok no; fi
case "$os" in
    Linux)
        if [ -r /etc/os-release ]; then
            probe awk -F= '$1=="PRETTY_NAME" {value=substr($0,index($0,"=")+1); if (value ~ /^".*"$/ || value ~ /^\047.*\047$/) value=substr(value,2,length(value)-2); print value; exit}' /etc/os-release
            if [ "$probe_status" = ok ] && [ -z "$probe_value" ]; then probe_status=unavailable; fi
            emit os_version "$probe_status" "$probe_value"
        else emit os_version unavailable ''; fi
        probe systemd-detect-virt
        if [ "$probe_status" = failed ] && [ "$probe_value" = none ]; then probe_status=ok; fi
        emit virtualization "$probe_status" "$probe_value" ;;
    Darwin) field_probe os_version sw_vers -productVersion; emit virtualization unavailable '' ;;
    FreeBSD|OpenBSD) field_probe os_version uname -r; field_probe virtualization sysctl -n kern.vm_guest ;;
    *) emit os_version unavailable ''; emit virtualization unavailable '' ;;
esac
# Helpers accept only known safe version switches. Never invoke GUI binaries.
[ -n "$client_path" ] || client_path=$(command -v gpclient 2>/dev/null || :)
[ -n "$auth_path" ] || auth_path=$(command -v gpauth 2>/dev/null || :)
[ -n "$gui_path" ] || gui_path=$(command -v gpgui 2>/dev/null || :)
if [ "$os" = Darwin ]; then
    bundle=/Applications/GP\ Connect.app/Contents
    gui_bundle=
    case "$gui_path" in *.app/Contents/MacOS/*) gui_bundle=${gui_path%/MacOS/*}; bundle=$gui_bundle ;; esac
    [ -n "$client_path" ] || { [ ! -x "$bundle/Helpers/gpclient" ] || client_path=$bundle/Helpers/gpclient; }
    [ -n "$auth_path" ] || { [ ! -x "$bundle/Helpers/gpauth" ] || auth_path=$bundle/Helpers/gpauth; }
    [ -n "$gui_path" ] || { [ ! -d "$bundle" ] || { gui_path=$bundle/MacOS/gpgui; gui_bundle=$bundle; }; }
fi
is_executable_file() { [ -f "$1" ] && [ -x "$1" ]; }
is_executable_file "$client_path" || client_path=
is_executable_file "$auth_path" || auth_path=
is_executable_file "$gui_path" || gui_path=
for component in gpclient gpauth gpgui; do
    case "$component" in gpclient) location=$client_path ;; gpauth) location=$auth_path ;; gpgui) location=$gui_path ;; esac
    if [ -n "$location" ]; then emit "${component}_path" ok "$location"; else emit "${component}_path" unavailable ''; fi
done
if [ -n "$client_version" ]; then emit gpclient_version ok "$client_version"; elif [ -n "$client_path" ]; then field_probe gpclient_version "$client_path" --version; else emit gpclient_version unavailable ''; fi
if [ -n "$auth_path" ]; then field_probe gpauth_version "$auth_path" --version; else emit gpauth_version unavailable ''; fi
if [ "$os" = Darwin ] && [ -n "$gui_path" ] && [ -n "$gui_bundle" ]; then
    field_probe gpgui_version /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$gui_bundle/Info.plist"
else
    emit gpgui_version unavailable ''
fi
case "$os" in
    Darwin) emit display_protocol not_applicable ''; probe system_profiler SPDisplaysDataType; graphics_value=$(printf '%s\n' "$probe_value" | awk '/Chipset Model:/ {print}'); emit graphics "$probe_status" "$graphics_value" ;;
    *)
        if [ -n "${WAYLAND_DISPLAY:-}" ]; then emit display_protocol ok wayland; elif [ -n "${DISPLAY:-}" ]; then emit display_protocol ok x11; else emit display_protocol unset ''; fi
        case "$os" in
            Linux) probe lspci -nnk; graphics_status=$probe_status; graphics_value=$(printf '%s\n' "$probe_value" | awk '/^[^ \t]/ {show=($0 ~ /VGA|3D controller|Display controller/)} show {print}') ;;
            FreeBSD) probe pciconf -lv; graphics_status=$probe_status; graphics_value=$(printf '%s\n' "$probe_value" | awk '/^[^ \t]/ {show=($0 ~ /^vgapci/)} show {print}') ;;
            OpenBSD) probe pcidump; graphics_status=$probe_status; graphics_value=$(printf '%s\n' "$probe_value" | awk '/VGA|Display|Graphics/ {print}') ;;
            *) graphics_status=not_applicable; graphics_value= ;;
        esac
        emit graphics "$graphics_status" "$graphics_value" ;;
esac
case "$os" in
    Linux)
        if command -v pacman >/dev/null 2>&1; then probe pacman -Q webkit2gtk-4.1 gtk3 mesa nvidia-utils nvidia-open nvidia egl-wayland
        elif command -v dpkg-query >/dev/null 2>&1; then probe dpkg-query -W '-f=${binary:Package} ${Version}\n' 'libwebkit2gtk*' 'libgtk-3-0*' 'libegl-mesa*' 'libgl1-mesa*' 'nvidia-driver*' 'libnvidia-egl-wayland*'
        elif command -v rpm >/dev/null 2>&1; then probe rpm -q webkit2gtk4.1 gtk3 mesa-dri-drivers mesa-libEGL xorg-x11-drv-nvidia egl-wayland
        else probe_status=unavailable; probe_value=; fi
        probe_value=$(printf '%s\n' "$probe_value" | awk '$1 ~ /^(webkit2gtk|libwebkit2gtk|gtk3|libgtk-3|mesa|libegl-mesa|libgl1-mesa|nvidia|xorg-x11-drv-nvidia|egl-wayland|libnvidia-egl-wayland)/ && $0 !~ /not installed|no packages found|unknown package/ {print}')
        [ -z "$probe_value" ] || probe_status=ok
        emit graphics_packages "$probe_status" "$probe_value"
        ;;
    FreeBSD) probe pkg query -a '%n %v'; package_value=$(printf '%s\n' "$probe_value" | awk '$1 ~ /^(webkit2-gtk3|webkit2-gtk_41|gtk3|mesa-dri|nvidia-driver)(-|$)/'); emit graphics_packages "$probe_status" "$package_value" ;;
    OpenBSD) probe pkg_info -a; package_value=$(printf '%s\n' "$probe_value" | awk '$1 ~ /^(webkitgtk|webkitgtk41|gtk\+3|mesa|nvidia)-/'); emit graphics_packages "$probe_status" "$package_value" ;;
    *) emit graphics_packages not_applicable '' ;;
esac
# Desktop binaries are not launched: version information comes from package metadata.
case "$os" in
    Linux)
        if command -v pacman >/dev/null 2>&1; then probe pacman -Q plasma-workspace hyprland sway gnome-shell cosmic-comp
        elif command -v dpkg-query >/dev/null 2>&1; then probe dpkg-query -W '-f=${binary:Package} ${Version}\n' plasma-workspace hyprland sway gnome-shell cosmic-comp
        elif command -v rpm >/dev/null 2>&1; then probe rpm -q plasma-workspace hyprland sway gnome-shell cosmic-comp
        else probe_status=unavailable; probe_value=; fi
        probe_value=$(printf '%s\n' "$probe_value" | awk '$1 ~ /^(plasma-workspace|hyprland|sway|gnome-shell|cosmic-comp)/ && $0 !~ /not installed|no packages found|unknown package/ {print}')
        [ -z "$probe_value" ] || probe_status=ok
        emit desktop_version "$probe_status" "$probe_value" ;;
    FreeBSD) probe pkg query -a '%n %v'; package_value=$(printf '%s\n' "$probe_value" | awk '$1 ~ /^(plasma6-plasma-workspace|hyprland|sway|gnome-shell)(-|$)/'); emit desktop_version "$probe_status" "$package_value" ;;
    OpenBSD) probe pkg_info -a; package_value=$(printf '%s\n' "$probe_value" | awk '$1 ~ /^(plasma|hyprland|sway|gnome-shell)-/'); emit desktop_version "$probe_status" "$package_value" ;;
    *) emit desktop_version not_applicable '' ;;
esac
for name in XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP XDG_SESSION_TYPE DESKTOP_SESSION WEBKIT_DISABLE_DMABUF_RENDERER WEBKIT_DISABLE_COMPOSITING_MODE LIBGL_ALWAYS_SOFTWARE __NV_DISABLE_EXPLICIT_SYNC GDK_BACKEND GDK_GL; do
    eval 'present=${'"$name"'+yes}; value=${'"$name"'-}'
    if [ "$present" = yes ]; then emit "env.$name" ok "$value"; else emit "env.$name" unset ''; fi
done
