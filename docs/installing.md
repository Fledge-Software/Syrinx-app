# P1-37: installer and browser registration

Status: **prepared, owner operation pending.** This note explains how Syrinx
gets onto the owner's machine and into Windows' Default apps list without an
agent changing Windows. It records the option chosen, the options rejected,
the exact registry keys involved and the steps only the owner performs.

## Why this slice exists

Plan 1's P1-37 row says installation must be reproducible without secretly
changing Windows. Today Syrinx runs only through `cargo syrinx` from a source
checkout. The owner's default-browser week (P1-40) needs an installed copy
that Windows can offer as a browser, that survives upgrades without losing
the owner's tabs and sign-ins, and that can be removed cleanly. Everything
system-facing here is an operator action under `AGENTS.md`: the owner, not
an agent, installs software, writes registration keys and picks the default
browser. So this slice builds artifacts and reviewed scripts and stops there.
No script in `packaging/` has been run by an agent.

## The setup program (owner decision, 2026-10-14)

Release 0.2.0 shipped only the zip described below. The owner downloaded it on
another machine, copied `syrinx.exe` out of it on its own and ran that copy.
The owner then ruled that what Syrinx ships must be one program that installs
itself: it creates the folders it needs in the right places, and running a
newer one updates the existing install. The owner chose a setup program
written in Rust over Inno Setup and a WiX MSI, and chose to register Syrinx
as a browser by default through a checkbox (`docs/decisions.md`, 2026-10-14).
The zip and its scripts stay: P1-39's updates use the same layout, and the
scripts remain the owner's manual route.

`syrinx-setup.exe` is a second binary of `syrinx-app`
(`crates/syrinx-app/src/bin/syrinx-setup.rs`). It was put there rather than in
a crate of its own for one reason. `cargo xtask check-deps` already lets
`syrinx-app` use the update crate and `syrinx-platform`, so the dependency
rules (P1-39's change 1) did not have to change. Its Windows calls (task
dialogs, the running-Syrinx check, one registry read) are in
`syrinx-platform/src/setup.rs`, as the platform boundary requires. The release
build carries the finished package inside the executable:

1. `cargo xtask package` builds the package folder and zip as before.
2. The release tool's new `payload` command
   (`syrinx-update-tool payload --package <folder> --output <file>`) packs the
   folder. The payload is a fixed header, P1-38's `manifest`, then every file
   the manifest lists, in its order. Packing checks each file's size and
   SHA-256 against the manifest.
3. `syrinx-app`'s build script reads `SYRINX_SETUP_PAYLOAD` and embeds that
   file in `syrinx-setup.exe`. Without the variable it embeds nothing, and
   such a setup program refuses to install, so the ordinary gate can build
   and lint it.
4. The result is `target\package\Syrinx-<version>-<commit>-x64-setup.exe`.
   The weekly release publishes it as the main download, with the zip and
   both checksums (`docs/releasing.md`).

**What running it does.** Everything happens per user, with no
administrator prompt:

1. It reads its payload and refuses a damaged one: any size, SHA-256 or
   layout mismatch, a missing `syrinx-apply.exe`, or trailing bytes.
2. It reads `%LOCALAPPDATA%\Programs\Syrinx\manifest` to learn what is
   installed. The heading says "Install", "Update 0.2.0 to 0.3.0",
   "Reinstall" or "Replace … with the older …".
3. It shows one dialog with Install (or Update) and Cancel, and the checkbox
   "Register Syrinx as a browser". The box is ticked on a first install. On
   an update it shows whether Syrinx is registered now.
4. If a Syrinx runs from `Syrinx`, `Syrinx.new` or `Syrinx.previous`, it asks
   the owner to close it and offers Retry. It never closes Syrinx itself.
5. `syrinx_update::install::install_package` does the rest:
   - It creates `Programs` and `Syrinx\data` if they are missing, and takes
     P1-39b's update lock, so an in-app update cannot run at the same time.
   - It refuses a `Syrinx` or `Syrinx.previous` folder that Syrinx's tooling
     did not make, and does so before writing anything.
   - It discards any in-app update waiting in `Syrinx.new`, together with its
     `Updates` records, so the next start does not offer it.
   - It writes every file into a fresh `Syrinx.new`, `syrinx-package.json`
     first, so an interrupted copy is still recognised as Syrinx's. A failed
     copy is removed.
   - It swaps that copy in with the same function the update helper uses, and
     keeps the version it replaced as `Syrinx.previous`. That function
     removes the old `Syrinx.previous` before it moves anything. A swap
     refused because the folder is in use (`ProgramInUse`) therefore costs
     that older copy, though the installed version stays.
   - If the installed version was moved aside but could not be put back,
     setup says so and names `Syrinx.previous`.
   - It never touches the profile in the data folder.
