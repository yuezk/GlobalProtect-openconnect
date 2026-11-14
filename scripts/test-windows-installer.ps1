# Exercise a candidate installer only on an empty CI machine.
[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string]$Installer,
  [Parameter(Mandatory=$true)][ValidateSet('x64','arm64')][string]$Architecture,
  [Parameter(Mandatory=$true)][AllowEmptyString()][string]$Build
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$destination = Join-Path $env:ProgramFiles 'GP Connect'
if ((Test-Path $destination) -or (Get-Service GPConnect -ErrorAction SilentlyContinue)) {
  throw 'Run this installer test on a clean Windows test machine.'
}
$process = Start-Process $Installer -ArgumentList '/S' -PassThru -Wait
if ($process.ExitCode -ne 0) {
  # This test requires an empty CI installation; this log contains only installer operations.
  $log = Join-Path $env:ProgramData 'GP Connect\installer.log'
  if (Test-Path $log) { Get-Content $log -Tail 80 | Write-Host }
  throw "The installer failed with exit code $($process.ExitCode)."
}
$info = Get-Content (Join-Path $destination 'build-info.json') -Raw | ConvertFrom-Json
if ($info.architecture -ne $Architecture -or [string]$info.build_version -ne $Build) { throw 'The installed package identity does not match.' }
$service = Get-CimInstance Win32_Service -Filter "Name='GPConnect'"
if ($service.State -ne 'Running' -or $service.StartMode -ne 'Auto' -or $service.StartName -ne 'LocalSystem') {
  throw 'The service did not start automatically as LocalSystem.'
}
foreach ($name in @('gpgui.exe', 'gpauth.exe')) {
  $process = Start-Process (Join-Path $destination $name) -ArgumentList '--version' -PassThru -Wait
  if ($process.ExitCode -ne 0) { throw "The installed $name cannot start." }
}
$state = Join-Path $env:LOCALAPPDATA 'GP Connect\config\protected-state'
New-Item -ItemType Directory -Force $state | Out-Null
$marker = Join-Path $state 'ci-install-retention'
[IO.File]::WriteAllText($marker, 'retain user settings')
$process = Start-Process (Join-Path $destination 'uninstall.exe') -ArgumentList '/S' -PassThru -Wait
if ($process.ExitCode -ne 0) { throw 'The uninstaller failed.' }
if ((Get-Service GPConnect -ErrorAction SilentlyContinue) -or (Test-Path $destination) -or (Test-Path ($destination + '.previous'))) {
  throw 'The uninstaller left service registration or runtime files.'
}
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\GP Connect.lnk'
if ((Test-Path $shortcut) -or (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\GPConnect')) {
  throw 'The uninstaller left its shortcut or registration.'
}
if ([IO.File]::ReadAllText($marker) -ne 'retain user settings') { throw 'The uninstaller changed user state.' }
Remove-Item $marker
Write-Host "Installed $Architecture runtime and service, launched both applications, and uninstalled successfully."
