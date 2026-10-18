# Runs the P1-06 idle CPU series: each build twice, interleaved, harness then Syrinx.
$s = $PSScriptRoot
$log = Join-Path $s 'runs\series.log'
"series start $(Get-Date -Format o); logical processors $([Environment]::ProcessorCount)" | Tee-Object -FilePath $log
foreach ($run in 1, 2) {
    foreach ($build in 'unpatched', 'patched') {
        & "$s\measure.ps1" -Label "harness-$build-$run" -Exe "$s\harness-$build.exe" -Arguments @('idle', '335') -SettleSeconds 30 -IdleSeconds 300 |
            Tee-Object -FilePath $log -Append
        $profile = Join-Path $env:TEMP ("syrinx-p1-06-idle-$build-$run-" + [guid]::NewGuid().ToString('N'))
        & "$s\measure.ps1" -Label "syrinx-$build-$run" -Exe "$s\syrinx-app-$build.exe" -Arguments @('--test-data-dir', $profile) -SettleSeconds 30 -IdleSeconds 300 -CloseAfter |
            Tee-Object -FilePath $log -Append
    }
}
"series end $(Get-Date -Format o)" | Tee-Object -FilePath $log -Append
