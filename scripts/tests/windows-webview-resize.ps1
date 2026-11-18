# Run in an unlocked Windows desktop; drives only its own isolated fixture.
param([Parameter(Mandatory)][string]$Binary)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class GPAuthLiveResize {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
}
'@
[GPAuthLiveResize]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$work=Join-Path $env:TEMP ('gp-resize-test-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $work | Out-Null
$previous=$env:WEBVIEW2_USER_DATA_FOLDER
$env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $work 'profile'
$old=New-Object GPAuthLiveResize+POINT
[GPAuthLiveResize]::GetCursorPos([ref]$old) | Out-Null
$p=$null
try {
  $p=Start-Process $Binary -ArgumentList @('https://preview.invalid','--saml-request','"<html><body><h1>Resize fixture</h1></body></html>"','--window-title','"GP Auth Resize Test"') -PassThru -RedirectStandardOutput "$work\result.json" -RedirectStandardError "$work\stderr.log"
  $deadline=[DateTime]::UtcNow.AddSeconds(12)
  $condition=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty,[int]$p.Id)
  $root=$null
  do {
    $roots=[Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,$condition)
    $root=@($roots | Where-Object {$_.Current.Name -eq 'GP Auth Resize Test' -and !$_.Current.IsOffscreen})[0]
    if ($root) {break}
    Start-Sleep -Milliseconds 100
  } while([DateTime]::UtcNow -lt $deadline)
  if (!$root) {throw 'Resize fixture did not open'}
  $hwnd=[GPAuthLiveResize]::GetAncestor([IntPtr]$root.Current.NativeWindowHandle,2)
  [GPAuthLiveResize]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 300
  $name=New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')
  $address=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$name)
  $bounds=$address.Current.BoundingRectangle
  $sample=New-Object GPAuthLiveResize+POINT
  $sample.X=[int]($bounds.Left+300);$sample.Y=[int]($bounds.Top+$bounds.Height/2)
  $pixel=New-Object Drawing.Bitmap(1,1)
  $graphics=[Drawing.Graphics]::FromImage($pixel)
  function Pixel {
    $graphics.CopyFromScreen($sample.X,$sample.Y,0,0,$pixel.Size)
    $pixel.GetPixel(0,0).ToArgb()
  }
  $baseline=Pixel
  $frame=New-Object GPAuthLiveResize+RECT
  [GPAuthLiveResize]::GetWindowRect($hwnd,[ref]$frame) | Out-Null
  $startX=$frame.Right-2;$startY=[int](($frame.Top+$frame.Bottom)/2)
  $edge=New-Object GPAuthLiveResize+POINT
  $edge.X=$startX;$edge.Y=$startY
  if ([GPAuthLiveResize]::GetAncestor([GPAuthLiveResize]::WindowFromPoint($edge),2) -ne $hwnd) {throw 'Resize edge is covered'}
  [GPAuthLiveResize]::SetCursorPos($startX,$startY) | Out-Null
  [GPAuthLiveResize]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  $bad=0;$total=0
  try {
    foreach($direction in @(1,-1,1,-1,1,-1)) {
      for($i=1;$i -le 40;$i++) {
        $offset=6*$i
        if ($direction -lt 0) {$offset=6*(40-$i)}
        [GPAuthLiveResize]::SetCursorPos($startX+$offset,$startY) | Out-Null
        Start-Sleep -Milliseconds 8
        $value=Pixel
        $total++
        if ($value -ne $baseline) {$bad++}
      }
    }
  } finally {[GPAuthLiveResize]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
  "Address samples while dragging: $bad hidden of $total; baseline $baseline"
  if ($bad -gt 0) {throw 'Address bar disappears during live resizing'}
  'PASS: address bar remains visible throughout live resize'
} finally {
  [GPAuthLiveResize]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  [GPAuthLiveResize]::SetCursorPos($old.X,$old.Y) | Out-Null
  if ($graphics) {$graphics.Dispose();$pixel.Dispose()}
  if ($p -and !$p.HasExited) {Stop-Process -Id $p.Id}
  $env:WEBVIEW2_USER_DATA_FOLDER=$previous
  Start-Sleep -Milliseconds 500
  Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
