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

To use the default browser for authentication with the CLI version, you need to use the following command:

```bash
sudo -E gpclient connect --default-browser <portal>
```

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

#### **Ubuntu 24.04**

The `libwebkit2gtk-4.0-37` package was [removed](https://bugs.launchpad.net/ubuntu/+source/webkit2gtk/+bug/2061914) from its repo, before [the issue](https://github.com/yuezk/GlobalProtect-openconnect/issues/351) gets resolved, you need to install them manually:

```bash
wget http://launchpadlibrarian.net/704701349/libwebkit2gtk-4.0-37_2.43.3-1_amd64.deb
wget http://launchpadlibrarian.net/704701345/libjavascriptcoregtk-4.0-18_2.43.3-1_amd64.deb

sudo dpkg --install *.deb
```

And the latest package is not available in the PPA, you can follow the [Install from deb package](#install-from-deb-package) section to install the latest package.

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

```
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

```
sudo dnf copr enable yuezk/globalprotect-openconnect
sudo dnf install globalprotect-openconnect
```

### openSUSE Leap 15.6 / openSUSE Tumbleweed

#### Install from OBS (openSUSE Build Service)

The package is also available on [OBS](https://build.opensuse.org/package/show/home:yuezk/globalprotect-openconnect) for various RPM-based distributions. You can follow the instructions [on this page](https://software.opensuse.org//download.html?project=home%3Ayuezk&package=globalprotect-openconnect) to install it.

### Other RPM-based distributions

#### Install from RPM package

Download the latest RPM package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.

```bash
sudo rpm -i globalprotect-openconnect-*.rpm
```
### Gentoo

Install from the ```rios``` or ```slonko``` overlays.  Example using rios:

1. # Enable the overlay:
```
eselect repository enable rios
```

2. # Sync with the repository:

  - If you have eix installed, use it:
```
eix-sync
```
  - Otherwise, use:
```
emerge --sync
```
3. # Install:

```emerge globalprotect-openconnect```


### Other distributions

- Install `openconnect >= 8.20`, `webkit2gtk`, `libsecret`, `libayatana-appindicator` or `libappindicator-gtk3`.
- Download `globalprotect-openconnect_${version}_${arch}.bin.tar.xz` from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.
- Extract the tarball with `tar -xJf globalprotect-openconnect_${version}_${arch}.bin.tar.xz`.
- Run `sudo make install` to install the client.

## Build from source

You can also build the client from source, steps are as follows:

### Prerequisites

- [Install Rust](https://www.rust-lang.org/tools/install)
- Install Tauri dependencies: https://tauri.app/v1/guides/getting-started/prerequisites/#setting-up-linux
- Install `perl`
- Install `openconnect >= 8.20` and `libopenconnect-dev` (or `openconnect-devel` on RPM-based distributions)
- Install `pkexec`, `gnome-keyring` (or `pam_kwallet` on KDE)

### Build

1. Download the source code tarball from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page. Choose `globalprotect-openconnect-${version}.tar.gz`.
2. Extract the tarball with `tar -xzf globalprotect-openconnect-${version}.tar.gz`.
3. Enter the source directory and run `make build BUILD_FE=0` to build the client.
3. Run `sudo make install` to install the client. (Note, `DESTDIR` is not supported)

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

## [License](./LICENSE)

GPLv3
