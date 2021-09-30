#!/bin/bash -e

./scripts/build.sh

sudo ./cmakew --install build

sudo systemctl enable gpservice.service
sudo systemctl daemon-reload
sudo systemctl restart gpservice.service

echo -e "\nSuccess. You can launch the GlobalProtect VPN client from the application dashboard.\n"