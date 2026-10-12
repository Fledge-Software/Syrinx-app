# P1-37: installer and browser registration

Status: **prepared, owner operation pending.** This note explains how Atlas
gets onto the owner's machine and into Windows' Default apps list without an
agent changing Windows. It records the option chosen, the options rejected,
the exact registry keys involved and the steps only the owner performs.

## Why this slice exists

Plan 1's P1-37 row says installation must be reproducible without secretly
changing Windows. Today Atlas runs only through `cargo atlas` from a source
checkout. The owner's default-browser week (P1-40) needs an installed copy
that Windows can offer as a browser, that survives upgrades without losing
the owner's tabs and sign-ins, and that can be removed cleanly. Everything
system-facing here is an operator action under `AGENTS.md`: the owner, not
an agent, installs software, writes registration keys and picks the default
browser. So this slice builds artifacts and reviewed scripts and stops there.
No script in `packaging/` has been run by an agent.

## What is on this machine

Checked on 2026-10-12 with `where` and the tools' default install folders,
without installing anything:

| Tool | Present | Notes |
| --- | --- | --- |
| WiX Toolset (`wix`, `candle`, `light`) | No | Not on PATH or in Program Files; no `dotnet` either, so no WiX v4+ .NET tool. |
| Inno Setup (`ISCC.exe`) | No | Not on PATH or in either Program Files folder. |
| `makeappx.exe`, `signtool.exe` | Yes, not on PATH | Windows SDK 10.0.26100 under `Program Files (x86)\Windows Kits\10\bin`. No signing certificate exists (D38). |
| Windows `tar.exe` (bsdtar) | Yes | `%SystemRoot%\System32\tar.exe`, part of Windows; writes zip files. |
| 7-Zip | Yes | Not needed; Windows tar suffices. |
| Windows PowerShell 5.1 | Yes | Used to parse the scripts in the gate, never to run them. |

## Options compared

Every option below installs per user, under the owner's own profile, with no
administrator prompt. A machine-wide install would need elevation and would
write to `HKLM`, which nothing in Atlas's scope requires.

**Recommended: a portable zip plus owner-run PowerShell scripts.** `cargo
xtask package` builds `atlas-app.exe` with D38's `local` profile and writes a
versioned folder and zip containing the executable, a manifest
(`atlas-package.json`) and three scripts: `install-atlas.ps1` copies a verified
package into `%LOCALAPPDATA%\Programs\Atlas`; `register-atlas.ps1` writes the
per-user browser registration; `unregister-atlas.ps1` removes it and, with
`-Uninstall`, the program files. Why this one:

- It needs nothing that is not already on the machine, so no agent or owner
  has to install a packaging tool first.
- Every change it makes to Windows is a line the owner can read before
  running it. The registry list is one Rust table
  (`crates/xtask/src/packaging/registration.rs`) rendered into
  `packaging/atlas-registration.ps1`; tests check that the file matches the
  table and that unregistering covers every value registering writes.
- It keeps install, registration and default selection as three separate
  owner decisions, which is what the operator-work list asks for.
- P1-38's signed update packages can reuse the same layout and manifest.

The cost is polish: there is no wizard, and PowerShell's default execution
policy on Windows 11 refuses unsigned scripts, so the owner starts each one
with `powershell -ExecutionPolicy Bypass -File ...`. That flag applies to
that one PowerShell process only; it changes no Windows setting.

**Rejected: MSIX.** `makeappx.exe` is present, but an MSIX package must be
signed by a certificate the machine trusts before Windows installs it. Atlas
has no certificate and signing is deferred (S8, D38). The unsigned routes
need Developer Mode or trusting a self-made certificate, both of which change
Windows security settings, which agents never do and the owner has not
chosen. MSIX also fits a browser poorly: packaged apps get a virtualised view
of `HKCU` and `%LOCALAPPDATA%`, so data written there lands under the
package's private folder and is deleted with the package, which conflicts
with D13 and with keeping data on uninstall. Browser registration in a
package has to go through manifest extensions rather than the
`StartMenuInternet` keys below (unverified here; it would have to be proven
before relying on it). Reconsider MSIX at productisation, once signing exists.

**Rejected for now: a per-user MSI built with WiX.** A WiX MSI gives an
Installed apps entry, clean upgrades through `MajorUpgrade` and a standard
uninstall. WiX is not installed, and installing it is the owner's decision.
An unsigned MSI also shows the same SmartScreen warning as the scripts, so
it buys no trust. Recent WiX releases are distributed as a .NET tool, and
WiX has historically used the Microsoft Reciprocal License, which is not on
`AGENTS.md`'s list; a build tool's licence does not bind Atlas's code, but
check it and its current terms before adopting (unverified here). If the owner wants a
conventional installer later, this is the recommended next step: install
WiX themselves, and its authoring can take the registry list from the same
Rust table.

