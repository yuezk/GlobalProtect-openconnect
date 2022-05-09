# Maintainer: Keinv Yue <yuezk001@gmail.com>

_pkgver="1.4.3"
_commit="28d832195876614c6c730b430978268242235ab3"
pkgname=globalprotect-openconnect-git
pkgver=${_pkgver}
pkgrel=1
pkgdesc="A GlobalProtect VPN client (GUI) for Linux based on Openconnect and built with Qt5, supports SAML auth mode. (development version)"
arch=(x86_64 aarch64)
url="https://github.com/yuezk/GlobalProtect-openconnect"
license=('GPL3')
backup=(
    etc/gpservice/gp.conf
)
install=gp.install
depends=('openconnect>=8.0.0' qt5-base qt5-webengine qt5-websockets)
makedepends=(git cmake)
conflicts=('globalprotect-openconnect')
provides=('globalprotect-openconnect' 'gpclient' 'gpservice')

source=(git+https://github.com/yuezk/GlobalProtect-openconnect#commit=${_commit})
sha256sums=('SKIP')

prepare() {
    cd GlobalProtect-openconnect
    echo "${_pkgver}" > VERSION
}

build() {
    cd GlobalProtect-openconnect
    cmake -B build \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_CXX_FLAGS_RELEASE=-s
    make -j$(nproc) -C build
}

package() {
    cd GlobalProtect-openconnect
    make DESTDIR="$pkgdir/" install -C build
}
