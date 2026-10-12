# Shared helpers for Atlas's owner-run install, register and unregister scripts (P1-37).
# Dot-sourced after atlas-registration.ps1; it changes nothing by itself.

# Resolve a folder relative to %LOCALAPPDATA%, the only place Atlas installs or keeps data.
function Get-AtlasLocalPath([string]$Relative) {
    if (-not $env:LOCALAPPDATA) { throw 'Windows local app data (%LOCALAPPDATA%) is not set.' }
    return (Join-Path $env:LOCALAPPDATA $Relative)
}

# Recognise a program folder this tooling created, by the package manifest inside it.
function Test-AtlasProgramFolder([string]$Folder) {
    return (Test-Path -LiteralPath (Join-Path $Folder 'atlas-package.json') -PathType Leaf)
}

# Read and sanity-check a package manifest.
function Read-AtlasManifest([string]$Folder) {
    $path = Join-Path $Folder 'atlas-package.json'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "No Atlas package manifest at $path." }
    $manifest = Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($manifest.format -ne 1 -or $manifest.product -ne $AtlasProduct) {
        throw "$path is not an Atlas package manifest this script understands."
    }
    return $manifest
}

# Refuse while an Atlas from this folder runs; the scripts never close it for the owner.
function Assert-AtlasClosed([string]$Folder) {
    $prefix = [System.IO.Path]::GetFullPath($Folder).TrimEnd('\') + '\'
    $running = @(Get-Process -Name ([System.IO.Path]::GetFileNameWithoutExtension($AtlasExecutable)) -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) })
    if ($running.Count -gt 0) {
        throw "Atlas is running from $Folder. Close Atlas, then run this script again."
    }
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