**Rejected for now: Inno Setup.** A small, mature, free installer builder
that writes per-user installs well. It is not installed. Its script language
would be a second place to keep the registry list, unless generated from the
same table, and an unsigned Inno installer meets the same SmartScreen
warning. It is a reasonable alternative to WiX if the owner prefers a wizard
over MSI semantics.

## Unsigned builds and SmartScreen

Atlas stays unsigned for now (D38). What the owner will see:

- **Files built on this machine** carry no Mark of the Web, the marker
  Windows adds to downloaded files, so SmartScreen's "Windows protected
  your PC" prompt does not appear for them.
- **A zip copied from elsewhere** (a browser download, an email) does carry
  the Mark of the Web. Running `atlas-app.exe` from it then shows "Windows
  protected your PC" with an "unknown publisher"; the owner chooses "More
  info", then "Run anyway". PowerShell also refuses or prompts for marked
  scripts. Building locally avoids this; `Unblock-File` removes the marker
  if the owner trusts the copy.
- **Smart App Control** can block unsigned executables outright, with no
  "Run anyway" (`docs/LOCAL-DEV.md`, `docs/trapstoknow.md`). The owner has
  turned it off on this machine (2026-10-12); a fresh Windows install may
  block Atlas until signing exists.
- **Default apps** shows Atlas by the name in its Capabilities key. Windows
  does not require a signature to list a browser there.

Nothing in this slice turns off or works around SmartScreen, Smart App
Control or execution policy.

## Where things live

| What | Where | Lifetime |
| --- | --- | --- |
| Program files | `%LOCALAPPDATA%\Programs\Atlas\` | Replaced on upgrade; removed by `unregister-atlas.ps1 -Uninstall`. |
| Previous version | `%LOCALAPPDATA%\Programs\Atlas.previous\` | One kept for manual rollback; replaced on the next upgrade. |
| Atlas data (current preview) | `%LOCALAPPDATA%\AtlasProductPreview\` (`State`, `WebView2`) | Never touched by install or upgrade; kept on uninstall unless the owner types DELETE. |
| Atlas data (platform default) | `%LOCALAPPDATA%\Atlas\data\` | Same. `WindowsDirectoryLease::discover` uses it; the app does not yet. |
| Registration | `HKCU` keys listed below | Written by `register-atlas.ps1`; removed by `unregister-atlas.ps1`. |
| Start menu shortcut | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Atlas.lnk` | Same as registration. |

`%LOCALAPPDATA%\Programs` is Windows' conventional per-user program folder.
All data stays in local, never roaming, app data (D13). The program folder
and data folders never overlap; `install-atlas.ps1` refuses an install
folder inside a data folder, and an xtask test checks the defaults.

The two data folders are an open thread for the owner, not something this
slice settles: the integrated preview writes `AtlasProductPreview`, while
the platform's production default is `Atlas\data`. Moving the app to the
production folder needs a data move or an import, which belongs to its own
slice. The uninstall script asks about each folder separately.

## Browser registration

These are every value `register-atlas.ps1` writes. All are under
`HKEY_CURRENT_USER`; nothing goes to `HKEY_LOCAL_MACHINE`. `{exe}` is the
installed `atlas-app.exe`, `{dir}` its folder and `{version}` the manifest's
version. The source of truth is `VALUES` in
`crates/xtask/src/packaging/registration.rs`.

`Software\Clients\StartMenuInternet\Atlas` registers Atlas as a browser:

