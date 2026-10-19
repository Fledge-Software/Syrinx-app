# Reproduces #59's WebView2 GPU-process breakpoint by running `syrinx.exe --smoke` under a restricted token.
# Usage and exit codes: docs/dev/LOCAL-DEV.md, "Restricted-token reproduction (#59)".
param(
    [Parameter(Mandatory)] [string]$Exe,
    [ValidateSet('normal', 'lua', 'restricted')] [string]$Mode = 'restricted',
    [switch]$DisableGpu,
    [int]$TimeoutMs = 90000
)
$ErrorActionPreference = 'Stop'

# Exit codes: 0 clean, 1 breakpoint reproduced, 2 cannot analyse, 3 inconclusive run.
$ExitGreen = 0
$ExitRed = 1
$ExitCannotAnalyse = 2
$ExitInconclusive = 3

# An unexpected error must never read as red: report it as "cannot analyse" and delete nothing.
trap {
    Write-Output "CANNOT ANALYSE: $($_.Exception.Message) Nothing deleted."
    exit $ExitCannotAnalyse
}

# Start a process with a restricted copy of this user's token, as a restricted (sandbox-style) launcher might.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RestrictedLaunch {
  [StructLayout(LayoutKind.Sequential)] public struct SidAndAttributes { public IntPtr Sid; public uint Attributes; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct StartupInfo {
    public int cb; public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError; }
  [StructLayout(LayoutKind.Sequential)] public struct ProcessInformation { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr LocalFree(IntPtr memory);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool CreateRestrictedToken(IntPtr existing, uint flags,
    uint disableCount, IntPtr disable, uint deleteCount, IntPtr delete, uint restrictCount, SidAndAttributes[] restrict, out IntPtr created);
  [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool ConvertStringSidToSid(string text, out IntPtr sid);
  [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode, EntryPoint = "CreateProcessAsUserW")] static extern bool CreateProcessAsUser(
    IntPtr token, string app, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags,
    IntPtr environment, string directory, ref StartupInfo startup, out ProcessInformation information);
  [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint code);

  const uint TokenAssignPrimary = 0x0001, TokenDuplicate = 0x0002, TokenQuery = 0x0008;
  const uint DisableMaxPrivilege = 0x1, LuaToken = 0x4;
  const uint WaitTimeout = 0x102, WaitFailed = 0xFFFFFFFF;
  const uint TimeoutExitCode = 124;

  static void Fail(string step) { throw new InvalidOperationException(step + " failed: " + Marshal.GetLastWin32Error()); }

  static void Close(IntPtr handle) { if (handle != IntPtr.Zero) CloseHandle(handle); }

  // Returns "<pid> <exit code>", or "<pid> timeout" after terminating a stalled run. Releases every handle and SID.
  public static string Run(string exe, string[] restrictingSids, uint timeout) {
    IntPtr own = IntPtr.Zero, token = IntPtr.Zero;
    restrictingSids = restrictingSids ?? new string[0];
    var launched = new ProcessInformation();
    var restrict = new SidAndAttributes[restrictingSids.Length];
    try {
      if (!OpenProcessToken(GetCurrentProcess(), TokenAssignPrimary | TokenDuplicate | TokenQuery, out own)) Fail("OpenProcessToken");
      for (int index = 0; index < restrictingSids.Length; index++)
        if (!ConvertStringSidToSid(restrictingSids[index], out restrict[index].Sid)) Fail("ConvertStringSidToSid");
      if (!CreateRestrictedToken(own, DisableMaxPrivilege | LuaToken, 0, IntPtr.Zero, 0, IntPtr.Zero,
          (uint)restrict.Length, restrict.Length > 0 ? restrict : null, out token)) Fail("CreateRestrictedToken");
      var startup = new StartupInfo(); startup.cb = Marshal.SizeOf(startup); startup.lpDesktop = "winsta0\\default";
      var command = new StringBuilder("\"" + exe + "\" --smoke");
      if (!CreateProcessAsUser(token, exe, command, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null,
          ref startup, out launched)) Fail("CreateProcessAsUser");
      return launched.dwProcessId + " " + WaitForExit(launched.hProcess, timeout);
    } finally {
      Close(launched.hThread);
      Close(launched.hProcess);
      Close(token);
      Close(own);
      foreach (var entry in restrict) if (entry.Sid != IntPtr.Zero) LocalFree(entry.Sid);
    }
  }

  // Waits for the process; a stalled run is terminated and reported as "timeout", a failed wait throws.
  static string WaitForExit(IntPtr process, uint timeout) {
    uint waited = WaitForSingleObject(process, timeout);
    if (waited == WaitFailed) Fail("WaitForSingleObject");
    if (waited == WaitTimeout) {
      if (!TerminateProcess(process, TimeoutExitCode)) Fail("TerminateProcess");
      return "timeout";
    }
    uint code;
    if (!GetExitCodeProcess(process, out code)) Fail("GetExitCodeProcess");
    return code.ToString();
  }
}
'@

# The restricting SIDs: this user, the logon session, Everyone, Users, Authenticated Users and RESTRICTED.
function Get-RestrictingSids {
    if ($Mode -ne 'restricted') { return @() }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $logon = $identity.Groups | Where-Object { $_.Value -like 'S-1-5-5-*' } | Select-Object -First 1
    @($identity.User.Value, 'S-1-1-0', 'S-1-5-32-545', 'S-1-5-11', 'S-1-5-12') + @($logon | ForEach-Object Value)
}

# The folder Syrinx's std::env::temp_dir() resolves to (TMP first), where --smoke makes its profile.
$tempRoot = [IO.Path]::GetTempPath()

# Names of the synthetic smoke profiles that exist right now.
function Get-SmokeProfileNames {
    @(Get-ChildItem -LiteralPath $tempRoot -Directory -Filter 'syrinx-window-smoke-*' | ForEach-Object Name)
}

# Run the smoke check, directly for `normal` or with a restricted token otherwise; returns "<pid> <exit|timeout>".
function Invoke-Smoke {
    if ($Mode -eq 'normal') {
        $process = Start-Process -FilePath $Exe -ArgumentList '--smoke' -PassThru
        try {
            $null = $process.Handle
            if (-not $process.WaitForExit($TimeoutMs)) { $process.Kill(); return "$($process.Id) timeout" }
            return "$($process.Id) $($process.ExitCode)"
        } finally { $process.Dispose() }
    }
    [RestrictedLaunch]::Run((Resolve-Path $Exe).Path, [string[]](Get-RestrictingSids), [uint32]$TimeoutMs)
}

# The msedgewebview2.exe processes whose command line names this run's profile.
function Get-ProfileChildren([string]$ProfileName) {
    @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" |
        Where-Object { $_.CommandLine -and $_.CommandLine.Contains($ProfileName) })
}

