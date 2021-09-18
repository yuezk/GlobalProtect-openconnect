#!/bin/bash -e

./cmakew -B build
./cmakew --build build
sudo ./cmakew --install build