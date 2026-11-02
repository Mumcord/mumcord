# Logs SimplySign Desktop in on a CI runner, so the Certum code signing certificate appears in
# the user's certificate store (Cert:\CurrentUser\My) for signtool. Used by the Windows release
# job in .github/workflows/build-and-smoke.yml; see docs/release-signing.md.
#
# SimplySign Desktop has no command-line login, so this types the account and a fresh TOTP code
# into its login window with simulated keystrokes (WScript.Shell SendKeys), the approach other CI
# setups use as well. It is fragile by nature: it needs the interactive desktop of a hosted
# runner and depends on the window title and field order of the SimplySign version installed by
# the workflow. When it breaks, sign by hand with scripts/sign-windows.ps1.
#
# Reads CERTUM_USER, CERTUM_TOTP_SECRET and CERTUM_CERT_SHA1 from the environment. Never prints
# the secret or a code.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/totp.ps1"

$exe = if ($env:SIMPLYSIGN_EXE) { $env:SIMPLYSIGN_EXE } else { 'C:\Program Files\Certum\SimplySign Desktop\SimplySignDesktop.exe' }
foreach ($name in 'CERTUM_USER', 'CERTUM_TOTP_SECRET', 'CERTUM_CERT_SHA1') {
    if (-not [Environment]::GetEnvironmentVariable($name)) { throw "$name is not set." }
}
if (-not (Test-Path $exe)) { throw "SimplySign Desktop not found at $exe." }
$thumbprint = ($env:CERTUM_CERT_SHA1 -replace '\s', '').ToUpperInvariant()
$totp = ConvertFrom-TotpSecret $env:CERTUM_TOTP_SECRET
$key = ConvertFrom-Base32 $totp.Seed
$shell = New-Object -ComObject WScript.Shell

function Get-LoginWindow {
    Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '*SimplySign*' } |
        Select-Object -First 1
}

function Test-CertificateReady {
    $cert = Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $thumbprint } | Select-Object -First 1
    $null -ne $cert -and $cert.HasPrivateKey
}

# SendKeys treats + ^ % ~ ( ) { } [ ] as commands; braces make them literal.
function ConvertTo-SendKeysText([string]$Text) {
    [regex]::Replace($Text, '[+^%~(){}\[\]]', { param($m) "{$($m.Value)}" })
}

# The first start launches the background service and tray icon; a second start opens the
# login window when the first did not.
Start-Process $exe
$window = $null
for ($i = 0; $i -lt 20 -and -not $window; $i++) {
    Start-Sleep -Seconds 3
    $window = Get-LoginWindow
    if (-not $window -and $i -eq 3) { Start-Process $exe }
}
if (-not $window) { throw 'The SimplySign Desktop login window did not appear.' }
Write-Host "SimplySign Desktop window found: '$($window.MainWindowTitle)'."

# Two attempts at most: repeated wrong codes can lock the SimplySign account.
for ($attempt = 1; $attempt -le 2; $attempt++) {
    if ($attempt -gt 1) {
        # Close the error message of the failed attempt and return to the login form.
        [void]$shell.AppActivate($window.Id)
        Start-Sleep -Milliseconds 500
        $shell.SendKeys('{ENTER}')
        Start-Sleep -Seconds 1
    }

    # Start with at least 15 seconds of validity left, so the code cannot expire while it is
    # typed and checked by Certum.
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $left = $totp.Period - ($now % $totp.Period)
    if ($left -lt 15) { Start-Sleep -Seconds ($left + 1) }
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $code = Get-TotpCode -Key $key -UnixTime $now -Algorithm $totp.Algorithm -Digits $totp.Digits -Period $totp.Period

    if (-not $shell.AppActivate($window.Id)) { [void]$shell.AppActivate($window.MainWindowTitle) }
    Start-Sleep -Milliseconds 500
    # Login form: e-mail field first (focused), then the token field.
    $shell.SendKeys('^a')
    $shell.SendKeys((ConvertTo-SendKeysText $env:CERTUM_USER))
    Start-Sleep -Milliseconds 200
    $shell.SendKeys('{TAB}')
    $shell.SendKeys('^a')
    $shell.SendKeys($code)
    Start-Sleep -Milliseconds 200
    $shell.SendKeys('{ENTER}')
    $code = $null

    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Seconds 2
        if (Test-CertificateReady) {
            Write-Host "SimplySign login succeeded after attempt ${attempt}: certificate available."
            exit 0
        }
    }
    Write-Host "::warning::SimplySign login attempt $attempt did not make the certificate available."
}
throw 'SimplySign login failed: the certificate with thumbprint CERTUM_CERT_SHA1 did not appear in Cert:\CurrentUser\My. Sign by hand with scripts/sign-windows.ps1 (docs/release-signing.md).'
