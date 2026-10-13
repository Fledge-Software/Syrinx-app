# Captures website screenshots of the built app on fresh synthetic profiles; never the owner's data.
param(
    [Parameter(Mandatory)] [string] $Executable,
    [Parameter(Mandatory)] [string] $OutputFolder
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win32 {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
}
'@

New-Item -ItemType Directory -Force -Path $OutputFolder | Out-Null
$executablePath = (Resolve-Path $Executable).Path
$profiles = Join-Path $env:RUNNER_TEMP 'syrinx-screenshot-profiles'

# Wait for the process's main window, maximise it and bring it forward.
function Wait-MainWindow([System.Diagnostics.Process] $process) {
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "Syrinx exited early with code $($process.ExitCode)." }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            [Win32]::ShowWindow($process.MainWindowHandle, 3) | Out-Null
            [Win32]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
            return $process.MainWindowHandle
        }
        Start-Sleep -Milliseconds 500
    }
    throw 'Syrinx did not open a window within 60 seconds.'
}

# Copy the window's area of the screen, so owned popups such as the palette are included.
function Save-Window([IntPtr] $window, [string] $path) {
    $rect = New-Object Win32+Rect
    [Win32]::GetWindowRect($window, [ref] $rect) | Out-Null
    $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $left = [Math]::Max($rect.Left, $screen.Left); $top = [Math]::Max($rect.Top, $screen.Top)
    $width = [Math]::Min($rect.Right, $screen.Right) - $left
    $height = [Math]::Min($rect.Bottom, $screen.Bottom) - $top
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($left, $top, 0, 0, $bitmap.Size)
    $graphics.Dispose()
    try {
        if (-not (Test-Detailed $bitmap)) { throw 'the capture is blank' }
        $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
}

# A real frame has many colours; a black or unpainted window has a handful.
function Test-Detailed([System.Drawing.Bitmap] $bitmap) {
    $colours = @{}
    for ($x = 0; $x -lt $bitmap.Width; $x += 16) {
        for ($y = 0; $y -lt $bitmap.Height; $y += 16) { $colours[$bitmap.GetPixel($x, $y).ToArgb()] = $true }
    }
    return $colours.Count -ge 24
}

# Launch Syrinx, let it settle, optionally send keys, capture, then close it.
function Capture-Shot([string] $name, [string[]] $arguments, [string] $keys) {
    $process = Start-Process -FilePath $executablePath -ArgumentList $arguments -PassThru
    try {
        $window = Wait-MainWindow $process
        Start-Sleep -Seconds 8
        if ($keys) { [System.Windows.Forms.SendKeys]::SendWait($keys); Start-Sleep -Seconds 2 }
        Save-Window $window (Join-Path $OutputFolder "$name.png")
        Write-Host "Captured $name.png"
    } catch {
        Write-Host "::warning::Skipped the $name screenshot: $_"
    } finally {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    }
}

# A fresh, empty synthetic profile per shot; reserved example domains only.
function New-SyntheticProfile([string] $name) {
    $path = Join-Path $profiles $name
    if (Test-Path $path) { Remove-Item -Recurse -Force $path }
    New-Item -ItemType Directory -Path $path | Out-Null
    return $path
}

Capture-Shot 'home' @('--test-data-dir', (New-SyntheticProfile 'home'))
Capture-Shot 'page' @('--test-data-dir', (New-SyntheticProfile 'page'), '--', 'https://example.com/')
Capture-Shot 'palette' @('--test-data-dir', (New-SyntheticProfile 'palette'), '--', 'https://example.com/') '^k'
Capture-Shot 'themes' @('--palette-sample')

$saved = @(Get-ChildItem $OutputFolder -Filter *.png).Count
Write-Host "Saved $saved of 4 screenshots to $OutputFolder."
