# GlobalProtect-openconnect
A GlobalProtect VPN client (GUI) for Linux based on Openconnect and built with Qt5, supports SAML auth mode, inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img src="https://user-images.githubusercontent.com/3297602/133869036-5c02b0d9-c2d9-4f87-8c81-e44f68cfd6ac.png">
</p>

## Features

- Similar user experience as the official client in macOS.
- Supports both SAML and non-SAML authentication modes.
- Supports automatically selecting the preferred gateway from the multiple gateways.
- Supports switching gateway from the system tray menu manually.

## Future plan

- [ ] Improve the release process
- [ ] Process bugs and feature requests
- [ ] Support for bypassing the `gpclient` parameters
- [ ] Support the CLI mode

## Passing the Custom Parameters to `OpenConnect` CLI

Custom parameters can be appended to the `OpenConnect` CLI with the following settings.

> Tokens with spaces can be surrounded by double quotes; three consecutive double quotes represent the quote character itself.


<p align="center">
  <img src="https://user-images.githubusercontent.com/3297602/130319209-744be02b-d657-4f49-a76d-d2c81b5c46d5.png" />
<p>
  
## Display the system tray icon on Gnome 40

Install the [AppIndicator and KStatusNotifierItem Support](https://extensions.gnome.org/extension/615/appindicator-support/) extension and you will see the system try icon (Restart the system after the installation).

<p align="center">
  <img src="https://user-images.githubusercontent.com/3297602/130831022-b93492fd-46dd-4a8e-94a4-13b5747120b7.png" />
<p>
 
## Prerequisites

- Openconnect v8.x
- Qt5, qt5-webengine, qt5-websockets

## Build & Install

Clone this repo with:

```sh
git clone https://github.com/yuezk/GlobalProtect-openconnect.git
cd GlobalProtect-openconnect
```

### Arch/Manjaro
Install from the [globalprotect-openconnect](https://aur.archlinux.org/packages/globalprotect-openconnect/) AUR.
### Ubuntu
For **Ubuntu 18.04**, add this [dwmw2/openconnect](https://launchpad.net/~dwmw2/+archive/ubuntu/openconnect) PPA first to install the latest openconnect.

```sh
sudo add-apt-repository ppa:dwmw2/openconnect
sudo apt update
```
...then build and install with:

```sh
./scripts/install-ubuntu.sh
```
### openSUSE

Build and install with:

```sh
./scripts/install-opensuse.sh
```

### Fedora

Build and install with:

```sh
./scripts/install-fedora.sh
```

### Other Linux

Install the Qt5 dependencies and OpenConnect:

- QtCore
- QtWebEngine
- QtWebSockets
- QtDBus
- openconnect v8.x

...then build and install with:

```sh
./scripts/install.sh
```

### Debian package

Relatively manual process for now:

* Clone the source tree

  ```
  git clone https://github.com/yuezk/GlobalProtect-openconnect.git
  cd GlobalProtect-openconnect
  ```

* Install git-archive-all using the pip. Remember to adjust the version numbers etc.

  ```
  pip install git-archive-all
  ```

* Next create an upstream source tree using git archive.

  ```
  git-archive-all --force-submodules --prefix=globalprotect-openconnect-1.3.0/ ../globalprotect-openconnect_1.3.0.orig.tar.gz
  ```

* Finally extract the source tree, build the debian package, and install it.

  ```
  cd ..
  tar -xzvf globalprotect-openconnect_1.3.0.orig.tar.gz
  cd globalprotect-openconnect-1.3.0
  fakeroot dpkg-buildpackage -uc -us -sa 2>&1 | tee ../build.log
  sudo dpkg -i globalprotect-openconnect_1.3.0-1ppa1_amd64.deb
  ```

### NixOS
  In `configuration.nix`:

  ```
  services.globalprotect = {
    enable = true;
    # if you need a Host Integrity Protection report
    csdWrapper = "${pkgs.openconnect}/libexec/openconnect/hipreport.sh";
  };
  
  environment.systemPackages = [ globalprotect-openconnect ];
  ```
  

## [License](./LICENSE)
GPLv3
