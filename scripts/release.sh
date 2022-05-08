#!/bin/bash -e

VERSION=$(cat VERSION)

# Update packaging, e.g., version, changelog, etc.
./scripts/prepare-packaging.sh

# Commit the changes
git commit -m "Release ${VERSION}" .
git tag v$VERSION -a -m "Release ${VERSION}"