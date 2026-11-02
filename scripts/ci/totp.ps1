# RFC 6238 TOTP for the SimplySign login in CI (scripts/ci/simplysign-login.ps1). Dot-source
# it. Written out here instead of using a third-party action, because the TOTP secret is the
# long-lived key to the signing certificate and must not pass through code we do not control.

# Reads the secret Certum shows at activation: either the full otpauth:// URI from the QR code
# (preferred: it states the algorithm, digits and period) or the bare base32 seed. A bare seed
# uses Certum's parameters: HMAC-SHA256, 6 digits, 30 seconds.
function ConvertFrom-TotpSecret {
    param([Parameter(Mandatory)][string]$Secret)

    $settings = @{ Seed = $Secret.Trim(); Algorithm = 'SHA256'; Digits = 6; Period = 30 }
    if ($settings.Seed -match '^otpauth://') {
        $query = ($settings.Seed -split '\?', 2)[1]
        if (-not $query) { throw 'The otpauth URI has no parameters.' }
        $settings.Seed = $null
        foreach ($pair in $query -split '&') {
            $name, $value = $pair -split '=', 2
            $value = [Uri]::UnescapeDataString("$value")
            switch ($name.ToLowerInvariant()) {
                'secret' { $settings.Seed = $value }
                'algorithm' { $settings.Algorithm = $value.ToUpperInvariant() }
                'digits' { $settings.Digits = [int]$value }
                'period' { $settings.Period = [int]$value }
            }
        }
        if (-not $settings.Seed) { throw 'The otpauth URI has no secret parameter.' }
    }
    if ($settings.Algorithm -notin 'SHA1', 'SHA256') {
        throw "Unsupported TOTP algorithm '$($settings.Algorithm)' (expected SHA1 or SHA256)."
    }
    if ($settings.Digits -lt 6 -or $settings.Digits -gt 8) { throw 'TOTP digits must be 6 to 8.' }
    if ($settings.Period -lt 1) { throw 'TOTP period must be positive.' }
    $settings
}

function ConvertFrom-Base32 {
    param([Parameter(Mandatory)][string]$Text)

    $alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
    $clean = ($Text -replace '[\s=-]', '').ToUpperInvariant()
    $bytes = [System.Collections.Generic.List[byte]]::new()
    $buffer = 0
    $bits = 0
    foreach ($char in $clean.ToCharArray()) {
        $index = $alphabet.IndexOf($char)
        if ($index -lt 0) { throw 'The TOTP secret is not valid base32.' }
        $buffer = (($buffer -shl 5) -bor $index) -band 0xFFFF
        $bits += 5
        if ($bits -ge 8) {
            $bits -= 8
            $bytes.Add([byte](($buffer -shr $bits) -band 0xFF))
        }
    }
    if ($bytes.Count -eq 0) { throw 'The TOTP secret is empty.' }
    , $bytes.ToArray()
}

# Returns the code for the given Unix time (seconds) as a zero-padded string.
function Get-TotpCode {
    param(
        [Parameter(Mandatory)][byte[]]$Key,
        [Parameter(Mandatory)][long]$UnixTime,
        [string]$Algorithm = 'SHA256',
        [int]$Digits = 6,
        [int]$Period = 30
    )

    $counter = [BitConverter]::GetBytes([long][Math]::Floor($UnixTime / $Period))
    if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($counter) }
    $hmac = if ($Algorithm -eq 'SHA1') {
        [System.Security.Cryptography.HMACSHA1]::new($Key)
    } else {
        [System.Security.Cryptography.HMACSHA256]::new($Key)
    }
    try { $hash = $hmac.ComputeHash($counter) } finally { $hmac.Dispose() }
    $offset = $hash[$hash.Length - 1] -band 0x0F
    $binary = (([int]$hash[$offset] -band 0x7F) -shl 24) -bor
        (([int]$hash[$offset + 1]) -shl 16) -bor
        (([int]$hash[$offset + 2]) -shl 8) -bor
        ([int]$hash[$offset + 3])
    ([long]$binary % [long][Math]::Pow(10, $Digits)).ToString().PadLeft($Digits, '0')
}
