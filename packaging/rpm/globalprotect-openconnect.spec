Name:           globalprotect-openconnect
Version:        1.3.0+SNAPSHOT20210829120923
Release:        1
Summary:        A GlobalProtect VPN client

License:        GPLv3
URL:            https://github.com/yuezk/GlobalProtect-openconnect
Source0:        %{url}/releases/download/latest/globalprotect-openconnect_%{version}.full.tar.gz

BuildRequires:  qt5-qtbase-devel qt5-qtwebengine-devel qt5-qtwebsockets-devel
Requires:       qt5-qtbase >= 5.12 qt5-qtwebengine >= 5.12 qt5-qtwebsockets >= 5.12 openconnect >= 8.0

%global debug_package %{nil}

%description
A GlobalProtect VPN client (GUI) for Linux based on OpenConnect and built with Qt5, supports SAML auth mode.


%prep
%autosetup


%build
qmake-qt5 CONFIG+=release
%make_build


%install
INSTALL_ROOT=${RPM_BUILD_ROOT} %make_install


%files
/etc/systemd/system/gpservice.service
/usr/bin/gpclient
/usr/bin/gpservice
/usr/share/applications/com.yuezk.qt.gpclient.desktop
/usr/share/dbus-1/system-services/com.yuezk.qt.GPService.service
/usr/share/dbus-1/system.d/com.yuezk.qt.GPService.conf
/usr/share/pixmaps/com.yuezk.qt.GPClient.svg