6. It runs the installed `register-syrinx.ps1` in a hidden Windows
   PowerShell, started from its fixed system path with
   `-ExecutionPolicy RemoteSigned`:
   - Ticked, the script writes the full registration as before.
   - Unticked, it runs with the new `-AppOnly` switch, which writes only the
     Installed apps entry and the Start menu shortcut.
   - Unticking after an earlier registration first runs
     `unregister-syrinx.ps1`.
   - The scripts carry no Mark of the Web, because setup wrote them, so
     `RemoteSigned` runs them. This keeps one reviewed implementation of the
     registry table rather than a second one in Rust.
7. It says Syrinx is installed and offers "Open Syrinx".

**What it does not do yet.** Each of these is a possible follow-up, not a
deferred-work marker in the code:

- It does not check for the WebView2 runtime the way `install-syrinx.ps1`
  does. Windows 11 ships the runtime (D2), but a missing one would only
  show up when Syrinx starts.
- Uninstall still runs `unregister-syrinx.ps1` from Installed apps.
- The payload is not compressed. The setup program is about the size of the
  unzipped package (around 45 MB at 0.2.0), not of the zip (15 MB).
  Windows' built-in compression API would close that gap without a new
  dependency.
- The setup program is unsigned (D38), so a downloaded copy shows
  SmartScreen's "Windows protected your PC" with an unknown publisher until
  signing exists.

**Tests.** `crates/syrinx-update/src/install/tests_setup.rs` covers these
cases:
- the payload round-trips;
- damaged payloads are refused;
- a package without the helper is refused;
- a first install works;
- upgrading twice keeps one previous version and leaves a synthetic profile
  file untouched;
- a foreign `Syrinx` folder is refused;
- a held update lock stops setup.

`syrinx-platform/src/setup.rs` tests the running-from-folder check, and the
setup binary tests its headings. The real install writes to the owner's
`%LOCALAPPDATA%` and per-user registry, so only the owner runs it (see the
checklist below).

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
write to `HKLM`, which nothing in Syrinx's scope requires.

**Recommended: a portable zip plus owner-run PowerShell scripts.** `cargo
xtask package` builds `syrinx.exe` with D38's `local` profile and writes a
versioned folder and zip containing the executable, a manifest
(`syrinx-package.json`) and three scripts: `install-syrinx.ps1` copies a verified
package into `%LOCALAPPDATA%\Programs\Syrinx`; `register-syrinx.ps1` writes the
per-user browser registration; `unregister-syrinx.ps1` removes it and, with
`-Uninstall`, the program files. Why this one:

- It needs nothing that is not already on the machine, so no agent or owner
  has to install a packaging tool first.
- Every change it makes to Windows is a line the owner can read before
  running it. The registry list is one Rust table
  (`crates/xtask/src/packaging/registration.rs`) rendered into
  `packaging/syrinx-registration.ps1`; tests check that the file matches the
  table and that unregistering covers every value registering writes.
- It keeps install, registration and default selection as three separate
  owner decisions, which is what the operator-work list asks for.
- P1-38's signed update packages can reuse the same layout and manifest.

The cost is polish: there is no wizard, and PowerShell's default execution
policy on Windows 11 client editions (`Restricted`) refuses every script, so
the owner starts each one with
`powershell -ExecutionPolicy RemoteSigned -File ...`. That flag applies to
that one PowerShell process only and changes no Windows setting. Under
`RemoteSigned`, local unsigned scripts run, but a script carrying the Mark of
the Web (downloaded, or unzipped from a downloaded zip) is refused unless
signed. `Bypass` was rejected because it would skip that check silently.

**Rejected: MSIX.** `makeappx.exe` is present, but an MSIX package must be
signed by a certificate the machine trusts before Windows installs it. Syrinx
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
`AGENTS.md`'s list; a build tool's licence does not bind Syrinx's code, but
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

