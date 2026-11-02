# Signs Windows files with the Certum code signing certificate from SimplySign Desktop, then
# verifies each signature. The release workflow calls it from Tauri's signCommand for
# mumcord.exe, the uninstaller and the NSIS installer; maintainers run the same script by hand
# when the automated SimplySign login fails (docs/release-signing.md).
#
# Needs SimplySign Desktop logged in (the certificate is then in Cert:\CurrentUser\My) and the
# certificate's SHA-1 thumbprint in -Thumbprint or CERTUM_CERT_SHA1.
#
#   pwsh -File scripts/sign-windows.ps1 path\to\file.exe [more files...]
#
# $Path needs an explicit Position: a ValueFromRemainingArguments parameter gets no implicit
# position, so a lone file path (how Tauri's signCommand and the NSIS uninstaller call this
# script) would bind to -Thumbprint instead and leave $Path empty. It is checked below rather
# than marked Mandatory so a missing path fails with a message naming what was received.
param(
    [Parameter(Position = 0, ValueFromRemainingArguments)][string[]]$Path,
    [string]$Thumbprint = $env:CERTUM_CERT_SHA1
)
$ErrorActionPreference = 'Stop'

if (-not $Path) {
    # Name only the parameters, never the thumbprint's value.
    $received = if ($PSBoundParameters.Count) { ($PSBoundParameters.Keys | ForEach-Object { "-$_" }) -join ', ' } else { 'none' }
    throw "No file to sign: pass the file path(s) as arguments. Parameters received: $received."
}

if (-not $Thumbprint) { throw 'Pass -Thumbprint or set CERTUM_CERT_SHA1 to the certificate thumbprint.' }
$Thumbprint = ($Thumbprint -replace '\s', '').ToUpperInvariant()

# signtool from SIGNTOOL, the PATH, or the newest x64 Windows SDK (preinstalled on GitHub's
# Windows runners and installed with Visual Studio's C++ tools).
function Find-SignTool {
    if ($env:SIGNTOOL) { return $env:SIGNTOOL }
    $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem $sdk -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Name -eq 'x64' -and $_.Directory.Parent.Name -match '^\d+(\.\d+)+$' } |
        Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
        Select-Object -First 1
    if (-not $found) { throw 'signtool.exe not found: install the Windows SDK or set SIGNTOOL.' }
    $found.FullName
}

$signtool = Find-SignTool
foreach ($file in $Path) {
    if (-not (Test-Path $file -PathType Leaf)) { throw "$file not found." }
    & $signtool sign /sha1 $Thumbprint /fd sha256 /tr http://time.certum.pl /td sha256 /d Mumcord $file
    if ($LASTEXITCODE -ne 0) { throw "signtool sign failed for $file (exit code $LASTEXITCODE)." }
    & $signtool verify /pa /v $file
    if ($LASTEXITCODE -ne 0) { throw "signtool verify failed for $file (exit code $LASTEXITCODE)." }
}
