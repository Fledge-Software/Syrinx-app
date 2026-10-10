# Owner decisions since the spec

Decisions the owner has made after `docs/spec.md` was written, newest first.
Each entry says what changed and why. The spec's "Decisions already made"
still stand unless an entry here says otherwise. Design decisions are
recorded separately, in `design/README.md`.

## 2026-10-10

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
`docs/plans/Atlas v1 — Plan.md`, decision number in brackets.

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
  CSV file only" below. Recorded in `docs/plans/Atlas v1 — Plan.md` (D10,
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
