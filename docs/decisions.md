# Owner decisions since the spec

Decisions the owner has made after `docs/spec.md` was written, newest first.
Each entry says what changed and why. The spec's "Decisions already made"
still stand unless an entry here says otherwise. Design decisions are
recorded separately, in `design/README.md`.

Entries keep the wording of their date: until the product was named Syrinx
on 2026-10-12, it was called Atlas, so older entries say Atlas.
Old paths map directly: `crates/atlas-*` is now `crates/syrinx-*`, and
`docs/plans/Atlas … .md` is now `docs/plans/Syrinx … .md`.

## 2026-10-15

- **Themes are built by the owner in an editor; only Graphite and Paper are
  presets (replaces "all five ship" in `design/README.md`).** The owner's
  window check that day passed Graphite and Paper. Tint, Haze and Glass
  did not match the design, and the owner judged that the design itself
  does not make transparency look good. The development-only theme lab
  showed what Windows can draw. The owner then chose an editor over
  picking new fixed palettes:
  - The default is Graphite with the orange accent, fully opaque.
  - Paper is the other preset.
  - Everything else is a custom theme: mode, background colour, solid or
    gradient, material (none, Mica, Mica Alt or Acrylic), transparency,
    glass edges, grain and accent.
  - Several themes can be saved by name.
  - Text is kept at 4.5:1 or better, pages and the command palette stay
    opaque, and Reduce transparency still turns materials off.
  - A previously chosen Haze, Glass or Tint becomes a saved custom theme of
    that name, so no look is lost.
  - The owner also approved merging the page-card fix first, with Graphite
    as the default.
  - See `docs/theme-editor.md`.

- **Plugins are built from a template repository, each in its own
  repository, as sandboxed WebAssembly loaded at run time; no plugin work
  starts until Plan 1 closes (changes D17 and D34).** The owner wants a
  plugin template made first, then Forge, Notes and Fledge each built from
  it, the way Obsidian's sample-plugin template and Zed's extensions work.
  Decided in chat:
  - *Repositories.* A template repository on GitHub (marked as a template)
    and one repository per plugin made from it, rather than the
    `crates/plugin-*` workspace crates the plan and `AGENTS.md` describe.
    The owner creates the repositories (operator work); agents prepare
    their contents. Rejected: a template folder inside this repository.
  - *Order.* After Plan 1 closes: the plugin interface and host first, then
    the template, then each plugin. Rejected: starting the interface and
    template now, ahead of Plan 1.
  - *Form.* Each plugin is Rust compiled to WebAssembly (WASM) and run by
    Syrinx in a sandbox (the Zed model). It reaches files, git, processes
    and the network only through host functions its manifest asks for and
    the owner grants (the Chrome extension model), and draws its interface
    in HTML and JavaScript inside Syrinx's own local web view, as Notes and
    Forge already planned for CodeMirror (D28). Rust state stays in the
    plugin's WASM module and Syrinx's core, keeping S3. Rejected: compiling
    built-ins into `syrinx.exe` (every plugin release would need a Syrinx
    release, which defeats separate repositories), Obsidian-style
    JavaScript plugins (no sandbox, state outside Rust) and native DLLs (no
    sandbox, and Rust has no stable binary interface).
  - *What changes.* D17's sandbox is no longer deferred until a third
    party's plugin exists: the built-ins use it from the start. D34's
    "`Plugin` trait" becomes a WASM interface definition (WIT) and host;
    plan 2 stays thin otherwise. The owner chose this after being told the
    cost: Forge needs the host to provide real capabilities
    (terminals, git, language servers, tree-sitter), as Zed's host does.
  - *Before building.* A research note compares the WASM runtime and
    interface choices (for example wasmtime and the component model,
    licences and maintenance, host-capability design, start-up and memory
    cost against D6) and is reviewed with the owner before Plan 2's slice
    breakdown. `AGENTS.md`'s layout and plugin guardrails are updated in
    that slice breakdown, not before.

- **Each platform's build uses that platform's own web engine: WebView2 on
  Windows, WKWebView on macOS. Syrinx bundles no Chromium.** The owner
  asked, in chat, for Disney+, Hulu, YouTube and Google's services to work
  without shipping Chromium, because a bundled Chromium (CEF, Electron) adds
  hundreds of megabytes and a new release every four to eight weeks
  (`docs/research/01-shell-and-engine.md`), and because the owner wants
  Syrinx's own settings, not Chrome's. Both engines come with the operating
  system, so Syrinx ships neither, and both play protected video through a
  DRM module the system supplies: WebView2 carries Google's Widevine (L3,
  observed in note 07, section 2.1, and Disney+ passed in Phase 0), and
  WKWebView carries Apple's FairPlay. Both are driven from Rust through wry,
  so the tab model, palette, store and plugins stay one code base; only
  `syrinx-platform`'s engine and window code is written once per platform.
  This does not change D2: v1 is still Windows 11 only, and the macOS build
  starts after v1, behind a macOS spike run on a Mac by the owner. ⚠️
  Unverified until that spike: that Disney+, Hulu, YouTube rentals and
  Google sign-in accept a WKWebView browser that is not Safari. Rejected:
  CEF on every platform (one engine, but bundled Chromium, Google's
  embedded-framework sign-in block names CEF, and Widevine needs Google's
  licence and signing); a Firefox (Gecko) fork (no embedding API, dropped on
  2026-10-09). A possible third build for Widevine where neither system
  engine provides it, chiefly Linux, is a proposal, not a decision:
  `docs/proposals/widevine-third-build.md`.

