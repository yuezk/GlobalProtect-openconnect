#!/bin/bash -e

OLD_VERSION=$(git tag --sort=-v:refname --list "v[0-9]*" | head -n 1 | cut -c 2-)
NEW_VERSION="$(cat VERSION)"
FULL_VERSION="$(cat VERSION VERSION_SUFFIX)"
HISTORY_ENTRIES=$(git log --format="  * %s" v${OLD_VERSION}.. | cat -n | sort -uk2 | sort -n | cut -f2-)

function update_debian_changelog() {
	local OLD_CHANGELOG=$(cat debian/changelog)

	cat > debian/changelog <<-EOF
	globalprotect-openconnect (${FULL_VERSION}-1) unstable; urgency=medium

	${HISTORY_ENTRIES}

	 -- Kevin Yue <k3vinyue@gmail.com>  $(date -R)

	${OLD_CHANGELOG}
	EOF
}

function update_rpm_changelog() {
	local OLD_CHANGELOG=$(cat packaging/obs/globalprotect-openconnect.changes)

	cat > packaging/obs/globalprotect-openconnect.changes <<-EOF
	-------------------------------------------------------------------
	$(LC_ALL=en.US date -u "+%a %b %e %T %Z %Y") - k3vinyue@gmail.com - ${FULL_VERSION}

	- Update to ${FULL_VERSION}
	${HISTORY_ENTRIES}

	${OLD_CHANGELOG}
	EOF
}

# Update rpm version
sed -i"" -re "s/(Version:\s+).+/\1${FULL_VERSION}/" packaging/obs/globalprotect-openconnect.spec

update_rpm_changelog
update_debian_changelog