Syrinx stays unsigned for now (D38). What the owner will see:

- **Files built on this machine** carry no Mark of the Web, the marker
  Windows adds to downloaded files. SmartScreen's "Windows protected your
  PC" prompt does not appear for them, and `RemoteSigned` runs the scripts.
- **A zip copied from elsewhere** (a browser download, an email) carries the
  Mark of the Web, and unzipping with Explorer passes it to every file.
  `RemoteSigned` then refuses the unsigned scripts, and `syrinx.exe` shows
  "Windows protected your PC" with an unknown publisher. This is the check
  working. If the owner has confirmed where the zip came from, they run
  `Unblock-File` on the zip before unzipping it (or on each extracted file),
  which removes the marker; otherwise they build the package locally.
- **Smart App Control** can block unsigned executables outright, with no
  "Run anyway" (`docs/dev/LOCAL-DEV.md`, `docs/dev/trapstoknow.md`). The owner has
  turned it off on this machine (2026-10-12); a fresh Windows install may
  block Syrinx until signing exists.
- **Default apps** shows Syrinx by the name in its Capabilities key. Windows
  does not require a signature to list a browser there.

Nothing in this slice turns off or works around SmartScreen, Smart App
Control or execution policy. Every script and the Installed apps uninstall
command use `-ExecutionPolicy RemoteSigned`; `cargo xtask check-packaging`
refuses `Bypass` in any script.

## Where things live

| What | Where | Lifetime |
| --- | --- | --- |
| Program files | `%LOCALAPPDATA%\Programs\Syrinx\` | Replaced on upgrade; removed by `unregister-syrinx.ps1 -Uninstall`. |
| Previous version | `%LOCALAPPDATA%\Programs\Syrinx.previous\` | One kept for manual rollback; replaced on the next upgrade. |
| Installed Syrinx's data | `%LOCALAPPDATA%\Syrinx\data\` (`State`, `WebView2`) | Never touched by install or upgrade; kept on uninstall unless the owner types DELETE. |
| Development and preview data | `%LOCALAPPDATA%\SyrinxProductPreview\` | Not used by the installed app; uninstall offers it only with `-IncludePreviewData`. |
| Registration | `HKCU` keys listed below | Written by `register-syrinx.ps1`; removed by `unregister-syrinx.ps1`. |
| Start menu shortcut | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Syrinx.lnk` | Same as registration. |

`%LOCALAPPDATA%\Programs` is Windows' conventional per-user program folder.
All data stays in local, never roaming, app data (D13). The program folder
and data folders never overlap; `install-syrinx.ps1` refuses an install
folder inside either data folder, and an xtask test checks the defaults.

The owner decided on 2026-10-12 that the installed Syrinx keeps its data in
`%LOCALAPPDATA%\Syrinx\data`, separate from `SyrinxProductPreview`, which
development and preview runs (`cargo syrinx`) keep using. Nothing is moved:
the installed app starts empty. `cargo xtask package` builds `syrinx-app`
with its `installed` Cargo feature, which switches one constant
(`DATA_FOLDER` in `crates/syrinx-app/src/main.rs`). A feature was chosen over
a run-time switch because the folder is then fixed in the shipped
executable and cannot be redirected by a command-line argument. An xtask
test checks that the constant and the packaging table name the same folders,
and the manifest records `data_folder`.

**Folders and registrations from before the rename.** Until 2026-10-12 the
product was called Atlas, and these folders were `%LOCALAPPDATA%\Atlas\data`,
`%LOCALAPPDATA%\AtlasProductPreview` and `%LOCALAPPDATA%\Programs\Atlas`,
with registry keys named `AtlasBrowser` and `AtlasBrowserURL`. Syrinx does
not read, move, migrate or delete any of them: it starts fresh under the new
names. The old folders may hold signed-in WebView2 profiles, so they are the
owner's to delete by hand once nothing in them is needed. If the owner ever
ran the old `register-atlas.ps1`, those registrations are the owner's to
remove with the old `unregister-atlas.ps1` from that package (it removes only
keys carrying its `atlas-packaging-v1` marker); the Syrinx scripts look only
for Syrinx's own marker and never touch the Atlas keys.

