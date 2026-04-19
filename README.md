# GlobalProtect-openconnect

A GUI for GlobalProtect VPN, based on OpenConnect, supports the SSO authentication method. Inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img width="300" src="https://github.com/yuezk/GlobalProtect-openconnect/assets/3297602/9242df9c-217d-42ab-8c21-8f9f69cd4eb5">
</p>

## Features

- [x] Better Linux support
- [x] Support both CLI and GUI
- [x] Support both SSO and non-SSO authentication
- [x] Support the FIDO2 authentication (e.g., YubiKey)
- [x] Support authentication using default browser
- [x] Support client certificate authentication
- [x] Support multiple portals
- [x] Support gateway selection
- [x] Support connect gateway directly
- [x] Support auto-connect on startup
- [x] Support system tray icon

## Usage

### CLI

The CLI version is always free and open source in this repo. It has almost the same features as the GUI version.

```
Usage: gpclient [OPTIONS] <COMMAND>

Commands:
  connect     Connect to a portal server
  disconnect  Disconnect from the server
  launch-gui  Launch the GUI
  help        Print this message or the help of the given subcommand(s)

Options:
      --fix-openssl        Get around the OpenSSL `unsafe legacy renegotiation` error
      --ignore-tls-errors  Ignore the TLS errors
  -h, --help               Print help
  -V, --version            Print version

See 'gpclient help <command>' for more information on a specific command.
```

To use the external browser for authentication with the CLI version, you need to use the following command:

```bash
sudo -E gpclient connect --browser default <portal>
```

Or you can try the following command if the above command does not work:

```bash
gpauth <portal> --browser default 2>/dev/null | sudo gpclient connect <portal> --cookie-on-stdin
```

You can specify the browser with the `--browser <browser>` option, e.g., `--browser firefox`, `--browser chrome`, etc.

### GUI

The GUI version is also available after you installed it. You can launch it from the application menu or run `gpclient launch-gui` in the terminal.

> [!Note]
>
> The GUI version is partially open source. Its background service is open sourced in this repo as [gpservice](./apps/gpservice/). The GUI part is a wrapper of the background service, which is not open sourced.

## Installation

### Debian/Ubuntu based distributions

#### Install from PPA (Ubuntu 18.04 and later, except 24.04)

```
sudo apt-get install gir1.2-gtk-3.0 gir1.2-webkit2-4.0
sudo add-apt-repository ppa:yuezk/globalprotect-openconnect
sudo apt-get update
sudo apt-get install globalprotect-openconnect
```

> [!Note]
>
> For Linux Mint, you might need to import the GPG key with: `sudo apt-key adv --keyserver keyserver.ubuntu.com --recv-keys 7937C393082992E5D6E4A60453FC26B43838D761` if you encountered an error `gpg: keyserver receive failed: General error`.

#### **Ubuntu 24.04 and later**

