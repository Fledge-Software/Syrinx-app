# Palette focus ordering review, 2026-10-10

This review separates a source-level page-focus ordering hazard from the
earlier unverified report that the palette does not accept typing. Losing keyboard
focus after dismissing or submitting the palette would make the browser
unreliable even when a caret or a correct page remains visible. No focus fix,
dependency change or new platform API was implemented by this review.

## Current manual outcome — 2026-10-11

The owner reports a successful focused-page cycle on the live synthetic build
with SHA-256 `0fe3b8daad72ddf3383f025756a5174b39f098d2fa7355d81bfd45eaec89c858`.
They opened `http://127.0.0.1:64751` through Ctrl K and Enter, clicked the local
fixture's Typing area and typed `before`. From the page they pressed Ctrl K,
typed `about:blank` in the palette, pressed Escape, then typed `after` without
clicking the page. The owner confirmed “Yes, beforeafter appeared”.

This is an owner-reported pass for page typing, page-focused Ctrl K, palette
typing and Escape focus return across one cycle. The earlier manual check also
confirmed `about:blank` text entry, a stable Go to row after two seconds and
Enter closing the palette. These observations supersede the earlier pending
typing/Escape checks below; they do not prove repeated reliability or Enter
focus return in other destinations. The source ordering concern remains, but
was not reproduced here. No focus fix was applied, the page-host prototype
remains isolated/unpromoted, and the original WebView2 breakpoint is unresolved.

## What the current app does

`crates/atlas-app/src/ui.rs:581` retains the popup's window handle and view
together. Reopening an existing popup requests native activation and focuses
its existing input, preserving the query and selection. A stale window handle
clears the retained state and reports an error. Fresh popup creation at line
623 requests focus; its deferred callback at line 651 activates the popup and
focuses the input after construction. Terminal event handling at lines 732,
737 and 749 clears the popup, and the receiver's entity-identity check at
line 691 prevents an old popup's events from acting on its replacement.

`crates/atlas-app/src/palette.rs:136` focuses the input through its existing
state. The capture handlers at lines 279 and 284 route the focused editor's
Up and Down actions to palette selection. The pinned input's ordinary Escape
handler propagates when it has no transient editing surface to dismiss, and
its default `clean_on_escape` is false. Its `secondary-enter` binding means
Ctrl+Enter on Windows, so it does not conflict with Atlas's Alt+Enter binding.
These source checks found no separate first-open typing defect.

## The ordering hazard

`crates/atlas-app/src/ui.rs:757` calls `window.activate_window()` and then
immediately sends `BrowserCommand::Focus` for the shown page. The page command
runs on a separate native worker. Its handler in
`crates/atlas-platform/src/browser/page.rs:126` calls wry's `focus`, which
calls WebView2's `MoveFocus` in `vendor/wry-0.57.0/src/webview2/mod.rs:1622`.

GPUI's activation call only schedules foreground work. Its Windows task later
calls `SetActiveWindow`, `SetFocus` on the main window, and
`SetForegroundWindow`. There is no acknowledgement between that task and the
page worker's focus command. Consequently, the main window's later native
focus call can follow the page's focus call. This is a source-confirmed lack
of ordering, not evidence that this sequence caused an observed GUI failure.

## Pinned source references

