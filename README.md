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

## Added feature (reason of fork)

- It now saves your credentials (not very secure, be aware of that)

  
## Build & Install from source code

Clone this repo with:

```sh
git clone https://github.com/CarloRamponi/GlobalProtect-openconnect.git
cd GlobalProtect-openconnect
```

### Ubuntu/Mint

> **⚠️ REQUIRED for Ubuntu 18.04 ⚠️**
> 
> Add this [dwmw2/openconnect](https://launchpad.net/~dwmw2/+archive/ubuntu/openconnect) PPA first to install the latest openconnect.
> 
> ```sh
> sudo add-apt-repository ppa:dwmw2/openconnect
> sudo apt update
> ```
  
Build and install with:

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

## Run

Once the software is installed, you can run `gpclient` to start the UI.

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
  
  
## Troubleshooting

The application logs can be found at: `~/.cache/GlobalProtect-openconnect/gpclient.log`

## [License](./LICENSE)
GPLv3
