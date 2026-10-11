# Diagnostics collection design

## Purpose

`gpclient diagnostics` collects support information without starting a VPN,
opening a window, accessing credentials, reading logs, or making network
requests. A standalone script provides the same collection for older releases.
Users review the report before attaching it to a public issue.

## Interface

- `gpclient diagnostics`: readable report on stdout, no sudo needed.
- `gpclient diagnostics --json`: the same fields as a JSON object, with a schema
  version. It contains information, not log events.
- `gpclient diagnostics --output PATH`: save the selected format to a new file;
  refuse to overwrite an existing file.
- `sh collect-diagnostics.sh`: the same readable report for older installations.
- `sh collect-diagnostics.sh --output PATH`: save to a new file. The standalone
  script needs only POSIX shell and common base utilities, not Python or jq.

The script is shipped in `scripts/collect-diagnostics.sh`. The CLI embeds that
exact file at compile time and runs it with `/bin/sh`; no installed script is
needed. It supplies its own build version instead of recursively invoking
`gpclient`. JSON formatting and file output are responsibilities of the Rust
command. The script emits stable `field: status: value` lines; values are
flattened to one line. JSON has `schema_version: 1` and `fields`, a map of field
names to `{status, value}` objects. Status is `ok`, `unset`, `unavailable`,
`failed`, `timed_out`, or `not_applicable`; absent values become null. Unknown
fields, statuses or duplicate keys are rejected by the Rust parser. Fields
cover system, app paths/versions, GUI metadata, execution context, desktop,
graphics/packages, and the environment allowlist below.

## Collection and platform coverage

Common fields: report schema, OS/kernel release and architecture, gpclient and
gpauth versions, executable locations, normal-user versus root execution,
presence of an SSH session, VM indicators when available, and installation
location hints. A location hint does not claim a package-manager provenance
that cannot be verified. Executable paths replace the home directory with `~`.
Only known version options are invoked; the GUI application is not launched to
discover its version.

Linux, FreeBSD and OpenBSD: raw desktop/session identifiers, display protocol
inferred from presence of display variables (their socket values are excluded),
graphics device/driver summaries, available package versions for WebKitGTK,
GTK and graphics drivers. Linux reads selected `/etc/os-release` fields and
uses available package-manager queries. BSD uses its own base utilities and
package names, rather than assuming Linux `/proc`, `lspci`, or `pacman` exist.
macOS uses `sw_vers`, bundle paths and a filtered graphics model probe; Linux
graphics and desktop fields are marked not applicable.

Environment allowlist: `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP`,
`XDG_SESSION_TYPE`, `DESKTOP_SESSION`, `WEBKIT_DISABLE_DMABUF_RENDERER`,
`WEBKIT_DISABLE_COMPOSITING_MODE`, `LIBGL_ALWAYS_SOFTWARE`,
`__NV_DISABLE_EXPLICIT_SYNC`, `GDK_BACKEND`, `GDK_GL`. Values are capped and
flattened; HOME, DISPLAY, WAYLAND_DISPLAY, DBUS addresses, hostname, username,
machine IDs, network configuration and arbitrary environment variables are
excluded. Desktop identifiers are raw user-controlled strings, so reports are
not guaranteed anonymous and must be reviewed before sharing.

Parse OS-release assignments as data without sourcing them. Strip control bytes
and line breaks, cap each value at 512 bytes, and decode invalid UTF-8 lossily
in Rust. Reports are capped at 64 KiB. Diagnostics dispatches before generic
OpenSSL setup even if global CLI compatibility flags are supplied.

Optional probes report `unavailable`, `timed_out`, or `failed` without stopping
collection. Each external probe has a three-second timeout and a 64-KiB output
cap. A watchdog terminates the probe and its discoverable descendants using
PID/parent-PID snapshots, and the collector reaps the direct child and cancels
the watchdog. Signal cleanup also stops active probes and watchdogs. This is
best-effort tree cleanup for trusted local system utilities: a process that
deliberately reparents/daemonizes can escape it. No such probes are selected.
Tests cover descendants, ignored TERM, output flooding and interruption. Probe
stderr is discarded rather than pasted into the report. No packages are
installed. No privilege escalation or rendering workaround is applied.

## Issue form

Replace the Markdown template with a GitHub YAML issue form. Require affected
component, app version, installation method, OS, and reproduction steps including
expected and actual behavior. Ask for last working version/system changes,
diagnostics, relevant logs and screenshots as optional fields. Provide the new
command and a download/review/run path for the standalone script. Reporters may
instead enter desktop, GPU/driver and WebKit information manually. Authentication
reports should name embedded/external browser and the failure stage; credential
storage details are requested only when relevant. Warn against sharing passwords,
cookies, tokens, SAML payloads or complete environment dumps. Do not require
running workaround flags or upgrading before reporting a regression.

## Verification

Test the shared collector with controlled command/environment fixtures: missing
tools, hung probes, sensitive data excluded, home path sanitization, version
recursion avoided, and OS-specific selection for Linux/FreeBSD/OpenBSD/macOS.
Test CLI parsing, schema/JSON conversion and overwrite refusal. Run shell syntax
checks, targeted Rust checks/tests, and a real local macOS report. BSD/Linux fixture
coverage validates selection/parsing; it is not native runtime verification.

## Scope

No log bundling, uploading, connectivity tests, package installation, configuration
changes, GUI integration or crash recovery. The diagnostics module owns collection
and formatting; existing session/service transport models are not involved.
