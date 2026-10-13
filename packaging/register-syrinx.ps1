#Requires -Version 5.1
# Registers an installed Syrinx as a browser the owner can choose in Default apps (P1-37).
# The owner runs this by hand. Per-user keys only (HKCU); it never sets the default browser.
[CmdletBinding()]
param(
    # Installed program folder; defaults to the folder holding this script.
    [string]$InstallDirectory = $PSScriptRoot,
    # Open Settings at Syrinx's Default apps page afterwards.
    [switch]$OpenDefaultApps
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'syrinx-registration.ps1')
. (Join-Path $PSScriptRoot 'syrinx-common.ps1')

# Refuse before writing anything if another product already uses Syrinx's key names.
function Assert-RegistrationIsOurs {
    foreach ($owned in $SyrinxOwnedKeys) {
        if ((Get-SyrinxKeyOwner $owned) -eq 'foreign') {
            throw "HKCU\$owned exists without Syrinx's owner marker; another program may own it. Nothing was changed."
        }
    }
    foreach ($shared in $SyrinxSharedValues) {
        $current = Get-SyrinxRegistryValue $shared.Key $shared.Name
        if ($null -ne $current -and $current -cne $shared.Data) {
            throw "HKCU\$($shared.Key)\$($shared.Name) already points elsewhere. Nothing was changed."
        }
    }
}

# Fill the {exe}, {dir} and {version} placeholders from the installed copy.
function Expand-SyrinxData([string]$Template, [hashtable]$Facts) {
    $text = $Template
    foreach ($name in $Facts.Keys) { $text = $text.Replace("{$name}", $Facts[$name]) }
    return $text
}

# Write one value from the generated table under HKEY_CURRENT_USER.
function Write-SyrinxValue($Entry, [hashtable]$Facts) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($Entry.Key)
    try {
        if ($Entry.Kind -eq 'DWord') {
            $key.SetValue($Entry.Name, [int]$Entry.Data, [Microsoft.Win32.RegistryValueKind]::DWord)
        } else {
            $key.SetValue($Entry.Name, (Expand-SyrinxData $Entry.Data $Facts), [Microsoft.Win32.RegistryValueKind]::String)
        }
    } finally {
        $key.Dispose()
    }
}

# Add Syrinx to the owner's Start menu.
function New-SyrinxShortcut([string]$Executable, [string]$Folder) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Get-SyrinxShortcutPath))
    $shortcut.TargetPath = $Executable
    $shortcut.WorkingDirectory = $Folder
    $shortcut.IconLocation = "$Executable,0"
    $shortcut.Description = 'Syrinx web browser'
    $shortcut.Save()
}

$InstallDirectory = Resolve-SyrinxFolder $InstallDirectory
$executable = Join-Path $InstallDirectory $SyrinxExecutable
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw "No $SyrinxExecutable in $InstallDirectory; install Syrinx first." }
$manifest = Read-SyrinxManifest $InstallDirectory

Assert-RegistrationIsOurs
$facts = @{ exe = $executable; dir = $InstallDirectory; version = [string]$manifest.version }
foreach ($entry in @($SyrinxRegistryValues) + @($SyrinxSharedValues)) { Write-SyrinxValue $entry $facts }
New-SyrinxShortcut $executable $InstallDirectory
Update-SyrinxShellAssociations

Write-Host "Registered Syrinx $($manifest.version) at $executable for this Windows user."
if (-not $SyrinxAcceptsOpenUrl) {
    Write-Warning ("This Syrinx build cannot open links passed by Windows yet (P1-34 adds links after '$SyrinxLinkMarker'). " +
        'Do not choose it as the default browser until a build that can is installed.')
}
Write-Host 'To make Syrinx the default: Settings > Apps > Default apps > Syrinx > Set default.'
if ($OpenDefaultApps) { Start-Process "ms-settings:defaultapps?registeredAppUser=$SyrinxRegistryName" }
