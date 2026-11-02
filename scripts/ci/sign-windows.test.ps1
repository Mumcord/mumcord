# Checks how scripts/sign-windows.ps1 binds its arguments, with a stub signtool that only records
# its calls. Each case starts a separate pwsh -File process, the way Tauri's signCommand and the
# NSIS uninstaller (!uninstfinalize) call the script. Runs in the release workflow before the
# SimplySign login, and locally with `pwsh -NoProfile -File scripts/ci/sign-windows.test.ps1`.
# Exits non-zero on the first failure.
$ErrorActionPreference = 'Stop'

function Assert-Equal($Actual, $Expected, $What) {
    if ($Actual -ne $Expected) { throw "$What`: expected '$Expected', got '$Actual'" }
}

$script = Join-Path $PSScriptRoot '..\sign-windows.ps1'
$pwsh = (Get-Process -Id $PID).Path
$work = Join-Path ([IO.Path]::GetTempPath()) "sign-windows-test-$PID"
New-Item -ItemType Directory -Force (Join-Path $work 'dir with space') | Out-Null
$calls = Join-Path $work 'calls.txt'
$stub = Join-Path $work 'signtool.ps1'
Set-Content $stub '"$args" | Add-Content $env:SIGNTOOL_CALLS; exit 0'
$app = Join-Path $work 'mumcord.exe'
$uninstaller = Join-Path $work 'dir with space\__uninstaller.exe'
New-Item -ItemType File -Force $app, $uninstaller | Out-Null

# Runs the script in a fresh process and returns its exit code and the signtool calls.
function Invoke-Sign([string[]]$Arguments) {
    Remove-Item $calls -ErrorAction SilentlyContinue
    $env:SIGNTOOL = $stub
    $env:SIGNTOOL_CALLS = $calls
    $env:CERTUM_CERT_SHA1 = 'ab cd'
    & $pwsh -NoProfile -NonInteractive -File $script @Arguments 2>&1 | Out-Null
    $code = $LASTEXITCODE
    $recorded = if (Test-Path $calls) { @(Get-Content $calls) } else { @() }
    @{ Code = $code; Calls = $recorded }
}

try {
    # Tauri's signCommand replaces the '%1' argument with the file path.
    $r = Invoke-Sign @($app)
    Assert-Equal $r.Code 0 'single path: exit code'
    Assert-Equal $r.Calls.Count 2 'single path: signtool calls'
    Assert-Equal $r.Calls[0] "sign /sha1 ABCD /fd sha256 /tr http://time.certum.pl /td sha256 /d Mumcord $app" 'single path: sign call'
    Assert-Equal $r.Calls[1] "verify /pa /v $app" 'single path: verify call'

    # NSIS puts the uninstaller's path, which may contain spaces, in place of a quoted "%1".
    $r = Invoke-Sign @($uninstaller)
    Assert-Equal $r.Code 0 'path with spaces: exit code'
    Assert-Equal $r.Calls[0] "sign /sha1 ABCD /fd sha256 /tr http://time.certum.pl /td sha256 /d Mumcord $uninstaller" 'path with spaces: sign call'

    # Maintainers sign several files at once by hand (docs/release-signing.md).
    $r = Invoke-Sign @($app, $uninstaller)
    Assert-Equal $r.Code 0 'two paths: exit code'
    Assert-Equal $r.Calls.Count 4 'two paths: signtool calls'

    $r = Invoke-Sign @('-Thumbprint', 'ef', $app)
    Assert-Equal $r.Code 0 'explicit thumbprint: exit code'
    Assert-Equal $r.Calls[0] "sign /sha1 EF /fd sha256 /tr http://time.certum.pl /td sha256 /d Mumcord $app" 'explicit thumbprint: sign call'

    $r = Invoke-Sign @()
    Assert-Equal ($r.Code -ne 0) $true 'no path: fails'
    Assert-Equal $r.Calls.Count 0 'no path: signtool calls'
} finally {
    Remove-Item -Recurse -Force $work
}
Write-Host 'sign-windows.ps1 tests passed.'
# The last case runs the script so that it fails on purpose; without this its exit code would
# leak out as $LASTEXITCODE and fail the CI step even though every assertion passed.
exit 0
