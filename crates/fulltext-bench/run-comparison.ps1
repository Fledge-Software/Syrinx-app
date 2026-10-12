# Wait for other builds before each fresh-process synthetic measurement.
param(
    [string]$Label = 'p1-23-comparison',
    [int]$QuietWaitMinutes = 10
)

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$executable = Join-Path $workspace 'target/local/fulltext-bench.exe'
$loadLog = Join-Path $workspace "target/$Label-load.csv"
if (Test-Path -LiteralPath $loadLog) { throw 'Use a fresh label to preserve evidence.' }
'utc,event,cargo,rustc' | Set-Content -LiteralPath $loadLog -Encoding UTF8

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
        $engines = if ($run -eq 2) { @('tantivy', 'fts5') } else { @('fts5', 'tantivy') }
        foreach ($engine in $engines) {
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
            while (-not $child.HasExited) {
                Start-Sleep -Seconds 10
                Write-Load "running-$name" (Get-BuildLoad)
                $child.Refresh()
            }
            $child.WaitForExit()
            if ($child.ExitCode -ne 0) { throw "$name failed with exit $($child.ExitCode); see $errors." }
            Write-Load "finish-$name" (Get-BuildLoad)
        }
    }
}
