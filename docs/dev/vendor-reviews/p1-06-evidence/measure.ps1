# Measures idle CPU of one launched process and its descendants, as a percentage of ONE core.
# Limit: a descendant that exits inside the window is not counted (only reported in
# "exited during window"); Windows job-object accounting would be needed to include it.
param(
    [string]$Label,
    [string]$Exe,
    [string[]]$Arguments,
    [int]$SettleSeconds = 30,
    [int]$IdleSeconds = 300,
    [switch]$CloseAfter
)

function Get-Tree([int]$RootId) {
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CreationDate
    $ids = @($RootId); $frontier = @($RootId)
    while ($frontier.Count -gt 0) {
        $next = @($all | Where-Object { $frontier -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } | ForEach-Object { $_.ProcessId })
        $ids += $next; $frontier = $next
    }
    $ids
}

function Get-CpuSeconds([int[]]$Ids) {
    $table = @{}
    foreach ($id in $Ids) {
        try {
            $process = Get-Process -Id $id -ErrorAction Stop
            $table[$id] = @{ Name = $process.ProcessName; Cpu = $process.TotalProcessorTime.TotalSeconds; Start = $process.StartTime }
        } catch { }
    }
    $table
}

$output = Join-Path $PSScriptRoot "runs\$Label.out"
$process = Start-Process -FilePath $Exe -ArgumentList $Arguments -PassThru -RedirectStandardOutput $output -RedirectStandardError "$output.err"
# Opening the handle now keeps ExitCode readable after exit; without it the EXIT line is blank.
$null = $process.Handle
Start-Sleep -Seconds $SettleSeconds
$before = Get-CpuSeconds (Get-Tree $process.Id)
$clock = [Diagnostics.Stopwatch]::StartNew()
Start-Sleep -Seconds $IdleSeconds
$after = Get-CpuSeconds (Get-Tree $process.Id)
$elapsed = $clock.Elapsed.TotalSeconds

# Count each process's CPU used inside the window; processes born inside it count in full.
$rows = @(foreach ($id in $after.Keys) {
    $start = if ($before.ContainsKey($id) -and $before[$id].Start -eq $after[$id].Start) { $before[$id].Cpu } else { 0 }
    [pscustomobject]@{ Pid = $id; Name = $after[$id].Name; CpuSeconds = [math]::Round($after[$id].Cpu - $start, 3) }
})
$exited = @($before.Keys | Where-Object { -not $after.ContainsKey($_) })
$rootCpu = ($rows | Where-Object Pid -eq $process.Id).CpuSeconds
$treeCpu = ($rows | Measure-Object CpuSeconds -Sum).Sum
"RESULT {0}: window {1:N1} s, root {2:N3} s CPU = {3:N3}% of one core; tree ({4} processes) {5:N3} s = {6:N3}% of one core; exited during window: {7}" -f `
    $Label, $elapsed, $rootCpu, (100 * $rootCpu / $elapsed), $rows.Count, $treeCpu, (100 * $treeCpu / $elapsed), $exited.Count
$rows | Sort-Object CpuSeconds -Descending | Format-Table -AutoSize | Out-String

if ($CloseAfter) {
    $null = $process.CloseMainWindow()
    if (-not $process.WaitForExit(20000)) { "WARNING: $Label did not close within 20 s" }
}
$process.WaitForExit(60000) | Out-Null
"EXIT {0}: {1}" -f $Label, $(if ($process.HasExited) { $process.ExitCode } else { 'still running' })
Get-Content $output -ErrorAction SilentlyContinue
