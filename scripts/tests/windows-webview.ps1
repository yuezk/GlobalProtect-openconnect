# Run in an unlocked Windows desktop. Start windows-webview-server.py first.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Binary,
  [string]$FixtureUrl = 'http://127.0.0.1:18769',
  [string]$TlsUrl = 'https://localhost:18770'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class GPWebviewFixture {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int height, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
$work = Join-Path $env:TEMP ('gp-webview-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $work | Out-Null
$previousProfile = $env:WEBVIEW2_USER_DATA_FOLDER
# Microsoft's override isolates cookie tests from the user's real sign-in profile.
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $work 'profile'
$script:current = $null
$script:index = 0
function Start-Auth([string]$Request, [string[]]$Extra = @()) {
  $script:index++
  $script:stdout = Join-Path $work "$script:index.json"
  $script:stderr = Join-Path $work "$script:index.log"
  $arguments = @($FixtureUrl, '--saml-request', ('"' + $Request + '"'), '--window-title', '"GP Auth Fixture"', '--os', 'Windows') + $Extra
  $script:current = Start-Process -FilePath $Binary -ArgumentList $arguments -PassThru -RedirectStandardOutput $script:stdout -RedirectStandardError $script:stderr
  $null = $script:current.Handle
}
function Finish-Auth([bool]$Success) {
  if (!$script:current.WaitForExit(20000)) { throw "Authentication timed out in case $script:index" }
  if ($script:current.ExitCode -ne 0) { throw "Authenticator exited with $($script:current.ExitCode): $(Get-Content $script:stderr -Tail 8 | Out-String)" }
  $result = Get-Content $script:stdout -Raw | ConvertFrom-Json
  if ($Success) {
    if ($result.success.username -ne 'webview-fixture') { throw "Expected successful fixture authentication in case $script:index" }
  } elseif ($result -ne 'cancelled') { throw "Expected cancellation in case $script:index" }
  $script:current = $null
}
function Find-Control([string]$Name) {
  $processCondition = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:current.Id)
  $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $processCondition)
  $condition = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $Name)
  foreach ($window in $windows) {
    $node = $window.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
    if ($node) { return $node }
  }
}
function Wait-Control([string]$Name) {
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  do {
    $node = Find-Control $Name
    if ($node -and !$node.Current.IsOffscreen) { return $node }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline -and !$script:current.HasExited)
  throw "Missing '$Name' in authentication case $script:index"
}
function Invoke-Control([string]$Name) {
  (Wait-Control $Name).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Close-Auth {
  $processCondition = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:current.Id)
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  do {
    $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $processCondition)
    foreach ($window in $windows) {
      if ($window.Current.Name -eq 'GP Auth Fixture' -and !$window.Current.IsOffscreen) {
        [GPWebviewFixture]::PostMessage([IntPtr]$window.Current.NativeWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
        return
      }
    }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw 'Sign-in window did not become visible'
}
try {
  foreach ($path in @('/body', '/headers', '/redirect', '/callback', '/user-agent')) {
    Start-Auth ($FixtureUrl + $path)
    Finish-Auth $true
    "PASS: $path"
  }
  Start-Auth '<html><saml-auth-status>1</saml-auth-status><saml-username>webview-fixture</saml-username><prelogin-cookie>synthetic-html-cookie</prelogin-cookie></html>'
  Finish-Auth $true
  'PASS: supplied HTML'
  Start-Auth ($FixtureUrl + '/cookie-set')
  Finish-Auth $true
  Start-Auth ($FixtureUrl + '/cookie-present')
  Finish-Auth $true
  Start-Auth ($FixtureUrl + '/cookie-clean') @('--clean')
  Finish-Auth $true
  'PASS: cookie persistence and clean sign-in'
  Start-Auth ($FixtureUrl + '/signin')
  $link = Wait-Control 'Complete fixture sign-in'
  $condition = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:current.Id)
  $roots = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $condition)
  $root = @($roots | Where-Object { $_.Current.Name -eq 'GP Auth Fixture' })[0]
  if (!$root) { throw 'Missing sign-in window' }
  $editCondition = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty, 'Edit')
  $address = $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $editCondition)
  if (!$address) { throw 'Missing native address bar' }
  $bounds = $address.Current.BoundingRectangle
  if ($link.Current.BoundingRectangle.Top -lt $bounds.Bottom) { throw 'Webview overlaps the address bar' }
  [GPWebviewFixture]::SetForegroundWindow([IntPtr]$root.Current.NativeWindowHandle) | Out-Null
  Start-Sleep -Milliseconds 200
  $point = New-Object GPWebviewFixture+POINT
  $point.X = [int]($bounds.Left + $bounds.Width / 2); $point.Y = [int]($bounds.Top + $bounds.Height / 2)
  if ([GPWebviewFixture]::WindowFromPoint($point) -ne [IntPtr]$address.Current.NativeWindowHandle) { throw 'Native address bar is covered' }
  $frame = $root.Current.BoundingRectangle
  [GPWebviewFixture]::SetWindowPos([IntPtr]$root.Current.NativeWindowHandle, [IntPtr]::Zero, 0, 0, [int]$frame.Width - 40, [int]$frame.Height - 40, 6) | Out-Null
  Start-Sleep -Milliseconds 300
  $bounds = $address.Current.BoundingRectangle
  $point.X = [int]($bounds.Left + $bounds.Width / 2); $point.Y = [int]($bounds.Top + $bounds.Height / 2)
  if ($link.Current.BoundingRectangle.Top -lt $bounds.Bottom -or [GPWebviewFixture]::WindowFromPoint($point) -ne [IntPtr]$address.Current.NativeWindowHandle) { throw 'Resizing broke the native address bar layout' }
  $link.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
  Finish-Auth $true
  'PASS: interactive embedded sign-in'
  Start-Auth ($FixtureUrl + '/signin')
  Close-Auth
  Finish-Auth $false
  'PASS: window cancellation'
  Start-Auth ($FixtureUrl + '/rejected')
  Invoke-Control 'Cancel'
  Finish-Auth $false
  'PASS: rejected sign-in recovery'
  Start-Auth ($FixtureUrl + '/temporary')
  Finish-Auth $true
  'PASS: automatic fresh-prelogin recovery'
  Start-Auth ($TlsUrl + '/body') @('--ignore-tls-errors')
  Finish-Auth $true
  'PASS: explicit ignore-TLS option'
  Start-Auth ($TlsUrl + '/body')
  Invoke-Control ('Continue Anyway' + [char]0x2026)
  Invoke-Control 'Yes'
  Finish-Auth $true
  'PASS: certificate warning and confirmed exception'
  Start-Auth ($TlsUrl + '/body')
  Invoke-Control 'Cancel'
  Finish-Auth $false
  'PASS: certificate exception does not survive the sign-in window'
  Start-Auth ($TlsUrl + '/cross-host')
  Invoke-Control ('Continue Anyway' + [char]0x2026)
  Invoke-Control 'Yes'
  Invoke-Control 'Cancel'
  Finish-Auth $false
  'PASS: certificate exception is scoped to the approved host'
} finally {
  if ($script:current -and !$script:current.HasExited) { Stop-Process -Id $script:current.Id; $script:current.WaitForExit() }
  $env:WEBVIEW2_USER_DATA_FOLDER = $previousProfile
  # WebView2's browser process releases its profile shortly after the window closes.
  for ($attempt = 0; $attempt -lt 10; $attempt++) {
    try { Remove-Item $work -Recurse -Force; break } catch { Start-Sleep -Milliseconds 500 }
  }
  if (Test-Path $work) { Write-Warning "Fixture profile remains at $work" }
}
