#!/bin/bash -e

VERSION="$(cat VERSION)"

rm -rf ./artifacts && mkdir -p ./artifacts/{obs,aur,flatpak}

# Add the version file
echo $VERSION > ./artifacts/VERSION

# Archive the source code
git-archive-all \
    --force-submodules \
    --prefix=globalprotect-openconnect-${VERSION}/ \
    ./artifacts/globalprotect-openconnect-${VERSION}.tar.gz

# Prepare the OBS package
cp -r ./packaging/obs ./artifacts
cp ./artifacts/*.tar.gz ./artifacts/obs/globalprotect-openconnect.tar.gz

# Prepare the AUR package
cp ./packaging/aur/PKGBUILD-git ./artifacts/aur/PKGBUILD
cp ./artifacts/*.tar.gz ./artifacts/aur/globalprotect-openconnect.tar.gz

# Prepare the flatpak package
cp ./packaging/flatpak/com.yuezk.qt.gpclient.yml ./artifacts/flatpak
cp ./artifacts/*.tar.gz ./artifacts/flatpak/globalprotect-openconnect.tar.gz