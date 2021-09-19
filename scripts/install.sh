#!/bin/bash -e

./cmakew -B build -DCMAKE_BUILD_TYPE=Release
MAKEFLAGS=-j$(nproc) ./cmakew --build build
sudo ./cmakew --install build

sudo systemctl daemon-reload
sudo systemctl restart gpservice.service

echo -e "\nSuccess. You can launch the GlobalProtect VPN client from the application dashboard.\n"