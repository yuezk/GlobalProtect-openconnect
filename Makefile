.SHELLFLAGS += -e

OFFLINE ?= 0
BUILD_FE ?= 1
INCLUDE_GUI ?= 0
CARGO ?= cargo

VERSION = $(shell $(CARGO) metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
REVISION ?= 1
PPA_REVISION ?= 1
PKG_NAME = globalprotect-openconnect
PKG = $(PKG_NAME)-$(VERSION)
SERIES ?= $(shell lsb_release -cs)
PUBLISH ?= 0

export DEBEMAIL = k3vinyue@gmail.com
export DEBFULLNAME = Kevin Yue

CARGO_BUILD_ARGS = --release

ifeq ($(OFFLINE), 1)
	CARGO_BUILD_ARGS += --frozen
endif

default: build

version:
	@echo $(VERSION)

clean-tarball:
	rm -rf .build/tarball
	rm -rf .vendor
	rm -rf vendor.tar.xz
	rm -rf .cargo

# Create a tarball, include the cargo dependencies if OFFLINE is set to 1
tarball: clean-tarball
	if [ $(BUILD_FE) -eq 1 ]; then \
		echo "Building frontend..."; \
		cd apps/gpgui-helper && pnpm install && pnpm build; \
	fi

	# Remove node_modules to reduce the tarball size
	rm -rf apps/gpgui-helper/node_modules

	mkdir -p .cargo
	mkdir -p .build/tarball

	# If OFFLINE is set to 1, vendor all cargo dependencies
	if [ $(OFFLINE) -eq 1 ]; then \
		$(CARGO) vendor .vendor > .cargo/config.toml; \
		tar -cJf vendor.tar.xz .vendor; \
	fi

	@echo "Creating tarball..."
	tar --exclude .vendor --exclude target --transform 's,^,${PKG}/,' -czf .build/tarball/${PKG}.tar.gz * .cargo

download-gui:
	rm -rf .build/gpgui

	if [ $(INCLUDE_GUI) -eq 1 ]; then \
		echo "Downloading GlobalProtect GUI..."; \
		mkdir -p .build/gpgui; \
		curl -sSL https://github.com/yuezk/GlobalProtect-openconnect/releases/download/v$(VERSION)/gpgui_$(VERSION)_$(shell uname -m).bin.tar.xz -o .build/gpgui/gpgui_$(VERSION)_x$(shell uname -m).bin.tar.xz; \
		tar -xJf .build/gpgui/*.tar.xz -C .build/gpgui; \
	else \
		echo "Skipping GlobalProtect GUI download (INCLUDE_GUI=0)"; \
	fi

build: download-gui build-fe build-rs

# Install and build the frontend
# If OFFLINE is set to 1, skip it
build-fe:
	if [ $(OFFLINE) -eq 1 ] || [ $(BUILD_FE) -eq 0 ]; then \
		echo "Skipping frontend build (OFFLINE=1 or BUILD_FE=0)"; \
	else \
		cd apps/gpgui-helper && pnpm install && pnpm build; \
	fi

	if [ ! -d apps/gpgui-helper/dist ]; then \
		echo "Error: frontend build failed"; \
		exit 1; \
	fi

build-rs:
	if [ $(OFFLINE) -eq 1 ]; then \
		tar -xJf vendor.tar.xz; \
	fi

	$(CARGO) build $(CARGO_BUILD_ARGS) -p gpclient -p gpservice -p gpauth
	$(CARGO) build $(CARGO_BUILD_ARGS) -p gpgui-helper --features "tauri/custom-protocol"

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
	install -Dm755 target/release/gpgui-helper $(DESTDIR)/usr/bin/gpgui-helper

	if [ -f .build/gpgui/gpgui_*/gpgui ]; then \
		install -Dm755 .build/gpgui/gpgui_*/gpgui $(DESTDIR)/usr/bin/gpgui; \
	fi

	install -Dm644 packaging/files/usr/share/applications/gpgui.desktop $(DESTDIR)/usr/share/applications/gpgui.desktop
	install -Dm644 packaging/files/usr/share/icons/hicolor/scalable/apps/gpgui.svg $(DESTDIR)/usr/share/icons/hicolor/scalable/apps/gpgui.svg
	install -Dm644 packaging/files/usr/share/icons/hicolor/32x32/apps/gpgui.png $(DESTDIR)/usr/share/icons/hicolor/32x32/apps/gpgui.png
	install -Dm644 packaging/files/usr/share/icons/hicolor/128x128/apps/gpgui.png $(DESTDIR)/usr/share/icons/hicolor/128x128/apps/gpgui.png
	install -Dm644 packaging/files/usr/share/icons/hicolor/256x256@2/apps/gpgui.png $(DESTDIR)/usr/share/icons/hicolor/256x256@2/apps/gpgui.png
	install -Dm644 packaging/files/usr/share/polkit-1/actions/com.yuezk.gpgui.policy $(DESTDIR)/usr/share/polkit-1/actions/com.yuezk.gpgui.policy

