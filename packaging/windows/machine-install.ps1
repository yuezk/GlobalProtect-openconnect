#Requires -RunAsAdministrator
[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('Install', 'Uninstall')][string]$Action,
  [string]$SourceDirectory,
  [string]$Version,
  [switch]$ConsumeSource
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($PSVersionTable.PSEdition -eq 'Desktop') {
  # A PowerShell 7 caller can pass incompatible modules to Windows PowerShell.
  $env:PSModulePath = Join-Path $PSHOME 'Modules'
}
if (![Environment]::Is64BitProcess) { throw 'Run the GP Connect installer with 64-bit PowerShell.' }
$destination = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'GP Connect'
$programData = [Environment]::GetFolderPath('CommonApplicationData')
$state = Join-Path $programData 'GP Connect'
$private = Join-Path $state 'Private'
$tunnelLock = $null
$serviceName = 'GPConnect'
$serviceBinary = Join-Path $destination 'gpservice.exe'
$shortcutPath = Join-Path $programData 'Microsoft\Windows\Start Menu\Programs\GP Connect.lnk'
$uninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\GPConnect'
$recoveryKey = 'HKLM:\SOFTWARE\GP Connect\NetworkRecovery'
$publicAcl = 'O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;BU)'
$privateAcl = 'O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)'
$backup = $destination + '.previous'
$stage = $null
$oldMoved = $false
$newMoved = $false
$serviceStopped = $false
$createdService = $false
$committed = $false
$oldMetadata = $null
$oldShortcut = $null

function Assert-RegularDirectory([string]$Path) {
  if (Test-Path -LiteralPath $Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
      throw "The installation folder must be a regular directory: $Path"
    }
  }
}

function Set-DirectoryAcl([string]$Path, [string]$Descriptor) {
  Assert-RegularDirectory $Path
  if (!(Test-Path -LiteralPath $Path)) { New-Item -ItemType Directory -Path $Path | Out-Null }
  $acl = Get-Acl -LiteralPath $Path
  $acl.SetSecurityDescriptorSddlForm($Descriptor)
  Set-Acl -LiteralPath $Path -AclObject $acl
}

function Wait-ForApplications {
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    $clientPath = Join-Path $destination 'cli\gpclient.exe'
    $guiPaths = @((Join-Path $destination 'gpgui.exe'), (Join-Path $destination 'gpauth.exe'))
    $running = @(Get-CimInstance Win32_Process -Filter "Name='gpgui.exe' OR Name='gpauth.exe' OR Name='gpclient.exe'")
    if ($running.ExecutablePath -contains $clientPath) {
      throw 'Close gpclient.exe before installing or removing GP Connect.'
    }
    $running = @($running | Where-Object { $_.ExecutablePath -in $guiPaths })
    if (!$running.Count) { return }
    if ([DateTime]::UtcNow -ge $deadline) {
      throw 'Quit GP Connect and finish or cancel sign-in in all Windows sessions, then try again.'
    }
    Start-Sleep -Milliseconds 250
  } while ($true)
}

function Wait-ServiceState([string]$Expected) {
  $controller = Get-Service -Name $serviceName
  try { $controller.WaitForStatus($Expected, [TimeSpan]::FromSeconds(30)) }
  finally { $controller.Dispose() }
}

function Stop-InstalledService {
  $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
  if (!$service) { return }
  try { $stopped = $service.Status -eq 'Stopped' } finally { $service.Dispose() }
  if (!$stopped) { Stop-Service -Name $serviceName; Wait-ServiceState 'Stopped' }
}

function Start-InstalledService {
  Start-Service -Name $serviceName
  Wait-ServiceState 'Running'
}

function Move-InstalledDirectoryToBackup {
  try { Move-Item -LiteralPath $destination -Destination $backup }
  catch [IO.IOException] {
    throw 'GP Connect files are in use. Close any GP Connect folders in File Explorer and try again.'
  }
}

