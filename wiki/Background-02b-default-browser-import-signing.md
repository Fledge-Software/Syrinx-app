> **Research note dated 2026-10-09.** Read for reasoning, not current fact; later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> An appendix to note 02, written for a Tauri build. Registering Syrinx as a browser is operator work for the owner, not an agent. Notes 07 and 08 correct parts of the earlier notes, and note 03 assumes Tauri. The product was called Atlas until 2026-10-12; read Atlas here as Syrinx.
>
> Moved from `docs/research/02b-default-browser-import-signing.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/02b-default-browser-import-signing.md)) on 2026-10-16.

# Default browser, importing, and signing

Checked 2026-10-09. A helper of the browser-scope researcher
([`02-browser-v1-scope.md`](Background-02-browser-v1-scope)) gathered this after that note was written, so it is
kept here as an appendix. Confidence is high unless marked (M) medium or (L)
low. "Inference" marks the helper's reasoning rather than a source's statement.

## Short answer

- **Default browser.** Atlas can register itself as a browser, but on Windows
  11 it cannot make itself the default: the user has to choose it in Settings.
  Atlas can open the right Settings page directly.
- **Import.** Bookmarks and history import cleanly from Chrome, Edge and
  Firefox. Chrome's cookies and newer passwords are bound to Chrome since Chrome
  127, so an importer can't decrypt them, and shouldn't try.
- **Signing.** Signing no longer buys instant SmartScreen trust; reputation
  builds over weeks. Smart App Control on Windows 11 blocks unsigned
  executables outright.
- **Microsoft Store.** Store policy says a browser must use the Chromium or
  Gecko engine, which would rule out a Servo-based Atlas there.

## 1. Becoming the default browser on Windows 11

### Registration

- **The Capabilities key** holds `ApplicationDescription` (required, or the
  app is not listed), optional `ApplicationName`, and the subkeys
  `FileAssociations`, `MIMEAssociations`, `UrlAssociations` and
  `Startmenu\StartMenuInternet`.
- **`SOFTWARE\RegisteredApplications`** maps the app name to that Capabilities
  path, under HKLM or HKCU.
- **ProgIDs:** every association points to an app-specific ProgID.
  Microsoft's Contoso browser example registers `.htm`, `.html`, `.shtml`,
  `.xht`, `.xhtml` and `http`, `https`, `ftp`.
- **`Clients\StartMenuInternet`** is deprecated since Windows 7, and its
  registrations are ignored. Browsers still write it by convention.
- Sources: https://learn.microsoft.com/en-us/windows/win32/shell/default-programs,
  https://learn.microsoft.com/en-us/windows/win32/shell/start-menu-reg

### Protection of the user's choice

- **No programmatic changes.** Windows does not allow defaults to be changed
  without the user acting in system UI.
- **Protected storage.** The choices are obfuscated (the "UserChoice hash"),
  and a filter driver (UCPD.sys) blocks apps from writing them.
- **Reading is allowed:** an app may query the current default with
  `IApplicationAssociationRegistration::QueryCurrentDefault`.
- **The Settings deep link** goes straight to Atlas's page in Settings:
  - `ms-settings:defaultapps?registeredAppUser=<escaped name from HKCU\Software\RegisteredApplications>`;
  - `registeredAppMachine=` for HKLM installs;
  - `registeredAUMID=` for packaged apps.

  It needs Windows 11 21H2 or 22H2 with the April 2023 update, or 23H2 and
  later.
- **The "Set default" button** sets http, https, .htm and .html everywhere. In
  the EEA, under the Digital Markets Act, it also sets more file types and can
  pin the browser to the taskbar and Start (rolled out from July 2025).
- **Deprecated APIs:** `SetAppAsDefault` and `LaunchAdvancedAssociationUI` no
  longer do what their names say.
- **Install notification:** after writing its keys, the installer should call
  `SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_DWORD|SHCNF_FLUSH, …)`. That lets
  Windows show its one-time "new browser" notification.
- Sources:
  https://learn.microsoft.com/en-us/windows/apps/develop/windows-integration/default-apps-platform,
  https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings,
  https://blogs.windows.com/windowsexperience/2023/03/17/a-principled-approach-to-app-pinning-and-app-defaults-in-windows/,
  https://blogs.windows.com/windows-insider/2025/06/02/updates-to-windows-for-the-digital-markets-act/

## 2. Importing from other browsers

### Chrome and Edge

- **Profile folders:**
  - Chrome on Windows: `%LOCALAPPDATA%\Google\Chrome\User Data`;
  - Chrome on macOS: `~/Library/Application Support/Google/Chrome`;
  - Edge on Windows: `%LOCALAPPDATA%\Microsoft\Edge\User Data` (M);
  - each profile is a subfolder, usually `Default`.
- **The Bookmarks file** is JSON:
  - `roots` holds `bookmark_bar`, `other` and `synced`, plus `version` and a
    checksum;
  - nodes carry `id`, `guid`, `name`, `type` (url or folder), `url`,
    `children` and dates;
  - dates are microseconds since 1601-01-01, as decimal strings.
- **The History SQLite database:**
  - tables: `urls` (url, title, visit_count, typed_count, last_visit_time,
    hidden) and `visits` (url id, visit_time, from_visit, transition,
    visit_duration);
  - times are microseconds since 1601;
  - Chrome's lock on it is advisory, and write-ahead logging is off, so it is
    a single file.

  Inference (M): a plain byte copy of the file works while Chrome runs; open
  the copy read-only.
- **App-Bound Encryption.** Since Chrome 127 on Windows (July 2024), cookies
  are bound to Chrome's identity through a privileged service, and other apps'
  attempts to decrypt them fail. Passwords followed later (L). Plan on
  importing bookmarks and history only, plus the user's own password CSV
  export.
- Sources: https://chromium.googlesource.com/chromium/src/+/main/docs/user_data_dir.md,
  https://chromium.googlesource.com/chromium/src/+/main/components/bookmarks/browser/bookmark_codec.cc,
  https://chromium.googlesource.com/chromium/src/+/main/components/history/core/browser/history_database.cc,
  https://chromium.googlesource.com/chromium/src/+/main/sql/database.h,
  https://www.sqlite.org/fileformat2.html,
  https://security.googleblog.com/2024/07/improving-security-of-chrome-cookies-on.html

### Firefox

- **Profiles:**
  - Windows: `%APPDATA%\Mozilla\Firefox\Profiles`;
  - macOS: `~/Library/Application Support/Firefox/Profiles`;
  - `profiles.ini` lists the profiles and each install's default;
  - a profile in use is locked (`parent.lock` on Windows, `.parentlock` on
    macOS).
- **places.sqlite:**
  - tables: `moz_places` (url, title, visit_count, last_visit_date),
    `moz_historyvisits` (place_id, visit_date, visit_type) and `moz_bookmarks`
    (type, fk, parent, position, title, dates);
  - times are microseconds since 1970;
  - it uses write-ahead logging, so a copy must include `places.sqlite-wal`.
    Mozilla's own backup opens it read-only and uses SQLite's backup API.
- **Bookmark backups:**
  - they live in `bookmarkbackups/` as `.jsonlz4`;
  - format (M): an 8-byte magic `mozLz40\0`, a 4-byte little-endian size, then
    one LZ4 block.
- **Reading while Firefox runs** (M) is plausible on Windows and blocked on
  macOS. Chromium itself asks the user to close Firefox before importing.
- Sources: https://firefox-source-docs.mozilla.org/toolkit/profile/index.html,
  https://firefox-source-docs.mozilla.org/browser/places/architecture-overview.html,
  https://searchfox.org/firefox-main/source/toolkit/components/places/nsPlacesTables.h,
  https://searchfox.org/firefox-main/source/toolkit/profile/nsProfileLock.cpp,
  https://codereview.chromium.org/2769383002

### Netscape bookmark HTML (any browser's export)

- **Format:**
  - the file starts `<!DOCTYPE NETSCAPE-Bookmark-file-1>`;
  - folders are `<DT><H3 ADD_DATE>` followed by a nested `<DL><p>`;
  - links are `<DT><A HREF ADD_DATE LAST_VISIT LAST_MODIFIED>`;
  - dates are seconds since 1970.
- **Safari** was not researched.
- Source: https://learn.microsoft.com/en-us/previous-versions/windows/internet-explorer/ie-developer/platform-apis/aa753582(v=vs.85)

## 3. Signing and SmartScreen

- **SmartScreen.**
  - EV certificates stopped bypassing SmartScreen in 2024, so EV- and
    OV-signed files now build reputation the same way.
  - Reputation comes from the publisher and the file hash. It can take
    "several weeks and hundreds of clean installs", and there is no way to
    submit for review.
  - Signing every release with the same identity carries reputation forward.
- **Smart App Control** on Windows 11 "may supersede SmartScreen", and blocks
  unsigned executables outright. (Locally built binaries ran fine on the
  owner's machine on 2026-10-09.)
- **Certificate rules (CA/Browser Forum):**
  - since June 2023, code-signing private keys must live in hardware;
  - certificates issued from 1 March 2026 last at most 460 days (in practice,
    CAs issue for 459).
- **Azure Artifact Signing** (formerly Trusted Signing):
  - about $9.99 a month for Basic (5,000 signatures);
  - its certificates last 72 hours, so signatures need a timestamp
    (`http://timestamp.acs.microsoft.com`);
  - individuals must be in the USA or Canada;
  - its CAs are scheduled to leave the Common CA Database, and it is not Store
    eligible.
- **OV certificates:**
  - Microsoft estimates $150–300 a year;
  - SSL.com charges $129 a year plus a $379 hardware token;
  - Sectigo starts at $536.25 a year on a five-year term (M).
- **Microsoft Store:**
  - individual developer registration is free (since September 2025);
  - EXE and MSI submissions must be silent, offline, hosted by you, and signed
    by a Trusted Root Program CA.
  - **Store policy 10.2.1 says browsers "must use either the Chromium or the
    Gecko open-source engine"**, at most two major versions behind. Whether a
    WebView2 shell qualifies is unconfirmed (L); a Servo-based browser would not.
- Sources:
  https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation,
  https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options,
  https://cabforum.org/2025/11/17/ballot-csc-31-maximum-validity-reduction,
  https://cabforum.org/working-groups/code-signing/requirements/,
  https://learn.microsoft.com/en-us/azure/artifact-signing/,
  https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart,
  https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/,
  https://learn.microsoft.com/en-us/windows/apps/publish/store-policies,
  https://www.ssl.com/certificates/code-signing/buy/,
  https://www.sectigo.com/ssl-certificates-tls/code-signing
