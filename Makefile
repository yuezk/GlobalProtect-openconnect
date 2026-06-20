SHELL := /bin/sh
.SHELLFLAGS := -ec

INCLUDE_GUI ?= 0
CARGO ?= cargo
DISABLE_RUST_TOOLCHAIN ?= 0
RUST_VERSION ?= 1.89
IGNORE_RUST_VERSION ?= 0

VERSION = $(shell grep '^version' Cargo.toml | head -1 | sed 's/version *= *"\(.*\)"/\1/')
SOURCE_COMMIT_VALUE := $(shell \
	if [ -n "$$SOURCE_GIT_COMMIT" ]; then \
		printf '%s' "$$SOURCE_GIT_COMMIT" | cut -c1-9; \
	elif [ -n "$$GITHUB_SHA" ]; then \
		printf '%s' "$$GITHUB_SHA" | cut -c1-9; \
	elif [ -f SOURCE_COMMIT ]; then \
		head -n1 SOURCE_COMMIT | tr -d '[:space:]' | cut -c1-9; \
	else \
		git rev-parse --short=9 HEAD 2>/dev/null || printf unknown; \
	fi)
GUI_LIBC_SUFFIX ?= $(shell ldd --version 2>&1 | grep -qi musl && echo -musl || true)
REVISION ?= 1
RPM_SOURCE ?= %{name}.tar.gz

PPA_REVISION ?= 1
PKG_NAME = globalprotect-openconnect
PKG = $(PKG_NAME)-$(VERSION)
SERIES ?= $(shell lsb_release -cs)
PUBLISH ?= 0

# Indicates whether to build the GUI components
BUILD_GUI_HELPER ?= 1

# Indicates whether to build embedded webview auth support into gpauth
BUILD_WEBVIEW_AUTH ?= 1
PREFIX ?= /usr/local
BSD_FLAVOR ?= $(shell uname -s | tr '[:upper:]' '[:lower:]')
GPGUI_BINARY ?= ../gpgui/target/release/gpgui

export DEBEMAIL = k3vinyue@gmail.com
export DEBFULLNAME = Kevin Yue
export SNAPSHOT = $(shell test -f SNAPSHOT && echo "true" || echo "false")
export OFFLINE_BUILD = $(shell test -f OFFLINE_BUILD && echo "1" || echo "0")
# If OFFLINE is not set, use OFFLINE_BUILD
ifndef OFFLINE
	OFFLINE = $(OFFLINE_BUILD)
endif

ifeq ($(SNAPSHOT), true)
	RELEASE_TAG = snapshot
else
	RELEASE_TAG = v$(VERSION)
endif

CARGO_BUILD_ARGS = --release

ifeq ($(OFFLINE), 1)
	CARGO_BUILD_ARGS += --frozen
endif

ifeq ($(IGNORE_RUST_VERSION), 1)
	CARGO_BUILD_ARGS += --ignore-rust-version
endif

default: build

version:
	@echo $(VERSION)

clean-tarball:
	rm -rf .build/tarball
	rm -rf .vendor
	rm -rf vendor.tar.xz
	rm -rf .cargo
	rm -f SOURCE_COMMIT

# Create a tarball, include the cargo dependencies if OFFLINE is set to 1
tarball: clean-tarball
	mkdir -p .cargo
	mkdir -p .build/tarball
	printf '%s\n' "$(SOURCE_COMMIT_VALUE)" > SOURCE_COMMIT

	# If OFFLINE is set to 1, vendor all cargo dependencies
	# Generate a OFFLINE_BUILD file to indicate offline build
	if [ $(OFFLINE) -eq 1 ]; then \
		$(CARGO) vendor .vendor > .cargo/config.toml; \
		tar -cJf vendor.tar.xz .vendor; \
		touch OFFLINE_BUILD; \
	fi

	@echo "Creating tarball..."
	tar --exclude .vendor --exclude target --transform 's,^,${PKG}/,' -czf .build/tarball/${PKG}.tar.gz * .cargo

