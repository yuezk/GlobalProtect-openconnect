#!/bin/bash -e

VERSION=$(cat VERSION)

# Clear the VERSION_SUFFIX
cat /dev/null > VERSION_SUFFIX

# Update packaging, e.g., version, changelog, etc.
./scripts/prepare-packaging.sh

# Commit the changes
git commit -m "Release ${VERSION}" .
git tag v$VERSION -a -m "Release ${VERSION}"