#Requires -Version 5.1
# Removes Syrinx's browser registration and, with -Uninstall, its program files (P1-37).
# The owner runs this by hand or from Installed apps. Data is kept unless the owner types DELETE.
[CmdletBinding()]
param(
    # Also remove the registered program folder and offer to delete Syrinx's data.
    [switch]$Uninstall,
    # Also offer the development preview folder (SyrinxProductPreview), asked separately.
    [switch]$IncludePreviewData,
    # Wait for Enter before closing; Installed apps passes this so messages stay readable.
    [switch]$PauseAtEnd,
    # Installed program folder; defaults to the folder holding this script.
    [string]$InstallDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'syrinx-registration.ps1')
. (Join-Path $PSScriptRoot 'syrinx-common.ps1')
# Windows PowerShell 5.1 leaves $PSScriptRoot empty in a param default under -File, so default here.
if (-not $InstallDirectory) { $InstallDirectory = $PSScriptRoot }

# Delete Syrinx's keys only where its owner marker shows Syrinx wrote them.
function Remove-SyrinxRegistration {
    foreach ($shared in $SyrinxSharedValues) {
        if ((Get-SyrinxRegistryValue $shared.Key $shared.Name) -cne $shared.Data) { continue }
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($shared.Key, $true)
        try { $key.DeleteValue($shared.Name, $false) } finally { $key.Dispose() }
    }
    foreach ($owned in $SyrinxOwnedKeys) {
        $owner = Get-SyrinxKeyOwner $owned
        if ($owner -eq 'foreign') { Write-Warning "HKCU\$owned lacks Syrinx's owner marker; left in place." }
        if ($owner -ne 'ours') { continue }
        [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($owned, $false)
    }
}

# Remove the Start menu shortcut only when it starts Syrinx from the given folder.
function Remove-SyrinxShortcut([string]$Folder) {
    $path = Get-SyrinxShortcutPath
    if (-not (Test-Path -LiteralPath $path)) { return }
    $target = (New-Object -ComObject WScript.Shell).CreateShortcut($path).TargetPath
    if ($target -ine (Join-Path $Folder $SyrinxExecutable)) {
        Write-Warning "$path starts $target, not this Syrinx; left in place."
        return
    }
    Remove-Item -LiteralPath $path
}

# True when this script runs from an installed Syrinx folder: a retry after registration is gone.
function Test-RunningFromInstalledFolder([string]$Folder) {
    if ($Folder -ine (Resolve-SyrinxFolder $PSScriptRoot)) { return $false }
    if (-not (Test-SyrinxProgramFolder $Folder)) { return $false }
    Read-SyrinxManifest $Folder | Out-Null
    return (Test-Path -LiteralPath (Join-Path $Folder $SyrinxExecutable) -PathType Leaf)
}

# Remove the program folder and its kept versions, only where registration names this folder,
# or, on a retry with registration already removed, where this script runs from that folder.
function Remove-SyrinxProgramFiles([string]$Folder, $Registered) {
    if ($null -eq $Registered -and -not (Test-RunningFromInstalledFolder $Folder)) {
        Write-Warning "No Syrinx registration names $Folder and this script is not running from it, so no program files were deleted."
        return
    }
    foreach ($candidate in @($Folder, "$Folder.previous", "$Folder.new")) {
        if (-not (Test-Path -LiteralPath $candidate)) { continue }
        Remove-SyrinxOwnedFolder $candidate
        Write-Host "Removed $candidate."
    }
}

# Ask about one data folder; anything but DELETE keeps it, and leftovers are reported.
function Remove-SyrinxDataIfConfirmed([string]$Relative, [string]$Description) {
    $folder = Get-SyrinxLocalPath $Relative
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
        'A WebView2 process may still hold files; close every Syrinx window, then delete the folder by hand.')
}

# Unregister, then optionally uninstall; returns normally or throws.
function Invoke-SyrinxUnregister {
    $folder = Resolve-SyrinxFolder $InstallDirectory
    $registered = Get-SyrinxRegisteredFolder
    if ($Uninstall -and $null -ne $registered -and $registered -ine $folder) {
        throw "Syrinx is registered at $registered, not $folder. Run this script from $registered; nothing was changed."
    }
    Assert-SyrinxClosed @($folder, "$folder.previous", "$folder.new")
    Remove-SyrinxRegistration
    Remove-SyrinxShortcut $folder
    Update-SyrinxShellAssociations
    Write-Host 'Removed Syrinx from Default apps, Installed apps and the Start menu.'
    Write-Host 'If Syrinx was your default browser, Windows will ask which browser to use for the next link.'

    if (-not $Uninstall) {
        Write-Host "Program files in $folder and all Syrinx data were kept."
        return
    }
    Remove-SyrinxProgramFiles $folder $registered
    Remove-SyrinxDataIfConfirmed $SyrinxDataFolder 'Syrinx data'
    if ($IncludePreviewData) {
        Remove-SyrinxDataIfConfirmed $SyrinxPreviewDataFolder 'Development preview data (not created by the installed Syrinx)'
    } elseif (Test-Path -LiteralPath (Get-SyrinxLocalPath $SyrinxPreviewDataFolder)) {
        Write-Host "Development preview data in %LOCALAPPDATA%\$SyrinxPreviewDataFolder was kept; -IncludePreviewData offers it."
    }
}

$failed = $false
try {
    Invoke-SyrinxUnregister
} catch {
    $failed = $true
    Write-Host "Syrinx unregister stopped: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    if ($PauseAtEnd) { Read-Host 'Press Enter to close' | Out-Null }
}
if ($failed) { exit 1 }
