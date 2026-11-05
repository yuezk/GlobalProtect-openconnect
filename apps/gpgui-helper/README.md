# GP Connect download helper

The desktop supervisor launches this window when the selected GUI is missing or
has an incompatible version. It downloads the selected Tauri or Native Shell
interface and requests installation through the service. Launches require the
existing service credential and lifetime channel on stdin.

## Appearance

`--theme system|light|dark` controls this window only. The default is `system`,
which follows system appearance and changes while the window is open. Explicit
`light` and `dark` modes override system appearance for that launch. Nothing is
saved by the helper.

Initial downloads follow the system. During Linux Tauri/Native Shell switches,
Desktop Core supplies the running app's saved theme choice to the supervisor,
which forwards it to the helper. There is no `--helper-theme` option on
`gpclient launch-gui`.

The native window and web content use the selected appearance before the window
is shown. Browser previews follow the system when no native mode is supplied.

## Development

```sh
pnpm build
pnpm test
```

The built `dist/` assets are tracked and must be rebuilt after frontend changes.
Run Rust checks and helper tests from the `gp` repository:

```sh
cargo check -p gpgui-helper
cargo test -p gpgui-helper --lib
```
