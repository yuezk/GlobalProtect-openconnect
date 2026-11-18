[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string[]]$Files,
  [Parameter(Mandatory=$true)][string]$Publisher
)
$ErrorActionPreference = 'Stop'
foreach ($file in $Files) {
  $signature = Get-AuthenticodeSignature -LiteralPath $file
  if ($signature.Status -ne 'Valid') { throw "Invalid Authenticode signature: $file ($($signature.Status))" }
  $name = $signature.SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
  if ($name -ne $Publisher) { throw "Unexpected signing publisher for ${file}: $name" }
  if (!$signature.TimeStamperCertificate) { throw "Missing signature timestamp: $file" }
  Write-Host "Verified $file (publisher: $name; timestamped)"
}
