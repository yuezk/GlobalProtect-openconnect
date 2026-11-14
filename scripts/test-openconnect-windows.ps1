[CmdletBinding()]
param([string]$MsysRoot = 'C:\msys64', [string]$Prefix = $env:OPENCONNECT_WINDOWS_PREFIX)
$repo = Split-Path $PSScriptRoot -Parent
if (!$Prefix) { $Prefix = Join-Path $repo 'target\native-windows' }
& "$MsysRoot\usr\bin\bash.exe" "$PSScriptRoot/test-openconnect-windows.sh" $repo $Prefix
if ($LASTEXITCODE -ne 0) { throw 'Native OpenConnect packet-pipe tests failed.' }
