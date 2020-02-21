# GlobalProtect-openconnect
A GlobalProtect VPN client (GUI) for Linux based on Openconnect and built with Qt5, supports SAML auth mode, inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img src="screenshot.png">
</p>

## Prerequisites

- Openconnect v8.x
- Qt5, qt5-webengine, qt5-websockets

### Ubuntu
1. Install openconnect v8.x
    Update openconnect to 8.x, for ubuntu 18.04 you might need to [build the latest openconnect from source code](https://gist.github.com/yuezk/ab9a4b87a9fa0182bdb2df41fab5f613).
2. Install the Qt dependencies
    ```sh
    sudo apt install qt5-default libqt5websockets5-dev qtwebengine5-dev
    ```
## Install

### Install from AUR (Arch/Manjaro)

Install [globalprotect-openconnect](https://aur.archlinux.org/packages/globalprotect-openconnect/).

### Build from source code

```sh
git clone https://github.com/yuezk/GlobalProtect-openconnect.git
cd GlobalProtect-openconnect
git submodule update --init
qmake CONFIG+=release
make
sudo make install
```
Open `GlobalProtect VPN` in the application dashboard.

## [License](./LICENSE)
GPLv3
