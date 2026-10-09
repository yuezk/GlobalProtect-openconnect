# GlobalProtect-openconnect

A GlobalProtect VPN client built on OpenConnect with support for SSO authentication. It provides command-line tools for Linux, FreeBSD, and OpenBSD, and the GP Connect desktop application for macOS, Linux, FreeBSD, and OpenBSD.

<p align="center">
  <img width="300" src="https://github.com/user-attachments/assets/2fb6116c-dc57-43f2-af75-9c3d97ab7122">
</p>

> **Inspired by** [gp-saml-gui](https://github.com/dlenski/gp-saml-gui)

## Table of Contents

- [Features](#features)
- [Usage](#usage)
  - [Command-Line Interface](#command-line-interface)
  - [Graphical User Interface](#graphical-user-interface)
- [Installation](#installation)
  - [macOS](#macos)
  - [Debian / Ubuntu](#debian--ubuntu)
  - [Arch Linux / Manjaro](#arch-linux--manjaro)
  - [Fedora 38+ / Rawhide](#fedora-38--rawhide)
  - [openSUSE Leap 15.6+ / Tumbleweed](#opensuse-leap-156--tumbleweed)
  - [Other RPM-based Distributions](#other-rpm-based-distributions)
  - [Alpine Linux](#alpine-linux)
  - [FreeBSD](#freebsd)
  - [OpenBSD](#openbsd)
  - [Gentoo](#gentoo)
  - [NixOS](#nixos)
  - [Official Docker Image](#official-docker-image)
  - [Other Distributions](#other-distributions)
- [Building from Source](#building-from-source)
- [Frequently Asked Questions](#frequently-asked-questions)
- [NetworkManager Integration](#networkmanager-integration)
- [Licensing](#licensing)

## Features

- **Desktop Platform Support** – macOS, Linux, FreeBSD, and OpenBSD
- **Dual Interface** – Available as both CLI and GUI applications
- **Flexible Authentication** – Supports SSO, non-SSO, FIDO2 (e.g., YubiKey), and client certificate authentication
- **Browser Integration** – Authenticate using your default browser or any specified browser
- **Multi-Portal Support** – Connect to multiple portals and gateways
- **Direct Gateway Connection** – Bypass portal selection when needed
- **Auto-Connect** – Automatically connect on system startup
- **System Tray Integration** – Convenient system tray icon (requires [gnome-shell-extension-appindicator](https://extensions.gnome.org/extension/615/appindicator-support/) on GNOME)

## Usage

### Command-Line Interface

The CLI is free and open source. Use it to authenticate, connect to portals or gateways, and generate HIP reports from the terminal.

#### Basic Commands

```bash
Usage: gpclient [OPTIONS] <COMMAND>

Commands:
  connect     Connect to a portal server
  disconnect  Disconnect from the server
  launch-gui  Launch the GUI
  hip         Generate HIP report
  help        Print this message or the help of the given subcommand(s)

Options:
      --fix-openssl              Uses extended compatibility mode for OpenSSL operations to support a broader range of systems and formats.
      --ignore-tls-errors        Ignore the TLS errors
      --lock-file <LOCK_FILE>    Path to the gpclient PID lock file
      --log-format <LOG_FORMAT>  Log output format. JSON is intended for non-interactive consumers; interactive prompts remain text. [default: text] [possible values: text, json]
  -v, --verbose...               Enable verbose output, -v for debug, -vv for trace
  -q, --quiet...                 Decrease logging verbosity, -q for warnings, -qq for errors
  -h, --help                     Print help (see more with '--help')
  -V, --version                  Print version
```

> **Tip:** Use `gpclient help <command>` for detailed information on a specific command.

#### HIP reports

HIP is disabled in the CLI unless enabled explicitly. Use `--hip` to generate reports inside `gpclient`, including later reports requested by the gateway:

```bash
sudo gpclient connect <portal> --hip
```

Use `--hip=/absolute/path/to/script` for a custom executable. It runs as the `gpclient` process user unless `--hip-user` specifies another user. No installed HIP script is discovered automatically. `--hip-user` applies only to custom executables.

`gpclient hip` generates a report on demand for inspection; see `gpclient hip --help` for report inputs. The GUI's HIP tab supports generated reports, edited XML, and HIP Off on all desktop platforms. Linux, FreeBSD, and OpenBSD also support desktop-user scripts and explicitly approved root scripts.

#### External Browser Authentication

Use `gpclient` directly for browser-based authentication. It opens the browser as your normal desktop user while keeping VPN tunnel setup privileged:

```bash
sudo gpclient connect <portal> --browser
```

To reuse a saved portal cookie for later connections, add `--cookie-cache`:

```bash
sudo gpclient connect <portal> --browser --cookie-cache
```

Add `--gateway <gateway>` to select a particular gateway. Cookie caching is handled by `gpclient`. Its default cache file is `$HOME/.config/gpclient/cookie.json`, which normally belongs to root when running with `sudo`. To choose a predictable location, specify the path explicitly:

```bash
sudo gpclient connect <portal> --browser --cookie-cache=/absolute/path/to/cookie.json
```

**Browser Options:**

- Use `--browser` to try your recognized default browser first, then another detected browser, and finally the system URL handler
- Use `--browser default` to use the system default browser
- Use `--browser <browser>` to specify a browser (e.g., `firefox`, `chrome`)
- Use `--browser remote` for headless servers – this provides a URL you can access from another machine to complete authentication

### Graphical User Interface

GP Connect provides a desktop interface for managing VPN connections. On macOS, open GP Connect from Applications. On Linux, FreeBSD, and OpenBSD, launch it from your application menu or via the terminal:

```bash
gpclient launch-gui
```

> [!Note]
>
> The GUI version is partially open source. The background service ([gpservice](./apps/gpservice/)) is open source, while the GUI wrapper is proprietary.

## Installation

> [!Note]
>
> For older Linux distributions, use [v2.3.13](https://github.com/yuezk/GlobalProtect-openconnect/releases/tag/v2.3.13) instead of the latest release. It provides release assets for common distro families, including Debian/Ubuntu (`.deb`), Arch Linux / Manjaro (`.pkg.tar.zst`), RPM-based distros such as Fedora / RHEL / Rocky / AlmaLinux / CentOS (`.rpm`), and generic Linux tarballs (`.bin.tar.xz`).

### macOS

Download the GP Connect `.dmg` from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page, open it, and drag GP Connect into Applications. The macOS build requires Apple silicon and macOS 13 or later.

### Debian / Ubuntu

#### Option 1: Install from PPA (Recommended)

```bash
sudo add-apt-repository ppa:yuezk/globalprotect-openconnect
sudo apt-get update
sudo apt-get install globalprotect-openconnect
```

> [!Note]
>
> **For Linux Mint users:** If you encounter a GPG key error, import the key manually:
> ```bash
> sudo apt-key adv --keyserver keyserver.ubuntu.com --recv-keys 7937C393082992E5D6E4A60453FC26B43838D761
> ```

#### Option 2: Install from DEB Package

Download the latest `.deb` package from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page, then install:

```bash
sudo apt install --fix-broken ./globalprotect-openconnect_*.deb
```

### Arch Linux / Manjaro

#### Option 1: Install from AUR

Package: [globalprotect-openconnect-git](https://aur.archlinux.org/packages/globalprotect-openconnect-git/)

You can install it using an AUR helper like [`yay`](https://github.com/Jguer/yay):

```bash
yay -S globalprotect-openconnect-git
```

#### Option 2: Install from the Official Extra Repository

The package is also available in the official Arch Linux Extra repository.

Package: [globalprotect-openconnect](https://archlinux.org/packages/extra/x86_64/globalprotect-openconnect/)

> [!Note]
>
> Since the official package does not include the system tray support dependency, you need to install `libappindicator` manually:

```bash
sudo pacman -S libappindicator globalprotect-openconnect
```

#### Option 3: Install from Package

Download the latest package from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page, then install:

```bash
sudo pacman -U globalprotect-openconnect-*.pkg.tar.zst
```

### Fedora 38+ / Rawhide

#### Install from COPR

The package is available on [COPR](https://copr.fedorainfracloud.org/coprs/yuezk/globalprotect-openconnect/) for RPM-based distributions:

```bash
sudo dnf copr enable yuezk/globalprotect-openconnect
sudo dnf install globalprotect-openconnect
```

### openSUSE Leap 15.6+ / Tumbleweed

#### Install from OBS (openSUSE Build Service)

Packages are available on the [openSUSE Build Service](https://build.opensuse.org/package/show/home:yuezk/globalprotect-openconnect). Follow the [installation instructions](https://software.opensuse.org//download.html?project=home%3Ayuezk&package=globalprotect-openconnect) for your distribution.

### Other RPM-based Distributions

#### Install from RPM Package

Download the latest RPM package from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page:

```bash
sudo rpm -i globalprotect-openconnect-*.rpm
```

### Alpine Linux

Download the latest `.apk` package from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page, then install:

```bash
sudo apk add --allow-untrusted globalprotect-openconnect-*.apk
```

The package uses Alpine's native musl build. Make sure the `community` repository is enabled so GUI dependencies such as `webkit2gtk-4.1`, `libsecret`, and `libayatana-appindicator` can be resolved. GUI-launched connections use polkit, and VPN tunnel creation requires `/dev/net/tun`.

### FreeBSD

Download the latest FreeBSD package from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page, then install:

```bash
sudo pkg install ./globalprotect-openconnect-*-freebsd-*.pkg
```

To build from source, see [Building from Source on FreeBSD and OpenBSD](./docs/bsd-source-build.md).

### OpenBSD

Download the latest OpenBSD package from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page, then install:

```bash
doas pkg_add -D unsigned ./globalprotect-openconnect-*-openbsd-*.tgz
```

To build from source, see [Building from Source on FreeBSD and OpenBSD](./docs/bsd-source-build.md).

### Gentoo

Available via the `guru` and `lamdness` overlays:

```bash
sudo eselect repository enable guru
sudo emerge --sync guru
sudo emerge --ask --verbose net-vpn/GlobalProtect-openconnect
```

### NixOS

This repository includes a flake for NixOS integration.

#### Installation Steps

Add the flake input and NixOS module to your `flake.nix`:

The module builds the package with your NixOS configuration's `pkgs`, so the
GlobalProtect GUI and the system use the same Nixpkgs dependency versions.
It also enables Polkit and its setuid `pkexec` wrapper for privileged operations.

```nix
{
  inputs = {
    # ... other inputs
    globalprotect-openconnect.url = "github:yuezk/GlobalProtect-openconnect";
  };

  outputs = { self, nixpkgs, globalprotect-openconnect, ... }:
    let
      system = "x86_64-linux"; # or "aarch64-linux" for ARM64
      hostname = "<your-host>";
    in {
      nixosConfigurations.${hostname} = nixpkgs.lib.nixosSystem {
        inherit system;

        modules = [
          ./configuration.nix
          globalprotect-openconnect.nixosModules.default
          {
            programs.globalprotect-openconnect.enable = true;

            environment.systemPackages = [
              nixpkgs.legacyPackages.${system}.gnomeExtensions.appindicator
            ];
          }
        ];
      };
    };
}
```

Apply:

```bash
sudo nixos-rebuild switch
loginctl terminate-user "$USER"
```

After logging back in to GNOME, enable AppIndicator support if needed:

```bash
gnome-extensions enable appindicatorsupport@rgcjonas.gmail.com
```

NixOS does not run an imperative package-uninstall hook when a package is
removed from the system configuration. Remove an installed replacement VPN
script from the app settings before removing the package.

The `prebuilt` package and NixOS module use the published release pinned in `flake.nix`. Both `prebuilt` and `fromSource` run natively on the host with packaged dependencies and installer paths. The `fromSource` package builds the current flake checkout, including its Git submodules:

```bash
git submodule update --init --recursive
nix build "git+file://$PWD?submodules=1#fromSource"
```

Local source builds require initialized Git submodules and a flake URL with `submodules=1`, as shown above. For a remote source flake, use a Git URL with submodules enabled, such as `git+https://github.com/yuezk/GlobalProtect-openconnect?submodules=1#fromSource`. The `prebuilt` package and NixOS module do not require Git submodules. Build output (`target`, `.build`, and `node_modules`) is excluded from the source input. Updating release asset hashes changes the prebuilt package, independently of source builds.

### Official Docker Image

The [official Docker image](https://hub.docker.com/r/yuezk/globalprotect-openconnect) provides the CLI tools on Alpine Linux:

```bash
docker pull yuezk/globalprotect-openconnect:<version>
```

Release images are tagged as `vX.Y.Z`, `X.Y.Z`, and `latest`.

Run it with access to the TUN device:

```bash
docker run --rm -it --cap-add=NET_ADMIN --device=/dev/net/tun \
  yuezk/globalprotect-openconnect:<version> \
  connect <portal>
```

For browser authentication in a headless environment, use remote browser authentication:

```bash
docker run --rm -it --cap-add=NET_ADMIN --device=/dev/net/tun \
  yuezk/globalprotect-openconnect:<version> \
  connect <portal> --browser remote
```

On a host with multiple network interfaces, use `--browser-listen` to select the IP address reachable from the browser machine:

```bash
gpclient connect <portal> --browser remote --browser-listen 192.168.107.15
```

On a Linux host, add host networking if the VPN routes should affect the host network namespace:

```bash
docker run --rm -it --network host --cap-add=NET_ADMIN --device=/dev/net/tun \
  yuezk/globalprotect-openconnect:<version> \
  connect <portal> --browser remote
```

Without `--network host`, the VPN connection stays inside the container's network namespace. Docker Desktop on macOS and Windows does not make the host use the VPN through `--network host`; run `gpclient` on the host or use a container gateway setup for host traffic.

The image includes `gpclient` and `gpauth` only. It does not include embedded webview authentication, `gpgui-helper`, or `gpgui`.

### Other Distributions

#### Manual Installation

1. **Install dependencies:**
   - WebKitGTK 4.1 (for example, `libwebkit2gtk-4.1-0` on Debian/Ubuntu)
   - `libsecret`
   - `libayatana-appindicator` or `libappindicator-gtk3`
   - polkit (`pkexec`) for GUI-launched VPN connections

2. **Download and extract:**
   Download `globalprotect-openconnect_${version}_${arch}.bin.tar.xz` from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page:
   ```bash
   tar -xJf globalprotect-openconnect_${version}_${arch}.bin.tar.xz
   cd globalprotect-openconnect_${version}
   ```

3. **Install:**
   ```bash
   sudo make install
   ```

## Building from Source

The instructions below build the open-source components on Linux. For FreeBSD and OpenBSD, see [Building from Source on FreeBSD and OpenBSD](./docs/bsd-source-build.md).

### Method 1: Using DevContainer

This project includes a DevContainer configuration for Linux development. The repository's `rust-toolchain.toml` selects the Rust toolchain; rustup may need to download it on the first build.

#### Prerequisites

- [Docker](https://docs.docker.com/get-docker/)
- [Visual Studio Code](https://code.visualstudio.com/) (optional, for IDE support)
- [Dev Containers extension](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) (if using VS Code)

#### Build Steps

1. **Clone the repository:**
   ```bash
   git clone https://github.com/yuezk/GlobalProtect-openconnect.git
   cd GlobalProtect-openconnect
   git submodule update --init --recursive
   ```

2. **Build the DevContainer image:**
   ```bash
   docker build -t gpoc-devcontainer .devcontainer/
   ```

3. **Build the project:**

   To build the open-source components, including the GUI helper, run:
   ```bash
   docker run --privileged --cap-add=NET_ADMIN --device=/dev/net/tun \
     --tty -v "$(pwd)":/workspace -w /workspace gpoc-devcontainer \
     bash -c "export PATH=/usr/local/cargo/bin:\$PATH && make build"
   ```
   To build without the GUI helper run the same command as above, but with `BUILD_GUI_HELPER=0` passed as an argument to `make`.

4. **Locate build artifacts:**

   The compiled binaries will be available in `target/release/`:
   - `gpclient` – CLI client
   - `gpservice` – Background service
   - `gpauth` – Authentication helper
   - `gpgui-helper` – GUI helper
   - `gp-hip-script-installer` – Privileged HIP script installer
   - `gp-vpnc-script-installer` – Privileged VPN script installer

   The proprietary GP Connect desktop application is distributed separately. Use `INCLUDE_GUI=1` with `make build` to download its matching published Linux build for installation.

#### Alternative: Using VS Code

1. Open the project in VS Code
2. When prompted, click "Reopen in Container" (or run **Dev Containers: Reopen in Container**)
3. Once the container is ready, open a terminal and run:
   ```bash
   make build
   ```

### Method 2: Local Development Build

#### Prerequisites

- [Rust 1.91 or later](https://www.rust-lang.org/tools/install)
- [Tauri dependencies](https://tauri.app/start/prerequisites/)
- OpenConnect source-build dependencies: `autoconf`, `automake`, `autopoint`/`gettext`, `libtool`, `patch`, `pkg-config`, `libxml2`, `zlib`, `lz4`, `gnutls`, `p11-kit`, `nettle`, and `gmp` development packages
- `pkexec` and `gnome-keyring` (or `pam_kwallet` on KDE)
- Node.js and pnpm if rebuilding the GUI helper frontend

#### Build Steps

1. **Download source code:**

   Download `globalprotect-openconnect-${version}.tar.gz` from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.

2. **Extract and build:**
   ```bash
   tar -xzf globalprotect-openconnect-${version}.tar.gz
   cd globalprotect-openconnect-${version}
   make build
   ```

3. **Install:**
   ```bash
   sudo make install
   ```

   To stage the files for packaging instead of installing them into the system, use `make install DESTDIR=/absolute/path/to/staging`.

### Testing Your Build

Verify the CLI client is working correctly:

```bash
./target/release/gpclient --help
```

### Build Options

- `BUILD_GUI_HELPER=0` – Skip building and installing the GUI helper
- `BUILD_WEBVIEW_AUTH=0` – Build `gpauth` without embedded browser authentication; use an external or remote browser
- `INCLUDE_GUI=1` – Download the matching published Linux desktop application for installation
- `OFFLINE=1` – Build in offline mode using vendored dependencies

## Frequently Asked Questions

### Q: How do I resolve the "Secure Storage not ready" error?

Unlock your desktop keyring and ensure the application can access it. On Linux and BSD, check that a keyring service such as GNOME Keyring is installed and running in your desktop session. On macOS, check Keychain access.

Linux and BSD can use file storage when secure storage is unavailable during initial setup. An existing configuration that uses the keyring still needs access to that keyring.

See related issues: [#321](https://github.com/yuezk/GlobalProtect-openconnect/issues/321), [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)

### Q: How do I fix the "cannot open display" error when using CLI?

Run the client from a terminal in your normal desktop session and use an external browser:

```bash
sudo gpclient connect <portal> --browser
```

See related issue: [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)

On a headless server, use `--browser remote` and complete authentication from another machine.

## NetworkManager Integration

- [WMP/GlobalProtect-SAML-NetworkManager](https://github.com/WMP/GlobalProtect-SAML-NetworkManager) – NetworkManager VPN plugin with SAML/SSO support for GNOME and KDE Plasma
- [jdpipe/networkmanager-gpclient](https://github.com/jdpipe/networkmanager-gpclient) – NetworkManager VPN plugin that uses `gpclient`

## Licensing

### Trial and Pricing

The **CLI version** is completely free and open source.
The **GUI version** is a paid application with a **7-day trial**. Start the trial in the app by verifying your email address with a one-time code. Installing the app alone does not start the trial.

### Open Source Licenses

This project consists of multiple components, each with its own license:

| Component | Type | License |
|-----------|------|---------|
| [gpapi](./crates/gpapi) | Crate | [MIT](./crates/gpapi/LICENSE) |
| [openconnect](./crates/openconnect) | Rust wrapper crate | [MIT](./crates/openconnect/LICENSE) |
| [upstream OpenConnect](./crates/openconnect/deps/openconnect) | C library | [LGPL-2.1](./crates/openconnect/deps/openconnect/COPYING.LGPL) |
| [common](./crates/common) | Crate | [MIT](./crates/common/LICENSE) |
| [browser-launcher](./crates/browser-launcher) | Crate | [MIT](./crates/browser-launcher/LICENSE) |
| [auth](./crates/auth) | Crate | [GPL-3.0](./crates/auth/LICENSE) |
| [gpservice](./apps/gpservice) | Application | [GPL-3.0](./apps/gpservice/LICENSE) |
| [gpclient](./apps/gpclient) | Application | [GPL-3.0](./apps/gpclient/LICENSE) |
| [gpauth](./apps/gpauth) | Application | [GPL-3.0](./apps/gpauth/LICENSE) |
| [gpgui-helper](./apps/gpgui-helper) | Application | [GPL-3.0](./apps/gpgui-helper/LICENSE) |
