# Creates one Syrinx release key (#55) in memory and hands its private half to exactly one place.
# The owner runs it outside any agent session; it writes nothing to disk. See docs/owner-checklist.md.
#
#   -Role Primary   stores the key as a secret of the release-signing environment (gh secret set).
#   -Role Backup    puts the key on the clipboard for the password manager, then clears it.
#   -Rehearse       makes a throwaway key and runs it through the workflow's own import and
#                   delete commands, so the format is proved before a real key exists.
param(
    [ValidateSet('Primary', 'Backup')]
    [string] $Role,
    [switch] $Rehearse,
    [string] $Repository = 'Fledge-Software/Syrinx-app',
    [string] $SecretName = 'SYRINX_RELEASE_KEY',
    [string] $Environment = 'release-signing'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Lowercase hex of a byte array.
function ConvertTo-Hex([byte[]] $Bytes) {
    -join ($Bytes | ForEach-Object { $_.ToString('x2') })
}

# An exportable P-256 key that lives only in this process (no name, so never stored).
function New-ReleaseKey {
    $parameters = New-Object System.Security.Cryptography.CngKeyCreationParameters
    $parameters.ExportPolicy = [System.Security.Cryptography.CngExportPolicies]::AllowPlaintextExport
    [System.Security.Cryptography.CngKey]::Create(
        [System.Security.Cryptography.CngAlgorithm]::ECDsaP256, [NullString]::Value, $parameters)
}

# The base64 PKCS#8 text the workflow's import step reads.
function Get-SecretText($Key) {
    [Convert]::ToBase64String($Key.Export([System.Security.Cryptography.CngKeyBlobFormat]::Pkcs8PrivateBlob))
}

# The 64-byte public point (X then Y) an agent pins, and its identifier: SHA-256's first 8 bytes.
function Get-PublicIdentity($Key) {
    $blob = $Key.Export([System.Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    $p256PublicMagic = [byte[]](0x45, 0x43, 0x53, 0x31)
    if ($blob.Length -ne 72 -or (Compare-Object $blob[0..3] $p256PublicMagic)) {
        throw 'Windows did not return a P-256 public key.'
    }
    $point = [byte[]]$blob[8..71]
    $digest = [System.Security.Cryptography.SHA256]::Create().ComputeHash($point)
    [pscustomobject]@{
        Point      = ConvertTo-Hex $point
        Identifier = ConvertTo-Hex $digest[0..7]
    }
}

# Store the text as the Actions secret; gh reads it from standard input, never the command line.
function Save-PrimarySecret([string] $Text) {
    & gh auth status *> $null
    if ($LASTEXITCODE -ne 0) { throw 'Sign in with `gh auth login` first.' }
    $Text | & gh secret set $SecretName --env $Environment --repo $Repository
    if ($LASTEXITCODE -ne 0) { throw "gh secret set $SecretName failed; does the $Environment environment exist?" }
    Write-Host "Stored the primary key as the $SecretName secret of $Repository's $Environment environment."
}

# A DWORD 0 clipboard value, which turns the named clipboard feature off for this entry.
function New-ClipboardFlag {
    New-Object System.IO.MemoryStream (, [byte[]](0, 0, 0, 0))
}

# Clipboard data holding the text, kept out of clipboard monitors, history and cloud clipboard.
function New-PrivateClipboardData([string] $Text) {
    Add-Type -AssemblyName System.Windows.Forms
    $data = New-Object System.Windows.Forms.DataObject
    $data.SetData([System.Windows.Forms.DataFormats]::UnicodeText, $Text)
    $data.SetData('ExcludeClipboardContentFromMonitorProcessing', (New-ClipboardFlag))
    $data.SetData('CanIncludeInClipboardHistory', (New-ClipboardFlag))
    $data.SetData('CanUploadToCloudClipboard', (New-ClipboardFlag))
    $data
}

# Put the text on the clipboard until the owner has pasted it; clear it however the wait ends.
function Save-BackupByClipboard([string] $Text) {
    if ([System.Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA') {
        throw 'The clipboard needs a single-threaded PowerShell; run this with powershell.exe.'
    }
    $data = New-PrivateClipboardData $Text
    try {
        [System.Windows.Forms.Clipboard]::SetDataObject($data, $true)
        Write-Host 'The backup key is on the clipboard. Paste it into a new secure note in your password'
        Write-Host 'manager named "Syrinx release key (backup)", save it, then press Enter here.'
        [void](Read-Host)
    } finally {
        # Runs on Enter, on an error and on Ctrl+C alike.
        [System.Windows.Forms.Clipboard]::Clear()
        Write-Host 'Cleared the clipboard.'
    }
}

# Run a throwaway key through the workflow's import and delete commands, then compare identifiers.
function Invoke-Rehearsal($Key) {
    $identity = Get-PublicIdentity $Key
    $name = "syrinx-release-rehearsal-$([guid]::NewGuid())"
    $variable = 'SYRINX_RELEASE_KEY_REHEARSAL'
    Push-Location (Join-Path $PSScriptRoot '..\..')
    try {
        Set-Item "env:$variable" (Get-SecretText $Key)
        $output = & cargo xtask import-release-key --key $name --from-env $variable
        if ($LASTEXITCODE -ne 0) { throw 'The workflow import refused the rehearsal key.' }
        if (-not ($output -match $identity.Identifier)) {
            throw "The import reported another identifier: $output"
        }
        Write-Host "Rehearsal passed: the workflow import stored key $($identity.Identifier)."
    } finally {
        Remove-Item "env:$variable"
        & cargo xtask delete-release-key --key $name
        $deleteStatus = $LASTEXITCODE
        Pop-Location
        if ($deleteStatus -ne 0) {
            throw "Could not delete the rehearsal key; run: cargo xtask delete-release-key --key $name"
        }
    }
}

if (-not $Rehearse -and -not $Role) {
    throw 'Pass -Role Primary, -Role Backup or -Rehearse.'
}

$key = New-ReleaseKey
try {
    if ($Rehearse) {
        Invoke-Rehearsal $key
        return
    }
    $identity = Get-PublicIdentity $key
    $text = Get-SecretText $key
    if ($Role -eq 'Primary') { Save-PrimarySecret $text } else { Save-BackupByClipboard $text }
    $text = $null

    Write-Host ''
    Write-Host "Give an agent only these two public lines for the $($Role.ToLower()) key, and record"
    Write-Host 'the identifier somewhere outside the repository (the password manager note is fine):'
    Write-Host "  $Role public point: $($identity.Point)"
    Write-Host "  $Role identifier:   $($identity.Identifier)"
} finally {
    $key.Dispose()
}
