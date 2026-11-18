#Requires -RunAsAdministrator
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$SourceDirectory)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
& (Join-Path $source 'installer\machine-install.ps1') -Action Install -SourceDirectory $source
