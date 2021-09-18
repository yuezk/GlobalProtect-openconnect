#!/bin/bash -e

./cmakew -B build
./cmakew --build build
sudo ./cmakew --install build

echo "Done. You can open it from the application dashboard."