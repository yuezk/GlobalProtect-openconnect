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
if ($env:WINDOWS_SIGNING_PUBLISHER) {
  $signed = @($Installer)
  foreach ($name in @('gpgui.exe', 'gpauth.exe', 'gpservice.exe', 'gp-posture.exe', 'libopenconnect-5.dll', 'uninstall.exe', 'cli\gpclient.exe', 'cli\libopenconnect-5.dll')) {
    $signed += Join-Path $destination $name
  }
  & (Join-Path $PSScriptRoot 'test-windows-signature.ps1') -Files $signed -Publisher $env:WINDOWS_SIGNING_PUBLISHER
}
$service = Get-CimInstance Win32_Service -Filter "Name='GPConnect'"
if ($service.State -ne 'Running' -or $service.StartMode -ne 'Auto' -or $service.StartName -ne 'LocalSystem') {
  throw 'The service did not start automatically as LocalSystem.'
}
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\GP Connect.lnk'
$shell = New-Object -ComObject Shell.Application
$link = $shell.Namespace((Split-Path $shortcut)).ParseName((Split-Path $shortcut -Leaf))
if (!$link -or $link.ExtendedProperty('System.AppUserModel.ID') -ne 'com.yuezk.gpgui') {
  throw 'The Start menu shortcut does not register the notification application identity.'
}
foreach ($name in @('gpgui.exe', 'gpauth.exe')) {
  $process = Start-Process (Join-Path $destination $name) -ArgumentList '--version' -PassThru -Wait
  if ($process.ExitCode -ne 0) { throw "The installed $name cannot start." }
}
$cli = Join-Path $destination 'cli\gpclient.exe'
Push-Location $env:TEMP
try {
  $version = & $cli --version
  if ($LASTEXITCODE -ne 0 -or $version -notmatch "^gpclient $([regex]::Escape($info.version))\s") {
    throw 'The installed CLI failed its version check.'
  }
  $help = & $cli connect --help | Out-String
  if ($LASTEXITCODE -ne 0 -or $help -notmatch '--cookie-only' -or $help -match '--background') {
    throw 'The installed Windows CLI failed its connect help check.'
  }
} finally { Pop-Location }
$helper = Join-Path $destination 'gp-posture.exe'
$output = Join-Path $env:TEMP 'gp-posture-install-test.json'
$process = Start-Process $helper -PassThru -Wait -RedirectStandardOutput $output
if ($process.ExitCode -ne 0) { throw 'The installed posture helper failed.' }
$report = Get-Content $output -Raw | ConvertFrom-Json
if ($null -eq $report.anti_malware -or $null -eq $report.firewall) { throw 'Invalid posture helper output.' }
Remove-Item $output
$state = Join-Path $env:LOCALAPPDATA 'GP Connect\config\protected-state'
New-Item -ItemType Directory -Force $state | Out-Null
$marker = Join-Path $state 'ci-install-retention'
[IO.File]::WriteAllText($marker, 'retain user settings')
# Start-Process otherwise uses the executable's directory, which the uninstaller must move.
$process = Start-Process (Join-Path $destination 'uninstall.exe') -WorkingDirectory (Split-Path $destination) -ArgumentList '/S' -PassThru -Wait
if ($process.ExitCode -ne 0) { throw 'The uninstaller failed.' }
if ((Get-Service GPConnect -ErrorAction SilentlyContinue) -or (Test-Path $destination) -or (Test-Path ($destination + '.previous'))) {
  throw 'The uninstaller left service registration or runtime files.'
}
if ((Test-Path $shortcut) -or (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\GPConnect')) {
  throw 'The uninstaller left its shortcut or registration.'
}
if ([IO.File]::ReadAllText($marker) -ne 'retain user settings') { throw 'The uninstaller changed user state.' }
Remove-Item $marker
Write-Host "Installed $Architecture runtime and service, launched the desktop, authenticator and CLI, and uninstalled successfully."
