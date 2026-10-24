# Prints Mumble server logs and process/port status (Windows smoke job, on failure).
param([string]$Dir)
$ErrorActionPreference = 'Continue'
foreach ($f in 'stdout.log', 'stderr.log', 'mumble-server.log') {
  Write-Host "--- $Dir\$f"; Get-Content "$Dir\$f" -ErrorAction SilentlyContinue
}
Write-Host '--- processes'; Get-Process -Name 'mumble-server', 'murmur' -ErrorAction SilentlyContinue | Out-String | Write-Host
Write-Host '--- services'; Get-Service | Where-Object { $_.Name -match 'mumble|murmur' } | Out-String | Write-Host
Write-Host '--- port 64738'; Get-NetTCPConnection -LocalPort 64738 -ErrorAction SilentlyContinue | Out-String | Write-Host
