$ErrorActionPreference = 'Stop'
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
switch ($request.operation) {
  'add' {
    Add-DnsClientNrptRule -Namespace ([string[]]$request.namespaces) -NameServers ([string[]]$request.servers) -Comment $request.tag | Out-Null
  }
  'remove' {
    Get-DnsClientNrptRule | Where-Object { $_.Comment -eq $request.tag } | ForEach-Object {
      Remove-DnsClientNrptRule -Name $_.Name -Force
    }
  }
  'clear-stale' {
    Get-DnsClientNrptRule | Where-Object { $_.Comment -like 'GPConnect:*' } | ForEach-Object {
      Remove-DnsClientNrptRule -Name $_.Name -Force
    }
  }
  default { throw 'Invalid DNS policy operation' }
}
