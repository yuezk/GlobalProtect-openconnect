# Windows installer

`installer.nsi` embeds a complete Windows payload and the canonical
`machine-install.ps1` installation implementation. Build it with NSIS 3 Unicode:

```powershell
& makensis /DTARGET_ARCH=x64 /DPAYLOAD_DIR=C:\build\payload /DOUTPUT_FILE=C:\build\GPConnect-Setup.exe /DPRODUCT_VERSION=3.4.0 /DBUILD_VERSION=3.4.0.0 /DDISPLAY_VERSION=3.4.0-snapshot.2026.1114.2 .\packaging\windows\installer.nsi
```

`TARGET_ARCH` is `x64` or `arm64` and must match every executable and DLL in
the payload. Each installer accepts only its native Windows architecture. ARM64
requires Windows 11; x64 requires Windows 10 build 19041 or newer.

The native library build accepts `scripts/build-openconnect-windows.ps1 -TunnelType 'GP Connect'`
(the default). This passes OpenConnect's `--with-wintun-tunnel-type` build option;
OpenConnect itself defaults to `OpenConnect`. Wintun appends ` Tunnel` to the
selected type. The gateway-based connection name is unchanged.

`BUILD_VERSION` must contain four numeric components. `PRODUCT_VERSION` must
match the payload's `build-info.json`. `DISPLAY_VERSION` is the full release
identity, including the snapshot build when applicable. The payload must contain matching
`gpgui.exe`, `gpauth.exe`, `gpservice.exe`, their DLL dependency closure, the
vendor-signed `wintun.dll`, `licenses`, and `build-info.json`. The NSIS compiler
adds the generated uninstaller; it is not a separately built executable.

The installer, extraction progress and uninstaller use the shared Windows icon
from `apps/gpauth/icons/windows/icon.ico`. The confirmation and Finish pages use `sidebar.bmp`,
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
are retained during upgrades and uninstall. Release builds use Azure Artifact
Signing with GitHub OIDC and RFC 3161 timestamps. Application executables,
the built OpenConnect DLL, and the generated NSIS uninstaller and installer
are signed before checksums and WinSparkle signatures are generated.
Vendor DLL signatures are preserved. WinSparkle independently verifies its
installer payload signature before handoff.

The GitHub `windows-signing` environment is trusted by the Entra application
`gp-connect-github-signing`. Its only Azure role is Artifact Signing Certificate
Profile Signer on `tangramly-signing/certificateProfiles/gp-connect`.
Environment deployment policies allow the `release/next` branch and `v*` tags.
Repository variables supply `AZURE_SIGNING_CLIENT_ID`, `AZURE_TENANT_ID`,
`AZURE_SUBSCRIPTION_ID`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`,
`AZURE_SIGNING_PROFILE`, and `WINDOWS_SIGNING_PUBLISHER`. No Azure client secret
or Authenticode private key is stored in GitHub. `build-windows.ps1 -Sign` requires
Microsoft's ArtifactSigning PowerShell module 0.1.20 and an Azure CLI login.
The build verifies the publisher, certificate trust and timestamp on each signed
file. The installer smoke test repeats those checks on the installed runtime
and uninstaller before testing removal.

The Start menu shortcut registers `System.AppUserModel.ID` as `com.yuezk.gpgui`,
matching the GUI and native toast sender. Windows uses the shortcut's name and
executable icon to identify GP Connect in Notification Center. Its identity is
part of the shortcut, so the existing shortcut rollback and uninstall own its cleanup.

Manual launches show an Install/Cancel confirmation page (Update/Cancel when
GP Connect is already installed). The update feed supplies `/UPDATE` through
WinSparkle's `sparkle:installerArguments` to skip that page after the user has
already confirmed the in-app update. This is not silent mode: elevation,
installation progress, errors, and the Finish page remain visible.

The service log rotates after 5 MiB and retains three numbered archives, keeping
each entry whole. The bundle includes `licenses/file-rotate.txt` for the Windows
service's rotation library.

On a disposable Windows installation, verify a normal A-to-B upgrade and a B
payload whose `gpservice.exe` is replaced with another valid PE executable that
cannot host a Windows service. The latter must fail service startup, restore A's
files and uninstall metadata, and leave A's service running. Also check held
runtime files, active GUI/auth windows, Wintun-signature failure, and concurrent
installer attempts. Do not run failure fixtures against a live company VPN.
