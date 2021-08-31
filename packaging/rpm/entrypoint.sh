#!/bin/bash -e

# Install the build tools
dnf install -y epel-release
rpm --import http://download.fedoraproject.org/pub/epel/RPM-GPG-KEY-EPEL-8
dnf install -y make rpm-build rpm-devel rpmlint rpmdevtools

# Install the build dependencies
dnf install -y qt5-qtbase-devel qt5-qtwebengine-devel qt5-qtwebsockets-devel

# Prepare the RPM build environment
rpmdev-setuptree
cp *.spec $HOME/rpmbuild/SPECS/
cp *.tar.gz $HOME/rpmbuild/SOURCES/

# Build
rpmbuild -ba $HOME/rpmbuild/SPECS/globalprotect-openconnect.spec

# Copy the package to the current directory
cp $HOME/rpmbuild/RPMS/x86_64/globalprotect-openconnect-*.rpm .
cp $HOME/rpmbuild/SRPMS/globalprotect-openconnect-*.src.rpm .