## Browser registration

The table below is every value the scripts handle. All are under
`HKEY_CURRENT_USER`; nothing goes to `HKEY_LOCAL_MACHINE`. `{exe}` is the
installed `syrinx.exe`, `{dir}` its folder and `{version}` the manifest's
version. The source of truth is `VALUES` and `SHARED_VALUES` in
`crates/xtask/src/packaging/registration.rs`; `cargo xtask
write-registration` renders this table and `packaging/syrinx-registration.ps1`
from it, and the gate fails if either differs.

<!-- BEGIN GENERATED REGISTRY TABLE -->
| Key under `HKCU\` | Value | Data |
| --- | --- | --- |
| `Software\Clients\StartMenuInternet\SyrinxBrowser` | `SyrinxPackagingOwner` | `syrinx-packaging-v1` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser` | `(default)` | `Syrinx` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\DefaultIcon` | `(default)` | `{exe},0` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\shell\open\command` | `(default)` | `"{exe}"` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities` | `ApplicationName` | `Syrinx` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities` | `ApplicationDescription` | `Syrinx web browser` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities` | `ApplicationIcon` | `{exe},0` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities\StartMenu` | `StartMenuInternet` | `SyrinxBrowser` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities\URLAssociations` | `http` | `SyrinxBrowserURL` |
| `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities\URLAssociations` | `https` | `SyrinxBrowserURL` |
| `Software\Classes\SyrinxBrowserURL` | `SyrinxPackagingOwner` | `syrinx-packaging-v1` |
| `Software\Classes\SyrinxBrowserURL` | `(default)` | `Syrinx URL` |
| `Software\Classes\SyrinxBrowserURL` | `FriendlyTypeName` | `Syrinx URL` |
| `Software\Classes\SyrinxBrowserURL\Application` | `ApplicationName` | `Syrinx` |
| `Software\Classes\SyrinxBrowserURL\Application` | `ApplicationDescription` | `Syrinx web browser` |
| `Software\Classes\SyrinxBrowserURL\Application` | `ApplicationIcon` | `{exe},0` |
| `Software\Classes\SyrinxBrowserURL\DefaultIcon` | `(default)` | `{exe},0` |
| `Software\Classes\SyrinxBrowserURL\shell\open\command` | `(default)` | `"{exe}" -- "%1"` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `SyrinxPackagingOwner` | `syrinx-packaging-v1` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `DisplayName` | `Syrinx` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `DisplayVersion` | `{version}` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `DisplayIcon` | `{exe},0` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `InstallLocation` | `{dir}` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `UninstallString` | `powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File "{dir}\unregister-syrinx.ps1" -Uninstall -PauseAtEnd` |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `NoModify` | DWORD 1 |
| `Software\Microsoft\Windows\CurrentVersion\Uninstall\SyrinxBrowser` | `NoRepair` | DWORD 1 |
| `Software\RegisteredApplications` | `SyrinxBrowser` | `Software\Clients\StartMenuInternet\SyrinxBrowser\Capabilities` |
<!-- END GENERATED REGISTRY TABLE -->

What the keys do:

- **`StartMenuInternet\SyrinxBrowser`** registers Syrinx as a browser;
  `Capabilities` says it handles `http` and `https` links through the
  `SyrinxBrowserURL` ProgID (programmatic identifier: the named handler a
  link scheme or file type is associated with).
- **`RegisteredApplications`** points Windows at those capabilities. Every
  browser shares that key, so Syrinx writes only its `SyrinxBrowser` value.
- **`Classes\SyrinxBrowserURL`** is the ProgID whose `shell\open\command`
  opens a link. It deliberately has no `URL Protocol` value: that value
  would turn the ProgID into a new `syrinxbrowserurl:` link scheme, which
  http and https handling does not need.
- **`Uninstall\SyrinxBrowser`** is the Installed apps entry. Its uninstall
  command passes `-PauseAtEnd` so the window stays open long enough to read.

**Ownership.** "Syrinx" is a common name, so every key Syrinx owns is named
`SyrinxBrowser` (or `SyrinxBrowserURL`) and carries an owner marker value,
`SyrinxPackagingOwner = syrinx-packaging-v1`. `register-syrinx.ps1` refuses,
before writing anything, if one of those keys exists without the marker or
if the shared `RegisteredApplications` value points elsewhere.
`unregister-syrinx.ps1` deletes a key tree only when it carries the marker,
and the shared value only while it still holds Syrinx's data. Tests check
that every owned root has a marker, that unregistering covers every value
registering writes, and that no shared key is ever deleted whole. After
writing or removing keys, both scripts call
`SHChangeNotify(SHCNE_ASSOCCHANGED)` so Explorer re-reads associations.

What is deliberately absent:

- **No file associations** (`.htm`, `.html`, `.pdf`). Syrinx's navigation
  accepts web addresses; opening local files is not specified yet. Adding
  them is a later owner decision and a one-line table change.
- **No `UserChoice` writes or any other attempt to set the default.**
  Windows 11 protects the default browser with a hashed `UserChoice` value
  and expects the owner to pick it in Settings > Apps > Default apps. The
  script can open that page (`-OpenDefaultApps`, using
  `ms-settings:defaultapps?registeredAppUser=SyrinxBrowser`) and stops there.
- **No legacy `InstallInfo` show/hide-icons commands** and no
  `AppUserModelId`: Syrinx sets no explicit application user model ID, and
  writing one the process does not use would mis-group taskbar entries.
- **No `App Paths` entry.** It would let Win+R start `syrinx-app`; it is
  convenience only and can be added later.

### The link command and P1-34

When the owner clicks a link elsewhere, Windows runs the `SyrinxBrowserURL`
ProgID's command, `"{exe}" -- "%1"`, with `%1` replaced by the link. This
is the argument form P1-34 decided: Syrinx accepts a link only as the single
argument after the `--` marker. Anything after `--` is never read as a
switch, so a crafted link cannot smuggle options into Syrinx, and P1-34
refuses a command line with more than one argument after the marker. P1-34
also accepts only http and https links and forwards them to an
already-running Syrinx. Starting Syrinx from the Start menu or Default apps
uses the `StartMenuInternet` command, `"{exe}"`, with no arguments.

**Today's `syrinx.exe` refuses any argument it does not know**
(`run_mode` in `crates/syrinx-app/src/main.rs`), so a registered build would
show an error instead of opening the link. `ACCEPTS_OPEN_URL` in the
registration table is therefore `false`; it is copied into the generated
script and the manifest, and `register-syrinx.ps1` warns the owner not to
choose Syrinx as default until a build that accepts links is installed.
P1-34 sets it to `true` when it lands.

**P1-34 and P1-37 must land together, before the owner registers.** The
registration written here already passes links as `-- "%1"`. Registered
against a build without P1-34, every link from another app would fail to
open; merged without this registration, P1-34's link handling would never
be reached. The owner should register only from a package built after both
are merged, when the warning no longer appears.

## Script safeguards

The scripts were reviewed but cannot be run by an agent, so their guards are
kept simple and shared through `packaging/syrinx-common.ps1`:

- Every folder goes through `Resolve-SyrinxFolder`, which makes it absolute,
  trims a trailing backslash and refuses a drive root. Without the trim,
  `...\Syrinx\` would put the staging folder inside the target.
- The install refuses to install into or below the unzipped package, and
  refuses while Syrinx runs from the program folder, `.previous` or `.new`.
- A manifest is accepted only when its file list equals the fixed package
  layout, so it cannot name paths outside the folder.
- A program folder is deleted only when its manifest shows this tooling made
  it; the manifest is deleted last, so a partial failure can be retried.
- Relative paths resolve against PowerShell's current location
  (`GetUnresolvedProviderPathFromPSPath`), not the process folder that .NET's
  `GetFullPath` would use.
- `-Uninstall` compares the folder it runs from with the registered
  `InstallLocation` and changes nothing if they differ, so running it from
  an unzipped package cannot delete the wrong folder.
- If `-Uninstall` stops after removing the registration (for example on a
  locked file), the retry finds no registration. It then deletes program
  files only when the script itself runs from that folder and the folder
  holds a valid Syrinx manifest and `syrinx.exe`. Installed apps no longer
  lists Syrinx at that point, so the owner retries with
  `unregister-syrinx.ps1 -Uninstall` from `%LOCALAPPDATA%\Programs\Syrinx`.
- If data deletion is partial (WebView2 can still hold files), the script
  reports how many items remain instead of claiming success.
- No script uses `using module`, the one construct that loads code while
  PowerShell parses a file; the gate checks this, then parses every script
  with `[System.Management.Automation.Language.Parser]::ParseFile`, which
  runs nothing.

## Upgrade

1. The owner builds or receives a new package and unzips it anywhere.
2. `install-syrinx.ps1` verifies every file's SHA-256 against the manifest,
   refuses if a Syrinx from the program folder is running, and never closes
   Syrinx itself. The owner closes Syrinx and runs the script again.
3. It copies the package to `Programs\Syrinx.new`, renames the current
   folder to `Programs\Syrinx.previous` (replacing an older one), then
   renames the new folder into place. If that last rename fails, it renames
   the previous version back.
4. Registration names the stable path `Programs\Syrinx\syrinx.exe`, so
   links keep working without re-registering. Re-running
   `register-syrinx.ps1` only refreshes the version in Installed apps.

Do not use Syrinx's own update commands ("Install update from file…", or
quitting with an update set to install) while `install-syrinx.ps1` is
running. Since P1-39b an installed Syrinx uses the same `Programs\Syrinx.new`
folder for its own updates and would replace the script's half-copied
folder. Merely starting Syrinx is safe: start-up removes a `Syrinx.new` only
when Syrinx's own `Updates\staging` mark says it made it. Rerunning the
script recovers (`docs/update-format.md`, "P1-39b").

Data is preserved because nothing in this path knows where data lives
except to refuse overlapping it. The store's own upgrade rules still apply
on first start: migrations run forward, and an older Syrinx restored from
`Syrinx.previous` may refuse a database a newer one migrated. The daily
backups (D13, P1-04) remain the recovery route; rollback is a manual owner
action, not automatic.

**Coordination with P1-34 (single instance).** Once P1-34 lands, only one
Syrinx runs per user, so "is Syrinx running?" has one answer. The scripts
check process paths and ask; they never kill Syrinx. P1-38/P1-39 may later
add an explicit "close for update" request over P1-34's channel, which
needs the owner's approval of restart behaviour (P1-39).

**Coordination with P1-38 (signed updates).** An update package is this
package plus a signature. P1-38 verifies the signature over
`syrinx-package.json` with an independently pinned key, then checks each
file's SHA-256 against the manifest, then performs the same staged swap as
step 3 in Rust. The manifest's `format` field lets P1-38 refuse a layout it
does not understand. The SHA-256 list in this slice detects a damaged copy;
it is not tamper protection, because the manifest itself is not yet signed.

## WebView2

Syrinx uses the Evergreen WebView2 runtime only and bundles no DRM; Widevine
lives inside the runtime (`AGENTS.md`). Windows 11 ships the runtime, so it
is normally present. `install-syrinx.ps1` reads, without changing, the
runtime's `pv` version value under
`EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` in the
machine-wide and per-user hives, as Microsoft documents for detecting the
runtime. If it is missing, the script stops and tells the owner to install
the Evergreen runtime from Microsoft's WebView2 page themselves. Nothing
downloads it automatically; a downloader would be a network call Syrinx makes
on its own, which needs the owner's approval. A fixed-version runtime is not
used: it would stop receiving security updates. P1-35 separately records
runtime version changes for the sign-in notice.

## Building the package

```powershell
cargo xtask package
```

This runs `cargo xtask check-packaging`, builds `syrinx-app` with the `local`
profile and its `installed` feature (data in `Syrinx\data`) into its own
Cargo target directory, `target\package-build`, and writes
`target\package\Syrinx-<version>-<commit>[-dirty]-x64\` and a zip of it,
using Windows' own `tar.exe`. A `-dirty` name means the working tree had
uncommitted changes. The manifest records the version, commit, dirty flag,
target, profile, `signed: false`, the executable, whether it accepts links,
the data folder, and each file's size and SHA-256. It holds no timestamp, so
one build always yields one manifest. Nothing is installed or registered.

`cargo xtask check-packaging` runs in the gate. It refuses
`packaging/syrinx-registration.ps1` or this document's registry table if
either differs from the Rust table (run `cargo xtask write-registration`
after changing the table), refuses `using module`, `ExecutionPolicy Bypass`
and machine-wide browser keys in any script, and parses every script with
PowerShell's parser without running it.

## Owner checklist

These steps are the owner's alone.

**Install or update with the setup program (since 2026-10-14)**

1. Download `Syrinx-<version>-<commit>-x64-setup.exe` from the release page
   and double-click it. Windows may first show "Windows protected your PC"
   because it is unsigned. Choose "More info", then "Run anyway", only after
   confirming where it came from.
2. Leave "Register Syrinx as a browser" ticked to see it in Default apps,
   then press Install (or Update). Close Syrinx first if setup asks.
3. Press "Open Syrinx". For a later version, run its setup program the same
   way: the tabs, saved pages and sign-ins in `%LOCALAPPDATA%\Syrinx\data`
   stay.

The script steps below remain the manual route. Run them in a normal (not
administrator) PowerShell window.

**Install with the scripts**

1. `cargo xtask package` in the repository, or receive a zip and confirm
   where it came from.
2. Unzip it, for example to `Downloads`. For a zip that was downloaded or
   emailed, and only after confirming its source, run `Unblock-File` on the
   zip first; otherwise `RemoteSigned` refuses its scripts.
3. `powershell -ExecutionPolicy RemoteSigned -File <unzipped>\install-syrinx.ps1`
4. Check the message names `%LOCALAPPDATA%\Programs\Syrinx` and a WebView2
   runtime version.

**Register and choose the default browser**

5. `powershell -ExecutionPolicy RemoteSigned -File "$env:LOCALAPPDATA\Programs\Syrinx\register-syrinx.ps1" -OpenDefaultApps`
6. If it warns that this build cannot open links yet, stop here: Syrinx is
   in the Start menu and Default apps, but do not make it the default until
   a P1-34 build is installed.
7. Otherwise, in Settings > Apps > Default apps > Syrinx, choose "Set
   default". Click a link in another app and check that it opens in Syrinx.

**Upgrade**

8. Close Syrinx.
9. Unzip the new package and run its `install-syrinx.ps1` as in step 3.
10. Start Syrinx and check that tabs, saved pages and sign-ins are intact.
    (The first installed start is empty by design: the installed app uses
    `Syrinx\data`, not the preview's `SyrinxProductPreview`.)
11. If something is wrong, close Syrinx and rename `Programs\Syrinx.previous`
    back to `Programs\Syrinx` (keep the newer folder aside); say if the older
    version refuses the database.

**Unregister only (keep Syrinx installed)**

12. `powershell -ExecutionPolicy RemoteSigned -File "$env:LOCALAPPDATA\Programs\Syrinx\unregister-syrinx.ps1"`
13. Choose another default browser if Windows asks.

**Uninstall (data is kept by default)**

14. Settings > Apps > Installed apps > Syrinx > Uninstall, or step 12 with
    `-Uninstall`, run from the installed folder.
15. When it shows `%LOCALAPPDATA%\Syrinx\data`, press Enter to keep it. Type
    `DELETE` only to erase saved pages, history, settings, backups and
    website sign-ins permanently. The development preview folder is offered
    only with `-IncludePreviewData`, as a separate question.
16. If it reports that items remain, close every Syrinx window and delete
    the folder it names by hand.
17. If uninstall stopped partway and Syrinx is no longer in Installed apps,
    run `unregister-syrinx.ps1 -Uninstall` again from
    `%LOCALAPPDATA%\Programs\Syrinx` to finish removing the program files.

## What "done" means for P1-37

Prepared in this slice: the design, `cargo xtask package`, the scripts,
the single registry table with tests, and the gate check. Still pending, and
only the owner can do it: running the install, registering, choosing Syrinx as
default after P1-34, one upgrade with data intact, and one uninstall that
keeps data. Until those are done and recorded, P1-37 is "prepared, owner
operation pending", not complete.

## Linux (2026-10-31)

Syrinx is one app for Windows and Linux (`docs/decisions.md`, 2026-10-31), so
`cargo xtask package` run on Linux builds the Linux package instead of the
setup program. Linux has no setup program, registry or WebView2: the package
is a portable tarball with two short shell scripts, which the person
installing it reads and runs themselves, the same "the owner runs it, the
agent never does" rule as the PowerShell scripts above.

**Runtime requirement.** Syrinx's pages render in WebKitGTK, so the machine
needs **WebKitGTK 4.1** (the `webkit2gtk-4.1` API, 2.38 or newer), **GTK 3**,
libsoup 3 and bubblewrap, WebKit's page sandbox. It needs an **X11 session,
or XWayland** under a Wayland compositor (GNOME, KDE, Hyprland): GTK can
embed a page only inside an X11 window (`docs/dev/LOCAL-DEV.md`, "On Linux").
On Arch-based systems that is `webkit2gtk-4.1 gtk3 libsoup3 bubblewrap`;
Debian and Ubuntu call the library `libwebkit2gtk-4.1-0`, Fedora
`webkit2gtk4.1`. Protected video (Disney+, Hulu) does not play, because
WebKitGTK has no Widevine.

**What `cargo xtask package` writes on Linux.** It runs `cargo xtask
check-packaging`, builds the `syrinx` binary with the `local` profile and the
`installed` feature into `target/package-build`, exactly as on Windows but
with debug information stripped (Windows keeps it in a separate `.pdb`; on
Linux it would grow the binary from about 90 MB to over 400 MB), and
writes `target/package/Syrinx-<version>-<commit>[-dirty]-linux-x64/` and
`Syrinx-<version>-<commit>[-dirty]-linux-x64.tar.gz`. The name is the Windows
package's name with `linux-` before the architecture
(`manifest::linux_package_name`). The tarball holds one folder of that name:

| File | What it is |
| --- | --- |
| `syrinx` | The program (mode 0755). |
| `syrinx.desktop` | The freedesktop desktop entry: name, icon, `Categories=Network;WebBrowser;` and `StartupWMClass=syrinx`, which matches the X11 window class Syrinx sets so the window gets the icon. |
| `syrinx.png` | The 256 px app icon, `design/brand/icon/syrinx-icon-256.png`. |
| `INSTALL.txt` | Requirements, install, update and uninstall in plain text. |
| `LICENSE` | Syrinx's MPL-2.0 licence. |
| `install.sh`, `uninstall.sh` | The per-user scripts below (mode 0755). |
| `SHA256SUMS` | Every file's SHA-256 in `sha256sum` format, so `sha256sum -c SHA256SUMS` checks the unpacked copy. |

The fixed files live in `packaging/linux/`. The tar entries are owned by
root with numeric ids and sorted by name, so the archive does not record who
built it. The gate checks that every source exists and parses both scripts
with `sh -n`, which runs nothing; tests in
`crates/xtask/src/packaging/linux.rs` pin the layout, the desktop entry and
the scripts' rules.

**`install.sh`** installs for the current user only, never with root (it
refuses to run as root, because a per-user install would land in root's
home):

1. It checks every file against `SHA256SUMS` and stops on a mismatch
   (`sha256sum` missing gives a warning instead).
2. It warns, without stopping, if `ldconfig` does not list WebKitGTK 4.1.
3. It copies `syrinx` to `~/.local/bin/syrinx` and the icon to
   `~/.local/share/icons/hicolor/256x256/apps/syrinx.png`, each through a
   temporary name and a rename, so a running Syrinx keeps its file.
4. It writes `~/.local/share/applications/syrinx.desktop` last, with `Exec=`
   naming the installed program by its full path, because `~/.local/bin` is
   not on every PATH. `$XDG_DATA_HOME` replaces `~/.local/share` when set.

It does not make Syrinx the default browser and does not declare it a link
handler: the entry has no `MimeType=` line. Syrinx accepts a link only after
`--` and refuses a bare `--`, so an entry that passed `%u` would fail when
started from the menu with no link. Handling links on Linux is a follow-up.

**`uninstall.sh`** removes only what `install.sh` wrote. It acts only if the
desktop entry carries the marker line `X-Syrinx-Packaging=syrinx-packaging-v1`,
removes the program and icon first and the entry last (so an interrupted run
can be repeated), and never touches the data folder,
`~/.local/share/Syrinx/data`; it names that folder for the person to delete
by hand if they want their data gone.

**Updates.** In-app updates install only on Windows. On Linux, Settings and
"Check for updates" say that Syrinx doesn't update itself on Linux yet and
point at syrinxapp.dev; updating means unpacking the newer tarball and
running its `install.sh`, which keeps the data folder.
