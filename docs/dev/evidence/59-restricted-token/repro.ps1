# Reproduces #59's msedgewebview2.exe breakpoint (0x80000003) by running a Syrinx smoke check under a restricted token.
#
# Usage: powershell -File repro.ps1 -Exe <path to syrinx.exe> [-Mode normal|lua|restricted]
# It goes red (exit 1) when the smoke run's fresh synthetic profile holds a WebView2 crash dump with
# STATUS_BREAKPOINT. It touches no Windows setting and no real profile: `--smoke` always makes a new
# profile under %TEMP%, which is deleted when the run is clean and kept, with its dumps, when it is not.
# Not part of `cargo xtask check`: the failure belongs to the WebView2 runtime under a restricted token,
# which Syrinx cannot change, so a gate test would stay red for every contributor.
param(
    [Parameter(Mandatory)] [string]$Exe,
    [ValidateSet('normal', 'lua', 'restricted')] [string]$Mode = 'restricted',
    [int]$TimeoutMs = 90000
)
$ErrorActionPreference = 'Stop'

# Start a process with a restricted copy of this user's token, as a sandboxed agent executor does.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RestrictedLaunch {
  [StructLayout(LayoutKind.Sequential)] public struct SidAndAttributes { public IntPtr Sid; public uint Attributes; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct StartupInfo {
    public int cb; public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError; }
  [StructLayout(LayoutKind.Sequential)] public struct ProcessInformation { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool CreateRestrictedToken(IntPtr existing, uint flags,
    uint disableCount, IntPtr disable, uint deleteCount, IntPtr delete, uint restrictCount, SidAndAttributes[] restrict, out IntPtr created);
  [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool ConvertStringSidToSid(string text, out IntPtr sid);
  [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool CreateProcessAsUser(IntPtr token, string app,
    string command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory,
    ref StartupInfo startup, out ProcessInformation information);
  [DllImport("kernel32.dll", SetLastError = true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
  [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint code);

  const uint DisableMaxPrivilege = 0x1, LuaToken = 0x4;

  static void Fail(string step) { throw new InvalidOperationException(step + " failed: " + Marshal.GetLastWin32Error()); }

  // Returns "<pid> <exit code>", or "<pid> timeout" after terminating a stalled run.
  public static string Run(string exe, string[] restrictingSids, uint timeout) {
    IntPtr own, token;
    restrictingSids = restrictingSids ?? new string[0];
    if (!OpenProcessToken(GetCurrentProcess(), 0x000F01FF, out own)) Fail("OpenProcessToken");
    SidAndAttributes[] restrict = null;
    if (restrictingSids.Length > 0) {
      restrict = new SidAndAttributes[restrictingSids.Length];
      for (int i = 0; i < restrictingSids.Length; i++)
        if (!ConvertStringSidToSid(restrictingSids[i], out restrict[i].Sid)) Fail("ConvertStringSidToSid");
    }
    if (!CreateRestrictedToken(own, DisableMaxPrivilege | LuaToken, 0, IntPtr.Zero, 0, IntPtr.Zero,
        (uint)restrictingSids.Length, restrict, out token)) Fail("CreateRestrictedToken");
    var startup = new StartupInfo(); startup.cb = Marshal.SizeOf(startup); startup.lpDesktop = "winsta0\\default";
    ProcessInformation launched;
    if (!CreateProcessAsUser(token, exe, "\"" + exe + "\" --smoke", IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null,
        ref startup, out launched)) Fail("CreateProcessAsUser");
    if (WaitForSingleObject(launched.hProcess, timeout) != 0) { TerminateProcess(launched.hProcess, 124); return launched.dwProcessId + " timeout"; }
    uint code; GetExitCodeProcess(launched.hProcess, out code);
    return launched.dwProcessId + " " + code;
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

# Run the smoke check, directly for `normal` or with a restricted token otherwise.
function Invoke-Smoke {
    if ($Mode -eq 'normal') {
        $process = Start-Process -FilePath $Exe -ArgumentList '--smoke' -PassThru
        $null = $process.Handle
        if (-not $process.WaitForExit($TimeoutMs)) { $process.Kill(); return "$($process.Id) timeout" }
        return "$($process.Id) $($process.ExitCode)"
    }
    [RestrictedLaunch]::Run((Resolve-Path $Exe).Path, [string[]](Get-RestrictingSids), [uint32]$TimeoutMs)
}

$processId, $exit = (Invoke-Smoke) -split ' '
$smokeProfile = Get-ChildItem $env:TEMP -Directory -Filter "syrinx-window-smoke-$processId-*" | Select-Object -First 1
if (-not $smokeProfile) { Write-Output "mode=$Mode exit=${exit}: no smoke profile found"; exit 2 }
# Give Crashpad a moment to finish writing after the browser process exits.
Start-Sleep -Seconds 3
$dumps = @(Get-ChildItem $smokeProfile.FullName -Recurse -Filter *.dmp)
$summary = if ($dumps) { python -I (Join-Path $PSScriptRoot 'dumpinfo.py') @($dumps.FullName) } else { @() }
Write-Output "mode=$Mode exit=$exit dumps=$($dumps.Count)"
$summary | ForEach-Object { Write-Output "  $_" }
if ($summary -match 'code=0x80000003') {
    Write-Output "RED: WebView2 hit STATUS_BREAKPOINT. Synthetic profile kept: $($smokeProfile.FullName)"
    exit 1
}
Remove-Item -LiteralPath $smokeProfile.FullName -Recurse -Force
Write-Output 'GREEN: no breakpoint dump.'
