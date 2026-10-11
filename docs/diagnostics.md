# Collect diagnostics for a bug report

The diagnostic collector gathers app and system versions, desktop and graphics
information, and a small set of rendering environment variables. It does not
connect to a VPN, launch the GUI, read credentials or logs, upload a report, or
change settings.

Run it from a terminal in your normal desktop session, **without sudo**, so it
can see the desktop environment used by the application. Linux, macOS, FreeBSD,
and OpenBSD are supported. Missing tools or unavailable information are reported
without preventing collection.

## Installed versions with the diagnostics command

```sh
gpclient diagnostics
```

To save a report to a new file:

```sh
gpclient diagnostics --output diagnostics.txt
```

For structured output:

```sh
gpclient diagnostics --json --output diagnostics.json
```

The collector refuses to overwrite an existing file. Choose another filename or
remove the previous report yourself before collecting again.

This command is available in 3.0.2 and later. If your installed version says the
command is unknown, use the standalone script below. You do not need to upgrade
before reporting a problem.

## Older versions: standalone script

The standalone script collects the same information in a readable format. It
requires a POSIX shell and common system utilities, without Python or jq.

Download the script to a local file:

```sh
curl --fail --location \
  https://raw.githubusercontent.com/yuezk/GlobalProtect-openconnect/dev/scripts/collect-diagnostics.sh \
  --output collect-diagnostics.sh
```

Open `collect-diagnostics.sh` in a text editor and review it before running it.
Then, as your normal desktop user:

```sh
sh collect-diagnostics.sh
```

Or save the report to a new file:

```sh
sh collect-diagnostics.sh --output diagnostics.txt
```

The download points to the development branch because this collector was added
after 3.0.1. It becomes available there when the change is published. From a
local checkout containing the change, run
`sh scripts/collect-diagnostics.sh` instead. Do not pipe a downloaded script
directly into a shell.

## Review before sharing

Read the report before pasting it into an issue or attaching the saved file.
It includes executable paths with your home directory shortened to `~`, raw
desktop/session identifiers, and selected rendering overrides. These values can
contain identifying information, so the report is **not guaranteed anonymous**.

Remove any private information before posting. Never share passwords, cookies,
tokens, SAML payloads, private VPN addresses, or a complete environment dump.

Diagnostics do not include logs. For CLI issues, share the relevant terminal
output separately after reviewing it. For GUI issues, use the app's built-in
option to open or export logs, if available. Avoid posting full verbose logs
unless requested, as authentication output can contain sensitive data.

Include your installation method and the exact command or GUI steps that fail.
Replace private portal names with `<portal>` and omit credentials. If you cannot
run the collector, provide the app version, OS, and relevant desktop,
GPU/driver, or WebKitGTK details manually. Graphics details are useful for
window failures but are not needed for every connection problem.
