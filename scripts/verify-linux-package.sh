#!/usr/bin/env bash
# Run inside the distro builder after install.sh installs the newly built package.
set -euo pipefail

package=globalprotect-openconnect
notice=/usr/share/licenses/$package/ICU4X-LICENSE.txt
for path in /usr/bin/gpgui /usr/bin/gpgui-nsh; do
  test -x "$path"
done
test -s "$notice"

case "${1:?Expected a package format}" in
  binary) exit 0 ;;
  deb|ubuntu-rolling)
    files=$(dpkg-query -L "$package")
    requires=$(dpkg-query -W -f='${Pre-Depends} ${Depends}\n' "$package")
    ;;
  rpm)
    files=$(rpm -ql "$package")
    requires=$(rpm -q --requires "$package")
    ;;
  apk)
    files=$(apk info -L "$package")
    requires=$(apk info -R "$package")
    ;;
  pkg)
    files=$(pacman -Qlq "$package")
    requires=$(bsdtar -xOf /workspace/artifacts/*.pkg.tar.zst .PKGINFO | sed -n 's/^depend = //p')
    ;;
  *) echo "Unknown package format: $1" >&2; exit 1 ;;
esac

# apk emits paths relative to /. Other package managers use absolute paths.
for path in /usr/bin/gpgui /usr/bin/gpgui-nsh "$notice"; do
  if ! grep -Fxq -e "$path" -e "${path#/}" <<<"$files"; then
    echo "$package does not own $path" >&2
    exit 1
  fi
done
if grep -Eq '(^|[[:space:],:(|])(lib)?gtk-?4([^[:alnum:]_]|$)' <<<"$requires"; then
  echo "Package metadata makes GTK4 mandatory: $requires" >&2
  exit 1
fi