The registry files below were read under
`C:\Users\imthe\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\`.
Paths and lines refer to the installed pinned sources at this checkpoint.

| Source below that directory | Relevant behavior |
| --- | --- |
| `gpui-pre-0.3.8/src/window.rs:6487` | `activate_window` forwards to the platform window. |
| `gpui-pre-windows-0.3.8/src/window.rs:814` | Activation schedules a foreground task; native activation/focus calls are at lines 828, 829 and 863. The task has no Rust await. |
| `gpui-pre-windows-0.3.8/src/events.rs:803` | `WM_ACTIVATE` schedules the status callback at lines 840–847 rather than delivering a request-specific completion. |
| `gpui-pre-0.3.8/src/window.rs:1940` | The status callback updates the active flag and invokes activation observers at lines 1945–1951. |
| `gpui-pre-0.3.8/src/app/context.rs:420` | `observe_window_activation` subscribes to activation/deactivation without an activation request identity. |
| `gpui-pre-0.3.8/src/executor.rs:352` | Foreground spawning enqueues work on the main thread. |
| `gpui-pre-0.3.8/src/platform_scheduler.rs:109` | Foreground work uses the default-priority main-thread dispatcher. |
| `gpui-pre-windows-0.3.8/src/dispatcher.rs:118` | Dispatch enqueues work and posts its wake message. |
| `gpui-pre-0.3.8/src/queue.rs:75` | Per-priority queues use `push_back`; receiver paths use `pop_front` at lines 289–305 and 362–378. |
| `gpui-pre-windows-0.3.8/src/platform.rs:1247` | The ordinary foreground loop runs queued tasks individually. |
| `gpui-pre-windows-0.3.8/src/events.rs:320` | The native size/move timer also drains foreground tasks, allowing dispatch during modal processing. |
| `gpui-pre-0.3.8/src/window.rs:2547` and `src/app.rs:2208` | `defer` queues an app effect, processed by `flush_effects` at `src/app.rs:1877`. It is not native activation completion. |
| `gpui-base-0.7.1/src/input/base/state.rs:750` and `:2079` | Escape does not clear this input by default; ordinary handling propagates at line 2116. |
| `gpui-pre-0.3.8/src/platform/keystroke.rs:143` | The `secondary` modifier maps to Control on Windows. |

## Why the small proposed fixes were not applied

A pending flag consumed by an activation observer is insufficient. An older
owner-activation notification, for example from popup closure, can already
be queued when the app requests another activation. That older notification
can consume the flag and send page focus before the new activation task
sets focus on the main window. The observer cannot identify which request
produced a notification.

Enqueuing another foreground task after activation gives the desired order
in ordinary FIFO dispatch. However, the source also permits foreground task
dispatch during native modal processing. Queue placement alone is not a
completion token that proves the earlier native call has returned under
reentrancy. This review did not establish such a guarantee and therefore did
not substitute that assumption for the existing race.

An app `defer`, a next-frame callback or an arbitrary delay does not provide
the missing acknowledgement either. None was added. Independent source
review agreed with this bounded conclusion.

## Earlier verification plan and the possible next change

The earlier Computer Use attempt targeted the main window and activated it
when typing; the owned popup was not separately enumerated. A visible caret
without entered text does not establish native keyboard ownership. A later
direct-launch capture showed the Windows lock screen, so the intervening
black captures and activation failures do not establish an Atlas rendering
or focus defect. Computer Use input stopped pending the owner's unlock at that
checkpoint; the October 11 manual outcome above supersedes that pending state.

The earlier plan was to verify fresh popup typing, clicking the main window then
Ctrl+K with the query preserved, Escape followed by page keyboard input, and
Enter followed by keyboard input in the selected destination. Use an actually
returned popup window or the owner's keyboard; do not invent a window target
or alter window ownership to accommodate automation. This review ran no app,
UI automation, build or tests.

If the restoration race is reproduced, a synchronous activation operation
with an explicit completion/error result in `atlas-platform` is a candidate
design. Page focus could then be sent after the main activation operation
returns, while checking that the request remains current, no replacement
popup is open and shutdown has not started. The existing Windows bindings
live in that crate; `atlas-app` has no direct Windows dependency. Such a
cross-crate platform API needs a separate proposal and the required approval
before implementation. This document neither requests that approval nor
changes a settled decision. Further bounded checks should cover repeated
focus return, existing-popup reactivation after clicking the main window and
Enter followed by keyboard input in the selected destination; the one passing
Escape cycle does not establish those outcomes.
