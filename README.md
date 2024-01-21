# GlobalProtect-openconnect

A GUI for GlobalProtect VPN, based on OpenConnect, supports the SSO authentication method. Inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img width="300" src="https://github.com/yuezk/GlobalProtect-openconnect/assets/3297602/9242df9c-217d-42ab-8c21-8f9f69cd4eb5">
</p>

## Features

- [x] Better Linux support
- [x] Support both CLI and GUI
- [x] Support both SSO and non-SSO authentication
- [x] Support multiple portals
- [x] Support gateway selection
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

### GUI

The GUI version is also available after you installed it. You can launch it from the application menu or run `gpclient launch-gui` in the terminal.

> [!Note]
>
> The GUI version is partially open source. Its background service is open sourced in this repo as [gpservice](./apps/gpservice/). The GUI part is a wrapper of the background service, which is not open sourced.

## Installation

> [!Note]
>
> This instruction is for the 2.x version. The 1.x version is still available on the [1.x](https://github.com/yuezk/GlobalProtect-openconnect/tree/1.x) branch, you can build it from the source code by following the instructions in the `README.md` file.

### Debian/Ubuntu based distributions

#### Install from PPA

```
sudo add-apt-repository ppa:yuezk/globalprotect-openconnect
sudo apt-get update
sudo apt-get install globalprotect-openconnect
```

> [!Note]
>
> For Linux Mint, you might need to import the GPG key with: `sudo apt-key adv --keyserver keyserver.ubuntu.com --recv-keys 7937C393082992E5D6E4A60453FC26B43838D761` if you encountered an error `gpg: keyserver receive failed: General error`.

#### Install from deb package

Download the latest deb package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page. Then install it with `dpkg`:

```bash
sudo dpkg -i globalprotect-openconnect_*.deb
```

### Arch Linux / Manjaro

#### Install from AUR

Install from AUR: [globalprotect-openconnect-git](https://aur.archlinux.org/packages/globalprotect-openconnect-git/)

#### Install from package

Download the latest package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page. Then install it with `pacman`:

```bash
sudo pacman -U globalprotect-openconnect-*.pkg.tar.zst
```

### Fedora/OpenSUSE/CentOS/RHEL

#### Install from COPR

The package is available on [COPR](https://copr.fedorainfracloud.org/coprs/yuezk/globalprotect-openconnect/) for various RPM-based distributions. You can install it with the following commands:

```
sudo dnf copr enable yuezk/globalprotect-openconnect
sudo dnf install globalprotect-openconnect
```

#### Install from OBS

The package is also available on [OBS](https://build.opensuse.org/package/show/home:yuezk/globalprotect-openconnect) for various RPM-based distributions. You can follow the instructions [on this page](https://software.opensuse.org//download.html?project=home%3Ayuezk&package=globalprotect-openconnect) to install it.

#### Install from RPM package

Download the latest RPM package from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.

### Other distributions

The project depends on `openconnect`, `webkit2gtk`, `libsecret`, `libayatana-appindicator` or `libappindicator-gtk3`. You can install them first and then download the latest binary release (i.e., `*.bin.tar.gz`) from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.

## [License](./LICENSE)

GPLv3
