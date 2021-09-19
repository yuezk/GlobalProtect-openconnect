#!/bin/bash -e

sudo zypper install -y \
    libqt5-qtbase-devel \
    libqt5-qtwebsockets-devel \
    libqt5-qtwebengine-devel \
    openconnect

./scripts/install.sh