| Key under `HKCU\` | Value | Data |
| --- | --- | --- |
| `Software\Clients\StartMenuInternet\Atlas` | (default) | `Atlas` |
| `...\Atlas\DefaultIcon` | (default) | `{exe},0` |
| `...\Atlas\shell\open\command` | (default) | `"{exe}"` |
| `...\Atlas\Capabilities` | `ApplicationName` | `Atlas` |
| `...\Atlas\Capabilities` | `ApplicationDescription` | `Atlas web browser` |
| `...\Atlas\Capabilities` | `ApplicationIcon` | `{exe},0` |
| `...\Atlas\Capabilities\StartMenu` | `StartMenuInternet` | `Atlas` |
| `...\Atlas\Capabilities\URLAssociations` | `http` | `AtlasURL` |
| `...\Atlas\Capabilities\URLAssociations` | `https` | `AtlasURL` |

`RegisteredApplications` points Windows at those capabilities. It is a key
every browser shares, so Atlas writes and later removes only its own value:

| Key under `HKCU\` | Value | Data |
| --- | --- | --- |
| `Software\RegisteredApplications` | `Atlas` | `Software\Clients\StartMenuInternet\Atlas\Capabilities` |

`AtlasURL` is the ProgID (programmatic identifier: the named handler a file
type or link scheme is associated with) that opens a link:

| Key under `HKCU\` | Value | Data |
| --- | --- | --- |
| `Software\Classes\AtlasURL` | (default) | `Atlas URL` |
| `Software\Classes\AtlasURL` | `FriendlyTypeName` | `Atlas URL` |
| `Software\Classes\AtlasURL` | `URL Protocol` | (empty) |
| `Software\Classes\AtlasURL\Application` | `ApplicationName` | `Atlas` |
| `Software\Classes\AtlasURL\Application` | `ApplicationDescription` | `Atlas web browser` |
| `Software\Classes\AtlasURL\Application` | `ApplicationIcon` | `{exe},0` |
| `Software\Classes\AtlasURL\DefaultIcon` | (default) | `{exe},0` |
| `Software\Classes\AtlasURL\shell\open\command` | (default) | `"{exe}" --open-url %1` |

The Installed apps entry, so the owner can uninstall from Settings:

| Key under `HKCU\` | Value | Data |
| --- | --- | --- |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas` | `DisplayName` | `Atlas` |
| same | `DisplayVersion` | `{version}` |
| same | `DisplayIcon` | `{exe},0` |
| same | `InstallLocation` | `{dir}` |
| same | `UninstallString` | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File "{dir}\unregister-atlas.ps1" -Uninstall` |
| same | `NoModify` | DWORD 1 |
| same | `NoRepair` | DWORD 1 |

`unregister-atlas.ps1` deletes the three key trees Atlas owns
(`StartMenuInternet\Atlas`, `Classes\AtlasURL`, `Uninstall\Atlas`) and the
single `Atlas` value under `RegisteredApplications`. A test fails if any
written value would be left behind, or if a shared key were ever listed for
whole-tree deletion. After writing or removing keys, both scripts call
`SHChangeNotify(SHCNE_ASSOCCHANGED)` so Explorer re-reads associations.

What is deliberately absent:

- **No file associations** (`.htm`, `.html`, `.pdf`). Atlas's navigation
  accepts web addresses; opening local files is not specified yet. Adding
  them is a later owner decision and a one-line table change.
- **No `UserChoice` writes or any other attempt to set the default.**
  Windows 11 protects the default browser with a hashed `UserChoice` value
  and expects the owner to pick it in Settings > Apps > Default apps. The
  script can open that page (`-OpenDefaultApps`, using
  `ms-settings:defaultapps?registeredAppUser=Atlas`) and stops there.
- **No legacy `InstallInfo` show/hide-icons commands** and no
  `AppUserModelId`: Atlas sets no explicit application user model ID, and
  writing one the process does not use would mis-group taskbar entries.
- **No `App Paths` entry.** It would let Win+R start `atlas-app`; it is
  convenience only and can be added later.

### The link command and P1-34

When the owner clicks a link elsewhere, Windows runs
`"{exe}" --open-url <link>`. The link is deliberately unquoted and last,
following Chrome's `--single-argument` pattern: P1-34 must read the raw
command line after `--open-url ` as exactly one URL, so a crafted link
containing quotes or spaces cannot smuggle extra switches into Atlas. P1-34
also validates the scheme (http and https only) and forwards the link to an
already-running Atlas.

**Today's `atlas-app.exe` refuses any argument it does not know**
(`run_mode` in `crates/atlas-app/src/main.rs`), so a registered build would
show an error instead of opening the link. `ACCEPTS_OPEN_URL` in the
registration table is therefore `false`; it is copied into the generated
script and the manifest, and `register-atlas.ps1` warns the owner not to
choose Atlas as default until a build that accepts links is installed.
P1-34 sets it to `true` when it lands.

## Upgrade

1. The owner builds or receives a new package and unzips it anywhere.
2. `install-atlas.ps1` verifies every file's SHA-256 against the manifest,
   refuses if an Atlas from the program folder is running, and never closes
   Atlas itself. The owner closes Atlas and runs the script again.
3. It copies the package to `Programs\Atlas.new`, renames the current
   folder to `Programs\Atlas.previous` (replacing an older one), then
   renames the new folder into place. If that last rename fails, it renames
   the previous version back.
4. Registration names the stable path `Programs\Atlas\atlas-app.exe`, so
   links keep working without re-registering. Re-running
   `register-atlas.ps1` only refreshes the version in Installed apps.

Data is preserved because nothing in this path knows where data lives
except to refuse overlapping it. The store's own upgrade rules still apply
on first start: migrations run forward, and an older Atlas restored from
`Atlas.previous` may refuse a database a newer one migrated. The daily
backups (D13, P1-04) remain the recovery route; rollback is a manual owner
action, not automatic.

**Coordination with P1-34 (single instance).** Once P1-34 lands, only one
Atlas runs per user, so "is Atlas running?" has one answer. The scripts
check process paths and ask; they never kill Atlas. P1-38/P1-39 may later
add an explicit "close for update" request over P1-34's channel, which
needs the owner's approval of restart behaviour (P1-39).

**Coordination with P1-38 (signed updates).** An update package is this
package plus a signature. P1-38 verifies the signature over
`atlas-package.json` with an independently pinned key, then checks each
file's SHA-256 against the manifest, then performs the same staged swap as
step 3 in Rust. The manifest's `format` field lets P1-38 refuse a layout it
does not understand. The SHA-256 list in this slice detects a damaged copy;
it is not tamper protection, because the manifest itself is not yet signed.

## WebView2

Atlas uses the Evergreen WebView2 runtime only and bundles no DRM; Widevine
lives inside the runtime (`AGENTS.md`). Windows 11 ships the runtime, so it
is normally present. `install-atlas.ps1` reads, without changing, the
runtime's `pv` version value under
`EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` in the
machine-wide and per-user hives, as Microsoft documents for detecting the
runtime. If it is missing, the script stops and tells the owner to install
the Evergreen runtime from Microsoft's WebView2 page themselves. Nothing
downloads it automatically; a downloader would be a network call Atlas makes
on its own, which needs the owner's approval. A fixed-version runtime is not
used: it would stop receiving security updates. P1-35 separately records
runtime version changes for the sign-in notice.

## Building the package

```powershell
cargo xtask package
```

This runs `cargo xtask check-packaging`, builds `atlas-app` with the `local`
profile, and writes `target\package\Atlas-<version>-<commit>[-dirty]-x64\`
and a zip of it, using Windows' own `tar.exe`. A `-dirty` name means the
working tree had uncommitted changes. The manifest records the version,
commit, dirty flag, target, profile, `signed: false`, the executable, whether
it accepts links, and each file's size and SHA-256. It holds no timestamp, so
one build always yields one manifest. Nothing is installed or registered.

`cargo xtask check-packaging` runs in the gate. It refuses
`packaging/atlas-registration.ps1` if it differs from the Rust table (run
`cargo xtask write-registration` after changing the table) and parses every
script with PowerShell's parser without running it.

## Owner checklist

These steps are the owner's alone. Run them in a normal (not
administrator) PowerShell window.

**Install**

1. `cargo xtask package` in the repository, or receive a zip and confirm
   where it came from.
2. Unzip it, for example to `Downloads`.
3. `powershell -ExecutionPolicy Bypass -File <unzipped>\install-atlas.ps1`
4. Check the message names `%LOCALAPPDATA%\Programs\Atlas` and a WebView2
   runtime version.

**Register and choose the default browser**

5. `powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Atlas\register-atlas.ps1" -OpenDefaultApps`
6. If it warns that this build cannot open links yet, stop here: Atlas is
   in the Start menu and Default apps, but do not make it the default until
   a P1-34 build is installed.
