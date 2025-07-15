# GlobalProtect-openconnect

A GUI for GlobalProtect VPN, based on OpenConnect, supports the SSO authentication method. Inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img width="300" src="https://github.com/yuezk/GlobalProtect-openconnect/assets/3297602/9242df9c-217d-42ab-8c21-8f9f69cd4eb5">
</p>

## 📚 Documentation

Complete documentation is now available in the [`docs/`](docs/) directory:

- **[📖 Documentation Index](docs/index.adoc)** - Project overview and navigation
- **[👨‍💻 Developer's Guide](docs/developers-guide.adoc)** - Complete development workflow with pixi
- **[🛠️ Operator's Guide](docs/operators-guide.adoc)** - Production deployment and operations
- **[🗺️ Project Roadmap](docs/roadmap.adoc)** - Strategic roadmap with actionable tasks

### Quick Documentation Links

- **New to the project?** Start with the [Documentation Index](docs/index.adoc)
- **Want to contribute?** Read the [Developer's Guide](docs/developers-guide.adoc)
- **Deploying in production?** See the [Operator's Guide](docs/operators-guide.adoc)
- **Planning contributions?** Check the [Project Roadmap](docs/roadmap.adoc)

## 🎯 Current Status (v2.4.4)

**✅ CLI Components Fully Production Ready**
- All CLI tools (gpclient, gpservice, gpauth) are fully functional
- Modern pixi-based development environment with conda-forge integration
- Professional conda packaging ready for distribution
- Comprehensive testing with 100% pass rate
- Complete documentation suite

**🔄 GUI Components In Development**
- Working on resolving WebKit2GTK-4.1 dependency for conda-forge compatibility
- Researching alternative GUI frameworks (see [roadmap](docs/roadmap.adoc))

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

#### Install from PPA

```
sudo add-apt-repository ppa:yuezk/globalprotect-openconnect
sudo apt-get install globalprotect-openconnect
```

> [!Note]
>
> For Linux Mint, you might need to import the GPG key with: `sudo apt-key adv --keyserver keyserver.ubuntu.com --recv-keys 7937C393082992E5D6E4A60453FC26B43838D761` if you encountered an error `gpg: keyserver receive failed: General error`.

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

```bash
sudo rpm -i globalprotect-openconnect-*.rpm
```

### Gentoo

It is available via `guru` and `lamdness` overlays.

```bash
sudo eselect repository enable guru
sudo emerge -r guru sync
sudo emerge -av net-vpn/GlobalProtect-openconnect
```

### Other distributions

- Install `openconnect >= 8.20`, `webkit2gtk`, `libsecret`, `libayatana-appindicator` or `libappindicator-gtk3`.
- Download `globalprotect-openconnect_${version}_${arch}.bin.tar.xz` from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page.
- Extract the tarball with `tar -xJf globalprotect-openconnect_${version}_${arch}.bin.tar.xz`.
- Run `sudo make install` to install the client.

## Build from source

**📖 For comprehensive build instructions, see the [Developer's Guide](docs/developers-guide.adoc)**

You can build the client from source using pixi (recommended for development), a devcontainer, or a local setup. Test scripts are available in the `tests/` directory to verify builds.

### Option 1: Using Pixi (Recommended for Development)

✅ **Current Status**: CLI build fully working with conda packaging

This project uses [pixi](https://pixi.sh/) for modern, reproducible development environments with conda-forge packages. The CLI components build successfully and are production-ready.

#### Prerequisites

- [Pixi](https://pixi.sh/latest/#installation) (cross-platform package manager)

#### Build Steps

1. Clone the repository:
   ```bash
   git clone https://github.com/yuezk/GlobalProtect-openconnect.git
   cd GlobalProtect-openconnect
   ```

2. Install dependencies:
   ```bash
   pixi install
   ```

3. Build the project:
   ```bash
   # Build CLI components (✅ fully working)
   pixi run build-cli
   
   # Test CLI functionality
   pixi run test-cli-comprehensive
   
   # Create conda package
   pixi run package-cli
   ```

4. The built binaries will be available in `target/release/`:
   - `gpclient` (4.0 MB) - CLI client ✅ Working
   - `gpservice` (3.9 MB) - Background service ✅ Working
   - `gpauth` (3.8 MB) - Authentication helper ✅ Working
   - `gpgui-helper` - GUI helper (🔄 In development)

5. Verify the build with comprehensive tests:
   ```bash
   # Run comprehensive test suite
   pixi run test-cli-comprehensive
   
   # Or run test scripts directly
   ./tests/test-cli-final.sh
   ```

#### WebKit Dependencies (GUI Build)

⚠️ **Note**: The full GUI build requires WebKit development packages that are not available in conda-forge. On immutable systems like Fedora Silverblue/Bluefin, you'll need to install system packages:

```bash
# Check WebKit dependencies
pixi run verify-webkit-deps

# For full GUI build, install system packages first:
sudo rpm-ostree install webkit2gtk4.1-devel gtk3-devel cairo-devel gdk-pixbuf2-devel pango-devel
sudo rpm-ostree apply-live  # Apply changes without reboot

# Verify packages are available
pixi run verify-webkit-deps

# Then build the full GUI
pixi run build
```

For detailed solutions, see the WebKit Dependencies section in [docs/developers-guide.adoc](docs/developers-guide.adoc).

**Recommended**: Use CLI-only build which works everywhere without additional dependencies.

#### Available Pixi Commands

```bash
# Setup development environment
pixi run setup

# Check WebKit dependencies (for GUI build)
pixi run verify-webkit-deps

# Build CLI components only
pixi run build-cli

# Build frontend (Node.js components)
pixi run build-frontend

# Build Rust components
pixi run build-rust

# Test the build
pixi run test

# Clean build artifacts
pixi run clean

# Format code
pixi run format

# Run linting
pixi run lint

# Create conda package with rattler-build (✅ working for CLI)
pixi run package-cli

# Complete CLI workflow (build + test + package)
pixi run ship-cli

# Test commands
pixi run test-cli-comprehensive  # Comprehensive test suite
./tests/test-cli-final.sh       # Direct test execution
```

#### Pixi Environments

The project supports multiple environments:

- `default` - Full build with GUI support
- `cli` - CLI-only build without GUI dependencies  
- `dev` - Development environment with additional tools

```bash
# Use CLI environment
pixi run -e cli build-cli

# Use development environment
pixi run -e dev build
```

#### Conda-Forge Packaging

✅ **Status**: CLI packaging fully working and ready for conda-forge submission

This project includes [rattler-build](https://github.com/prefix-dev/rattler-build) configuration for creating conda-forge compatible packages:

```bash
# Create CLI conda package (✅ working)
pixi run package-cli

# Or directly with rattler-build
rattler-build build --recipe recipe-cli.yaml
```

The generated conda package (3.7 MB) includes all CLI binaries and is ready for conda-forge submission.

### Option 2: Using DevContainer

This project includes a devcontainer configuration that provides a consistent build environment with all dependencies pre-installed.

#### Prerequisites

- [Docker](https://docs.docker.com/get-docker/)
- [VS Code](https://code.visualstudio.com/) with [Dev Containers extension](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) (optional, for IDE support)

#### Build Steps

1. Clone the repository:
   ```bash
   git clone https://github.com/yuezk/GlobalProtect-openconnect.git
   cd GlobalProtect-openconnect
   ```

2. Build the devcontainer image:
   ```bash
   docker build -t gpoc-devcontainer .devcontainer/
   ```

3. Install `jq` in the container and build the project:
   ```bash
   docker run --privileged --cap-add=NET_ADMIN --device=/dev/net/tun \
     -v "$(pwd)":/workspace -w /workspace --user root gpoc-devcontainer \
     bash -c "apt-get update && apt-get install -y jq"
   
   docker run --privileged --cap-add=NET_ADMIN --device=/dev/net/tun \
     -v "$(pwd)":/workspace -w /workspace gpoc-devcontainer \
     bash -c "export PATH=/usr/local/cargo/bin:\$PATH && make build"
   ```

4. The built binaries will be available in `target/release/`:
   - `gpclient` - CLI client
   - `gpservice` - Background service
   - `gpauth` - Authentication helper
   - `gpgui-helper` - GUI helper

#### Alternative: VS Code DevContainer

1. Open the project in VS Code
2. When prompted, click "Reopen in Container" or run the command "Dev Containers: Reopen in Container"
3. Once the container is built and running, open a terminal in VS Code and run:
   ```bash
   make build
   ```

### Option 3: Local Build

#### Prerequisites

- [Install Rust 1.80 or later](https://www.rust-lang.org/tools/install)
- Install Tauri dependencies: https://tauri.app/start/prerequisites/
- Install `perl` and `jq`
- Install `openconnect >= 8.20` and `libopenconnect-dev` (or `openconnect-devel` on RPM-based distributions)
- Install `pkexec`, `gnome-keyring` (or `pam_kwallet` on KDE)
- Install `nodejs` and `pnpm` (optional only if you downloaded the source tarball from the release page and run with the `BUILD_FE=0` flag, see below)

#### Build Steps

1. Download the source code tarball from [releases](https://github.com/yuezk/GlobalProtect-openconnect/releases) page. Choose `globalprotect-openconnect-${version}.tar.gz`.
2. Extract the tarball with `tar -xzf globalprotect-openconnect-${version}.tar.gz`.
3. Enter the source directory and run `make build BUILD_FE=0` to build the client.
4. Run `sudo make install` to install the client. (Note, `DESTDIR` is not supported)

### Testing the Build

After building, you can test the CLI client:

```bash
# Basic functionality test
./target/release/gpclient --help

# Comprehensive test suite
pixi run test-cli-comprehensive

# Direct test script execution
./tests/test-cli-final.sh
```

### Build Options

- `BUILD_GUI=0` - Disable GUI components (CLI only)
- `BUILD_FE=0` - Skip frontend build (use pre-built assets)
- `OFFLINE=1` - Build in offline mode using vendored dependencies

### Conda-Forge Packaging

✅ **Status**: CLI packaging fully working and ready for conda-forge submission

This project uses [rattler-build](https://github.com/prefix-dev/rattler-build) to create conda-forge compatible packages.

#### Creating a Conda Package

```bash
# CLI package (✅ working)
pixi run package-cli

# Or directly with rattler-build
rattler-build build --recipe recipe-cli.yaml
```

#### Package Structure

The CLI conda package (3.7 MB) includes:
- All CLI binary executables (`gpclient`, `gpservice`, `gpauth`)
- Proper conda metadata and dependencies
- Ready for conda-forge submission

#### Distribution

The generated CLI package:
- ✅ Ready for conda-forge submission
- ✅ Can be installed locally with `conda install`
- ✅ Available for private conda channels

For detailed packaging procedures, see the [Developer's Guide](docs/developers-guide.adoc) and [Operator's Guide](docs/operators-guide.adoc).

## FAQ

1. How to deal with error `Secure Storage not ready`

   Try upgrade the client to `2.2.0` or later, which will use a file-based storage as a fallback.

   You need to install the `gnome-keyring` package, and restart the system (See [#321](https://github.com/yuezk/GlobalProtect-openconnect/issues/321), [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)).

2. How to deal with error `(gpauth:18869): Gtk-WARNING **: 10:33:37.566: cannot open display:`

   If you encounter this error when using the CLI version, try to run the command with `sudo -E` (See [#316](https://github.com/yuezk/GlobalProtect-openconnect/issues/316)).

## About Trial

The CLI version is always free and open source, while the GUI version is paid. There are two trial modes for the GUI version:

1. 10-day trial: You can use the GUI stable release for 10 days after the installation.
2. 14-day trial: Each beta release has a fresh trial period (at most 14 days) after released.

**Note**: As of v2.4.4, the CLI components are fully functional and production-ready. GUI development is ongoing with framework research (see [roadmap](docs/roadmap.adoc)).

## Contributing

We welcome contributions! See the [Developer's Guide](docs/developers-guide.adoc) for development setup and the [Project Roadmap](docs/roadmap.adoc) for available tasks.

### Quick Links
- **🐛 Bug Reports**: [GitHub Issues](https://github.com/yuezk/GlobalProtect-openconnect/issues)
- **💡 Feature Requests**: [GitHub Discussions](https://github.com/yuezk/GlobalProtect-openconnect/discussions)
- **📖 Documentation**: [docs/](docs/) directory
- **🗺️ Roadmap**: [docs/roadmap.adoc](docs/roadmap.adoc)

## License

- crate [gpapi](./crates/gpapi): [MIT](./crates/gpapi/LICENSE)
- crate [openconnect](./crates/openconnect): [GPL-3.0](./crates/openconnect/LICENSE)
- crate [common](./crates/common): [GPL-3.0](./crates/common/LICENSE)
- crate [auth](./crates/auth): [GPL-3.0](./crates/auth/LICENSE)
- app [gpservice](./apps/gpservice): [GPL-3.0](./apps/gpservice/LICENSE)
- app [gpclient](./apps/gpclient): [GPL-3.0](./apps/gpclient/LICENSE)
- app [gpauth](./apps/gpauth): [GPL-3.0](./apps/gpauth/LICENSE)
- app [gpgui-helper](./apps/gpgui-helper): [GPL-3.0](./apps/gpgui-helper/LICENSE)
