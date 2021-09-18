#!/bin/bash -e

./cmakew -B build
./cmakew --build build
sudo ./cmakew --install build

sudo systemctl daemon-reload
sudo systemctl restart gpservice.service

echo ""
echo "Done. You can launch the GlobalProtect VPN client from the application dashboard."
echo ""