7. Otherwise, in Settings > Apps > Default apps > Atlas, choose "Set
   default". Click a link in another app and check that it opens in Atlas.

**Upgrade**

8. Close Atlas.
9. Unzip the new package and run its `install-atlas.ps1` as in step 3.
10. Start Atlas and check that tabs, saved pages and sign-ins are intact.
11. If something is wrong, close Atlas and rename `Programs\Atlas.previous`
    back to `Programs\Atlas` (keep the newer folder aside); say if the older
    version refuses the database.

**Unregister only (keep Atlas installed)**

12. `powershell -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\Programs\Atlas\unregister-atlas.ps1"`
13. Choose another default browser if Windows asks.

**Uninstall (data is kept by default)**

14. Settings > Apps > Installed apps > Atlas > Uninstall, or step 12 with
    `-Uninstall`.
15. For each Atlas data folder it lists, press Enter to keep it. Type
    `DELETE` only to erase saved pages, history, settings, backups and
    website sign-ins permanently.

## What "done" means for P1-37

Prepared in this slice: the design, `cargo xtask package`, the scripts,
the single registry table with tests, and the gate check. Still pending, and
only the owner can do it: running the install, registering, choosing Atlas as
default after P1-34, one upgrade with data intact, and one uninstall that
keeps data. Until those are done and recorded, P1-37 is "prepared, owner
operation pending", not complete.