uninstall:
	@echo "Uninstalling $(PKG_NAME)..."

	rm -f $(DESTDIR)/usr/bin/gpclient
	rm -f $(DESTDIR)/usr/bin/gpauth
	rm -f $(DESTDIR)/usr/bin/gpservice
	rm -f $(DESTDIR)/usr/bin/gpgui-helper
	rm -f $(DESTDIR)/usr/bin/gpgui

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
	cp -f packaging/deb/rules .build/deb/$(PKG)/debian/rules

	sed -i "s/@OFFLINE@/$(OFFLINE)/g" .build/deb/$(PKG)/debian/rules

	rm -f .build/deb/$(PKG)/debian/changelog

deb: init-debian
	# Remove the rust build depdency from the control file
	sed -i "s/@RUST@//g" .build/deb/$(PKG)/debian/control

	cd .build/deb/$(PKG) && dch --create --distribution unstable --package $(PKG_NAME) --newversion $(VERSION)-$(REVISION) "Bugfix and improvements."

	cd .build/deb/$(PKG) && debuild --preserve-env -e PATH -us -uc -b

check-ppa:
	if [ $(OFFLINE) -eq 0 ]; then \
		echo "Error: ppa build requires offline mode (OFFLINE=1)"; \
	fi

# Usage: make ppa SERIES=focal OFFLINE=1 PUBLISH=1
ppa: check-ppa init-debian
	sed -i "s/@RUST@/rust-all(>=1.70)/g" .build/deb/$(PKG)/debian/control

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
	sed -i "s/@OFFLINE@/$(OFFLINE)/g" .build/rpm/globalprotect-openconnect.spec
	sed -i "s/@DATE@/$(shell date "+%a %b %d %Y")/g" .build/rpm/globalprotect-openconnect.spec

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
	sed -i "s/@OFFLINE@/$(OFFLINE)/g" .build/pkgbuild/PKGBUILD

pkgbuild: init-pkgbuild
	cd .build/pkgbuild && makepkg -s --noconfirm

clean-binary:
	rm -rf .build/binary

binary: clean-binary tarball
	mkdir -p .build/binary

	cp .build/tarball/${PKG}.tar.gz .build/binary
	tar -xzf .build/binary/${PKG}.tar.gz -C .build/binary

	mkdir -p .build/binary/$(PKG_NAME)_$(VERSION)/artifacts

	make -C .build/binary/${PKG} build OFFLINE=$(OFFLINE) BUILD_FE=0
	make -C .build/binary/${PKG} install DESTDIR=$(PWD)/.build/binary/$(PKG_NAME)_$(VERSION)/artifacts

	cp packaging/binary/Makefile.in .build/binary/$(PKG_NAME)_$(VERSION)/Makefile

	# Create a tarball for the binary package
	tar -cJf .build/binary/$(PKG_NAME)_$(VERSION)_$(shell uname -m).bin.tar.xz -C .build/binary $(PKG_NAME)_$(VERSION)

	# Generate sha256sum
	cd .build/binary && sha256sum $(PKG_NAME)_$(VERSION)_$(shell uname -m).bin.tar.xz | cut -d' ' -f1 > $(PKG_NAME)_$(VERSION)_$(shell uname -m).bin.tar.xz.sha256