function Wait-ForRuntimeRelease {
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    $locked = $false
    $directories = @($destination, (Join-Path $destination 'cli')) | Where-Object { Test-Path -LiteralPath $_ -PathType Container }
    foreach ($file in Get-ChildItem -LiteralPath $directories -File | Where-Object { $_.Extension -in @('.exe', '.dll') }) {
      try {
        $stream = [IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
        $stream.Dispose()
      } catch [IO.IOException] { $locked = $true; break }
    }
    if (!$locked) { return }
    if ([DateTime]::UtcNow -ge $deadline) { throw 'An application file is still in use. Close GP Connect in all Windows sessions and try again.' }
    Start-Sleep -Milliseconds 250
  } while ($true)
}

function Open-TunnelLock {
  Set-DirectoryAcl $private $privateAcl
  $path = Join-Path $private 'system-tunnel.lock'
  try { return [IO.File]::Open($path, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
  catch [IO.IOException] {
    if (($_.Exception.GetBaseException().HResult -band 0xffff) -eq 32) {
      throw 'A GP Connect system tunnel is active. Disconnect it before running setup.'
    }
    throw
  }
}

function Assert-NetworkClean {
  $records = @()
  if (Test-Path $recoveryKey) { $records = @((Get-Item $recoveryKey).GetValueNames() | Where-Object { $_ -like 'Route-*' }) }
  $policies = @(Get-DnsClientNrptRule | Where-Object { $_.Comment -like 'GPConnect:*' })
  if ($records.Count -or $policies.Count) {
    throw 'VPN network cleanup is incomplete. Start or restart the GPConnect service before trying again.'
  }
}

function Invoke-ServiceCommand([string[]]$Arguments) {
  & "$env:SystemRoot\System32\sc.exe" @Arguments | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "Windows service configuration failed ($LASTEXITCODE)." }
}

function Set-MachineState {
  Set-DirectoryAcl $state $publicAcl
  Set-DirectoryAcl $private $privateAcl
  foreach ($name in @('gpservice.lock', 'gpservice.log')) {
    $path = Join-Path $state $name
    if (Test-Path -LiteralPath $path) {
      if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Service state file must not be a link: $path"
      }
      $acl = Get-Acl -LiteralPath $path
      $acl.SetSecurityDescriptorSddlForm($publicAcl)
      Set-Acl -LiteralPath $path -AclObject $acl
    }
  }
  $registry = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey('SOFTWARE\GP Connect')
  try {
    $recovery = $registry.CreateSubKey('NetworkRecovery', [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [Microsoft.Win32.RegistryOptions]::Volatile)
    try {
      foreach ($key in @($registry, $recovery)) {
        $acl = New-Object System.Security.AccessControl.RegistrySecurity
        $acl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)')
        $key.SetAccessControl($acl)
      }
    } finally { $recovery.Dispose() }
  } finally { $registry.Dispose() }
}

function Set-ShortcutAppUserModelId([string]$Path, [string]$ApplicationId) {
  if (-not ('GPConnect.Installer.ShortcutIdentity' -as [type])) {
    # NSIS also extracts a native System.dll; reference the loaded .NET assembly explicitly.
    Add-Type -ReferencedAssemblies ([System.Uri].Assembly.Location) -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace GPConnect.Installer {
  public static class ShortcutIdentity {
    [StructLayout(LayoutKind.Sequential)]
    private struct PropertyKey { public Guid Format; public uint Id; }

    // PROPVARIANT is 24 bytes in both supported 64-bit architectures.
    [StructLayout(LayoutKind.Explicit, Size = 24)]
    private struct PropVariant {
      [FieldOffset(0)] public ushort Type;
      [FieldOffset(8)] public IntPtr Value;
    }

    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore {
      void GetCount(out uint count);
      void GetAt(uint index, out PropertyKey key);
      void GetValue(ref PropertyKey key, out PropVariant value);
      void SetValue(ref PropertyKey key, ref PropVariant value);
      void Commit();
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    private static extern void SHGetPropertyStoreFromParsingName(
      string path, IntPtr context, uint flags, ref Guid iid, out IPropertyStore store);

    public static void Set(string path, string applicationId) {
      var iid = typeof(IPropertyStore).GUID;
      IPropertyStore store;
      SHGetPropertyStoreFromParsingName(path, IntPtr.Zero, 2, ref iid, out store); // GPS_READWRITE
      var value = new PropVariant { Type = 31, Value = Marshal.StringToCoTaskMemUni(applicationId) }; // VT_LPWSTR
      try {
        var key = new PropertyKey { Format = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), Id = 5 };
        store.SetValue(ref key, ref value); // PKEY_AppUserModel_ID
        store.Commit();
      } finally {
        Marshal.FreeCoTaskMem(value.Value);
        Marshal.ReleaseComObject(store);
      }
    }
  }
}
'@
  }
  [GPConnect.Installer.ShortcutIdentity]::Set($Path, $ApplicationId)
}

