# GlobalProtect-openconnect

A modern GlobalProtect VPN client for Linux, built on OpenConnect with full support for SSO authentication. This project provides both command-line and graphical interfaces for seamless VPN connectivity.

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
  - [Debian / Ubuntu](#debian--ubuntu)
  - [Arch Linux / Manjaro](#arch-linux--manjaro)
  - [Fedora 38+ / Rawhide](#fedora-38--rawhide)
  - [openSUSE Leap 15.6+ / Tumbleweed](#opensuse-leap-156--tumbleweed)
  - [Other RPM-based Distributions](#other-rpm-based-distributions)
  - [Gentoo](#gentoo)
  - [NixOS](#nixos)
  - [Other Distributions](#other-distributions)
- [Building from Source](#building-from-source)
- [Frequently Asked Questions](#frequently-asked-questions)
- [License](#license)

## Features

- **Cross-Platform Linux Support** – Optimized for various Linux distributions
- **Dual Interface** – Available as both CLI and GUI applications
- **Flexible Authentication** – Supports SSO, non-SSO, FIDO2 (e.g., YubiKey), and client certificate authentication
- **Browser Integration** – Authenticate using your default browser or any specified browser
- **Multi-Portal Support** – Connect to multiple portals and gateways
- **Direct Gateway Connection** – Bypass portal selection when needed
- **Auto-Connect** – Automatically connect on system startup
- **System Tray Integration** – Convenient system tray icon (requires [gnome-shell-extension-appindicator](https://extensions.gnome.org/extension/615/appindicator-support/) on GNOME)

## Usage

### Command-Line Interface

The CLI version is fully open source and feature-rich, providing nearly identical functionality to the GUI version.

#### Basic Commands

```bash
Usage: gpclient [OPTIONS] <COMMAND>

Commands:
  connect     Connect to a portal server
  disconnect  Disconnect from the server
  launch-gui  Launch the GUI
  help        Print this message or the help of the given subcommand(s)

Options:
      --fix-openssl        Get around the OpenSSL 'unsafe legacy renegotiation' error
      --ignore-tls-errors  Ignore TLS errors
  -h, --help               Print help
  -V, --version            Print version
```

> **Tip:** Use `gpclient help <command>` for detailed information on a specific command.

#### External Browser Authentication

For browser-based authentication with the CLI:

**Method 1:** Using sudo with environment preservation:
```bash
sudo -E gpclient connect --browser <portal>
```

**Method 2:** Using authentication piping:
```bash
gpauth <portal> --browser 2>/dev/null | sudo gpclient connect <portal> --cookie-on-stdin
```

**Browser Options:**
- Use `--browser <browser>` to specify a browser (e.g., `firefox`, `chrome`)
- Use `--browser remote` for headless servers – this provides a URL you can access from another machine to complete authentication

### Graphical User Interface

The GUI application provides an intuitive interface for managing VPN connections. Launch it from your application menu or via the terminal:

```bash
gpclient launch-gui
```

> [!Note]
>
> The GUI version is partially open source. The background service ([gpservice](./apps/gpservice/)) is open source, while the GUI wrapper is proprietary.

## Installation

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
sudo apt install --fix-broken globalprotect-openconnect_*.deb
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

1. Add the flake to your `flake.nix`:

    ```nix
    {
      inputs = {
        # ... other inputs
        globalprotect-openconnect.url = "github:yuezk/GlobalProtect-openconnect";
      };

      outputs = { nixpkgs, ... }@inputs: {
        nixosConfigurations.<your-host> = nixpkgs.lib.nixosSystem {
          specialArgs = { inherit inputs; };
          modules = [
            ./configuration.nix
          ];
        };
      };
    }
    ```

2. Add the package to your `configuration.nix`:

    ```nix
    { config, pkgs, inputs, ... }:

    {
      # ... other configurations
      environment.systemPackages = with pkgs; [
        # ... other packages
      ] ++ [
        inputs.globalprotect-openconnect.packages.${pkgs.system}.default
      ];
    }
    ```

3. Apply the changes:

    ```bash
    sudo nixos-rebuild switch
    ```

### Other Distributions

#### Manual Installation

1. **Install dependencies:**
   - `webkit2gtk`
   - `libsecret`
   - `libayatana-appindicator` or `libappindicator-gtk3`

2. **Download and extract:**
   Download `globalprotect-openconnect_${version}_${arch}.bin.tar.xz` from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page:
   ```bash
   tar -xJf globalprotect-openconnect_${version}_${arch}.bin.tar.xz
   ```

3. **Install:**
   ```bash
   sudo make install
   ```

## Building from Source

You can build the application from source using either a DevContainer (recommended) or a local development environment.

### Method 1: Using DevContainer (Recommended)

This project includes a DevContainer configuration that provides a consistent, reproducible build environment with all dependencies pre-installed.

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
   ```bash
   docker run --privileged --cap-add=NET_ADMIN --device=/dev/net/tun \
     -v "$(pwd)":/workspace -w /workspace gpoc-devcontainer \
     bash -c "export PATH=/usr/local/cargo/bin:\$PATH && make build"
   ```

4. **Locate build artifacts:**

   The compiled binaries will be available in `target/release/`:
   - `gpclient` – CLI client
   - `gpservice` – Background service
   - `gpauth` – Authentication helper
   - `gpgui-helper` – GUI helper

#### Alternative: Using VS Code

1. Open the project in VS Code
2. When prompted, click "Reopen in Container" (or run **Dev Containers: Reopen in Container**)
3. Once the container is ready, open a terminal and run:
   ```bash
   make build
   ```

### Method 2: Local Development Build

#### Prerequisites

- [Rust 1.85 or later](https://www.rust-lang.org/tools/install)
- [Tauri dependencies](https://tauri.app/start/prerequisites/)
- `libopenconnect-dev` (or `openconnect-devel` on RPM-based systems)
- `pkexec` and `gnome-keyring` (or `pam_kwallet` on KDE)
- `nodejs` and `pnpm` (optional if using pre-built release tarballs with `BUILD_FE=0`)

#### Build Steps

1. **Download source code:**

   Download `globalprotect-openconnect-${version}.tar.gz` from the [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.

2. **Extract and build:**
   ```bash
   tar -xzf globalprotect-openconnect-${version}.tar.gz
   cd globalprotect-openconnect-${version}
   make build BUILD_FE=0
   ```

3. **Install:**
   ```bash
   sudo make install
   ```

   > **Note:** `DESTDIR` is not currently supported.

### Testing Your Build

Verify the CLI client is working correctly:

```bash
./target/release/gpclient --help
```

### Build Options

- `BUILD_GUI=0` – Build CLI components only (excludes GUI)
- `BUILD_FE=0` – Skip frontend build (uses pre-built assets)
- `OFFLINE=1` – Build in offline mode using vendored dependencies

## Frequently Asked Questions

### Q: How do I resolve the "Secure Storage not ready" error?

**Solution 1:** Update to version 2.2.0 or later, which includes a file-based storage fallback.

**Solution 2:** Install the `gnome-keyring` package and restart your system.

See related issues: [#321](https://github.com/yuezk/GlobalProtect-openconnect/issues/321), [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)

### Q: How do I fix the "cannot open display" error when using CLI?

If you encounter `(gpauth:18869): Gtk-WARNING **: 10:33:37.566: cannot open display:`, try running the command with `sudo -E`:

```bash
sudo -E gpclient connect <portal>
```

See related issue: [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)

## Licensing

### Trial and Pricing

The **CLI version** is completely free and open source.
The **GUI version** is a paid application with a **10-day trial period** after installation.

### Open Source Licenses

This project consists of multiple components, each with its own license:

| Component | Type | License |
|-----------|------|---------|
| [gpapi](./crates/gpapi) | Crate | [MIT](./crates/gpapi/LICENSE) |
| [openconnect](./crates/openconnect) | Crate | [GPL-3.0](./crates/openconnect/LICENSE) |
| [common](./crates/common) | Crate | [GPL-3.0](./crates/common/LICENSE) |
| [auth](./crates/auth) | Crate | [GPL-3.0](./crates/auth/LICENSE) |
| [gpservice](./apps/gpservice) | Application | [GPL-3.0](./apps/gpservice/LICENSE) |
| [gpclient](./apps/gpclient) | Application | [GPL-3.0](./apps/gpclient/LICENSE) |
| [gpauth](./apps/gpauth) | Application | [GPL-3.0](./apps/gpauth/LICENSE) |
| [gpgui-helper](./apps/gpgui-helper) | Application | [GPL-3.0](./apps/gpgui-helper/LICENSE) |
