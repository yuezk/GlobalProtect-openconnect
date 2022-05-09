#!/bin/bash -e

VERSION="v$(cat VERSION)"
git describe --tags --match "${VERSION}" | sed -re 's/^v([^-]+)-([^-]+)-(.+)/\1+\2snapshot.\3/' > VERSION