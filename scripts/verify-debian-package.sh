#!/bin/bash -e

sudo apt update
sudo apt install -y \
	build-essential \
	qtbase5-dev \
	libqt5websockets5-dev \
	qtwebengine5-dev \
	cmake \
	debhelper

mkdir -p build

cp ./artifacts/*.tar.gz build/ && cd build
tar -xzf *.tar.gz && cd globalprotect-openconnect-*

dpkg-buildpackage -us -uc