The `libwebkit2gtk-4.0-37` package was [removed](https://bugs.launchpad.net/ubuntu/+source/webkit2gtk/+bug/2061914) from its repo. You can use the [`deb-install.sh`](./scripts/deb-install.sh) script to install the package:

```bash
curl -o- https://raw.githubusercontent.com/yuezk/GlobalProtect-openconnect/main/scripts/deb-install.sh \
  | bash -s -- 2.3.9
```

#### **Ubuntu 18.04**

The latest package is not available in the PPA either, but you still needs to add the `ppa:yuezk/globalprotect-openconnect` repo beforehand to use the required `openconnect` package. Then you can follow the [Install from deb package](#install-from-deb-package) section to install the latest package.

#### Install from deb package

Download the latest deb package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page. Then install it with `apt`:

```bash
sudo apt install --fix-broken globalprotect-openconnect_*.deb
```

### Arch Linux / Manjaro

#### Install from AUR

Install from AUR: [globalprotect-openconnect-git](https://aur.archlinux.org/packages/globalprotect-openconnect-git/)

```bash
yay -S globalprotect-openconnect-git
```

#### Install from package

Download the latest package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page. Then install it with `pacman`:

```bash
sudo pacman -U globalprotect-openconnect-*.pkg.tar.zst
```

### Fedora 38 and later / Fedora Rawhide

#### Install from COPR

The package is available on [COPR](https://copr.fedorainfracloud.org/coprs/yuezk/globalprotect-openconnect/) for various RPM-based distributions. You can install it with the following commands:

```bash
sudo dnf copr enable yuezk/globalprotect-openconnect
sudo dnf install globalprotect-openconnect
```

### openSUSE Leap 15.6 / openSUSE Tumbleweed

#### Install from OBS (openSUSE Build Service)

The package is also available on [OBS](https://build.opensuse.org/package/show/home:yuezk/globalprotect-openconnect) for various RPM-based distributions. You can follow the instructions [on this page](https://software.opensuse.org//download.html?project=home%3Ayuezk&package=globalprotect-openconnect) to install it.

### Other RPM-based distributions

#### Install from RPM package

Download the latest RPM package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.

On RHEL / Rocky Linux / AlmaLinux / CentOS, enable EPEL first:

```bash
sudo dnf install epel-release
```

```bash
sudo rpm -i globalprotect-openconnect-*.rpm
```

### Gentoo

It is available via `guru` and `lamdness` overlays.

```bash
sudo eselect repository enable guru
sudo emerge -r guru sync
sudo emerge -av net-vpn/globalprotect-openconnect
```

### Other distributions

- Install `openconnect >= 8.20`, `webkit2gtk`, `libsecret`, `libayatana-appindicator` or `libappindicator-gtk3`.
- Download `globalprotect-openconnect_${version}_${arch}.bin.tar.xz` from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.
- Extract the tarball with `tar -xJf globalprotect-openconnect_${version}_${arch}.bin.tar.xz`.
- Run `sudo make install` to install the client.

## Build from source

The source build now vendors OpenConnect in `crates/openconnect/deps/openconnect`.
It can also build libxml2 from `crates/openconnect/deps/libxml2` when needed.
You do not need to install `openconnect` or `libopenconnect-dev` to build from source.

If you build from a git checkout, make sure the vendored sources are present:

```bash
git clone --recursive https://github.com/yuezk/GlobalProtect-openconnect.git
```

Or, if you already cloned the repo without submodules:

```bash
git submodule update --init --recursive
```

If you build from the release source tarball (`globalprotect-openconnect-${version}.tar.gz`), the vendored sources are already included, so you do not need the submodule step.

### Prerequisites

- [Install Rust 1.75 or later](https://www.rust-lang.org/tools/install)
- Install Tauri dependencies: https://tauri.app/v1/guides/getting-started/prerequisites/#setting-up-linux
- Install `make`, `pkg-config`, `autoconf`, `automake`, `libtool`, `patch`, `perl`, and `jq`
- Install development libraries for `gnutls`, `zlib`, `lz4`, `p11-kit`, `nettle`, `gmp`, and `openssl`
- Install `pkexec`, `gnome-keyring` (or `pam_kwallet` on KDE)
- Install `nodejs` and `pnpm` if you are building from a git checkout
- Install `libxml2` development headers if you want to link against the system libxml2

On Debian/Ubuntu, that usually means:

```bash
sudo apt-get install \
  autoconf automake libtool patch pkg-config jq make perl \
  zlib1g-dev liblz4-dev libp11-kit-dev nettle-dev libgnutls28-dev libgmp-dev libssl-dev libxml2-dev \
  libsecret-1-dev libayatana-appindicator3-dev libwebkit2gtk-4.0-dev \
  gnome-keyring
```

On RHEL 9 / Rocky Linux 9 / AlmaLinux 9 / CentOS Stream 9, that usually means:

```bash
sudo dnf install \
  autoconf automake libtool patch pkgconf-pkg-config jq make perl gcc-c++ \
  zlib-devel lz4-devel p11-kit-devel nettle-devel gnutls-devel gmp-devel openssl-devel libxml2-devel \
  libsecret libappindicator-gtk3 webkit2gtk4.0-devel
```

If your distro uses different package names, check `packaging/deb/control.in` and `packaging/rpm/globalprotect-openconnect.spec.in` for the dependency lists used by the project packaging.

### Build

#### From a git checkout

Use the default build if you have `nodejs` and `pnpm` installed:

```bash
make build
sudo make install
```

#### From the release source tarball

The release source tarball already includes the generated frontend assets, so you can skip the frontend build:

```bash
tar -xzf globalprotect-openconnect-${version}.tar.gz
cd globalprotect-openconnect-${version}
make build BUILD_FE=0
sudo make install
```

#### Build with vendored libxml2

By default, the build links against the system `libxml2`.
If you also want to build libxml2 from the vendored source, use:

```bash
make build LIBXML2_STATIC=1
```

Or, when building from the release tarball:

```bash
make build BUILD_FE=0 LIBXML2_STATIC=1
```

## FAQ

1. How to deal with error `Secure Storage not ready`

   Try upgrade the client to `2.2.0` or later, which will use a file-based storage as a fallback.

   You need to install the `gnome-keyring` package, and restart the system (See [#321](https://github.com/yuezk/GlobalProtect-openconnect/issues/321), [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)).

2. How to deal with error `(gpauth:18869): Gtk-WARNING **: 10:33:37.566: cannot open display:`

   If you encounter this error when using the CLI version, try to run the command with `sudo -E` (See [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)).

## About Trial

The CLI version is always free, while the GUI version is paid. There are two trial modes for the GUI version:

1. 10-day trial: You can use the GUI stable release for 10 days after the installation.
2. 14-day trial: Each beta release has a fresh trial period (at most 14 days) after released.

## License

- crate [gpapi](./crates/gpapi): [MIT](./crates/gpapi/LICENSE)
- crate [openconnect](./crates/openconnect): [GPL-3.0](./crates/openconnect/LICENSE)
- crate [common](./crates/common): [GPL-3.0](./crates/common/LICENSE)
- app [gpservice](./apps/gpservice): [GPL-3.0](./apps/gpservice/LICENSE)
- app [gpclient](./apps/gpclient): [GPL-3.0](./apps/gpclient/LICENSE)
- app [gpauth](./apps/gpauth): [GPL-3.0](./apps/gpauth/LICENSE)
- app [gpgui-helper](./apps/gpgui-helper): [GPL-3.0](./apps/gpgui-helper/LICENSE)
