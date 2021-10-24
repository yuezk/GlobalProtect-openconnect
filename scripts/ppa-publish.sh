#!/bin/bash -e

VERSION="$(cat VERSION VERSION_SUFFIX)"
OLD_REVISION="1"

PPA_REPO="ppa:yuezk/globalprotect-openconnect-snapshot"

while [[ $# -gt 0 ]]; do
    key="$1"

    case $key in
        --stable)
            PPA_REPO="ppa:yuezk/globalprotect-openconnect"
            shift
            ;;
        "18.04")
            DISTRIBUTION="18.04"
            DISTRIBUTION_NAME="bionic"
            shift
            ;;
        "20.04")
            DISTRIBUTION="20.04"
            DISTRIBUTION_NAME="focal"
            shift
            ;;
        "21.04")
            DISTRIBUTION="21.04"
            DISTRIBUTION_NAME="hirsute"
            shift
            ;;
        "21.10")
            DISTRIBUTION="21.10"
            DISTRIBUTION_NAME="impish"
            shift
            ;;
        *)
            echo "Unkown options $key"
            exit 1
            ;;
    esac
done

[ -z $DISTRIBUTION ] && echo "The distribuation is required" && exit 1;

NEW_REVISION="ppa1~ubuntu${DISTRIBUTION}"

sed -i"" "1s/${VERSION}-${OLD_REVISION}/${VERSION}-${NEW_REVISION}/;1s/unstable/${DISTRIBUTION_NAME}/" debian/changelog
debmake
debuild -S -sa \
    -k"${PPA_GPG_KEYID}" \
    -p"gpg --batch --passphrase ${PPA_GPG_PASSPHRASE} --pinentry-mode loopback"

dput $PPA_REPO ../globalprotect-openconnect_${VERSION}-${NEW_REVISION}_source.changes
