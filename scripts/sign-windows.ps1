[CmdletBinding()]
param([Parameter(Mandatory=$true)][string[]]$Files)
$ErrorActionPreference = 'Stop'
foreach ($name in @('AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_SIGNING_PROFILE', 'WINDOWS_SIGNING_PUBLISHER')) {
  if (![Environment]::GetEnvironmentVariable($name)) { throw "Missing signing configuration: $name" }
}
Import-Module ArtifactSigning -RequiredVersion 0.1.20
$credentials = @{
  ExcludeEnvironmentCredential = $true
  ExcludeWorkloadIdentityCredential = $true
  ExcludeManagedIdentityCredential = $true
  ExcludeSharedTokenCacheCredential = $true
  ExcludeVisualStudioCredential = $true
  ExcludeVisualStudioCodeCredential = $true
  ExcludeAzurePowerShellCredential = $true
  ExcludeAzureDeveloperCliCredential = $true
  ExcludeInteractiveBrowserCredential = $true
}
# Azure CLI holds the short-lived GitHub OIDC login; no client secret is used.
Invoke-ArtifactSigning -Endpoint $env:AZURE_SIGNING_ENDPOINT `
  -CodeSigningAccountName $env:AZURE_SIGNING_ACCOUNT -CertificateProfileName $env:AZURE_SIGNING_PROFILE `
  -Files ($Files -join ',') -FileDigest SHA256 -TimestampDigest SHA256 `
  -TimestampRfc3161 'http://timestamp.acs.microsoft.com' -Description 'GP Connect' @credentials
& (Join-Path $PSScriptRoot 'test-windows-signature.ps1') -Files $Files -Publisher $env:WINDOWS_SIGNING_PUBLISHER
