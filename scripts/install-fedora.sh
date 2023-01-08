#!/bin/bash -e

sudo dnf install -y \
    qt5-qtbase-devel \
    qt5-qtwebengine-devel \
    qt5-qtwebsockets-devel \
    qtkeychain-qt5-devel \
    openconnect

./scripts/install.sh