function Set-InstallationMetadata([string]$InstalledVersion) {
  $shell = New-Object -ComObject WScript.Shell
  $shortcut = $shell.CreateShortcut($shortcutPath)
  $shortcut.TargetPath = Join-Path $destination 'gpgui.exe'
  $shortcut.WorkingDirectory = $destination
  $shortcut.Description = 'A GUI client for GlobalProtect VPN'
  $shortcut.IconLocation = (Join-Path $destination 'gpgui.exe') + ',0'
  $shortcut.Save()
  Set-ShortcutAppUserModelId $shortcutPath 'com.yuezk.gpgui'
  if (Test-Path -LiteralPath (Join-Path $destination 'uninstall.exe')) {
    New-Item -Path $uninstallKey -Force | Out-Null
    $values = @{
      DisplayName = 'GP Connect'; DisplayVersion = $InstalledVersion; Publisher = 'GP Connect'
      InstallLocation = $destination; DisplayIcon = (Join-Path $destination 'gpgui.exe')
      UninstallString = '"' + (Join-Path $destination 'uninstall.exe') + '"'
    }
    foreach ($entry in $values.GetEnumerator()) {
      New-ItemProperty -Path $uninstallKey -Name $entry.Key -Value $entry.Value -PropertyType String -Force | Out-Null
    }
    foreach ($name in @('NoModify', 'NoRepair')) {
      New-ItemProperty -Path $uninstallKey -Name $name -Value 1 -PropertyType DWord -Force | Out-Null
    }
  }
}

function Restore-InstallationMetadata {
  if (Test-Path $uninstallKey) { Remove-Item $uninstallKey -Recurse -Force }
  if ($oldMetadata) {
    New-Item -Path $uninstallKey -Force | Out-Null
    foreach ($entry in $oldMetadata.GetEnumerator()) {
      New-ItemProperty -Path $uninstallKey -Name $entry.Key -Value $entry.Value.Value -PropertyType $entry.Value.Kind -Force | Out-Null
    }
  }
  if ($oldShortcut) { [IO.File]::WriteAllBytes($shortcutPath, $oldShortcut) }
  elseif (Test-Path -LiteralPath $shortcutPath) { Remove-Item -LiteralPath $shortcutPath -Force }
}

