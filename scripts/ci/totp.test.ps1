# Checks scripts/ci/totp.ps1 against the RFC 6238 test vectors (appendix B) and Base32 / otpauth
# parsing. Runs in the release workflow before the SimplySign login, and locally with
# `pwsh -NoProfile -File scripts/ci/totp.test.ps1`. Exits non-zero on the first failure.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/totp.ps1"

function Assert-Equal($Actual, $Expected, $What) {
    if ($Actual -ne $Expected) { throw "$What`: expected '$Expected', got '$Actual'" }
}

$sha1Key = [Text.Encoding]::ASCII.GetBytes('12345678901234567890')
$sha256Key = [Text.Encoding]::ASCII.GetBytes('12345678901234567890123456789012')
$vectors = @(
    @{ Time = 59; Sha1 = '94287082'; Sha256 = '46119246' },
    @{ Time = 1111111109; Sha1 = '07081804'; Sha256 = '68084774' },
    @{ Time = 1234567890; Sha1 = '89005924'; Sha256 = '91819424' },
    @{ Time = 20000000000; Sha1 = '65353130'; Sha256 = '77737706' }
)
foreach ($v in $vectors) {
    Assert-Equal (Get-TotpCode -Key $sha1Key -UnixTime $v.Time -Algorithm SHA1 -Digits 8) $v.Sha1 "SHA1 at $($v.Time)"
    Assert-Equal (Get-TotpCode -Key $sha256Key -UnixTime $v.Time -Algorithm SHA256 -Digits 8) $v.Sha256 "SHA256 at $($v.Time)"
}

# 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ' is base32 for the SHA1 test key above.
$decoded = ConvertFrom-Base32 'gezd gnbv gy3t qojq gezd gnbv gy3t qojq'
Assert-Equal ([Text.Encoding]::ASCII.GetString($decoded)) '12345678901234567890' 'base32 decoding'

$bare = ConvertFrom-TotpSecret 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'
Assert-Equal "$($bare.Algorithm)/$($bare.Digits)/$($bare.Period)" 'SHA256/6/30' 'bare seed defaults'
$uri = ConvertFrom-TotpSecret 'otpauth://totp/Certum:user%40example.org?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&algorithm=sha1&digits=8&period=60'
Assert-Equal "$($uri.Seed)/$($uri.Algorithm)/$($uri.Digits)/$($uri.Period)" 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ/SHA1/8/60' 'otpauth parameters'

$failed = $false
try { ConvertFrom-TotpSecret 'otpauth://totp/x?secret=AAAA&algorithm=SHA512' | Out-Null } catch { $failed = $true }
if (-not $failed) { throw 'SHA512 must be rejected' }

Write-Host 'TOTP self-test passed.'
