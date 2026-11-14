# Requires MSYS2 base-devel/autoconf/automake/libtool and the MINGW64 gcc,
# pkgconf, gnutls, libxml2 and lz4 packages, plus Visual Studio C++ Build Tools.
[CmdletBinding()]
param(
  [string]$MsysRoot = 'C:\msys64',
  [ValidateSet('mingw64','ucrt64')][string]$MsysSubsystem = 'mingw64',
  [string]$Prefix,
  [int]$Jobs = 4
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$Prefix) { $Prefix = Join-Path $repo 'target\native-windows' }
$Prefix = [System.IO.Path]::GetFullPath($Prefix)
$bash = Join-Path $MsysRoot 'usr\bin\bash.exe'
if (!(Test-Path $bash)) { throw 'MSYS2 is required to build OpenConnect.' }
$ErrorActionPreference = 'Continue'
& $bash "$PSScriptRoot/build-openconnect-windows.sh" $repo $Prefix $Jobs $MsysSubsystem
$nativeExit = $LASTEXITCODE
$ErrorActionPreference = 'Stop'
if ($nativeExit -ne 0) { throw 'OpenConnect compilation failed.' }

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw 'Visual Studio C++ Build Tools are required.' }
$tools = (Get-ChildItem "$vs\VC\Tools\MSVC\*\bin\Hostx64\x64\lib.exe" |
  Sort-Object FullName | Select-Object -Last 1).DirectoryName
$dll = Join-Path $Prefix 'bin\libopenconnect-5.dll'
$exports = & "$tools\dumpbin.exe" /nologo /exports $dll
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect OpenConnect exports.' }
$names = @($exports | ForEach-Object {
  if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)') { $Matches[1] }
})
foreach ($symbol in @('openconnect_setup_cmd_pipe', 'openconnect_setup_tun_pipe', 'openconnect_set_configure_tun_handler', 'openconnect_set_tunnel_handler')) {
  if ($names -notcontains $symbol) { throw "OpenConnect is missing $symbol." }
}
$definition = Join-Path $Prefix 'lib\openconnect.def'
[System.IO.File]::WriteAllLines($definition, @('LIBRARY libopenconnect-5.dll', 'EXPORTS') + $names)
& "$tools\lib.exe" /nologo /machine:x64 "/def:$definition" "/out:$Prefix\lib\openconnect.lib"
if ($LASTEXITCODE -ne 0) { throw 'Could not generate the MSVC import library.' }

# Copy only the native DLL's transitive MinGW runtime dependencies.
$pending = [System.Collections.Generic.Queue[string]]::new()
$visited = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$pending.Enqueue($dll)
while ($pending.Count) {
  $current = $pending.Dequeue()
  if (!$visited.Add([System.IO.Path]::GetFileName($current))) { continue }
  $dependencies = & "$tools\dumpbin.exe" /nologo /dependents $current
  if ($LASTEXITCODE -ne 0) { throw "Could not inspect dependencies of $current." }
  foreach ($line in $dependencies) {
    if ($line -notmatch '^\s+(\S+\.dll)\s*$') { continue }
    $name = $Matches[1]
    $source = Join-Path "$MsysRoot\$MsysSubsystem\bin" $name
    if (Test-Path $source) {
      $target = Join-Path "$Prefix\bin" $name
      Copy-Item $source $target -Force
      $pending.Enqueue($target)
    } elseif (!(Test-Path "$env:SystemRoot\System32\$name") -and $name -notmatch '^(api|ext)-ms-win-') {
      throw "Missing runtime dependency: $name"
    }
  }
}
Write-Host "OpenConnect native files: $Prefix"
Write-Host "Set OPENCONNECT_WINDOWS_PREFIX to this directory before building gpservice."