# Wait, bounded, until this run's WebView2 children have ended; returns whether they all did.
function Wait-ProfileChildren([string]$ProfileName, [int]$Seconds = 20) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if ((Get-ProfileChildren $ProfileName).Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return (Get-ProfileChildren $ProfileName).Count -eq 0
}

# Poll, bounded, until the profile's crash dumps stop changing, so Crashpad has finished writing.
function Wait-StableDumps([string]$ProfilePath, [int]$Seconds = 15) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    $previous = $null
    do {
        $dumps = @(Get-ChildItem -LiteralPath $ProfilePath -Recurse -Filter *.dmp)
        $current = ($dumps | ForEach-Object { "$($_.FullName)|$($_.Length)" }) -join ';'
        if ($null -ne $previous -and $current -eq $previous) { return ,$dumps }  # the comma keeps an empty list from becoming $null
        $previous = $current
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)
    return $null
}

# Print a verdict and exit with its code.
function Exit-With([int]$Code, [string]$Message) {
    Write-Output $Message
    exit $Code
}

if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    Exit-With $ExitCannotAnalyse 'CANNOT ANALYSE: python is not on PATH; dumpinfo.py needs it. Nothing was run.'
}

# Launch (with --disable-gpu for this run only, if asked), then identify this run's profile as the
# one new folder named for its process id.
$before = Get-SmokeProfileNames
$previousArguments = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
if ($DisableGpu) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--disable-gpu' }
try { $processId, $exit = (Invoke-Smoke) -split ' ' }
finally { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousArguments }
$created = @(Get-SmokeProfileNames | Where-Object { $before -notcontains $_ -and $_ -like "syrinx-window-smoke-$processId-*" })
if ($created.Count -ne 1) {
    Exit-With $ExitCannotAnalyse "CANNOT ANALYSE: mode=$Mode exit=$exit; expected one new smoke profile for pid $processId, found $($created.Count). Nothing deleted."
}
$profileName = $created[0]
$profilePath = Join-Path $tempRoot $profileName
$shownProfile = "%TEMP%\$profileName"

# Stop a timed-out run's own WebView2 children (one may exit first; the wait below confirms they all
# ended), then wait for every child of this profile to end.
if ($exit -eq 'timeout') {
    Get-ProfileChildren $profileName | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
$childrenEnded = Wait-ProfileChildren $profileName

$dumps = Wait-StableDumps $profilePath
if ($null -eq $dumps) {
    Exit-With $ExitCannotAnalyse "CANNOT ANALYSE: mode=$Mode exit=$exit; crash dumps kept changing. Profile kept: $shownProfile"
}

# Parse every dump; a failed or partial parse never reads as clean.
$summary = @()
if ($dumps.Count -gt 0) {
    $summary = @(python -I (Join-Path $PSScriptRoot 'dumpinfo.py') @($dumps.FullName))
    if ($LASTEXITCODE -ne 0 -or $summary.Count -ne $dumps.Count) {
        $summary | ForEach-Object { Write-Output "  $_" }
        Exit-With $ExitCannotAnalyse "CANNOT ANALYSE: dumpinfo.py exited $LASTEXITCODE with $($summary.Count) of $($dumps.Count) lines. Profile kept: $shownProfile"
    }
}
Write-Output "mode=$Mode disable-gpu=$([bool]$DisableGpu) exit=$exit dumps=$($dumps.Count)"
$summary | ForEach-Object { Write-Output "  $_" }

# Verdict: a breakpoint dump is red; anything short of a clean, finished run is inconclusive.
if ($summary -match 'code=0x80000003') {
    Exit-With $ExitRed "RED: WebView2 hit STATUS_BREAKPOINT. Profile kept: $shownProfile"
}
if ($exit -ne '0' -or $dumps.Count -gt 0 -or -not $childrenEnded) {
    Exit-With $ExitInconclusive "INCONCLUSIVE: exit=$exit, $($dumps.Count) other dumps, children ended=$childrenEnded. Profile kept: $shownProfile"
}
Remove-Item -LiteralPath $profilePath -Recurse -Force
Exit-With $ExitGreen 'GREEN: clean exit and no crash dump. Profile deleted.'
