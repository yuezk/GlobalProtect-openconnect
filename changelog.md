# Changelog

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