- **The usage switch's approved note is restored (AN-02).** With the report
  preview, Save report… and Copy report built, Customize's Usage report note
  again reads the approved sentence, "Nothing is sent anywhere. You can
  view, export or delete the counts at any time.", replacing the shortened
  note the owner chose on 2026-10-14 until AN-02 landed. This restores the
  approved proposal (`docs/proposals/an-01-02-local-analytics.md`); it is
  recorded here because the 2026-10-14 entry promised it.

- **Ctrl W on a focused sidebar row closes that row's live page (DW-63).**
  DW-61 took the rows' small buttons out of the Tab order, which left
  "Unload page" (closing a saved page's live tab while keeping its saved
  row) with no key: the owner had to press Enter to open the page, then
  Ctrl W. The owner chose, in chat, Ctrl W while a row has focus: it unloads
  a live saved page and closes an open tab, bound in the row's key context
  so the window's Ctrl W is unchanged whenever the page or another control,
  not a row, has focus. On a folder row or a saved page with no live tab it
  does nothing rather than fall through and close the active page of a
  different row (the agent's choice, approved in review). The rejected
  alternative was Backspace, which no row uses but which does not mean
  "close" anywhere else in Syrinx. See DW-63 in the plan's "Deferred work"
  register and `docs/p1-36-accessibility.md`, "DW-63".

## 2026-10-14

- **The weekly release workflow signs updates; the owner keeps no release
  key on a device (P1-39d, DW-30; replaces the 2026-10-13 key arrangement).**
  The 2026-10-13 approval had the owner create two offline keys, a primary on
  a hardware key and a backup stored separately. Walking through P1-39d, the
  owner declined to keep a key on any device, because a lost or destroyed
  device would strand every installed copy, and asked why updates do not
  simply trust GitHub, where the weekly builds already appear. Trusting
  GitHub alone was rejected: HTTPS already stops a network man-in-the-middle,
  but the signature exists to stop someone who breaks into the update source
  (the website, Vercel, the site-push token), and GitHub Releases of the
  private repository would need a token that can read the source (the
  reason recorded on 2026-10-13). Keeping the owner-held keys was the other
  alternative. The owner chose, in chat: the primary release key is held as
  an encrypted GitHub Actions secret and `.github/workflows/weekly-release.yml`
  signs each release and re-signs `latest` every week, which also removes
  the 60-day manual re-signing; the backup key is an encrypted file kept in
  the owner's password manager, not on a device. Both public keys are still
  pinned in `PRODUCTION_KEYS`. The accepted trade-off: whoever takes over
  the owner's GitHub account or the workflow can sign updates, while a
  break-in to the website or Vercel alone still cannot. The key must still be
  created outside every agent session, and no agent sees the private key or
  the secret's value. Building this is an agent slice that changes the
  release workflow; see DW-30 in the plan's "Deferred work" register.

- **Sidebar rows get their own right-click menu (DW-63 follow-up).** When
  choosing Ctrl W for DW-63 (recorded under 2026-10-15), the owner also
  asked for a per-row right-click menu holding Unload page and the row's
  other actions (Close tab, Remove saved page, Rename and Remove folder),
  each showing its key, so every row action is reachable by mouse as well
  as by keyboard. Today a right-click anywhere in the sidebar opens only
  the general ⋯ menu. See DW-63 in the plan's "Deferred work" register.

- **Syrinx ships as a setup program that installs and updates itself
  (P1-37).** Release 0.2.0 shipped a zip holding `syrinx.exe` and PowerShell
  scripts. Copied out of that zip on its own, the exe ran, but nothing was
  installed or registered. A downloaded zip's scripts are also refused by
  Windows unless unblocked first. The owner's requirement: what Syrinx ships
  must stand alone, set up the folders it needs where they belong on first
  run, and update the existing install when a newer one is run.
  - The owner chose `syrinx-setup.exe`, written in Rust, over two
    alternatives. An Inno Setup wizard would need the tool on the build
    runner and a second script language. A WiX MSI is the heaviest to
    author and needs a licence check.
  - The setup program carries the release package and installs it per user
    into `%LOCALAPPDATA%\Programs\Syrinx`. It upgrades in place with the same
    swap P1-39b's helper uses, keeping one previous version, and never
    touches `%LOCALAPPDATA%\Syrinx\data`.
  - Its checkbox "Register Syrinx as a browser" is ticked by default, as the
    owner chose. It writes only the per-user keys P1-37 already lists and
    never sets the default browser.
  - The weekly release publishes the setup program as the download, beside
    the zip.
  - Every executable now links the C runtime statically
    (`.cargo/config.toml`). 0.2.0's `syrinx.exe` imported `VCRUNTIME140.dll`,
    so Windows would refuse to start it on a machine without the Visual C++
    redistributable.
  - This replaces the "portable zip plus scripts" recommendation as the way
    people install Syrinx. The scripts remain the manual route. See
    `docs/p1-37-installer-design.md`, "The setup program".

- **The focus ring shows only after keyboard input, decided by Syrinx's own
  keyboard-or-pointer flag (DW-62).** P1-36 drew the ring on any focus, so
  a chrome button clicked with the mouse kept a ring until focus moved on.
  GPUI's built-in `focus_visible` cannot fix that: it judges keyboard use
  from the keys GPUI itself receives, so it never sees the F6 or Tab that
  leaves a web page through WebView2, and the ring would vanish exactly
  when a keyboard user leaves a page; it also counts a bare mouse move as
  pointer use. P1-36 recommended accepting the ring after clicks for v1,
  since that errs toward keyboard users; the other alternative was a
  vendored GPUI patch exposing a way to mark keyboard use, which would be a
  further change to the pinned toolkit. The owner chose, in chat, neither:
  Syrinx keeps its own flag, set to pointer by a mouse press in any Syrinx
  window and to keyboard by any key Syrinx's windows receive and by every
  key a focused page hands over (F6, Tab out of the page and the other
  shortcuts), and draws the ring only in keyboard mode. This needs no GPUI
  change and keeps the ring visible after F6 from a page. See
  `docs/p1-36-accessibility.md`, "DW-62".

- **GPUI's existing Windows network and remote-call features are approved,
  pinned to the crates that ask for them (DW-68; narrows D9's check, not
  D9).** D9 keeps typing on the machine, and P1-39c made WinHTTP in
  `syrinx-update-net` the one exception (2026-10-13, question 8).
  `cargo xtask check-deps` checked only what Syrinx's own crates declare, so
  a third-party crate could switch on another networking namespace and Cargo
  would unify it into the build every crate shares. DW-68 now reads the
  features Cargo actually resolves on every `windows`, `windows-sys` and
  `winapi` version, and found that GPUI's own stack already turns some on:
  - `Win32_Networking_WinSock` on the pinned `windows 0.62.2`, requested by
    `gpui-pre` and `gpui-pre-windows` 0.3.8;
  - on `windows 0.57.0`, `sysinfo 0.31.4`'s IpHelper, Ndis, NetManagement
    and WinSock features and `Win32_System_Rpc` (used by its WMI sensor
    reading), reached through GPUI's `zed-scap`;
  - on `windows-sys 0.61.2`, WinSock from mio, polling, quinn-udp, rustix,
    socket2, uds_windows and zbus, and IpHelper from zbus.

  The owner approved exactly those eight entries, each with its requesting
  crates and versions, in `THIRD_PARTY_NETWORK_FEATURES`
  (`crates/xtask/src/dependency_policy/resolved_windows.rs`); RPC was
  approved as the eighth after a review widened the namespaces checked.
  Any new requester, version, feature or namespace fails the check, and so
  does an entry that no longer matches or lists a crate that stopped asking,
  so the list cannot outlive its reason.

  What is enforced, exactly: because `windows 0.62.2` is the build
  `syrinx-platform` and `syrinx-app` also use, its WinSock bindings compile
  in those crates. The source scan in `crates/xtask/src/network_sources.rs`
  refuses `WinSock`, `NetworkManagement`, any `WSA`-prefixed name (as a
  token) and the `ws2_32`, `wsock32`, `mswsock` and `iphlpapi` libraries
  (in any letter case, with or without `.dll`) in every Syrinx crate except
  the xtask tooling, and WinHTTP in every crate but the update download.
  Manifests are checked twice. The declaration rule refuses a Syrinx crate
  that asks `windows` or `windows-sys` for a networking feature. A Syrinx
  crate asking `winapi` for a network module is refused by the
  resolved-feature check instead, because no approved entry names it.
  These are text and manifest checks, not a sandbox: a feature check cannot
  see a raw `#[link]` or `windows-link` extern at all, and the source scan
  sees one only when it names one of those libraries.

  `winapi 0.3.9` also resolves `inaddr` and `in6addr` (requested by
  `ntapi 0.4.3`). These modules define only the IPv4 and IPv6 address
  structs and declare no functions, like std's `Ipv4Addr`, so they are not
  on winapi's network list. The owner deferred this classification; it was
  the agent's call, recorded here so it can be revisited.

  Rejected: patching GPUI's vendored manifest to drop WinSock, a new vendor
  patch that would not reach `sysinfo` through `zed-scap` and risks GPUI's
  own socket use; and closing DW-68 as a known limit, which would leave a
  third-party crate free to add networking unseen.

- **AN-01 starts before P1-39d, an exception to the plan's order.** The
  approved analytics proposal put AN-01 after P1-39. P1-39d needs the
  owner's release keys and update host, which are not ready, while AN-01's
  local collector needs neither, so the owner told the session to start
  AN-01 now rather than wait. Only the order changes: AN-02 still precedes
  P1-40.
- **AN-01's frozen command list is approved as built.** The proposal made the
  owner's review of the 20 built-in command counters plus `command.other`
  (listed in `docs/an-01-local-analytics.md`) a done condition; the owner
  approved them unchanged. A new built-in command still needs a new schema
  version and the owner's approval.
- **The usage switch's note is shortened until AN-02.** The approved note
  says the counts can be viewed, exported or deleted, but viewing and
  exporting arrive only with AN-02. Until then Customize says "Nothing is
  sent anywhere. You can delete the counts at any time."; AN-02 restores
  the approved sentence when it adds the preview and export. Showing a
  promise the build cannot keep was the rejected alternative.

- **Where Windows cannot list sync roots, Syrinx treats the machine as
  having none (narrows D13).** D13 keeps the database out of OneDrive and
  similar synced folders; the storage guard
  (`crates/syrinx-platform/src/storage/native.rs`, `registered_roots`) asks
  Windows for its registered sync roots through
  `StorageProviderSyncRootManager::GetCurrentSyncRoots`. On the Windows
  Server machines GitHub uses to build releases, that call jumps to an
  invalid address and crashes the process (weekly-release run 4 captured
  the native stack), so the release gate could never pass there. The owner
  chose, over building releases on their own PC, to have the check cope
  without that API. The first attempt asked
  `StorageProviderSyncRootManager::IsSupported()`, Microsoft's documented
  check, first; run 5 showed that call crashes the same way. Syrinx
  therefore asks Windows whether it is a server edition (`IsOS(OS_ANYSERVER)`
  in shlwapi, which makes no WinRT call) and, if so, treats the machine as
  having no registered sync roots. On Windows 11, Syrinx's only supported
  system (D2), nothing changes. On Windows Server, Syrinx no longer refuses
  a synced folder; the rejected alternative was to fail closed there, which
  would keep every storage test failing on the release runner.

## 2026-10-13

- **P1-39 update delivery is approved as proposed.** The owner answered yes
  to all nine questions in `docs/proposals/p1-39-update-delivery.md`.
  Syrinx checks for updates only when the owner presses "Check for updates"
  in Settings or the palette, never at start-up or in the background;
  start-up checks, a periodic check, BITS background transfers and
  downloading through a WebView2 page were rejected, because each makes
  network calls the owner did not ask for or leaves a timeline of launches
  in the host's logs. Updates come from fixed HTTPS files under
  `https://syrinxapp.dev/updates/v1/`, compiled into the executable with no
  setting to redirect it; GitHub Releases of the private repository were
  rejected because Syrinx would have to ship a token that can also read the
  source. "Install update from file…" uses the same checks with no network.
  The owner is willing to publish signed Syrinx builds at that address
  before the licence is chosen, once P1-39b is done (the proposal's
  recommendation was "yes, once P1-39b is done; your call"). Syrinx accepts
  a release only through a signed latest-version statement that expires at
  most 60 days after it was issued, which the owner re-signs at each release
  and between releases, so a frozen host can hold Syrinx back for at most
  that window. An update is downloaded only after the owner confirms its
  version and size, and installed only through Restart now or the owner's
  next quit, with the previous version kept and restored if the new one does
  not start. After relaunching for an update, Syrinx does not force itself
  to the front. The owner creates two offline release keys, a primary on a
  hardware key and a backup stored separately, outside the repository and
  outside every agent session; both public keys are pinned (DW-30), and a
  separate statement-only key was deferred. `cargo xtask check-deps` changes
  only as question 8 lists: `syrinx-app` and `syrinx-update-net` may depend
  on the update verifier; `syrinx-update-net` may use the `windows` crate
  with its own exact feature list; and that crate alone may use
  `Win32_Networking_WinHttp`, the only exception to D9's no-network rule.
  `reqwest` with `rustls` was rejected to keep a TLS stack and `tokio` out
  of Syrinx's own crates. P1-39 is split into P1-39a to P1-39d as the
  proposal's slice table defines, with no other renumbering. The release
  keys remain the owner's to create, so production updates wait on DW-30.

- **Syrinx is on GitHub at `Fledge-Software/Syrinx-app`.** The owner created
  the repository under the Fledge Software organisation and added it as
  `origin`, and confirmed on 2026-10-13 that the remote is theirs. This
  replaces "local only, no remote and no pushes" in D41 and the 2026-10-12
  entry below. `main` still changes only through the reviewed `--no-ff`
  merge once `cargo xtask check` passes. This entry does not make the
  repository public, choose its licence or move outstanding work from the
  plan's registers into GitHub issues; each of those is still the owner's
  decision. An agent pushes only the branch its session was assigned, and
  pushes `main` or opens a pull request only when the owner asks; `CLAUDE.md`
  and `AGENTS.md` say the same.

- **The Syrinx logo is the hawk emblem in `design/brand/syrinx-logo.svg`.**
  It is used for the app icon, taskbar and Start entries, installer,
  favicon and website.
  - **Icons sit on a dark tile.** Every icon is the hawk on a near-black
    rounded square. The hawk's head is cream (#FFFDF7) and disappears on
    light backgrounds such as light taskbars, the Paper theme and light
    browser tabs. A transparent hawk is used only where the background is
    known to be dark.
  - **The default accent matches the logo's orange, #FF612E**, replacing
    #FF8A65. It stays customisable, and must still meet the 4.5:1 text
    contrast rule in all five palettes, with a per-palette adjustment only
    where needed.
  - **A saved #FF8A65 now means the default.** When settings load, a stored
    accent equal to the former default #FF8A65 is treated as the default,
    now #FF612E, so people who kept the old default follow the logo. Any
    other stored colour stays exactly as chosen.
  - **Haze and Glass lighten an accent only when it actually fails.** On
    those see-through palettes the accent's text-and-mark colour is lifted
    to a lighter tone of its own hue only if it fails 4.5:1 as text or 3:1
    as a mark there. An accent that already passes, such as the green
    (#6FD3A8) and gold (#E8C16A) presets, keeps its own colour.
  - **The executable's icon is compiled by the Windows SDK's `rc.exe`** from
    a small build script, with no new dependency. The `embed-resource`
    crate was the rejected alternative. The rasters and `.ico` are
    generated once from the SVG and committed. Building the project must
    not require a rasterizer.

## 2026-10-12

- **The product is named Syrinx.** "Atlas" was a working name. It collides
  with OpenAI's ChatGPT Atlas, a browser in the same category, and the name
  is crowded across developer tools. The syrinx is a bird's vocal organ,
  which fits beside Fledge. The owner holds the domain `syrinxapp.dev`.
  Code, crates, the data folder and Windows registration names still say
  Atlas until a dedicated rename slice changes them together. That slice
  must also handle the data folder (`%LOCALAPPDATA%\Atlas\data`) before the
  owner's first real install (P1-37). New work should not spread the old
  name further than it must.
- **Finished work merges to main; slices are built in local worktrees.**
  This replaces D41's routing through the integration branch
  `codex/integrated-browser-preview`, which left main at P1-04 while
  reviewed work piled up elsewhere. Each slice is built on its own branch
  in a local worktree. It merges to `main` with `--no-ff` once an
  independent review approves it and `cargo xtask check` passes after the
  merge. The owner directed all finished work to main on 2026-10-12. The
  repository stays local, with no remote and no pushes. Commits still
  happen only within this approved slice workflow.

- **Atlas never takes the foreground from another application.** When the
  owner has moved to another program, Atlas does not bring itself to the
  front: no `SetForegroundWindow`, no synthetic Alt press and no GPUI
  `activate_window`. A page that should take the keys while Atlas is behind
  waits, and takes them when the owner returns to Atlas; Ctrl K while
  Atlas is not in front leaves the palette closed rather than float a popup
  over the other program. The review of DW-90 found that the palette's Esc
  fallback called GPUI's `activate_window`, which forces the foreground with
  a synthetic Alt key. Keeping that fallback was rejected: focus returning a
  little sooner does not justify Atlas stealing the screen from whatever the
  owner is doing.

  There are two exceptions, both following the owner's own action. Launching
  Atlas brings it to the front (`cx.activate(true)` in
  `crates/syrinx-app/src/main.rs`). A link the owner clicks in another program
  while Atlas is their browser also brings it forward
  (`ui/incoming_links.rs`): the forwarding instance grants the foreground
  through `AllowSetForegroundWindow` (`crates/syrinx-platform/src/instance/pipe.rs`). Every
  other activation, including Find in page, F6 pane cycling and the bookmark
  import preview, activates natively only while Atlas is already in front.

- **Palette ranking is judged from key press to rows on screen (P1-33).**
  A keystroke's ranked rows must be presented within two display refresh
  intervals at the 95th percentile (33.3 ms at 60 Hz), with the maximum
  always reported. D48's 5 ms limit still applies to the search itself and
  stays with P1-05. Two intervals is the smallest budget that allows for the
  frame wait: a reply that lands just after a frame must wait for the next.
  Judging only the search core was rejected because it hides the frame wait
  and the reply's trip back to the window.

- **"Opens within one frame" is judged at the 95th percentile (P1-33).**
  The palette's first presented frame must follow Ctrl K within one refresh
  interval at p95, with every sample and the maximum reported. This follows
  D48: a single Windows scheduling stall should not decide the verdict, and
  hiding the slow samples would conceal real responsiveness. Requiring every
  opening within one frame was rejected for that reason.

- **The palette popup is created once and then shown and hidden (DW-90).**
  P1-33 measured about half a second from Ctrl K to the palette's first
  frame. The cause is provisionally attributed to creating a new native
  window for every opening; the owner's foreground runs will confirm it. The
  owner approved keeping one owned, non-topmost popup hidden between uses.
  It is a separate slice after P1-33 merges, rerun through P1-33's harness.
  Keeping a new window per opening was rejected, because it cannot meet the
  one-frame target.

- **Page text skips every exit from an excluded site unless Atlas started it
  (P1-25, "skip all exits").** A page reached by leaving an excluded document
  (Google, a bank or payment site, or a site the owner excluded) is never
  read, unless Atlas itself started that navigation from the address bar,
  the palette, a bookmark, a saved page, a tile or Home. Links, page
  scripts, auto-submitting forms, popups, back, forward and reload all count
  as leaving. The taint carries on: a page reached that way passes it to the
  next page it leads to, until Atlas navigates the tab, so a sign-in
  callback that moves on by script is not read either. A popup opened from
  an excluded or tainted page starts tainted. This replaces the
  user-initiated test in the entry below: WebView2's `IsUserInitiated`
  follows Chromium's short-lived user activation, so a "Continue" click on a
  bank page counted as the owner's own navigation and its next page was
  read. Keeping the user-initiated test was rejected for that reason. The
  form-`POST` rule below stands.

- **Page text skips sign-in hops (P1-25).** *(Its user-initiated test was
  replaced the same day by "skip all exits" above; the `POST` rule stands.)* A page reached by a navigation
  that an excluded site (Google, a bank or payment site, or a site the owner
  excluded) started by itself, through its own script or an auto-submitting
  form, is not read. Neither is any page produced by a form submission (a
  `POST`) on any site, including one that redirects to an ordinary page
  afterwards. A link the owner follows out of an excluded site still leads to
  a readable page if that page is otherwise eligible. Sign-in flows carry
  tokens and personal details from the excluded site to the next page this
  way, which the redirect rule alone did not catch. Atlas tells these apart
  from WebView2's own `IsUserInitiated` flag and the request method it
  reports, with no request filter and no injected script. Reading such pages
  and relying only on the exclusion lists was rejected.

- **A split `no-store` header is an accepted limit (P1-25, F2).** When a site
  sends `Cache-Control` as several separate header lines, WebView2 shows
  Atlas only the first, so a `no-store` on a later line is missed and the
  page can be read. The owner accepted this as a known limit rather than
  adding a request filter, which the Google guardrails forbid. A report to
  Microsoft is possible but is sent only with the owner's approval; nothing
  has been sent.

- **Page text uses the browser's `visibility` style (P1-25).** The snapshot
  asks WebView2 for each node's computed `visibility`, so text a page hides
  with `visibility: hidden` is left out, as it is for `display: none`. This
  departs from the proposal's empty style list; the owner approved it after
  the F1 test showed hidden text being kept.

- **A background tab finishes loading before it is put to sleep (P1-15).**
  The five-second countdown before a hidden page is suspended now starts
  when its navigation completes, not when it starts, so a slow page opened in
  the background is fully loaded when the owner switches to it. Suspending
  mid-load froze such pages half-loaded and was found as the cause of an
  intermittent native-test timeout. Keeping the earlier timing, which saves
  a little more memory, was rejected. A page that never finishes still
  sleeps after a bounded wait.

- **History is kept for 90 days (DW-4).** Older visits are deleted
  automatically through the same path as Clear browsing data, so they stay
  deleted after a backup is restored (D47). Ninety days matches Chrome: long
  enough for the palette to recall a page, short enough to keep the database
  small. The owner rejected keeping history forever, a year and 30 days.

- **The installed Atlas keeps its data in `%LOCALAPPDATA%\Atlas\data` (P1-37).**
  Development and preview builds keep using `AtlasProductPreview`, so test
  runs never mix with the owner's real browsing data. Nothing is moved or
  deleted between them; the installed app starts empty and bookmarks are
  imported again. Keeping the preview folder was rejected for that reason.

- **Page text is read with one DevTools snapshot, never a script (P1-25).**
  The owner approved `docs/proposals/p1-25-page-text-extraction.md` as
  recommended. Atlas captures an eligible page's visible text with a single
  `DOMSnapshot.captureSnapshot` call and reads only response headers, so
  nothing runs inside any page. Google's own sites (YouTube included), banks
  and payment sites, pages with password or card fields, pages not proven
  cacheable and redirect chains through an excluded site are never read. If
  testing shows a site can detect the capture, the work stops and returns to
  the owner. This keeps D35's no-injection rule absolute.

- **Local usage counting is approved, off until switched on (AN-01/02).**
  The owner approved `docs/proposals/an-01-02-local-analytics.md` as
  recommended: the fixed version 1 schema, turning it off deletes the counts,
  30-day periods with two kept, never backed up or restored, and export only
  as a JSON file the owner saves or copies. AN-01 follows P1-39 and AN-02
  precedes P1-40. This implements D39 without uploads or identifiers.

- **Pages get find, history, print and zoom keys first (P1-13).** When a web
  page has focus, Ctrl F, Ctrl H, Ctrl P and the zoom keys go to the page
  first, as in Chrome. If the page doesn't use them, the engine's own find,
  print and zoom apply. Atlas's own find and history stay reachable from
  Ctrl K and the toolbar. Grabbing them first broke Google Docs' in-document
  find and its Ctrl H find and replace. Ctrl T, W, L, K and Tab stay Atlas's.

- **Smart App Control is off for development.** Windows Application Control
  repeatedly blocked locally built, unsigned executables (os error 4551),
  breaking gate runs at random. The owner turned Smart App Control off on
  their machine. This was the owner's own action; agents still never change
  Windows security settings. Atlas stays unsigned for now (D38), and a fresh
  Windows install may still block it.

- **The five palettes are approved (P1-07).** The owner reviewed the
  `--palette-sample` window against the design and approved it. The D44
  drag thresholds stay proposed until they can be tried in a draggable
  sidebar, because a static sample can't show how a drag feels.

- **Folder removal keeps saved pages by default (P1-21).** Removing a folder
  moves its contents up one level unless the owner explicitly chooses to
  delete them, and deleting contents asks for confirmation first. That way,
  tidying folders can never silently lose saved pages. Empty folder names
  are refused.
- **Bookmark import defaults (P1-22).** Exported toolbar folders import as
  ordinary folders, not as Favorites (D45 Favorites stay a deliberate
  choice). Only UTF-8 files are accepted; anything else is refused with a
  request to re-export, rather than guessing the encoding. The parser's
  default limits (32 MiB, 50,000 entries, 32 levels of nesting) stand.
- **`atlas-core` may depend on `url` (DW-1).** So that import and navigation
  share one address rule instead of two copies, the core crate may take the
  `url` crate as its first outside dependency when the import step is built.
- **The recorded contrast shortfalls are accepted for now (P1-07).** Sixteen
  colour pairs in the canvas palettes miss WCAG minimums, mostly muted text
  and worst in Glass. The owner accepted them for now rather than changing
  the canvas. `atlas-ui`'s test keeps the list fixed, so any new shortfall
  fails the gate.

## 2026-10-11

- **Plan 1 slices may run in parallel.** To speed up delivery, the owner
  approved working independent Plan 1 slices at the same time, each on its
  own branch with independent review. Plans 2–4 still wait for Plan 1's
  acceptance.
- **Deferred-work markers are allowed when tracked.** `TODO`, `FIXME` and
  `HACK` may appear in Atlas code if they carry a `DW-n` tag listed in the
  plan's "Deferred work" register; each becomes an issue once on GitHub.
  `cargo xtask check` enforces this.
- **Two dependency approvals.** Vendoring `gpui-pre-windows 0.3.8` for the
  D37 idle patch, with its unchanged upstream TODO comments, and adding
  Tantivy for the full-text bake-off. `AGENTS.md` records both, with their
  limits.

## 2026-10-10

- **Build the integrated browser next (D49).** After finding that the
  morning's product libraries had no browser window, the owner explicitly
  approved bringing integration forward: connect the existing tab model,
  saved-page storage, backups and search to GPUI vertical tabs, WebView2
  navigation and Ctrl K. This replaces strict slice order for this preview,
  without dropping remaining features or performance requirements. The
  owner still approves commits and merges. The target is daily use of the
  full planned browser before Monday, 2026-10-12; that date is a goal, not a
  claim that incomplete or untested features are ready.
- **Local Atlas must run unsigned for now (D38 clarification).** The owner
  has no signing certificate and explicitly requires development to continue
  without one. Signing remains deferred; it is not an application prerequisite.
  Keep Windows security settings unchanged and verify the actual application
  executable. A successful unsigned launch does not guarantee that Windows
  accepts every rebuilt application or development tool.
- **Search's 5 ms target uses the 95th percentile (D48).** The owner chose
  requiring 95% of repeated searches to finish within 5 ms, with the worst
  sample also reported. This accounts for occasional Windows scheduling
  interruptions without hiding them. Measure the complete 100,000-row
  ranking, scope-count and highlight path, not only the underlying matcher.
- **History clearing survives backup restoration (D47).** To keep deleted
  history from returning while preserving older recovery points, the owner
  chose a local deletion record containing only cleared date ranges and data
  categories, never URLs or page contents. Restoration must honor that
  record. This replaces the alternative of discarding all older backups
  whenever history is cleared.
- **Restoring a damaged database requires confirmation (D46).** Because an
  older backup can lose newer state, Atlas offers the newest verified backup
  with its date and asks the owner before replacing the current database.
  Damaged files remain preserved locally. The owner chose confirmation rather
  than automatic restoration.
- **The top square tiles are Favorites (D45).** They provide quick access
  to websites, plugin pages and other supported Atlas destinations, separate
  from inline saved pages and folders. A plugin Favorite uses the normal
  plugin open/focus lifecycle. Before that plugin exists, its tile keeps the
  not-yet-implemented indication rather than pretending to open it.
- **The sidebar can be resized by dragging (D44).** To give the page more
  room without losing navigation, dragging smoothly changes the sidebar
  from wide with labels, to favicon-only, to hidden; dragging back restores
  it. The existing hidden-sidebar hover peek and Ctrl Alt S remain. The
  owner's screenshot supplies direction, not a new image asset in the
  repository. Exact width thresholds and compact-folder controls will be
  reviewed with the later UI slice.
- **Pin means Save, with saved pages and folders inline (D43).** To keep
  useful destinations beside ordinary tabs, saving creates a persistent
  sidebar row. Closing it unloads only its live page; opening an unloaded
  row uses its saved address, while opening an already-live row focuses it.
  Live navigation does not rewrite the saved title or address. Remove or
  Unpin removes the saved row and leaves an open page as an ordinary tab.
  Saved rows precede ordinary tabs; saving appends to the saved group and
  removing an open saved page puts its tab at the start of ordinary tabs.
  HTML bookmark import preserves folder hierarchy in this same model and
  leaves imported pages unloaded. This replaces D42's temporary pin flag,
  while preserving its web-tab close/history rules. The owner approved
  the revised 40-slice map: flat saved-page lifecycle now, folder management
  and import in their existing later slices.
- **Tab transitions follow visible order and keep Atlas open (D42,
  clarified by D43).** Closing the active page selects the next open page
  to its right, otherwise its left, skipping unloaded saved rows. Closing
  the last live page shows Home and leaves Atlas open. New ordinary web tabs
  append and become active. Reordering stays within saved or ordinary groups.
  Keep the 20 most recent web-tab closures, including saved-page instances,
  not 100. Reopen selects the restored ordinary tab with its metadata and
  position, without live page or form contents. For a saved-page closure,
  reopen focuses its existing live instance or opens its fixed saved target
  if the row still exists. If the row was removed, history may reopen the
  page as ordinary but never recreates the saved record. Plugins reopen from
  their permanent rows and do not enter this history.
- **Keep the current WebView2 engine and SmartScreen.** The owner confirmed
  the engine works after reviewing Microsoft's required runtime diagnostics
  and SmartScreen collection. Disclose those runtime behaviors separately
  from Atlas's own analytics; the custom-crash setting still keeps crash
  dumps local (D39).
- **Atlas may measure its own use locally to guide improvements.** This
  replaces the earlier blanket "no telemetry" rule. The owner chose local
  aggregate counters and explicit report export: feature usage, command
  counts, timings and error categories, without account/device identifiers,
  URLs, searches, file paths, document text or a detailed event timeline.
  There are no automatic uploads. A model may suggest changes from a report
  the owner chooses to share; the owner reviews product changes, and Atlas
  does not adapt its interface automatically. With one v1 user, removing
  identifiers cannot guarantee that a report's author is unknown by context.
  Collection and export need a reviewed schema and implementation breakdown
  before they are built; this is not permission to read or share real usage
  data during development (D39).
- **Seven days applies to retained page text, not just search visibility.**
  Keep the expiring page-text cache and its index outside Atlas's seven daily
  durable-state backups. Remove expired content from active storage and
  prevent restart, replay or restore from reintroducing it. This covers
  Atlas-managed retention, not forensic erasure or owner-managed backups
  (D40, clarifying D13/D18/D35).
- **The four plans and plan 1's 40-slice breakdown are approved.** The owner
  also approved local implementation: one branch per slice, independent
  review and the owner's approval before every merge. This supersedes D16's
  GitHub-before-implementation gate. The repository remains local with no
  remote or pushes; commits still require the owner's request. Preserve the
  planning baseline with an explicitly requested documentation commit before
  scaffolding. Implementation remains in phase order (D41).

## 2026-10-09

These entries depart from `docs/spec.md`. Each is argued in
`docs/plans/Syrinx v1 — Plan.md`, decision number in brackets.

- **v1 runs on Windows 11 only** (D2). No macOS or Linux work or testing in
  v1, so the Phase 0 verdict is for Windows, not per OS.
- **Pages render in WebView2, driven from Rust through wry, without Tauri**
  (D3). Servo is added later as a per-tab engine.
- **Atlas's interface is drawn in Rust with GPUI, not Svelte** (D4), unless
  Phase 0's comparison with a Tauri + Svelte build trips the rule written
  in the plan.
- **Notes and Forge edit in CodeMirror 6 inside a local web view** (D28).
  CodeMirror owns the live buffer and undo while typing; Rust owns disk,
  saves, parsing, git and the language-server client.
- **Forge writes through the git command line, not libgit2** (D29), with
  fast reads in process. It requires Git for Windows.
- **Phase 0 runs locally before anything else is planned in detail** (D16).
- **Import is the browser's bookmarks export, not a CSV.** Edge, Chrome and
  Firefox export bookmarks only as the standard HTML bookmarks file, so
  Atlas imports that file. v1 imports no history and no passwords; password
  import arrives with a password manager after v1. This replaces "Import is a
  CSV file only" below. Recorded in `docs/plans/Syrinx v1 — Plan.md` (D10,
  D14, D15).
- **Performance is the deciding goal for the engine and UI.** The owner wants
  Atlas to be as efficient as possible: low memory per tab, fast start, low
  typing and scrolling latency, low idle CPU. TypeScript and Svelte are
  allowed. Atlas does not have to be written entirely in Rust, but all state
  and logic still live in Rust, as the spec says.
- **The browser engine is open again.** The spec's direction (Tauri 2 with OS
  web views) is one option, alongside Servo, Chromium through CEF, and
  hybrids. The research in `docs/research/` compares them. The choice is the
  first decision of the v1 plan and is made with the owner, not by default.
  The rule "embed an existing engine, don't build one" still holds.
- **Import is a CSV file only for v1.** Atlas doesn't read other browsers'
  profiles. What the CSV holds (bookmarks, history or both) is still to be
  confirmed. *(Replaced the same day: see the bookmarks-export entry above.)*
- **Signing, SmartScreen and the Microsoft Store wait.** They come back only if
  Atlas is productised. The Store's rule that browsers use the Chromium or
  Gecko engine does not constrain the engine choice now.
- **Atlas will probably be open source, released on GitHub.** That brings a
  public repository and a licence choice when the repository moves to GitHub.
  It stays local until the owner says otherwise.
- **The next session asks the owner questions.** Planning v1 is a
  conversation: the engine, the UI approach, the v1 scope and anything else
  the owner hasn't decided are put to the owner in question rounds, never
  settled by picking a default.
- **A hidden sidebar peeks over the page on hover, everywhere in Atlas.** See
  `design/README.md`.
- **A plugin is a tab that stays in the Plugins list when closed.** See
  `design/README.md`.
