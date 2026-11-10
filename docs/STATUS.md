# Syrinx status — 2026-11-10

Syrinx is **one app**: one repository, one Cargo workspace, one folder. The
same `syrinx-app` binary is built for every desktop. Platform differences sit
behind `syrinx-platform` (WebView2 on Windows, WebKitGTK on Linux, WKWebView
planned for macOS). No platform gets its own fork, branch line or folder.

Issues and PRs are the source of truth; this page is a snapshot.

## Finished (on `main`)

- **Core browser on Windows:** tabs and sidebar, palette, find, zoom per site,
  back/forward lists (#212, #213), settings tab, themes (Glass, Paper),
  downloads list, session saving and recovery, background-tab sleeping,
  clear-data journalling.
- **Linux, same app:** the shared app runs on Linux through WebKitGTK and
  XWayland (#226), with Settings stating its protection and streaming limits
  (#229), page-text limits documented (#228), repeatable performance
  measurement (#231), and native fixture plus palette acceptance checks (#247).
- **Search:** Tantivy tier-2 full-text index, wired into the app (#152, #186).
- **Page text:** owner setting, seven-day cache, erase-on-off (#198; Windows).
- **Plugin runtime 1a:** `syrinx-plugin-host`, frozen WIT 0.1.0, test host (#230).
- **Plans recorded:** product map A2, paid-services plan, included-features plan.

## On this branch (`syrinx/cross-platform-followups`)

- Chromium reference measured through XWayland, without changing browser
  isolation (replaces the stale draft PR #237, whose branch no longer merges
  into `main`).

## In progress (open draft PRs)

- **Plugin runtime 1b:** manifest, signed folder packages, registration (#236).
- **Signed weekly update files** (#119), which needs the owner.

## Not finished

- **One layout for platform backends** instead of `#[path]` includes (#239).
- **Linux follow-ups:** owner landing checks (#241), media prerequisites (#246),
  Find completions (#245), installer `XDG_DATA_HOME` (#244), non-Latin
  shortcuts (#243), creating the Downloads folder (#242), signed checksums (#238),
  socket-path error (#227), GDK frame-clock warnings (#225), landing guards
  (#224), update transport (#223), CI gate under Xvfb (#222, #221), page-text
  method decision (#219), measured idle CPU and memory (#218). Epic #217.
- **macOS:** WKWebView build and streaming/sign-in spike (#208, #205, #146). Not started.
- **Media engine / Widevine:** Chromium engine spike, licence audit, castLabs
  request (#207, #206, #204). Not started. Epic #211.
- **Plugin runtime 1c–4:** lifecycle, grants, code cache, plugin view, tabs,
  install flow, palette feeds, bundled plugins (#135–#142). Blocked on 1b.
- **Services:** accounts, Sync, AI, VPN, ad/tracker blocker (#174–#180). Blocked.
- **Notes folders** (#168) and Databases plugin (#167). Blocked.
- **Known bugs:** popup downloads missing from the list (#143), full-text index
  missing a `.pos` file (#240), backup restore and site exclusions (#199).
