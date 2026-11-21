# Wait for other builds before each fresh-process synthetic measurement.
param(
    [string]$Label = 'p1-23-comparison',
    [int]$QuietWaitMinutes = 10,
    # Add 'adapter' to measure the production index (#53) against the same workload.
    [string[]]$Engines = @('fts5', 'tantivy')
)

$ErrorActionPreference = 'Stop'
# Accept a comma-joined list too, as `powershell -File` passes one string, and refuse an
# unknown engine before any quiet-window wait.
$engineList = @($Engines | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } |
    Where-Object { $_ })
if ($engineList.Count -eq 0) { throw 'Name at least one engine.' }
foreach ($engineName in $engineList) {
    if ($engineName -notin @('fts5', 'tantivy', 'adapter')) { throw "Unknown engine '$engineName'." }
}
if (@($engineList | Select-Object -Unique).Count -ne $engineList.Count) {
    throw 'Name each engine once.'
}
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$executable = Join-Path $workspace 'target/local/fulltext-bench.exe'
$loadLog = Join-Path $workspace "target/$Label-load.csv"
$memoryLog = Join-Path $workspace "target/$Label-memory.csv"
if ((Test-Path -LiteralPath $loadLog) -or (Test-Path -LiteralPath $memoryLog)) {
    throw 'Use a fresh label to preserve evidence.'
}
'utc,event,cargo,rustc' | Set-Content -LiteralPath $loadLog -Encoding UTF8
'name,engine,corpus,run,exit_code,peak_working_set_bytes,peak_paged_memory_bytes' |
    Set-Content -LiteralPath $memoryLog -Encoding UTF8

# .NET clears PeakWorkingSet64 and PeakPagedMemorySize64 once a process exits, so read
# the same counters (PeakWorkingSetSize, PeakPagefileUsage) through the retained handle.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class ExitedProcessMemory {
    [StructLayout(LayoutKind.Sequential)]
    public struct Counters {
        public uint Size; public uint PageFaultCount;
        public UIntPtr PeakWorkingSetSize; public UIntPtr WorkingSetSize;
        public UIntPtr QuotaPeakPagedPoolUsage; public UIntPtr QuotaPagedPoolUsage;
        public UIntPtr QuotaPeakNonPagedPoolUsage; public UIntPtr QuotaNonPagedPoolUsage;
        public UIntPtr PagefileUsage; public UIntPtr PeakPagefileUsage;
    }
    [DllImport("kernel32.dll", EntryPoint = "K32GetProcessMemoryInfo", SetLastError = true)]
    static extern bool GetProcessMemoryInfo(IntPtr process, out Counters counters, uint size);
    public static Counters Read(IntPtr process) {
        Counters counters;
        if (!GetProcessMemoryInfo(process, out counters, (uint)Marshal.SizeOf(typeof(Counters)))) {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        return counters;
    }
}
'@

function Get-BuildLoad {
    $processes = @(Get-Process | Where-Object { $_.ProcessName -in @('cargo', 'rustc') })
    [pscustomobject]@{
        Cargo = @($processes | Where-Object ProcessName -eq 'cargo').Count
        Rustc = @($processes | Where-Object ProcessName -eq 'rustc').Count
    }
}

function Write-Load($eventName, $load) {
    "{0},{1},{2},{3}" -f [DateTime]::UtcNow.ToString('o'), $eventName, $load.Cargo, $load.Rustc |
        Add-Content -LiteralPath $loadLog -Encoding UTF8
}

# Alternate engine order by repetition; both corpora use the same seed and count.
foreach ($corpus in @('overlap', 'broad')) {
    foreach ($run in 1..3) {
        # Index into a new array: reversing in place would also reverse later runs' order.
        $order = if ($run -eq 2) { $engineList[($engineList.Count - 1)..0] } else { $engineList }
        foreach ($engine in $order) {
            $name = "$Label-$corpus-$engine-$run"
            $deadline = [DateTime]::UtcNow.AddMinutes($QuietWaitMinutes)
            do {
                $load = Get-BuildLoad
                Write-Load "wait-$name" $load
                if ($load.Cargo -eq 0 -and $load.Rustc -eq 0) { break }
                if ([DateTime]::UtcNow -ge $deadline) { throw "No quiet build window for $name; no process started." }
                Start-Sleep -Seconds 10
            } while ($true)

            $destination = Join-Path $workspace "target/$name"
            $output = "$destination.csv"
            $errors = "$destination.stderr"
            if ((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $output)) {
                throw "Refusing to reuse $name."
            }
            Write-Load "start-$name" $load
            $child = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru `
                -WorkingDirectory $workspace -ArgumentList @(('"{0}"' -f $destination), '100000', $engine, $corpus) `
                -RedirectStandardOutput $output -RedirectStandardError $errors
            # Hold the process handle so its exit code and memory counters outlive the process.
            $handle = $child.Handle
            while (-not $child.HasExited) {
                Start-Sleep -Seconds 10
                Write-Load "running-$name" (Get-BuildLoad)
                $child.Refresh()
            }
            $child.WaitForExit()

            # Record peak memory for failed runs too; the crash-writer grandchild is not included.
            $memory = [ExitedProcessMemory]::Read($handle)
            "{0},{1},{2},{3},{4},{5},{6}" -f $name, $engine, $corpus, $run, $child.ExitCode,
                $memory.PeakWorkingSetSize, $memory.PeakPagefileUsage |
                Add-Content -LiteralPath $memoryLog -Encoding UTF8
            "process_peak_working_set_bytes,{0}" -f $memory.PeakWorkingSetSize |
                Add-Content -LiteralPath $output -Encoding UTF8
            "process_peak_paged_memory_bytes,{0}" -f $memory.PeakPagefileUsage |
                Add-Content -LiteralPath $output -Encoding UTF8
            if ($child.ExitCode -ne 0) { throw "$name failed with exit $($child.ExitCode); see $errors." }
            Write-Load "finish-$name" (Get-BuildLoad)
        }
    }
}
