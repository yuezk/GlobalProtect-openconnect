#!/bin/bash -e

./cmakew -B build \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_FLAGS_RELEASE=-s

MAKEFLAGS=-j$(nproc) ./cmakew --build build