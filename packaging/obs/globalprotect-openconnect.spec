Name:           globalprotect-openconnect
Version:        1.4.3
Release:        1
Summary:        A GlobalProtect VPN client powered by OpenConnect
Group:          Productivity/Networking/PPP
BuildRoot:      %{_tmppath}/%{name}-%{version}-build

License:        GPL-3.0
URL:            https://github.com/yuezk/GlobalProtect-openconnect
Source0:        %{name}.tar.gz
BuildRequires:  cmake cmake(Qt5) cmake(Qt5Gui) cmake(Qt5WebEngine) cmake(Qt5WebSockets) cmake(Qt5DBus)
BuildRequires:  systemd-rpm-macros
Requires:       openconnect >= 8.0
Conflicts:      globalprotect-openconnect-snapshot


%global debug_package %{nil}

%description
A GlobalProtect VPN client (GUI) for Linux based on OpenConnect and built with Qt5, supports SAML auth mode.


%prep
%autosetup -n "globalprotect-openconnect-%{version}"


%pre

%if 0%{?suse_version}
    %service_add_pre gpservice.service
%endif


%post

%if 0%{?suse_version}
    %service_add_post gpservice.service
%else
    %systemd_post gpservice.service
%endif


%preun

%if 0%{?suse_version}
    %service_del_preun gpservice.service
%else
    %systemd_preun gpservice.service
%endif


%postun

%if 0%{?suse_version}
    %service_del_postun gpservice.service
%else
    %systemd_postun_with_restart gpservice.service
%endif


%build

%cmake -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_FLAGS_RELEASE=-s

%if 0%{?fedora_version} && 0%{?fedora_version} <= 32
    %make_build
%else
    %cmake_build
%endif


%install

%if 0%{?fedora_version} && 0%{?fedora_version} <= 32
    %make_install
%else
    %cmake_install
%endif

%files
%defattr(-,root,root)
%{_unitdir}/gpservice.service
%{_bindir}/gpclient
%{_bindir}/gpservice
%{_datadir}/applications/com.yuezk.qt.gpclient.desktop
%{_datadir}/dbus-1/system-services/com.yuezk.qt.GPService.service
%{_datadir}/dbus-1/system.d/com.yuezk.qt.GPService.conf
%{_datadir}/icons/hicolor/scalable/apps/com.yuezk.qt.gpclient.svg
%{_datadir}/metainfo/com.yuezk.qt.gpclient.metainfo.xml
%config %{_sysconfdir}/gpservice/gp.conf

%dir %{_sysconfdir}/gpservice
%dir %{_datadir}/icons/hicolor
%dir %{_datadir}/icons/hicolor/scalable
%dir %{_datadir}/icons/hicolor/scalable/apps

%changelog
