#Requires -Version 5.1
# Removes Atlas's browser registration and, with -Uninstall, its program files (P1-37).
# The owner runs this by hand or from Installed apps. Atlas's data is kept unless the owner types DELETE.
[CmdletBinding()]
param(
    # Also remove the program folder and offer to delete data.
    [switch]$Uninstall,
    # Installed program folder; defaults to the folder holding this script.
    [string]$InstallDirectory = $PSScriptRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'atlas-registration.ps1')
. (Join-Path $PSScriptRoot 'atlas-common.ps1')

# Delete the keys Atlas owns and its single values in shared keys, nothing else.
function Remove-AtlasRegistration {
    foreach ($entry in $AtlasSharedValues) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($entry.Key, $true)
        if ($null -eq $key) { continue }
        try { $key.DeleteValue($entry.Name, $false) } finally { $key.Dispose() }
    }
    foreach ($owned in $AtlasOwnedKeys) {
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($owned, $false)
    }
}

# Remove the program folder and its kept previous version, only if this tooling made them.
function Remove-AtlasProgramFiles([string]$Folder) {
    foreach ($candidate in @($Folder, "$Folder.previous", "$Folder.new")) {
        if (-not (Test-Path -LiteralPath $candidate)) { continue }
        if (-not (Test-AtlasProgramFolder $candidate)) {
            Write-Warning "$candidate was not made by Atlas's install script; left in place."
            continue
        }
        Remove-Item -LiteralPath $candidate -Recurse -Force
        Write-Host "Removed $candidate."
    }
}

# Ask about each data folder separately; anything but DELETE keeps it.
function Remove-AtlasDataIfConfirmed {
    foreach ($relative in $AtlasDataFolders) {
        $folder = Get-AtlasLocalPath $relative
        if (-not (Test-Path -LiteralPath $folder)) { continue }
        Write-Host ''
        Write-Host "Atlas data: $folder"
        Write-Host 'It holds your saved pages, history, settings, backups and signed-in website sessions.'
        $answer = Read-Host 'Type DELETE to erase it permanently, or press Enter to keep it'
        if ($answer -ceq 'DELETE') {
            Remove-Item -LiteralPath $folder -Recurse -Force
            Write-Host "Deleted $folder."
        } else {
            Write-Host "Kept $folder."
        }
    }
}

$InstallDirectory = [System.IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
Assert-AtlasClosed $InstallDirectory
Remove-AtlasRegistration
$shortcut = Get-AtlasShortcutPath
if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut }
Update-AtlasShellAssociations
Write-Host 'Removed Atlas from Default apps, Installed apps and the Start menu.'
Write-Host 'If Atlas was your default browser, Windows will ask which browser to use for the next link.'

if (-not $Uninstall) {
    Write-Host "Program files in $InstallDirectory and all Atlas data were kept."
    return
}
Remove-AtlasProgramFiles $InstallDirectory
Remove-AtlasDataIfConfirmed
