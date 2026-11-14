# Windows installer

`installer.nsi` embeds a complete Windows payload and the canonical
`machine-install.ps1` installation implementation. Build it with NSIS 3 Unicode:

```powershell
& makensis /DTARGET_ARCH=x64 /DPAYLOAD_DIR=C:\build\payload /DOUTPUT_FILE=C:\build\GPConnect-Setup.exe /DPRODUCT_VERSION=3.4.0 /DBUILD_VERSION=3.4.0.0 /DDISPLAY_VERSION=3.4.0-snapshot.2026.1114.2 .\packaging\windows\installer.nsi
```

`TARGET_ARCH` is `x64` or `arm64` and must match every executable and DLL in
the payload. Each installer accepts only its native Windows architecture. ARM64
requires Windows 11; x64 requires Windows 10 build 19041 or newer.

`BUILD_VERSION` must contain four numeric components. `PRODUCT_VERSION` must
match the payload's `build-info.json`. `DISPLAY_VERSION` is the full release
identity, including the snapshot build when applicable. The payload must contain matching
`gpgui.exe`, `gpauth.exe`, `gpservice.exe`, their DLL dependency closure, the
vendor-signed `wintun.dll`, `licenses`, and `build-info.json`. The NSIS compiler
adds the generated uninstaller; it is not a separately built executable.

The installer, extraction progress and uninstaller use the shared Windows icon
from `apps/gpauth/icons/windows/icon.ico`. The Finish page uses `sidebar.bmp`,
derived from that same icon. After regenerating application icons, run
`powershell -NoProfile -File scripts/generate-windows-installer-artwork.ps1`
on Windows and retain the updated bitmap before building installers.

The development ZIP includes the same machine implementation as
`installer/machine-install.ps1`. Its install/uninstall entrypoints call that
implementation rather than maintaining a second file replacement algorithm.
Use the EXE installer to upgrade an installation that already has an NSIS
uninstaller; the development ZIP does not contain that generated executable.

Installation waits for the installed GUI/auth paths in all Windows sessions;
it does not stop arbitrary same-name processes. Service shutdown uses SCM,
checks the existing network-recovery criteria, and waits for file release.
Application files are staged under Program Files with protected ACLs. The old
directory is renamed to `GP Connect.previous` until the new service is Running.
Handled replacement/start failures restore old files and metadata and restart
the previous service. An interrupted installer with a retained backup requires
explicit repair; it is not automatically recovered.

Disk use includes the downloaded installer and one expanded new payload. NSIS
extracts into its temporary directory, then the machine implementation consumes
those disposable files into protected staging instead of copying the whole bundle.
When temp and Program Files use different volumes, file moves may temporarily
duplicate the file currently being moved. The old installation is renamed, not
copied. The machine implementation checks the installation volume before staging;
NSIS extraction errors must complete before service shutdown.
Successful installation removes the old copy and staging. Installer logs keep
the current and previous attempt in ProgramData. No updater cache is added.

The installer never launches the GUI as administrator. Users open GP Connect
from the Start menu after completion. User configuration and licensing state
are retained during upgrades and uninstall. Authenticode setup is separate;
WinSparkle must still verify the installer payload signature before handoff.

The service log rotates after 5 MiB and retains three numbered archives, keeping
each entry whole. The bundle includes `licenses/file-rotate.txt` for the Windows
service's rotation library.

On a disposable Windows installation, verify a normal A-to-B upgrade and a B
payload whose `gpservice.exe` is replaced with another valid PE executable that
cannot host a Windows service. The latter must fail service startup, restore A's
files and uninstall metadata, and leave A's service running. Also check held
runtime files, active GUI/auth windows, Wintun-signature failure, and concurrent
installer attempts. Do not run failure fixtures against a live company VPN.
