# Starts the official Mumble server in the foreground (Windows smoke job). Must run in the SAME
# workflow step as the smoke test: GitHub runners kill processes left over when a step ends.
# Usage: pwsh start-win-server.ps1 <mumble-server.exe> <dir>
param([string]$Server, [string]$Dir)
$ErrorActionPreference = 'Stop'
# The installer may register a Windows service that grabs 64738; stop and disable it.
Get-Service | Where-Object { $_.Name -match 'mumble|murmur' -or $_.DisplayName -match 'mumble|murmur' } | ForEach-Object {
  Write-Host "stopping service $($_.Name) ($($_.Status))"
  Stop-Service $_ -Force -ErrorAction SilentlyContinue
  Set-Service $_.Name -StartupType Disabled -ErrorAction SilentlyContinue
}
Get-Process -Name 'mumble-server', 'murmur' -ErrorAction SilentlyContinue | Stop-Process -Force
$p = Start-Process $Server -ArgumentList '-fg', '-ini', "$Dir\mumble-server.ini" `
  -WorkingDirectory (Split-Path $Server) -PassThru -WindowStyle Hidden `
  -RedirectStandardOutput "$Dir\stdout.log" -RedirectStandardError "$Dir\stderr.log"
$p.Id | Set-Content "$Dir\server.pid"
$up = $false
for ($i = 0; $i -lt 30 -and -not $up; $i++) {
  if ($p.HasExited) { break }
  try { $c = [Net.Sockets.TcpClient]::new(); $c.Connect('127.0.0.1', 64738); $c.Close(); $up = $true }
  catch { Start-Sleep 1 }
}
if (-not $up) { Write-Host "server did not open port 64738 (exited: $($p.HasExited))"; exit 1 }
Write-Host "server pid $($p.Id) listening"
