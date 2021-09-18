#!/bin/bash -e

./cmakew -B build
./cmakew --build build
sudo ./cmakew --install build

echo ""
echo "Done. You can launch the GlobalProtect VPN client from the application dashboard."
echo ""