#Requires -Version 5.1
# Copies a verified Syrinx package into the owner's per-user program folder (P1-37).
# The owner runs this by hand. It never touches Syrinx's data, the registry or the default browser.
[CmdletBinding()]
param(
    # Unzipped package folder; defaults to the folder holding this script.
    [string]$PackageDirectory = $PSScriptRoot,
    # Program folder; defaults to %LOCALAPPDATA%\Programs\Syrinx.
    [string]$InstallDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'syrinx-registration.ps1')
. (Join-Path $PSScriptRoot 'syrinx-common.ps1')

# Refuse without the Evergreen WebView2 runtime; Syrinx never downloads it itself. Reads only.
function Assert-WebView2Runtime {
    $client = 'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    $keys = @("HKLM:\SOFTWARE\WOW6432Node\$client", "HKLM:\SOFTWARE\$client", "HKCU:\Software\$client")
    foreach ($key in $keys) {
        $version = (Get-ItemProperty -LiteralPath $key -Name 'pv' -ErrorAction SilentlyContinue).pv
        if ($version -and $version -ne '0.0.0.0') { return $version }
    }
    throw ('The Microsoft Edge WebView2 Evergreen runtime was not found. Install it yourself from ' +
        'https://developer.microsoft.com/microsoft-edge/webview2/ and run this script again.')
}

# Check every packaged file against the manifest before copying anything.
function Assert-PackageIntact($Manifest) {
    foreach ($file in $Manifest.files) {
        $path = Join-Path $PackageDirectory $file.path
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "The package is missing $($file.path)." }
        $digest = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($digest -ne $file.sha256) { throw "$($file.path) does not match the package manifest; unzip the package again." }
    }
}

# Keep program files out of every Syrinx data folder, and the reverse.
function Assert-SeparateFromData([string]$Folder) {
    $target = $Folder + '\'
    foreach ($relative in @($SyrinxDataFolder, $SyrinxPreviewDataFolder)) {
        $data = (Resolve-SyrinxFolder (Get-SyrinxLocalPath $relative)) + '\'
        if ($target.StartsWith($data, [System.StringComparison]::OrdinalIgnoreCase) -or
            $data.StartsWith($target, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "$Folder overlaps Syrinx's data folder $data; choose another program folder."
        }
    }
}

# Copy the package beside the program folder, then swap it in, keeping one previous version.
function Install-SyrinxFiles($Manifest, [string]$Target) {
    $staging = "$Target.new"
    $previous = "$Target.previous"
    Remove-SyrinxOwnedFolder $staging
    New-Item -ItemType Directory -Path $staging | Out-Null
    # Manifest first, so an interrupted copy is still recognised and replaced next time.
    $names = @('syrinx-package.json') + @($Manifest.files | ForEach-Object { $_.path })
    foreach ($name in $names) {
        Copy-Item -LiteralPath (Join-Path $PackageDirectory $name) -Destination (Join-Path $staging $name)
    }

    if (Test-Path -LiteralPath $Target) {
        if (-not (Test-SyrinxProgramFolder $Target)) { throw "$Target exists but was not installed by this script." }
        Remove-SyrinxOwnedFolder $previous
        Rename-Item -LiteralPath $Target -NewName (Split-Path $previous -Leaf)
    }
    try {
        Rename-Item -LiteralPath $staging -NewName (Split-Path $Target -Leaf)
    } catch {
        if (Test-Path -LiteralPath $previous) { Rename-Item -LiteralPath $previous -NewName (Split-Path $Target -Leaf) }
        throw
    }
}

if (-not $InstallDirectory) { $InstallDirectory = Get-SyrinxLocalPath $SyrinxInstallFolder }
$InstallDirectory = Resolve-SyrinxFolder $InstallDirectory
$PackageDirectory = Resolve-SyrinxFolder $PackageDirectory
$packagePrefix = $PackageDirectory + '\'
if ($InstallDirectory -ieq $PackageDirectory -or ($InstallDirectory + '\').StartsWith($packagePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Install outside the unzipped package folder, not into or below it.'
}

$runtime = Assert-WebView2Runtime
$manifest = Read-SyrinxManifest $PackageDirectory
Assert-PackageIntact $manifest
Assert-SeparateFromData $InstallDirectory
Assert-SyrinxClosed @($InstallDirectory, "$InstallDirectory.previous", "$InstallDirectory.new")
Install-SyrinxFiles $manifest $InstallDirectory

Write-Host "Installed Syrinx $($manifest.version) ($($manifest.commit)) into $InstallDirectory."
Write-Host "WebView2 runtime $runtime found. Syrinx's data in %LOCALAPPDATA%\$SyrinxDataFolder was not touched."
Write-Host "First install: run register-syrinx.ps1 from $InstallDirectory to add Syrinx to Default apps."
Write-Host 'Upgrade: registration already points at this folder; run it again only to refresh the version shown.'
