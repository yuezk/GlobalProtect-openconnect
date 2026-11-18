# Regenerate the NSIS sidebar from the shared Windows application icon.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName PresentationCore
$root = Split-Path $PSScriptRoot -Parent
$iconPath = Join-Path $root 'apps\gpauth\icons\windows\icon.ico'
$outputPath = Join-Path $root 'packaging\windows\sidebar.bmp'
# WPF decodes PNG-compressed ICO frames correctly; GDI+ Icon.ToBitmap does not.
$decoder = [Windows.Media.Imaging.BitmapDecoder]::Create(
  [Uri]$iconPath,
  [Windows.Media.Imaging.BitmapCreateOptions]::PreservePixelFormat,
  [Windows.Media.Imaging.BitmapCacheOption]::OnLoad
)
$frame = $decoder.Frames | Sort-Object PixelWidth -Descending | Select-Object -First 1
$encoder = New-Object Windows.Media.Imaging.PngBitmapEncoder
$encoder.Frames.Add($frame)
$stream = New-Object IO.MemoryStream
$encoder.Save($stream)
$stream.Position = 0
$image = [Drawing.Image]::FromStream($stream)
$canvas = New-Object Drawing.Bitmap(164, 314, ([Drawing.Imaging.PixelFormat]::Format24bppRgb))
$graphics = [Drawing.Graphics]::FromImage($canvas)
try {
  $graphics.Clear([Drawing.Color]::White)
  $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $graphics.DrawImage($image, 34, 54, 96, 96)
  $canvas.Save($outputPath, [Drawing.Imaging.ImageFormat]::Bmp)
} finally {
  $graphics.Dispose()
  $canvas.Dispose()
  $image.Dispose()
  $stream.Dispose()
}
