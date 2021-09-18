#!/bin/bash -e

sudo apt update
sudo apt install -y \
    build-essential \
    qtbase5-dev \
    libqt5websockets5-dev \
    qtwebengine5-dev \
    openconnect

./install.sh