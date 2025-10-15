# Changelog

## 2.4.6 - 2025-10-15

- GUI: support the default configuration file for GUI client (fix [#492](https://github.com/yuezk/GlobalProtect-openconnect/issues/492))
- GUI: add the option to not reuse the authentication cookies (fix [#540](https://github.com/yuezk/GlobalProtect-openconnect/issues/540))
- GUI: improve the license validation logic (fix [#502](https://github.com/yuezk/GlobalProtect-openconnect/issues/502))
- CLI: support the `--browser remote` option to use the remote browser for authentication ([#544](https://github.com/yuezk/GlobalProtect-openconnect/pull/544) by [@dark12](https://github.com/dark12))
- CLI: fix gpclient disconnect bailing with client is already running issue ([#542](https://github.com/yuezk/GlobalProtect-openconnect/pull/542) by [@zeroepoch](https://github.com/zeroepoch))
- CLI: fix the `--passwd-on-stdin` reads again on gateway failure ([#546](https://github.com/yuezk/GlobalProtect-openconnect/issues/546))

## 2.4.5 - 2025-07-16

- GUI/CLI: fix the issue that the custom port is not supported issue (fix [#404](https://github.com/yuezk/GlobalProtect-openconnect/issues/404))
- CLI: add the `--force-dpd` option to specify the interval for DPD (Dead Peer Detection).
- CLI: add the `-i/--interface` option to specify the interface to use.

## 2.4.4 - 2025-02-09

- GUI: fix multiple tray icons issue (fix [#464](https://github.com/yuezk/GlobalProtect-openconnect/issues/464))
- CLI: check the cli running state before running the `gpclient` command (fix [#447](https://github.com/yuezk/GlobalProtect-openconnect/issues/447))

## 2.4.3 - 2025-01-21

- Do not use static default value for `--os-version` option.

## 2.4.2 - 2025-01-20

- Disconnect the VPN when sleep (fix [#166](https://github.com/yuezk/GlobalProtect-openconnect/issues/166), [#267](https://github.com/yuezk/GlobalProtect-openconnect/issues/267))

## 2.4.1 - 2025-01-09

- Fix the network issue with OpenSSL < 3.0.4
- GUI: fix the Wayland compatibility issue
- Support configure the log level
- Log the detailed error message when network error occurs

## 2.4.0 - 2024-12-26

- Upgrade to Tauri 2.0
- Support Ubuntu 22.04 and later

## 2.3.9 - 2024-11-02

- Enhance the OpenSSL compatibility mode (fix [#437](https://github.com/yuezk/GlobalProtect-openconnect/issues/437))

## 2.3.8 - 2024-10-31

- GUI: support configure the external browser to use for authentication (fix [#423](https://github.com/yuezk/GlobalProtect-openconnect/issues/423))
- GUI: add option to remember the credential (fix [#420](https://github.com/yuezk/GlobalProtect-openconnect/issues/420))
- GUI: fix the credential not saved issue (fix [#420](https://github.com/yuezk/GlobalProtect-openconnect/issues/420))
- CLI: fix the default browser detection issue (fix [#416](https://github.com/yuezk/GlobalProtect-openconnect/issues/416))

## 2.3.7 - 2024-08-16

- Fix the Rust type inference regression [issue in 1.80](https://github.com/rust-lang/rust/issues/125319).

## 2.3.6 - 2024-08-15

- CLI: enhance the `gpauth` command to support external browser authentication
- CLI: add the `--cookie-on-stdin` option to support read the cookie from stdin
- CLI: support usage: `gpauth <portal> --browser <browser> 2>/dev/null | sudo gpclient connect <portal> --cookie-on-stdin`
- CLI: fix the `--browser <browser>` option not working

## 2.3.5 - 2024-08-14

- Support configure `no-dtls` option
- GUI: fix the tray icon disk usage issue (#398)
- CLI: support specify the browser with `--browser <browser>` option (#405, #407, #397)
- CLI: fix the `--os` option not working

## 2.3.4 - 2024-07-08

- Support the Internal Host Detection (fix [#377](https://github.com/yuezk/GlobalProtect-openconnect/issues/377))
- CLI: support pass the password from stdin (fix [#381](https://github.com/yuezk/GlobalProtect-openconnect/issues/381))

## 2.3.3 - 2024-06-23

- GUI: add the remark field for the license activation
- GUI: check the saved secret key length

## 2.3.2 - 2024-06-17

- Fix the CAS callback parsing issue (fix [#372](https://github.com/yuezk/GlobalProtect-openconnect/issues/372))
- CLI: fix the `/tmp/gpauth.html` deletion issue (fix [#366](https://github.com/yuezk/GlobalProtect-openconnect/issues/366))
- GUI: fix the license not working after reboot (fix [#376](https://github.com/yuezk/GlobalProtect-openconnect/issues/376))
- GUI: add the license activation management link

## 2.3.1 - 2024-05-21

- Fix the `--sslkey` option not working

## 2.3.0 - 2024-05-20

- Support client certificate authentication (fix [#363](https://github.com/yuezk/GlobalProtect-openconnect/issues/363))
- Support `--disable-ipv6`, `--reconnect-timeout` parameters (related: [#364](https://github.com/yuezk/GlobalProtect-openconnect/issues/364))
- Use default labels if label fields are missing in prelogin response (fix [#357](https://github.com/yuezk/GlobalProtect-openconnect/issues/357))

## 2.2.1 - 2024-05-07

- GUI: Restore the default browser auth implementation (fix [#360](https://github.com/yuezk/GlobalProtect-openconnect/issues/360))

## 2.2.0 - 2024-04-30

- CLI: support authentication with external browser (fix [#298](https://github.com/yuezk/GlobalProtect-openconnect/issues/298))
- GUI: support using file-based storage when the system keyring is not available.

## 2.1.4 - 2024-04-10

- Support MFA authentication (fix [#343](https://github.com/yuezk/GlobalProtect-openconnect/issues/343))
- Improve the Gateway switcher UI

## 2.1.3 - 2024-04-07

- Support CAS authentication (fix [#339](https://github.com/yuezk/GlobalProtect-openconnect/issues/339))
- CLI: Add `--as-gateway` option to connect as gateway directly (fix [#318](https://github.com/yuezk/GlobalProtect-openconnect/issues/318))
- GUI: Support connect the gateway directly (fix [#318](https://github.com/yuezk/GlobalProtect-openconnect/issues/318))
- GUI: Add an option to use symbolic tray icon (fix [#341](https://github.com/yuezk/GlobalProtect-openconnect/issues/341))

## 2.1.2 - 2024-03-29

- Treat portal as gateway when the gateway login is failed (fix #338)

## 2.1.1 - 2024-03-25

- Add the `--hip` option to enable HIP report
- Fix not working in OpenSuse 15.5 (fix #336, #322)
- Treat portal as gateway when the gateway login is failed (fix #338)
- Improve the error message (fix #327)

## 2.1.0 - 2024-02-27

- Update distribution channel for `gpgui` to complaint with the GPL-3 license.
- Add `mtu` option.
- Retry auth if failed to obtain the auth cookie

## 2.0.0 - 2024-02-05

- Refactor using Tauri
- Support HIP report
- Support pass vpn-slice command
- Do not error when the region field is empty
- Update the auth window icon
