# Building from Source on FreeBSD and OpenBSD

This guide builds the open-source components from the `GlobalProtect-openconnect` repository on FreeBSD or OpenBSD.

## Workspace

Clone `GlobalProtect-openconnect`:

```sh
mkdir gp-build
cd gp-build
git clone --recurse-submodules https://github.com/yuezk/GlobalProtect-openconnect.git gp
```

## FreeBSD

Install build and runtime dependencies:

```sh
sudo pkg install git rust libiconv gettext-tools autoconf automake libtool patch \
  gmake pkgconf libxml2 gnutls p11-kit nettle gmp gnome-keyring \
  libayatana-appindicator polkit webkit2-gtk_41 xdg-utils
```

Build and install:

```sh
cd gp
cargo build --release --workspace

sudo gmake install-bsd
```

Verify the installed binaries:

```sh
gpclient --version
gpauth --version
```

## OpenBSD

Install build and runtime dependencies:

```sh
doas pkg_add -u -I
doas pkg_add -I git rust libiconv gettext-tools autoconf-2.72 automake-1.17 \
  libtool gmake libxml gnutls p11-kit gmp lz4 dbus \
  gnome-keyring polkit webkitgtk41 xdg-utils
```

OpenBSD provides `patch` and `pkg-config` in the base system. GnuTLS installs
Nettle as a dependency. The separately built GUI uses a D-Bus StatusNotifier
tray backend on OpenBSD and requires a desktop with StatusNotifier support.

Use the installed Autoconf and Automake versions:

```sh
export AUTOCONF_VERSION=2.72
export AUTOMAKE_VERSION=1.17
```

If your shell cannot resolve the versioned tools automatically, add the usual unversioned command links:

```sh
doas ln -sf /usr/local/bin/autoreconf-2.72 /usr/local/bin/autoreconf
doas ln -sf /usr/local/bin/autoconf-2.72 /usr/local/bin/autoconf
doas ln -sf /usr/local/bin/autoheader-2.72 /usr/local/bin/autoheader
doas ln -sf /usr/local/bin/autom4te-2.72 /usr/local/bin/autom4te
doas ln -sf /usr/local/bin/aclocal-1.17 /usr/local/bin/aclocal
doas ln -sf /usr/local/bin/automake-1.17 /usr/local/bin/automake
```

Build and install:

```sh
cd gp
cargo build --release --workspace

doas gmake install-bsd
```

Verify the installed binaries:

```sh
gpclient --version
gpauth --version
```

## Verify

After installation, check the CLI help:

```sh
gpclient --help
```
