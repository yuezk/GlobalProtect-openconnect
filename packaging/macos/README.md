# macOS packaging

## GPCallback.app — handler for the `globalprotectcallback:` URL scheme

External-browser authentication (`gpclient connect --browser`) ends with the
identity provider redirecting the browser to:

```
globalprotectcallback:<base64 auth data>
```

`gpauth` listens on a local socket for that data and records the port in
`$TMPDIR/gpcallback.port`. Something has to receive the URL from the browser and
forward it to that socket.

On Linux and BSD that job belongs to `gpgui.desktop`, which claims the scheme with
`MimeType=x-scheme-handler/globalprotectcallback` and runs `gpclient launch-gui <url>`.
See `packaging/files/usr/share/applications/gpgui.desktop` and `packaging/bsd/gpgui.desktop`.

macOS had no equivalent. A URL scheme can only be claimed by an application
bundle, and the macOS build ships bare binaries, so the browser had nowhere to
deliver the callback — the SSO would complete, the browser would report
*"Safari cannot open the page because the address is invalid"*, and `gpclient`
would wait forever.

`GPCallback.app` is that missing hop: a minimal AppleScript applet whose
`on open location` handler runs `gpclient launch-gui` with the callback URL.

### Build and install

```sh
./build-gpcallback.sh --install
```

This builds the bundle into `./build`, installs it to `~/Applications`, and
registers the scheme with LaunchServices. Verify:

```sh
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
  -dump | grep globalprotectcallback
```

Then authenticate normally:

```sh
sudo gpclient connect <portal> --browser
```

To remove it:

```sh
./build-gpcallback.sh --uninstall
```

### Notes

- Only tools bundled with macOS are used: `osacompile`, `PlistBuddy`, `codesign`,
  `lsregister`. The compiled `.app` is a build artifact and is not checked in.
- `gpclient` is looked up in `/opt/homebrew/bin` then `/usr/local/bin`, matching
  the Apple Silicon and Intel paths in `crates/common/src/constants.rs`.
- The applet resolves `TMPDIR` with `getconf DARWIN_USER_TEMP_DIR` instead of
  inheriting it. `do shell script` runs with a minimal environment, and with
  `TMPDIR` unset Rust's `std::env::temp_dir()` falls back to `/tmp`, where the
  port file does not live.
- Handler output goes to `$TMPDIR/gpcallback.log` — the same file `gpauth` points
  to when the handoff hangs. Check it first when debugging.
- Testing the handler by hand while a tunnel is already up fails with
  *"Another instance of the client is already running"*. The single-instance gate
  in `apps/gpclient/src/cli.rs` exempts only `disconnect`, and the lock file is
  written by `write_pid_file()` in `apps/gpclient/src/connect/gateway.rs` once the
  tunnel connects. During a real login the lock does not exist yet, so the
  callback gets through; disconnect first if you want to test manually.
- If you only need a one-off connection without installing anything, use
  `--browser remote`, which prints the URL and reads the callback from stdin.