download-gui:
	rm -rf .build/gpgui

	if [ $(INCLUDE_GUI) -eq 1 ]; then \
		echo "Downloading GlobalProtect GUI..."; \
		mkdir -p .build/gpgui; \
		curl -sSL https://github.com/yuezk/GlobalProtect-openconnect/releases/download/$(RELEASE_TAG)/gpgui_$(shell uname -m)$(GUI_LIBC_SUFFIX).bin.tar.xz \
			-o .build/gpgui/gpgui_$(shell uname -m)$(GUI_LIBC_SUFFIX).bin.tar.xz; \
		tar -xJf .build/gpgui/*.tar.xz -C .build/gpgui; \
	else \
		echo "Skipping GlobalProtect GUI download (INCLUDE_GUI=0)"; \
	fi

build: download-gui build-rs

build-rs:
	if [ $(OFFLINE) -eq 1 ]; then \
		tar -xJf vendor.tar.xz; \
	fi

	# Remove the rust-toolchain.toml if DISABLE_RUST_TOOLCHAIN is set to 1
	if [ $(DISABLE_RUST_TOOLCHAIN) -eq 1 ]; then \
		rm -vf rust-toolchain.toml; \
	fi

	$(CARGO) build $(CARGO_BUILD_ARGS) -p gpclient -p gpservice

	# Build gpauth with or without embedded webview auth support
	if [ $(BUILD_WEBVIEW_AUTH) -eq 1 ]; then \
		$(CARGO) build $(CARGO_BUILD_ARGS) -p gpauth; \
	else \
		$(CARGO) build $(CARGO_BUILD_ARGS) -p gpauth --no-default-features; \
	fi

	# Only build the GUI components if BUILD_GUI_HELPER is set to 1
	if [ $(BUILD_GUI_HELPER) -eq 1 ]; then \
		$(CARGO) build $(CARGO_BUILD_ARGS) -p gpgui-helper; \
	fi

clean:
	$(CARGO) clean
	rm -rf .build
	rm -rf .vendor
	rm -rf apps/gpgui-helper/node_modules

install:
	@echo "Installing $(PKG_NAME)..."

	install -Dm755 target/release/gpclient $(DESTDIR)/usr/bin/gpclient
	install -Dm755 target/release/gpauth $(DESTDIR)/usr/bin/gpauth
	install -Dm755 target/release/gpservice $(DESTDIR)/usr/bin/gpservice

	# Install the GUI components if BUILD_GUI_HELPER is set to 1
	if [ $(BUILD_GUI_HELPER) -eq 1 ]; then \
		install -Dm755 target/release/gpgui-helper $(DESTDIR)/usr/bin/gpgui-helper; \
	fi

	if [ -f .build/gpgui/gpgui_*/gpgui ]; then \
		install -Dm755 .build/gpgui/gpgui_*/gpgui $(DESTDIR)/usr/bin/gpgui; \
	fi

	install -Dm755 packaging/files/usr/libexec/gpclient/vpnc-script $(DESTDIR)/usr/libexec/gpclient/vpnc-script
	install -Dm755 packaging/files/usr/libexec/gpclient/hipreport.sh $(DESTDIR)/usr/libexec/gpclient/hipreport.sh

	# Install the disconnect hooks
	install -Dm755 packaging/files/usr/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down $(DESTDIR)/usr/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down
	install -Dm755 packaging/files/usr/lib/NetworkManager/dispatcher.d/gpclient-nm-hook $(DESTDIR)/usr/lib/NetworkManager/dispatcher.d/gpclient-nm-hook

	install -Dm644 packaging/files/usr/share/applications/gpgui.desktop $(DESTDIR)/usr/share/applications/gpgui.desktop
	install -Dm644 packaging/files/usr/share/icons/hicolor/scalable/apps/gpgui.svg $(DESTDIR)/usr/share/icons/hicolor/scalable/apps/gpgui.svg
	install -Dm644 packaging/files/usr/share/icons/hicolor/32x32/apps/gpgui.png $(DESTDIR)/usr/share/icons/hicolor/32x32/apps/gpgui.png
	install -Dm644 packaging/files/usr/share/icons/hicolor/128x128/apps/gpgui.png $(DESTDIR)/usr/share/icons/hicolor/128x128/apps/gpgui.png
	install -Dm644 packaging/files/usr/share/icons/hicolor/256x256@2/apps/gpgui.png $(DESTDIR)/usr/share/icons/hicolor/256x256@2/apps/gpgui.png
	install -Dm644 packaging/files/usr/share/polkit-1/actions/com.yuezk.gpgui.policy $(DESTDIR)/usr/share/polkit-1/actions/com.yuezk.gpgui.policy

