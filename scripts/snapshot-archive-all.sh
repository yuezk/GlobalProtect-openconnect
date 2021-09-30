#!/bin/bash -e

./scripts/snapshot-version.sh
./scripts/prepare-packaging.sh
./scripts/_archive-all.sh

# Update the OBS packaging
mv ./artifacts/obs/globalprotect-openconnect.tar.gz ./artifacts/obs/globalprotect-openconnect-snapshot.tar.gz
mv ./artifacts/obs/globalprotect-openconnect.spec ./artifacts/obs/globalprotect-openconnect-snapshot.spec
mv ./artifacts/obs/globalprotect-openconnect.changes ./artifacts/obs/globalprotect-openconnect-snapshot.changes
mv ./artifacts/obs/globalprotect-openconnect-rpmlintrc ./artifacts/obs/globalprotect-openconnect-snapshot-rpmlintrc
sed -i"" -re "s/(Name:\s+).+/\1globalprotect-openconnect-snapshot/" \
    -re "s/(Conflicts:\s+).+/\1globalprotect-openconnect/" \
    ./artifacts/obs/globalprotect-openconnect-snapshot.spec

# Update the AUR package
cp ./packaging/aur/PKGBUILD-git ./artifacts/aur/PKGBUILD