function Remove-OwnedCallback {
  $callback = 'HKCU:\Software\Classes\globalprotectcallback'
  $commandKey = "$callback\shell\open\command"
  if (Test-Path $commandKey) {
    $command = (Get-Item $commandKey).GetValue('')
    $authBinary = Join-Path $destination 'gpauth.exe'
    $owned = @("`"$authBinary`" auth-callback `"%1`"", "`"\\?\$authBinary`" auth-callback `"%1`"")
    if ($command -in $owned) { Remove-Item $callback -Recurse -Force }
  }
}

$mutex = New-Object System.Threading.Mutex($false, 'Global\GPConnect.Installer')
$owned = $false
$transcript = $false
try {
  try { $owned = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $owned = $true }
  if (!$owned) { throw 'Another GP Connect installation is running. Wait for it to finish.' }
  Assert-RegularDirectory $destination
  Assert-RegularDirectory $state
  if (Test-Path -LiteralPath $backup) {
    throw "A previous installation needs repair. The retained files are in $backup. Do not delete them before restoring GP Connect."
  }
  # Keep only this attempt and one previous diagnostic log.
  Set-DirectoryAcl $state $publicAcl
  $log = Join-Path $state 'installer.log'
  $previousLog = Join-Path $state 'installer.previous.log'
  foreach ($path in @($log, $previousLog)) {
    if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
      throw 'Installer log must not be a link.'
    }
  }
  if (Test-Path -LiteralPath $previousLog) { Remove-Item -LiteralPath $previousLog -Force }
  if (Test-Path -LiteralPath $log) { Move-Item -LiteralPath $log -Destination $previousLog }
  Start-Transcript -Path $log -Force | Out-Null
  $transcript = $true
  $existing = Get-CimInstance Win32_Service -Filter "Name='$serviceName'"
  if ($existing -and $existing.PathName -ne ('"' + $serviceBinary + '"')) {
    throw 'The GPConnect service is registered to another installation.'
  }
  $wasRunning = $existing -and $existing.State -ne 'Stopped'

  if ($Action -eq 'Install') {
    if (!$SourceDirectory) { throw 'An installation source is required.' }
    $source = (Resolve-Path -LiteralPath $SourceDirectory).Path.TrimEnd('\')
    if ($source -eq $destination -or $source.StartsWith($destination + '\', [StringComparison]::OrdinalIgnoreCase)) {
      throw 'Install from an extracted package outside the application folder.'
    }
    foreach ($name in @('gpservice.exe', 'gpauth.exe', 'gpgui.exe', 'gp-posture.exe', 'libopenconnect-5.dll', 'wintun.dll', 'build-info.json', 'cli\gpclient.exe', 'cli\libopenconnect-5.dll', 'cli\wintun.dll')) {
      if (!(Test-Path -LiteralPath (Join-Path $source $name) -PathType Leaf)) { throw "Missing Windows package file: $name" }
    }
    $info = Get-Content -LiteralPath (Join-Path $source 'build-info.json') -Raw | ConvertFrom-Json
    if ((Test-Path -LiteralPath (Join-Path $destination 'uninstall.exe')) -and
        !(Test-Path -LiteralPath (Join-Path $source 'uninstall.exe'))) {
      throw 'Use the Windows installer to update this installation.'
    }
    if (!$Version) { $Version = $info.version }
    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    if ($info.product -ne 'GP Connect' -or $info.architecture -notin @('x64','arm64') -or $info.architecture -ne $architecture -or !$Version -or $info.version -ne $Version) {
      throw 'The package version or architecture does not match this installer.'
    }
    $build = [Environment]::OSVersion.Version.Build
    if ($architecture -eq 'arm64' -and $build -lt 22000) {
      throw 'GP Connect for ARM64 requires Windows 11.'
    }
    if ($architecture -eq 'x64' -and $build -lt 17763) {
      throw 'GP Connect for x64 requires Windows 10 version 1809 or later.'
    }
    if ((Get-ItemPropertyValue 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -Name InstallationType) -eq 'Server Core') {
      throw 'GP Connect requires Windows Server with Desktop Experience.'
    }
    foreach ($library in @('mfplat.dll', 'mfreadwrite.dll')) {
      if (!(Test-Path -LiteralPath (Join-Path ([Environment]::SystemDirectory) $library) -PathType Leaf)) {
        throw 'GP Connect requires Media Foundation. On Windows N, install the Media Feature Pack, then run setup again.'
      }
    }
    $cliDirectory = Join-Path $source 'cli'
    Assert-RegularDirectory $cliDirectory
    foreach ($item in Get-ChildItem -LiteralPath $cliDirectory -Force) {
      if ($item.PSIsContainer -or ($item.Name -ne 'gpclient.exe' -and $item.Extension -ne '.dll')) {
        throw "Unexpected Windows CLI runtime file: $($item.Name)"
      }
    }
    $payload = @(Get-ChildItem -LiteralPath $source -Force | Where-Object {
      ($_.PSIsContainer -and $_.Name -in @('licenses', 'cli')) -or
      (!$_.PSIsContainer -and ($_.Extension -eq '.dll' -or
        $_.Name -in @('gpservice.exe', 'gpauth.exe', 'gpgui.exe', 'gp-posture.exe', 'uninstall.exe', 'build-info.json')))
    })
    foreach ($item in $payload) {
      $items = @($item)
      if ($item.PSIsContainer) { $items += @(Get-ChildItem -LiteralPath $item.FullName -Force -Recurse) }
      if ($items | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'The package must not contain file links.' }
    }
    $requiredBytes = [long]0
    foreach ($item in $payload) {
      if ($item.PSIsContainer) { $requiredBytes += [long](Get-ChildItem -LiteralPath $item.FullName -File -Recurse | Measure-Object -Property Length -Sum).Sum }
      else { $requiredBytes += $item.Length }
    }
    $volume = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($destination))
    $additionalBytes = $requiredBytes + 16MB
    if ($ConsumeSource -and [IO.Path]::GetPathRoot($source) -eq $volume.Name) {
      # The extracted files already occupy this volume; renaming them adds no copy.
      $additionalBytes = 16MB
    }
    if ($volume.AvailableFreeSpace -lt $additionalBytes) { throw 'There is not enough free space to install GP Connect.' }
    $stage = $destination + '.stage-' + [Guid]::NewGuid().ToString('N')
    Set-DirectoryAcl $stage $publicAcl
    foreach ($item in $payload) {
      if (!$ConsumeSource) {
        Copy-Item -LiteralPath $item.FullName -Destination $stage -Recurse -Force
        continue
      }
      # NSIS's extracted payload is disposable. Move files individually so temp
      # and Program Files may be on different volumes without keeping two bundles.
      $files = @($item)
      if ($item.PSIsContainer) { $files = @(Get-ChildItem -LiteralPath $item.FullName -File -Recurse) }
      foreach ($file in $files) {
        $relative = $file.Name
        if ($item.PSIsContainer) {
          # Enumerated paths share the same expanded spelling, including 8.3 temp paths.
          $child = $file.FullName.Substring($item.FullName.Length).TrimStart('\')
          $relative = Join-Path $item.Name $child
        }
        $target = Join-Path $stage $relative
        $parent = Split-Path $target -Parent
        if (!(Test-Path -LiteralPath $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
        Move-Item -LiteralPath $file.FullName -Destination $target
      }
    }
    # Copies inherit the staging directory's protected policy, even when source ACLs differ.
    foreach ($item in Get-ChildItem -LiteralPath $stage -Force -Recurse) {
      $acl = Get-Acl -LiteralPath $item.FullName
      $acl.SetSecurityDescriptorSddlForm($publicAcl)
      Set-Acl -LiteralPath $item.FullName -AclObject $acl
    }
    $signature = Get-AuthenticodeSignature (Join-Path $stage 'wintun.dll')
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'WireGuard LLC') {
      throw 'The package requires the vendor-signed Wintun driver DLL.'
    }
    foreach ($dll in Get-ChildItem (Join-Path $stage 'cli') -File -Filter '*.dll') {
      $shared = Join-Path $stage $dll.Name
      if ((Test-Path -LiteralPath $shared) -and (Get-FileHash $shared).Hash -ne (Get-FileHash $dll.FullName).Hash) {
        throw "The Windows CLI runtime file differs from the application copy: $($dll.Name)"
      }
    }
    if ((Test-Path -LiteralPath $destination) -and !$existing) { throw 'The service is not registered. Repair the existing installation before updating.' }
    if ($existing -and !(Test-Path -LiteralPath $destination)) { throw 'The registered installation is missing. Restore its files before updating.' }
    if (Test-Path $uninstallKey) {
      $key = Get-Item $uninstallKey
      $oldMetadata = @{}
      foreach ($name in $key.GetValueNames()) { $oldMetadata[$name] = @{ Value = $key.GetValue($name); Kind = $key.GetValueKind($name).ToString() } }
    }
    if (Test-Path -LiteralPath $shortcutPath) { $oldShortcut = [IO.File]::ReadAllBytes($shortcutPath) }
    Wait-ForApplications
    if ($existing) {
      $serviceStopped = $true
      Stop-InstalledService
    }
    $tunnelLock = Open-TunnelLock
    if ($existing) {
      Assert-NetworkClean
      Wait-ForRuntimeRelease
    }
    if (Test-Path -LiteralPath $destination) { Move-InstalledDirectoryToBackup; $oldMoved = $true }
    Move-Item -LiteralPath $stage -Destination $destination
    $stage = $null
    $newMoved = $true
    Set-MachineState
    if (!$existing) {
      New-Service -Name $serviceName -DisplayName 'GP Connect' -BinaryPathName ('"' + $serviceBinary + '"') -StartupType Automatic -Description 'Connects GP Connect to your GlobalProtect VPN network.' | Out-Null
      $createdService = $true
    } else { Set-Service -Name $serviceName -StartupType Automatic }
    Invoke-ServiceCommand @('failure', $serviceName, 'reset=', '86400', 'actions=', 'restart/3000/restart/10000/none/0')
    Set-InstallationMetadata $Version
    $tunnelLock.Dispose(); $tunnelLock = $null
    Start-InstalledService
    $committed = $true
    if ($oldMoved) { Remove-Item -LiteralPath $backup -Recurse -Force; $oldMoved = $false }
    Write-Host "Installed GP Connect $Version. Open it from the Start menu."
  } else {
    if (!$existing) {
      if (Test-Path -LiteralPath $destination) { throw 'The service is not registered. Review the installation folder before removing it.' }
      Write-Host 'GP Connect is not installed.'
      return
    }
    Wait-ForApplications
    $serviceStopped = $true
    Stop-InstalledService
    $tunnelLock = Open-TunnelLock
    Assert-NetworkClean
    Wait-ForRuntimeRelease
    # Move first: a locked runtime aborts while its registration and files are intact.
    Move-InstalledDirectoryToBackup
    $oldMoved = $true
    Invoke-ServiceCommand @('delete', $serviceName)
    $committed = $true
    Remove-Item -LiteralPath $backup -Recurse -Force
    $oldMoved = $false
    if (Test-Path -LiteralPath $shortcutPath) { Remove-Item -LiteralPath $shortcutPath -Force }
    if (Test-Path $uninstallKey) { Remove-Item $uninstallKey -Recurse -Force }
    if (Test-Path $recoveryKey) { Remove-Item $recoveryKey -Force }
    Remove-OwnedCallback
    Write-Host 'Removed GP Connect. User settings and service logs are kept.'
  }
} catch {
  $failure = $_
  if (!$committed) {
    try {
      if ($newMoved) { Stop-InstalledService; Remove-Item -LiteralPath $destination -Recurse -Force }
      if ($oldMoved) { Move-Item -LiteralPath $backup -Destination $destination; $oldMoved = $false }
      if ($createdService) { Invoke-ServiceCommand @('delete', $serviceName) }
      if ($newMoved) { Restore-InstallationMetadata }
      if ($tunnelLock) { $tunnelLock.Dispose(); $tunnelLock = $null }
      if ($existing -and $serviceStopped -and $wasRunning) { Start-InstalledService }
    } catch {
      Write-Host "Could not restore GP Connect: $($_.Exception.Message)"
      Write-Host "Keep any retained files in $backup and run the installer again after restoring the installation."
    }
  }
  Write-Host "Installation failed: $($failure.Exception.Message)"
  Write-Host "Details: $(Join-Path $state 'installer.log')"
  throw $failure
} finally {
  if ($tunnelLock) { $tunnelLock.Dispose() }
  if ($stage -and (Test-Path -LiteralPath $stage)) {
    try { Remove-Item -LiteralPath $stage -Recurse -Force } catch { Write-Warning "Could not remove temporary files: $stage" }
  }
  if ($transcript) { Stop-Transcript | Out-Null }
  if ($owned) { $mutex.ReleaseMutex() }
  $mutex.Dispose()
}
