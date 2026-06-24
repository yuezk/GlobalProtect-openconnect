# GlobalProtect-openconnect Docker image

This image provides the GlobalProtect-openconnect command-line tools on Alpine Linux. It includes:

- `gpclient` - connect to and disconnect from GlobalProtect VPN portals and gateways
- `gpauth` - complete authentication separately and pipe the result into `gpclient`

The image does not include embedded webview authentication, `gpgui-helper`, or the graphical `gpgui` application.

## Pull

```bash
docker pull yuezk/globalprotect-openconnect:<version>
```

Release images are tagged as `vX.Y.Z`, `X.Y.Z`, and `latest`.

## Usage

VPN tunnel creation requires access to `/dev/net/tun` and the `NET_ADMIN` capability:

```bash
docker run --rm -it --cap-add=NET_ADMIN --device=/dev/net/tun \
  yuezk/globalprotect-openconnect:<version> \
  connect <portal> --cookie-on-stdin
```

For browser authentication in a headless environment, use remote browser authentication:

```bash
docker run --rm -it --cap-add=NET_ADMIN --device=/dev/net/tun \
  yuezk/globalprotect-openconnect:<version> \
  connect <portal> --browser remote
```

On a Linux host, add host networking if the VPN routes should affect the host network namespace:

```bash
docker run --rm -it --network host --cap-add=NET_ADMIN --device=/dev/net/tun \
  yuezk/globalprotect-openconnect:<version> \
  connect <portal> --browser remote
```

Without `--network host`, the VPN connection stays inside the container network namespace. Docker Desktop on macOS and Windows does not make the host use the VPN through `--network host`; run `gpclient` on the host or use a container gateway setup for host traffic.

You can also run `gpauth` separately and pipe its remote-browser output into `gpclient`:

```bash
docker run --rm -it --entrypoint gpauth yuezk/globalprotect-openconnect:<version> \
  <portal> --browser remote 2>/dev/null \
  | docker run --rm -i --cap-add=NET_ADMIN --device=/dev/net/tun \
      yuezk/globalprotect-openconnect:<version> \
      connect <portal> --cookie-on-stdin
```

## Common commands

Show top-level help:

```bash
docker run --rm yuezk/globalprotect-openconnect:<version> --help
```

Show help for a command:

```bash
docker run --rm yuezk/globalprotect-openconnect:<version> help connect
```

## Project

GlobalProtect-openconnect is a modern GlobalProtect VPN client for Linux, built on OpenConnect with support for SSO, non-SSO, FIDO2, client certificate authentication, multiple portals and gateways, and direct gateway connections.

Source: https://github.com/yuezk/GlobalProtect-openconnect
