# Signs Windows files with the Certum code signing certificate from SimplySign Desktop, then
# verifies each signature. The release workflow calls it from Tauri's signCommand for
# mumcord.exe, the uninstaller and the NSIS installer; maintainers run the same script by hand
# when the automated SimplySign login fails (docs/release-signing.md).
#
# Needs SimplySign Desktop logged in (the certificate is then in Cert:\CurrentUser\My) and the
# certificate's SHA-1 thumbprint in -Thumbprint or CERTUM_CERT_SHA1.
#
#   pwsh -File scripts/sign-windows.ps1 path\to\file.exe [more files...]
param(
    [Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Path,
    [string]$Thumbprint = $env:CERTUM_CERT_SHA1
)
$ErrorActionPreference = 'Stop'

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
