#Requires -Version 5.1
# Registers an installed Atlas as a browser the owner can choose in Default apps (P1-37).
# The owner runs this by hand. Per-user keys only (HKCU); it never sets the default browser.
[CmdletBinding()]
param(
    # Installed program folder; defaults to the folder holding this script.
    [string]$InstallDirectory = $PSScriptRoot,
    # Open Settings at Atlas's Default apps page afterwards.
    [switch]$OpenDefaultApps
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'atlas-registration.ps1')
. (Join-Path $PSScriptRoot 'atlas-common.ps1')

# Refuse before writing anything if another product already uses Atlas's key names.
function Assert-RegistrationIsOurs {
    foreach ($owned in $AtlasOwnedKeys) {
        if ((Get-AtlasKeyOwner $owned) -eq 'foreign') {
            throw "HKCU\$owned exists without Atlas's owner marker; another program may own it. Nothing was changed."
        }
    }
    foreach ($shared in $AtlasSharedValues) {
        $current = Get-AtlasRegistryValue $shared.Key $shared.Name
        if ($null -ne $current -and $current -cne $shared.Data) {
            throw "HKCU\$($shared.Key)\$($shared.Name) already points elsewhere. Nothing was changed."
        }
    }
}

# Fill the {exe}, {dir} and {version} placeholders from the installed copy.
function Expand-AtlasData([string]$Template, [hashtable]$Facts) {
    $text = $Template
    foreach ($name in $Facts.Keys) { $text = $text.Replace("{$name}", $Facts[$name]) }
    return $text
}

# Write one value from the generated table under HKEY_CURRENT_USER.
function Write-AtlasValue($Entry, [hashtable]$Facts) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($Entry.Key)
    try {
        if ($Entry.Kind -eq 'DWord') {
            $key.SetValue($Entry.Name, [int]$Entry.Data, [Microsoft.Win32.RegistryValueKind]::DWord)
        } else {
            $key.SetValue($Entry.Name, (Expand-AtlasData $Entry.Data $Facts), [Microsoft.Win32.RegistryValueKind]::String)
        }
    } finally {
        $key.Dispose()
    }
}

# Add Atlas to the owner's Start menu.
function New-AtlasShortcut([string]$Executable, [string]$Folder) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Get-AtlasShortcutPath))
    $shortcut.TargetPath = $Executable
    $shortcut.WorkingDirectory = $Folder
    $shortcut.IconLocation = "$Executable,0"
    $shortcut.Description = 'Atlas web browser'
    $shortcut.Save()
}

$InstallDirectory = Resolve-AtlasFolder $InstallDirectory
$executable = Join-Path $InstallDirectory $AtlasExecutable
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw "No $AtlasExecutable in $InstallDirectory; install Atlas first." }
$manifest = Read-AtlasManifest $InstallDirectory

Assert-RegistrationIsOurs
$facts = @{ exe = $executable; dir = $InstallDirectory; version = [string]$manifest.version }
foreach ($entry in @($AtlasRegistryValues) + @($AtlasSharedValues)) { Write-AtlasValue $entry $facts }
New-AtlasShortcut $executable $InstallDirectory
Update-AtlasShellAssociations

Write-Host "Registered Atlas $($manifest.version) at $executable for this Windows user."
if (-not $AtlasAcceptsOpenUrl) {
    Write-Warning ("This Atlas build cannot open links passed by Windows yet (P1-34 adds $AtlasOpenUrlSwitch). " +
        'Do not choose it as the default browser until a build that can is installed.')
}
Write-Host 'To make Atlas the default: Settings > Apps > Default apps > Atlas > Set default.'
if ($OpenDefaultApps) { Start-Process "ms-settings:defaultapps?registeredAppUser=$AtlasRegistryName" }
