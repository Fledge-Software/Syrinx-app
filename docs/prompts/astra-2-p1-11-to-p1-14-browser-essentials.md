# Astra task 2: browser essentials, P1-11 to P1-14

Paste `docs/prompts/astra-common-rules.md` above this prompt.

## Why this matters

The owner wants Atlas usable as a daily browser before any plugin work. The
preview can browse, but it can't do things every browser does:

- it can't complete a Google or GitHub sign-in that opens a popup;
- it has no permission prompts;
- it has no find, downloads, print, context menu or zoom;
- it doesn't truthfully show whether a connection is secure.

These are Plan 1 slices P1-11 to P1-14. They run in order because each
builds on the previous one's page plumbing.

## Branches

Use one branch per slice, each started from the previous slice's tip:
`codex/p1-11-popups`, then `codex/p1-12-permissions`, then
`codex/p1-13-essentials`, then `codex/p1-14-security-display`. Do them in
order. Commit and gate each slice before starting the next. If one slice
blocks on an owner decision, stop there and report; don't skip ahead.

## Reading list (stay inside it)

- `docs/plans/Atlas v1 — Plan.md`: grep for `P1-11`, `P1-12`, `P1-13` and
  `P1-14`, and read those rows plus the Phase 2 section around them
  (`grep -n "Phase 2"`).
- `AGENTS.md`, section "Page web views and Google". **This is
  load-bearing.** No user agent, no IPC handler, no injected script, no
  request filter on page web views. Breaking it breaks Google sign-in.
- `docs/trapstoknow.md`, section "Page tabs: WebView2 through wry". In
  particular: a popup must use the opener's environment and must not have
  navigated, or `window.opener` breaks; and accelerator handling must only
  set `Handled` and post a message.
- `docs/research/08-gpui-and-wry-on-windows.md`, section 5 only.
- `docs/research/02-*.md` (the browser-scope note): the "Musts" list only.
- Source: `crates/atlas-platform/src/browser.rs`,
  `crates/atlas-platform/src/browser/page.rs` (line 80 currently denies
  every new-window request), `crates/atlas-app/src/session.rs` and the parts
  of `crates/atlas-app/src/ui.rs` that create and route pages (grep for
  `browser::`).
- `docs/page-close-event-review.md` and `docs/wry-failed-construction-patch-review.md`,
  summaries only. The vendored wry patches you must keep intact.

## Per-slice outcome

- **P1-11 popups.** Adopt `NewWindowRequested` targets as Atlas tabs through
  the WebView2 deferral, keeping the opener relationship: the same
  environment and profile, and a target that has not navigated. Prove it
  with a synthetic loopback fixture (two local pages, one calling
  `window.open` and then reading `window.opener` and posting back). There
  must be no real OAuth provider in tests. Model it on
  `crates/atlas-platform/examples/page_close.rs`. Remember the trap: an
  accepted socket must be set blocking.
- **P1-12 permissions.** Atlas-drawn permission prompts for the WebView2
  `PermissionRequested` event: allow, deny, remember for the site. Nothing is
  auto-granted, and clipboard auto-grant stays off. Decisions persist in
  `atlas-store` through a migration with tests. Notifications follow the
  plan row. Verify with disposable synthetic profiles only.
- **P1-13 essentials.** Find in page (Ctrl F), downloads (Ctrl J) with a
  minimal list, print (Ctrl P), the default context menu, and zoom (Ctrl +,
  Ctrl -, Ctrl 0) with the level visible. Route each shortcut through the
  existing accelerator path so it works while the page has focus. Add
  focused-page routing tests.
- **P1-14 security display.** Show the scheme and certificate state
  truthfully. HTTPS alone is never labelled "secure". Invalid certificates
  must never silently succeed; prove that with a local self-signed fixture.
  Check `ServerCertificateErrorDetected` handling, and make sure the default
  stays "cancel".

## Constraints

- Any new wry or WebView2 API you need that wry 0.57.0 doesn't expose: use
  `webview2-com` through `WebViewExtWindows::controller()`, the same way the
  accelerator handler does. **Do not add another vendor patch.** If one
  seems unavoidable, stop and write the case.
- If a slice needs a GPUI drawing change (prompts, the find bar, the
  downloads list), keep it inside new modules under `crates/atlas-app/src/ui/`.
  Touch `ui.rs` only to wire them in. Another agent is building the design
  tokens crate (P1-07), so use the existing colour constants for now.
- After any change the owner sees, the rule is to run the app and look at
  the window. If you can't inspect the window yourself, write the exact
  manual check for the owner in your report (the steps and what they should
  see) instead of claiming visual acceptance.

## Done means

Each slice has a committed branch with a green gate, a passing synthetic
native fixture where the slice needs one, its ledger row updated, and a list
of the owner's manual checks.
