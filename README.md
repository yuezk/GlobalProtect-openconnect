# GlobalProtect-openconnect
A GlobalProtect VPN client (GUI) for Linux based on Openconnect and built with Qt5, supports SAML auth mode, inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img src="screenshot.png">
</p>

## Features

- Similar user experience as the official client in macOS.
- Supports both SAML and non-SAML authentication modes.
- Supports automatically selecting the preferred gateway from the multiple gateways.
- Supports switching gateway from the system tray menu manually.

## Prerequisites

- Openconnect v8.x
- Qt5, qt5-webengine, qt5-websockets

### Ubuntu
1. Install openconnect v8.x

   For Ubuntu 18.04 you might need to [build the latest openconnect from source code](https://gist.github.com/yuezk/ab9a4b87a9fa0182bdb2df41fab5f613).
   
2. Install the Qt dependencies
    ```sh
    sudo apt install qt5-default libqt5websockets5-dev qtwebengine5-dev qttools5-dev
    ```
### OpenSUSE
Install the Qt dependencies

```sh
sudo zypper install libqt5-qtbase-devel libqt5-qtwebsockets-devel libqt5-qtwebengine-devel
```

## Install

### Install from AUR (Arch/Manjaro)

Install [globalprotect-openconnect](https://aur.archlinux.org/packages/globalprotect-openconnect/).

### Build from source code

```sh
git clone https://github.com/yuezk/GlobalProtect-openconnect.git
cd GlobalProtect-openconnect
git submodule update --init

# qmake or qmake-qt5
qmake CONFIG+=release
make
sudo make install
```
Open `GlobalProtect VPN` in the application dashboard.

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

* Finally extract the source tree and build the debian package.

  ```
  cd ..
  tar -xzvf globalprotect-openconnect_1.3.0.orig.tar.gz
  cd globalprotect-openconnect-1.3.0
  fakeroot dpkg-buildpackage -uc -us -sa 2>&1 | tee ../build.log
  ```

  

## [License](./LICENSE)
GPLv3