install-bsd:
	@echo "Installing $(PKG_NAME) for BSD under $(PREFIX)..."

	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 target/release/gpclient $(DESTDIR)$(PREFIX)/bin/gpclient
	install -m 755 target/release/gpauth $(DESTDIR)$(PREFIX)/bin/gpauth
	install -m 755 target/release/gpservice $(DESTDIR)$(PREFIX)/bin/gpservice

	if [ "$(BUILD_GUI_HELPER)" = "1" ]; then \
		install -m 755 target/release/gpgui-helper $(DESTDIR)$(PREFIX)/bin/gpgui-helper; \
	fi

	if [ -x "$(GPGUI_BINARY)" ]; then \
		install -m 755 "$(GPGUI_BINARY)" $(DESTDIR)$(PREFIX)/bin/gpgui; \
	fi

	install -d $(DESTDIR)$(PREFIX)/libexec/gpclient
	install -m 755 packaging/files/usr/libexec/gpclient/vpnc-script $(DESTDIR)$(PREFIX)/libexec/gpclient/vpnc-script
	install -m 755 packaging/files/usr/libexec/gpclient/hipreport.sh $(DESTDIR)$(PREFIX)/libexec/gpclient/hipreport.sh

	install -d $(DESTDIR)$(PREFIX)/share/applications
	install -m 644 packaging/bsd/gpgui.desktop $(DESTDIR)$(PREFIX)/share/applications/gpgui.desktop
	install -d $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps
	install -d $(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps
	install -d $(DESTDIR)$(PREFIX)/share/icons/hicolor/128x128/apps
	install -d $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256@2/apps
	install -m 644 packaging/files/usr/share/icons/hicolor/scalable/apps/gpgui.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/gpgui.svg
	install -m 644 packaging/files/usr/share/icons/hicolor/32x32/apps/gpgui.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/32x32/apps/gpgui.png
	install -m 644 packaging/files/usr/share/icons/hicolor/128x128/apps/gpgui.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/128x128/apps/gpgui.png
	install -m 644 packaging/files/usr/share/icons/hicolor/256x256@2/apps/gpgui.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256@2/apps/gpgui.png
	install -d $(DESTDIR)$(PREFIX)/share/polkit-1/actions
	install -m 644 packaging/bsd/com.yuezk.gpgui.policy $(DESTDIR)$(PREFIX)/share/polkit-1/actions/com.yuezk.gpgui.policy

clean-bsd-package:
	rm -rf .build/$(BSD_FLAVOR)

bsd-gpgui-tarball:
	test -x "$(GPGUI_BINARY)"
	rm -rf .build/$(BSD_FLAVOR)/gpgui
	mkdir -p .build/$(BSD_FLAVOR)/artifacts .build/$(BSD_FLAVOR)/gpgui/gpgui_$(VERSION)
	cp "$(GPGUI_BINARY)" .build/$(BSD_FLAVOR)/gpgui/gpgui_$(VERSION)/
	tar -cf - -C .build/$(BSD_FLAVOR)/gpgui gpgui_$(VERSION) | xz -c > .build/$(BSD_FLAVOR)/artifacts/gpgui_$(BSD_FLAVOR)_$(shell uname -m).bin.tar.xz
	file=.build/$(BSD_FLAVOR)/artifacts/gpgui_$(BSD_FLAVOR)_$(shell uname -m).bin.tar.xz; \
		if command -v sha256sum >/dev/null 2>&1; then sha256sum "$$file" | cut -d ' ' -f 1; \
		elif command -v sha256 >/dev/null 2>&1; then sha256 -q "$$file"; \
		else cksum -a sha256 "$$file" | awk '{print $$1}'; fi > "$$file.sha256"

package-freebsd: BSD_FLAVOR=freebsd
package-freebsd: clean-bsd-package
	test -x "$(GPGUI_BINARY)"
	mkdir -p .build/freebsd/pkgroot .build/freebsd/artifacts
	$(MAKE) install-bsd DESTDIR=$(CURDIR)/.build/freebsd/pkgroot PREFIX=$(PREFIX) GPGUI_BINARY="$(GPGUI_BINARY)"
	find .build/freebsd/pkgroot$(PREFIX) -type f -print | sed 's|^.build/freebsd/pkgroot$(PREFIX)/||' | sort > .build/freebsd/PLIST
	freebsd_major=$$(freebsd-version -u | sed 's/\..*//'); \
	freebsd_arch=$$(uname -m | sed -e 's/x86_64/amd64/' -e 's/arm64/aarch64/'); \
	sed \
		-e 's/@PKG_NAME@/$(PKG_NAME)/g' \
		-e 's/@VERSION@/$(VERSION)/g' \
		-e "s/@ABI@/FreeBSD:$$freebsd_major:$$freebsd_arch/g" \
		-e "s/@ARCH@/freebsd:$$freebsd_major:$$freebsd_arch/g" \
		-e 's|@PREFIX@|$(PREFIX)|g' \
		packaging/bsd/freebsd/MANIFEST.in > .build/freebsd/+MANIFEST
	pkg create -r .build/freebsd/pkgroot -M .build/freebsd/+MANIFEST -p .build/freebsd/PLIST -o .build/freebsd/artifacts
	freebsd_arch=$$(uname -m | sed -e 's/x86_64/amd64/' -e 's/arm64/aarch64/'); \
		mv .build/freebsd/artifacts/$(PKG_NAME)-$(VERSION).pkg .build/freebsd/artifacts/$(PKG_NAME)-$(VERSION)-freebsd-$$freebsd_arch.pkg
	$(MAKE) bsd-gpgui-tarball BSD_FLAVOR=freebsd GPGUI_BINARY="$(GPGUI_BINARY)"

package-openbsd: BSD_FLAVOR=openbsd
package-openbsd: clean-bsd-package
	test -x "$(GPGUI_BINARY)"
	mkdir -p .build/openbsd/pkgroot .build/openbsd/artifacts
	$(MAKE) install-bsd DESTDIR=$(CURDIR)/.build/openbsd/pkgroot PREFIX=$(PREFIX) GPGUI_BINARY="$(GPGUI_BINARY)"
	cp packaging/bsd/openbsd/COMMENT .build/openbsd/+COMMENT
	cp packaging/bsd/openbsd/DESC .build/openbsd/+DESC
	find .build/openbsd/pkgroot$(PREFIX) -type f -print | sed 's|^.build/openbsd/pkgroot$(PREFIX)/||' | sort > .build/openbsd/PLIST
	comment=$$(cat .build/openbsd/+COMMENT); \
		openbsd_arch=$$(uname -m | sed 's/x86_64/amd64/'); \
		gnome_keyring_pkg=$$(pkg_info -e 'gnome-keyring-*' | sed 's/^inst://' | head -n 1); \
		polkit_pkg=$$(pkg_info -e 'polkit-*' | sed 's/^inst://' | head -n 1); \
		webkitgtk_pkg=$$(pkg_info -e 'webkitgtk41-*' | sed 's/^inst://' | head -n 1); \
		pkg_create \
			-B .build/openbsd/pkgroot \
			-D COMMENT="$$comment" \
			-d .build/openbsd/+DESC \
			-f .build/openbsd/PLIST \
			-p $(PREFIX) \
			-P x11/gnome/keyring:gnome-keyring-*:$$gnome_keyring_pkg \
			-P sysutils/polkit:polkit-*:$$polkit_pkg \
			-P www/webkitgtk4,webkitgtk41:webkitgtk41-*:$$webkitgtk_pkg \
			.build/openbsd/artifacts/$(PKG_NAME)-$(VERSION)-openbsd-$$openbsd_arch.tgz
	$(MAKE) bsd-gpgui-tarball BSD_FLAVOR=openbsd GPGUI_BINARY="$(GPGUI_BINARY)"

uninstall:
	@echo "Uninstalling $(PKG_NAME)..."

	rm -f $(DESTDIR)/usr/bin/gpclient
	rm -f $(DESTDIR)/usr/bin/gpauth
	rm -f $(DESTDIR)/usr/bin/gpservice
	rm -f $(DESTDIR)/usr/bin/gpgui-helper
	rm -f $(DESTDIR)/usr/bin/gpgui

	rm -f $(DESTDIR)/usr/libexec/gpclient/vpnc-script
	rm -f $(DESTDIR)/usr/libexec/gpclient/hipreport.sh

	rm -f $(DESTDIR)/usr/lib/NetworkManager/dispatcher.d/pre-down.d/gpclient.down
	rm -f $(DESTDIR)/usr/lib/NetworkManager/dispatcher.d/gpclient-nm-hook

	rm -f $(DESTDIR)/usr/share/applications/gpgui.desktop
	rm -f $(DESTDIR)/usr/share/icons/hicolor/scalable/apps/gpgui.svg
	rm -f $(DESTDIR)/usr/share/icons/hicolor/32x32/apps/gpgui.png
	rm -f $(DESTDIR)/usr/share/icons/hicolor/128x128/apps/gpgui.png
	rm -f $(DESTDIR)/usr/share/icons/hicolor/256x256@2/apps/gpgui.png
	rm -f $(DESTDIR)/usr/share/polkit-1/actions/com.yuezk.gpgui.policy

clean-debian:
	rm -rf .build/deb

# Generate the debian package structure, without the changelog
init-debian: clean-debian tarball
	mkdir -p .build/deb
	cp .build/tarball/${PKG}.tar.gz .build/deb

	tar -xzf .build/deb/${PKG}.tar.gz -C .build/deb
	cd .build/deb/${PKG} && debmake

	cp -f packaging/deb/control.in .build/deb/$(PKG)/debian/control
	cp -f packaging/deb/rules.in .build/deb/$(PKG)/debian/rules
	cp -f packaging/deb/postrm .build/deb/$(PKG)/debian/postrm
	cp -f packaging/deb/compat .build/deb/$(PKG)/debian/compat

	sed -i "s/@RUST_VERSION@/$(RUST_VERSION)/g" .build/deb/$(PKG)/debian/control

	# Remove the GUI dependencies if BUILD_GUI_HELPER is set to 0
	if [ $(BUILD_GUI_HELPER) -eq 0 ]; then \
		sed -i "/libsecret-1-0/d" .build/deb/$(PKG)/debian/control; \
		sed -i "/libayatana-appindicator3-1/d" .build/deb/$(PKG)/debian/control; \
		sed -i "/gnome-keyring/d" .build/deb/$(PKG)/debian/control; \
	fi

	# Remove the WebKitGTK build dependency only if neither gpauth webview auth nor gpgui-helper needs it
	if [ $(BUILD_GUI_HELPER) -eq 0 ] && [ $(BUILD_WEBVIEW_AUTH) -eq 0 ]; then \
		sed -i "/libwebkit2gtk-4.1-dev/d" .build/deb/$(PKG)/debian/control; \
	fi

	sed -i "s/@BUILD_GUI_HELPER@/$(BUILD_GUI_HELPER)/g" .build/deb/$(PKG)/debian/rules
	sed -i "s/@BUILD_WEBVIEW_AUTH@/$(BUILD_WEBVIEW_AUTH)/g" .build/deb/$(PKG)/debian/rules
	sed -i "s/@RUST_VERSION@/$(RUST_VERSION)/g" .build/deb/$(PKG)/debian/rules

	rm -f .build/deb/$(PKG)/debian/changelog

deb: init-debian
	cd .build/deb/$(PKG) && dch --create --distribution unstable --package $(PKG_NAME) --newversion $(VERSION)-$(REVISION) "Bugfix and improvements."

	# Install build dependencies
	cd .build/deb/$(PKG) && sudo mk-build-deps --install --remove debian/control || echo "mk-build-deps failed, continuing"

	cd .build/deb/$(PKG) && debuild --preserve-env -e PATH -us -uc -b -d

check-ppa:
	if [ $(OFFLINE) -eq 0 ]; then \
		echo "Error: ppa build requires offline mode (OFFLINE=1)"; \
	fi

# Usage: make ppa SERIES=focal OFFLINE=1 PUBLISH=1
ppa: check-ppa init-debian
	$(eval SERIES_VER = $(shell distro-info --series $(SERIES) -r | cut -d' ' -f1))
	@echo "Building for $(SERIES) $(SERIES_VER)"

	rm -rf .build/deb/$(PKG)/debian/changelog
	cd .build/deb/$(PKG) && dch --create --distribution $(SERIES) --package $(PKG_NAME) --newversion $(VERSION)-$(REVISION)ppa$(PPA_REVISION)~ubuntu$(SERIES_VER) "Bugfix and improvements."

	cd .build/deb/$(PKG) && echo "y" | debuild -e PATH -S -sa -k"$(GPG_KEY_ID)" -p"gpg --batch --passphrase $(GPG_KEY_PASS) --pinentry-mode loopback"

	if [ $(PUBLISH) -eq 1 ]; then \
		cd .build/deb/$(PKG) && dput ppa:yuezk/globalprotect-openconnect ../*.changes; \
	else \
		echo "Skipping ppa publish (PUBLISH=0)"; \
	fi

clean-rpm:
	rm -rf .build/rpm

# Generate RPM sepc file
init-rpm: clean-rpm
	mkdir -p .build/rpm

	cp packaging/rpm/globalprotect-openconnect.spec.in .build/rpm/globalprotect-openconnect.spec
	cp packaging/rpm/globalprotect-openconnect.changes.in .build/rpm/globalprotect-openconnect.changes

	sed -i "s/@VERSION@/$(VERSION)/g" .build/rpm/globalprotect-openconnect.spec
	sed -i "s/@REVISION@/$(REVISION)/g" .build/rpm/globalprotect-openconnect.spec
	sed -i "s|@SOURCE@|$(RPM_SOURCE)|g" .build/rpm/globalprotect-openconnect.spec
	sed -i "s/@DATE@/$(shell LC_ALL=en.US date "+%a %b %d %Y")/g" .build/rpm/globalprotect-openconnect.spec

	sed -i "s/@VERSION@/$(VERSION)/g" .build/rpm/globalprotect-openconnect.changes
	sed -i "s/@DATE@/$(shell LC_ALL=en.US date -u "+%a %b %e %T %Z %Y")/g" .build/rpm/globalprotect-openconnect.changes

rpm: init-rpm tarball
	rm -rf $(HOME)/rpmbuild
	rpmdev-setuptree

	cp .build/tarball/${PKG}.tar.gz $(HOME)/rpmbuild/SOURCES/${PKG_NAME}.tar.gz
	rpmbuild -ba .build/rpm/globalprotect-openconnect.spec

	# Copy RPM package from build directory
	cp $(HOME)/rpmbuild/RPMS/$(shell uname -m)/$(PKG_NAME)*.rpm .build/rpm

	# Copy the SRPM only for x86_64.
	if [ "$(shell uname -m)" = "x86_64" ]; then \
		cp $(HOME)/rpmbuild/SRPMS/$(PKG_NAME)*.rpm .build/rpm; \
	fi

clean-pkgbuild:
	rm -rf .build/pkgbuild

init-pkgbuild: clean-pkgbuild tarball
	mkdir -p .build/pkgbuild

	cp .build/tarball/${PKG}.tar.gz .build/pkgbuild
	cp packaging/pkgbuild/PKGBUILD.in .build/pkgbuild/PKGBUILD

	sed -i "s/@PKG_NAME@/$(PKG_NAME)/g" .build/pkgbuild/PKGBUILD
	sed -i "s/@VERSION@/$(VERSION)/g" .build/pkgbuild/PKGBUILD
	sed -i "s/@REVISION@/$(REVISION)/g" .build/pkgbuild/PKGBUILD

pkgbuild: init-pkgbuild
	cd .build/pkgbuild && makepkg -s --noconfirm

clean-apk:
	rm -rf .build/apk

init-apk: clean-apk tarball
	mkdir -p .build/apk

	cp .build/tarball/${PKG}.tar.gz .build/apk
	cp packaging/apk/APKBUILD.in .build/apk/APKBUILD

	sed -i "s/@PKG_NAME@/$(PKG_NAME)/g" .build/apk/APKBUILD
	sed -i "s/@VERSION@/$(VERSION)/g" .build/apk/APKBUILD
	sed -i "s/@REVISION@/$(REVISION)/g" .build/apk/APKBUILD
	checksum=$$(sha512sum .build/apk/${PKG}.tar.gz | cut -d' ' -f1); \
		sed -i "s/@SHA512@/$$checksum/g" .build/apk/APKBUILD

apk: init-apk
	cd .build/apk && abuild -r -P "$(CURDIR)/.build/apk/packages"

	find .build/apk/packages -type f -name "$(PKG_NAME)-*.apk" -exec cp {} .build/apk \;

clean-binary:
	rm -rf .build/binary

binary: clean-binary tarball
	mkdir -p .build/binary

	cp .build/tarball/${PKG}.tar.gz .build/binary
	tar -xzf .build/binary/${PKG}.tar.gz -C .build/binary

	mkdir -p .build/binary/$(PKG_NAME)_$(VERSION)/artifacts

	make -C .build/binary/${PKG} build INCLUDE_GUI=$(INCLUDE_GUI)
	make -C .build/binary/${PKG} install DESTDIR=$(PWD)/.build/binary/$(PKG_NAME)_$(VERSION)/artifacts

	cp packaging/binary/Makefile.in .build/binary/$(PKG_NAME)_$(VERSION)/Makefile

	# Create a tarball for the binary package
	tar -cJf .build/binary/$(PKG_NAME)_$(VERSION)_$(shell uname -m).bin.tar.xz -C .build/binary $(PKG_NAME)_$(VERSION)

	# Generate sha256sum
	cd .build/binary && sha256sum $(PKG_NAME)_$(VERSION)_$(shell uname -m).bin.tar.xz | cut -d' ' -f1 > $(PKG_NAME)_$(VERSION)_$(shell uname -m).bin.tar.xz.sha256
