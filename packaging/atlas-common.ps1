# Shared helpers for Atlas's owner-run install, register and unregister scripts (P1-37).
# Dot-sourced after atlas-registration.ps1; it changes nothing by itself.

# Resolve a folder relative to %LOCALAPPDATA%, the only place Atlas installs or keeps data.
function Get-AtlasLocalPath([string]$Relative) {
    if (-not $env:LOCALAPPDATA) { throw 'Windows local app data (%LOCALAPPDATA%) is not set.' }
    return (Join-Path $env:LOCALAPPDATA $Relative)
}

# Normalize a folder to a full path without a trailing backslash, refusing a drive root.
function Resolve-AtlasFolder([string]$Folder) {
    if (-not $Folder) { throw 'A folder path is required.' }
    # Resolve relative paths against PowerShell's current location; .NET would use the process folder.
    $provider = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Folder)
    $full = [System.IO.Path]::GetFullPath($provider).TrimEnd('\')
    $root = [System.IO.Path]::GetPathRoot($full).TrimEnd('\')
    if (-not $full -or $full -ieq $root) { throw "$Folder is a drive root; choose a folder inside it." }
    return $full
}

# Recognise a program folder this tooling created, by the package manifest inside it.
function Test-AtlasProgramFolder([string]$Folder) {
    return (Test-Path -LiteralPath (Join-Path $Folder 'atlas-package.json') -PathType Leaf)
}

# Read a package manifest, accepting only the fixed file list with plain names.
function Read-AtlasManifest([string]$Folder) {
    $path = Join-Path $Folder 'atlas-package.json'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "No Atlas package manifest at $path." }
    $manifest = Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($manifest.format -ne 1 -or $manifest.product -ne $AtlasProduct) {
        throw "$path is not an Atlas package manifest this script understands."
    }
    $listed = @($manifest.files | ForEach-Object { [string]$_.path })
    if (($listed -join '|') -cne ($AtlasPackageFiles -join '|')) {
        throw "$path lists files other than the fixed Atlas package layout."
    }
    return $manifest
}

# Refuse while an Atlas from any of these folders runs; the scripts never close it for the owner.
function Assert-AtlasClosed([string[]]$Folders) {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($AtlasExecutable)
    $processes = @(Get-Process -Name $name -ErrorAction SilentlyContinue | Where-Object { $_.Path })
    foreach ($folder in $Folders) {
        $prefix = $folder.TrimEnd('\') + '\'
        $running = @($processes | Where-Object { $_.Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) })
        if ($running.Count -gt 0) { throw "Atlas is running from $folder. Close Atlas, then run this script again." }
    }
}

# Delete a program folder this tooling made, removing the manifest last so a failure can be retried.
function Remove-AtlasOwnedFolder([string]$Folder) {
    if (-not (Test-Path -LiteralPath $Folder)) { return }
    if (-not (Test-AtlasProgramFolder $Folder)) { throw "$Folder was not made by Atlas's install script; move it aside first." }
    Get-ChildItem -LiteralPath $Folder -Force |
        Where-Object { $_.Name -ne 'atlas-package.json' } |
        Remove-Item -Recurse -Force
    Remove-Item -LiteralPath (Join-Path $Folder 'atlas-package.json') -Force
    Remove-Item -LiteralPath $Folder -Recurse -Force
}

# Classify an owned key root: absent, ours (carries the owner marker) or foreign.
function Get-AtlasKeyOwner([string]$KeyPath) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($KeyPath)
    if ($null -eq $key) { return 'absent' }
    try {
        if ($key.GetValue($AtlasOwnerMarkerName) -ceq $AtlasOwnerMarkerData) { return 'ours' }
        return 'foreign'
    } finally {
        $key.Dispose()
    }
}

# Read one HKCU value, or $null when the key or value is missing.
function Get-AtlasRegistryValue([string]$KeyPath, [string]$Name) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($KeyPath)
    if ($null -eq $key) { return $null }
    try { return $key.GetValue($Name) } finally { $key.Dispose() }
}

# Program folder recorded by a registration this tooling wrote, or $null.
function Get-AtlasRegisteredFolder {
    if ((Get-AtlasKeyOwner $AtlasUninstallKey) -ne 'ours') { return $null }
    $location = Get-AtlasRegistryValue $AtlasUninstallKey 'InstallLocation'
    if (-not $location) { return $null }
    return (Resolve-AtlasFolder $location)
}

# Tell Explorer and Default apps that associations changed (SHCNE_ASSOCCHANGED).
function Update-AtlasShellAssociations {
    if (-not ('AtlasPackaging.Shell' -as [type])) {
        Add-Type -Namespace 'AtlasPackaging' -Name 'Shell' -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("shell32.dll")]
public static extern void SHChangeNotify(int eventId, uint flags, System.IntPtr item1, System.IntPtr item2);
'@
    }
    [AtlasPackaging.Shell]::SHChangeNotify(0x08000000, 0, [System.IntPtr]::Zero, [System.IntPtr]::Zero)
}

# Start menu shortcut path in the owner's own Start menu.
function Get-AtlasShortcutPath {
    if (-not $env:APPDATA) { throw 'Windows roaming app data (%APPDATA%) is not set.' }
    return (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$AtlasProduct.lnk")
}
