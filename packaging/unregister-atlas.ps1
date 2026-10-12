#Requires -Version 5.1
# Removes Atlas's browser registration and, with -Uninstall, its program files (P1-37).
# The owner runs this by hand or from Installed apps. Data is kept unless the owner types DELETE.
[CmdletBinding()]
param(
    # Also remove the registered program folder and offer to delete Atlas's data.
    [switch]$Uninstall,
    # Also offer the development preview folder (AtlasProductPreview), asked separately.
    [switch]$IncludePreviewData,
    # Wait for Enter before closing; Installed apps passes this so messages stay readable.
    [switch]$PauseAtEnd,
    # Installed program folder; defaults to the folder holding this script.
    [string]$InstallDirectory = $PSScriptRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'atlas-registration.ps1')
. (Join-Path $PSScriptRoot 'atlas-common.ps1')

# Delete Atlas's keys only where its owner marker shows Atlas wrote them.
function Remove-AtlasRegistration {
    foreach ($shared in $AtlasSharedValues) {
        if ((Get-AtlasRegistryValue $shared.Key $shared.Name) -cne $shared.Data) { continue }
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($shared.Key, $true)
        try { $key.DeleteValue($shared.Name, $false) } finally { $key.Dispose() }
    }
    foreach ($owned in $AtlasOwnedKeys) {
        $owner = Get-AtlasKeyOwner $owned
        if ($owner -eq 'foreign') { Write-Warning "HKCU\$owned lacks Atlas's owner marker; left in place." }
        if ($owner -ne 'ours') { continue }
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($owned, $false)
    }
}

# Remove the Start menu shortcut only when it starts Atlas from the given folder.
function Remove-AtlasShortcut([string]$Folder) {
    $path = Get-AtlasShortcutPath
    if (-not (Test-Path -LiteralPath $path)) { return }
    $target = (New-Object -ComObject WScript.Shell).CreateShortcut($path).TargetPath
    if ($target -ine (Join-Path $Folder $AtlasExecutable)) {
        Write-Warning "$path starts $target, not this Atlas; left in place."
        return
    }
    Remove-Item -LiteralPath $path
}

# Remove the program folder and its kept versions, only where registration names this folder.
function Remove-AtlasProgramFiles([string]$Folder, $Registered) {
    if ($null -eq $Registered) {
        Write-Warning "No Atlas registration names a program folder, so none was deleted. Delete $Folder by hand if wanted."
        return
    }
    foreach ($candidate in @($Folder, "$Folder.previous", "$Folder.new")) {
        if (-not (Test-Path -LiteralPath $candidate)) { continue }
        Remove-AtlasOwnedFolder $candidate
        Write-Host "Removed $candidate."
    }
}

# Ask about one data folder; anything but DELETE keeps it, and leftovers are reported.
function Remove-AtlasDataIfConfirmed([string]$Relative, [string]$Description) {
    $folder = Get-AtlasLocalPath $Relative
    if (-not (Test-Path -LiteralPath $folder)) { return }
    Write-Host ''
    Write-Host "$Description`: $folder"
    Write-Host 'It holds saved pages, history, settings, backups and signed-in website sessions.'
    $answer = Read-Host 'Type DELETE to erase it permanently, or press Enter to keep it'
    if ($answer -cne 'DELETE') {
        Write-Host "Kept $folder."
        return
    }
    Remove-Item -LiteralPath $folder -Recurse -Force -ErrorAction SilentlyContinue -ErrorVariable problems
    if (-not (Test-Path -LiteralPath $folder)) {
        Write-Host "Deleted $folder."
        return
    }
    $left = @(Get-ChildItem -LiteralPath $folder -Recurse -Force -ErrorAction SilentlyContinue).Count
    Write-Warning ("$folder was only partly deleted: $left items remain ($($problems.Count) errors). " +
        'A WebView2 process may still hold files; close every Atlas window, then delete the folder by hand.')
}

# Unregister, then optionally uninstall; returns normally or throws.
function Invoke-AtlasUnregister {
    $folder = Resolve-AtlasFolder $InstallDirectory
    $registered = Get-AtlasRegisteredFolder
    if ($Uninstall -and $null -ne $registered -and $registered -ine $folder) {
        throw "Atlas is registered at $registered, not $folder. Run this script from $registered; nothing was changed."
    }
    Assert-AtlasClosed @($folder, "$folder.previous", "$folder.new")
    Remove-AtlasRegistration
    Remove-AtlasShortcut $folder
    Update-AtlasShellAssociations
    Write-Host 'Removed Atlas from Default apps, Installed apps and the Start menu.'
    Write-Host 'If Atlas was your default browser, Windows will ask which browser to use for the next link.'

    if (-not $Uninstall) {
        Write-Host "Program files in $folder and all Atlas data were kept."
        return
    }
    Remove-AtlasProgramFiles $folder $registered
    Remove-AtlasDataIfConfirmed $AtlasDataFolder 'Atlas data'
    if ($IncludePreviewData) {
        Remove-AtlasDataIfConfirmed $AtlasPreviewDataFolder 'Development preview data (not created by the installed Atlas)'
    } elseif (Test-Path -LiteralPath (Get-AtlasLocalPath $AtlasPreviewDataFolder)) {
        Write-Host "Development preview data in %LOCALAPPDATA%\$AtlasPreviewDataFolder was kept; -IncludePreviewData offers it."
    }
}

$failed = $false
try {
    Invoke-AtlasUnregister
} catch {
    $failed = $true
    Write-Host "Atlas unregister stopped: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    if ($PauseAtEnd) { Read-Host 'Press Enter to close' | Out-Null }
}
if ($failed) { exit 1 }
