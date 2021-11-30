# GlobalProtect-openconnect
A GlobalProtect VPN client (GUI) for Linux based on Openconnect and built with Qt5, supports SAML auth mode, inspired by [gp-saml-gui](https://github.com/dlenski/gp-saml-gui).

<p align="center">
  <img src="https://user-images.githubusercontent.com/3297602/133869036-5c02b0d9-c2d9-4f87-8c81-e44f68cfd6ac.png">
</p>

<a href="https://paypal.me/zongkun" target="_blank"><img src="https://cdn.jsdelivr.net/gh/everdrone/coolbadge@5ea5937cabca5ecbfc45d6b30592bd81f219bc8d/badges/Paypal/Coffee/Blue/Small.png" alt="Buy me a coffee via Paypal" style="height: 32px; width: 268px;" ></a>
<a href="https://ko-fi.com/M4M75PYKZ" target="_blank"><img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="Support me on Ko-fi" style="height: 32px; width: 238px;"></a>
<a href="https://www.buymeacoffee.com/yuezk" target="_blank"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me A Coffee" style="height: 32px; width: 114px;" ></a>


## Features

- Similar user experience as the official client in macOS.
- Supports both SAML and non-SAML authentication modes.
- Supports automatically selecting the preferred gateway from the multiple gateways.
- Supports switching gateway from the system tray menu manually.


## Install

|OS|Stable version | Development version|
|---|--------------|--------------------|
|Linux Mint, Ubuntu 18.04 or later|[ppa:yuezk/globalprotect-openconnect](https://launchpad.net/~yuezk/+archive/ubuntu/globalprotect-openconnect)|[ppa:yuezk/globalprotect-openconnect-snapshot](https://launchpad.net/~yuezk/+archive/ubuntu/globalprotect-openconnect-snapshot)|
|Arch, Manjaro|[AUR: globalprotect-openconnect](https://aur.archlinux.org/packages/globalprotect-openconnect/)|[AUR: globalprotect-openconnect-git](https://aur.archlinux.org/packages/globalprotect-openconnect-git/)|
|Fedora|[copr: yuezk/globalprotect-openconnect](https://copr.fedorainfracloud.org/coprs/yuezk/globalprotect-openconnect/)|[copr: yuezk/globalprotect-openconnect](https://copr.fedorainfracloud.org/coprs/yuezk/globalprotect-openconnect/)|
|openSUSE, CentOS 8|[OBS: globalprotect-openconnect](https://build.opensuse.org/package/show/home:yuezk/globalprotect-openconnect)|[OBS: globalprotect-openconnect-snapshot](https://build.opensuse.org/package/show/home:yuezk/globalprotect-openconnect-snapshot)|

Add the repository in the above table and install it with your favorite package manager tool.

### Linux Mint, Ubuntu 18.04 or later

```sh
sudo add-apt-repository ppa:yuezk/globalprotect-openconnect
sudo apt-get update
sudo apt install globalprotect-openconnect
```

> For Linux Mint, you might need to import the GPG key with: `sudo apt-key adv --keyserver keyserver.ubuntu.com --recv-keys 7937C393082992E5D6E4A60453FC26B43838D761` if you encountered an error `gpg: keyserver receive failed: General error`.

### Arch Linux

```sh
sudo pacman -S globalprotect-openconnect
```

### Manjaro

```sh
yay -S globalprotect-openconnect
```

### Fedora

```sh
sudo dnf copr enable yuezk/globalprotect-openconnect
sudo dnf install globalprotect-openconnect
```

### openSUSE

- openSUSE Tumbleweed
  ```sh
  sudo zypper ar https://download.opensuse.org/repositories/home:/yuezk/openSUSE_Tumbleweed/home:yuezk.repo
  sudo zypper ref
  sudo zypper install globalprotect-openconnect
  ```

- openSUSE Leap

  ```sh
  sudo zypper ar https://download.opensuse.org/repositories/home:/yuezk/openSUSE_Leap_15.2/home:yuezk.repo
  sudo zypper ref
  sudo zypper install globalprotect-openconnect
  ```
### CentOS 8

1. Add the repository: `https://download.opensuse.org/repositories/home:/yuezk/CentOS_8/home:yuezk.repo`
1. Install `globalprotect-openconnect`

  
## Build & Install from source code

Clone this repo with:

```sh
git clone https://github.com/yuezk/GlobalProtect-openconnect.git
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

Once the software is installed, you can run `gpclient` to start the UI,

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

  

## Future plan

- [x] Improve the release process
- [ ] Process bugs and feature requests
- [ ] Support for bypassing the `gpclient` parameters
- [ ] Support the CLI mode
  
  
## Troubleshooting

The application logs can be found at: `~/.cache/GlobalProtect-openconnect/gpclient.log`

## [License](./LICENSE)
GPLv3
