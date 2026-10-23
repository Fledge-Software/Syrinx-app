# wry's web view drop and the parent window's thread (issue #83)

Status: proposal for the owner, 2026-10-21. Nothing in `vendor/wry-0.57.0` has
changed. Any change there needs the owner's explicit approval under the
vendor-patch rules in `AGENTS.md`, and so does sending anything upstream.

## Why this was looked at

While diagnosing the WebView2 breakpoint crash (#59, draft #70), we noticed
that wry's `Drop for InnerWebView` calls `RemoveWindowSubclass` on the
*parent* window from whatever thread drops the web view. Windows' subclassing
helpers are documented as single-threaded. In Syrinx the parent is the GPUI
main window, owned by the GUI thread, while every page web view lives on the
`syrinx-pages` browser worker. So on paper every tab close makes a
documented-unsupported call into another thread's window. The worry in #83
was that the removal could fail silently and leave a subclass procedure
holding a pointer to a freed WebView2 controller, which would crash the next
time the GPUI window received a resize or focus message.

This review checks whether that can happen in Syrinx today, what actually
happens instead, and what to do about it.

## Short answer

The freed-controller hazard cannot occur in Syrinx. wry only installs the
parent subclass for a web view that fills its parent window. Syrinx builds
every page with `build_as_child`, which never installs it. The drop still
makes two cross-thread calls into the GPUI window, but both are harmless: a
`SendMessageW` that GPUI passes to `DefWindowProcW`, and a
`RemoveWindowSubclass` for a subclass that does not exist, which returns
`FALSE` and changes nothing. The recommendation is **no local vendor patch**,
and, with the owner's approval, an upstream report to wry carrying the small
diff below. No Syrinx code change is needed.

## What the source establishes

### wry's side (pinned 0.57.0 with Syrinx's approved patches)

All references are to `vendor/wry-0.57.0/src/webview2/mod.rs`.

- **The subclass is installed only for non-child web views.** Construction
  calls `attach_parent_subclass(parent, ...)` inside `if !is_child`
  (lines 187–190). The subclass's reference data is a boxed clone of the
  controller (lines 1381–1388).
- **The drop detaches unconditionally.** `Drop for InnerWebView`
  (lines 74–82) closes the controller, destroys the container window if
  `is_child`, and then calls `dettach_parent_subclass(parent)` (line 80)
  whether or not a subclass was ever attached.
- **Detaching does the work twice.** `dettach_parent_subclass`
  (lines 1391–1398) first `SendMessageW`s `PARENT_DESTROY_MESSAGE`
  (`WM_USER + 0x65`, line 44) to the parent. If the subclass is present, its
  procedure handles that message on the parent's own thread: it removes
  itself and frees the boxed controller (lines 1365–1372). The function then
  calls `RemoveWindowSubclass` again directly (line 1393), from the dropping
  thread. That second call is the cross-thread one, and it is redundant: by
  the time `SendMessageW` returns, the owner thread has already removed the
  subclass.
- **`reparent` uses the same pair** (lines 1817–1822), but only inside
  `if !self.is_child`, so it matches the attach.
- The two other subclasses wry installs, the main-thread dispatcher
  (lines 886 and 1138, via 1300–1307) and the failed-construction observer
  in `construction.rs` (lines 85–130), are on the container window, which
  the same thread creates and destroys. They are not affected.

### Syrinx's side

- **Every page is a child web view.** `Page::new` builds with
  `.build_as_child(&parent)` at `crates/syrinx-platform/src/browser/page.rs:114`.
  It is the only `WebViewBuilder` in the workspace; adopted popups go through
  the same `Page::new` (`crates/syrinx-platform/src/browser.rs:763`). So
  `is_child` is always true and the parent subclass is never attached.
- **The parent is the GPUI main window.** The UI passes its own window handle
  to `BrowserHost::spawn` (`crates/syrinx-app/src/ui.rs:554`); that window
  belongs to the GUI thread.
- **Pages are created and dropped only on the browser worker.** The worker is
  the `syrinx-pages` thread spawned at `crates/syrinx-platform/src/browser.rs:436–438`.
  All pages live in `BrowserState::pages` (line 731), a local of `run`
  (line 610). They are dropped there on `BrowserCommand::Close` (line 803),
  on a renderer crash (`browser/recovery.rs:127–133`), on browser-process
  recovery (`recovery.rs:234`) and when `run` returns. The compiler enforces
  this: `Page` holds `Rc<RefCell<…>>` (`page.rs:60`) and wry's `InnerWebView`
  holds `RefCell<HWND>` and COM pointers, so neither is `Send` and neither
  can be moved to the GUI thread.

So yes, every drop happens off the parent window's thread, as #83 said. What
that drop does to the parent window is the next question.

## What Microsoft documents

- `SetWindowSubclass` and `RemoveWindowSubclass` both carry the warning
  "You cannot use the subclassing helper functions to subclass a window
  across threads." Both return `FALSE` on failure and give no other error.
  ([SetWindowSubclass](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-setwindowsubclass),
  [RemoveWindowSubclass](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-removewindowsubclass))
- For `SendMessageW` to a window created by another thread, the system runs
  the window procedure on that thread, which happens only when it executes
  message-retrieval code. The sender is blocked until then, but it keeps
  processing nonqueued messages sent to it while it waits.
  ([SendMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagew))

## What a synthetic test showed

`wry-drop-thread-evidence/subclass_probe.ps1` creates a message-only `STATIC`
window on a helper thread, subclasses it there, and then calls the helpers
from the main thread. It touches no Syrinx window or profile; `Add-Type`
does write its compiled helper assembly to `%TEMP%`. Run on this
machine (Windows 11 10.0.26300, comctl32 5.82.26100.8328 from WinSxS), twice, with identical results:

| Call from the other thread | Result |
|---|---|
| `SetWindowSubclass` | `FALSE`: refused |
| `RemoveWindowSubclass` of a subclass that exists | `TRUE`: removed, and the procedure stopped receiving messages |
| `RemoveWindowSubclass` of an id that does not exist | `FALSE`, no effect |

The probe's raw output, the same on both runs:

```
owner-thread set=True; hits before=1; cross-thread set=False; cross-thread remove=True; hits after=1 (subclass gone); cross-thread remove of absent id=False
```

So on this machine a cross-thread *removal* does not fail silently, as #83
supposed; it succeeds. That does not make it safe. It is still unsupported,
and it edits the window's subclass list with no synchronisation against the
owning thread, which may be running that list at the same moment. Two limits
apply: the probe ran in Windows PowerShell, which loaded comctl32 5.82, and
`syrinx.exe` takes its manifest from GPUI and may load Common Controls 6. We
did not check whether version 6 behaves the same; nothing in Syrinx's
current path depends on it, because Syrinx never has the subclass to remove.

## What happens in Syrinx today, and what can go wrong

For each closed, crashed or recovered page, on the worker thread:

1. `controller.Close()` and `DestroyWindow(container)`: same thread as
   creation, fine.
2. `SendMessageW(gpui_window, WM_USER + 0x65)`: the worker blocks until the
   GUI thread next retrieves messages. GPUI's window procedure does not
   recognise `0x465` (its own messages are `WM_USER + 1` to `+ 9`,
   `vendor/gpui-pre-windows-0.3.8/src/events.rs:23–31`). It falls through to
   `DefWindowProcW` (`events.rs:167–172`). Syrinx's caption subclass, which
   sits in front of GPUI, passes it on unchanged: every message goes to
   `DefSubclassProc` (`crates/syrinx-platform/src/caption.rs:115`), and
   anything other than `WM_NCHITTEST` returns that answer as it is
   (`caption.rs:116–117`). Nothing happens.
3. `RemoveWindowSubclass(gpui_window, parent_subclass_proc, …)`: no such
   subclass exists, so it returns `FALSE` with no effect, as the probe's
   third row shows.

The failure modes, and how likely they are:

- **Freed controller left in a subclass (the hazard in #83).** Not possible
  while every page is a child web view. It would become possible only if
  Syrinx ever built a non-child page web view on a thread other than the
  parent window's. That would be a new design and would need this review
  again.
- **Deadlock on the cross-thread `SendMessageW`.** It needs the GUI thread
  to wait on the worker without retrieving messages while the worker is
  dropping a page. There are two such waits. `BrowserHost::spawn` blocks on
  `ready.recv()` (`browser.rs:465`), but only until the new worker reports
  its thread ID, before `run` starts and before any page exists, so no drop
  can be in progress. The other is `BrowserHost::join`
  (`browser.rs:491–499`). It refuses to wait until the worker has set
  `stopped`, which `HostCompletion::drop` (`browser.rs:582`) does only after
  `run` has returned and every page is gone. The UI calls it only on
  `BrowserEvent::Stopped` (`ui.rs:1013–1014`). A `SendMessage` from the GUI
  thread to a worker window, if one happens, does not deadlock either,
  because each side keeps processing sent messages while it waits. Unlikely
  today, but it depends on that ordering staying true.
- **Close latency.** Each page drop waits for one GUI message-retrieval
  turn. That is normally well under a frame and has not been observed as a
  problem.
- **Reentrancy into GPUI.** The sent message is delivered only at
  message-retrieval points and goes straight to `DefWindowProcW`. The GPUI
  traps about native calls made inside an `update` do not apply.

## Options

**(a) Syrinx-side: drop web views only on the owning thread.** This is
already true for the web view's own thread: the worker creates and drops
every page, and the type system keeps it that way. Moving drops to the
*parent's* thread instead is not possible. WebView2 controllers belong to the
single-threaded COM apartment (STA) of the thread that created them, so they
must be closed on the worker, and a non-`Send` `Page` cannot cross to the GUI
thread anyway. Since the subclass is never attached, there is nothing on the
Syrinx side to fix. No code change.

**(b) A minimal wry patch** (written out, not applied). Detach only what was
attached, and let the subclass remove itself on its own thread. The diff
below is real `git diff` output, made against a scratch copy of the vendored
`src/webview2/mod.rs`, so its line ranges match the file as it is pinned in
Syrinx (upstream 0.57.0 plus the approved patches). The paths are rewritten
to wry's own layout, and the patched copy was not compiled:

```diff
diff --git a/src/webview2/mod.rs b/src/webview2/mod.rs
index 7bb5649..e694939 100644
--- a/src/webview2/mod.rs
+++ b/src/webview2/mod.rs
@@ -76,8 +76,10 @@ impl Drop for InnerWebView {
     let _ = unsafe { self.controller.Close() };
     if self.is_child {
       let _ = unsafe { DestroyWindow(self.hwnd) };
+    } else {
+      // Child web views never attach the parent subclass (see new_in_hwnd).
+      unsafe { Self::dettach_parent_subclass(*self.parent.borrow()) }
     }
-    unsafe { Self::dettach_parent_subclass(*self.parent.borrow()) }
   }
 }
 
@@ -1389,12 +1391,8 @@ impl InnerWebView {
 
   #[inline]
   unsafe fn dettach_parent_subclass(parent: HWND) {
+    // The subclass removes itself on the parent's thread; the helpers are single-threaded.
     SendMessageW(parent, PARENT_DESTROY_MESSAGE, None, None);
-    let _ = RemoveWindowSubclass(
-      parent,
-      Some(Self::parent_subclass_proc),
-      PARENT_SUBCLASS_ID as _,
-    );
   }
 
   #[inline]
```

The first hunk removes both cross-thread calls for child web views, the only
kind Syrinx uses. The second makes the non-child path correct from any
thread: the `PARENT_DESTROY_MESSAGE` arm (lines 1365–1372) already removes
the subclass and frees the controller box on the owner's thread, and if the
parent is already destroyed, its `WM_DESTROY` arm has done the same. For
Syrinx the patch changes no observable behaviour; it only removes one
cross-thread round trip per page close. Its cost is a third patched hunk in
`mod.rs` to carry and re-audit on every wry update, next to the two approved
patches already there.

**(c) Report upstream.** wry upstream has the same code, and its non-child
mode, which most wry users use, is where the documented-unsupported
cross-thread removal can matter. It matters for anyone who drops a web view
on a thread other than the window's. The diff in (b) is small and does not
depend on Syrinx's other patches.

## Recommendation

Take (c) without (b): send the diff in (b) to wry as an issue or pull request,
and carry no local patch for now. Syrinx is not exposed to the hazard #83
describes, the remaining cross-thread calls do nothing, and a local patch
would add maintenance with no behaviour change. If wry merges the fix, Syrinx
gets it with the next audited wry update.

Revisit this if Syrinx ever builds a page web view that is not a child, adds
a second thread that owns windows the web views are parented to, or adds any
GUI-thread wait on the browser worker other than `BrowserHost::spawn`'s
start-up handshake and `BrowserHost::join` after `Stopped`.

## What needs the owner

- Approval to send the upstream report in (c). `AGENTS.md` requires it for
  any upstream message; none has been sent.
- Whether #83 can close on this review, or should stay open until wry
  responds.
- Only if the owner prefers it over (c): approval of (b) as a narrow vendor
  patch, recorded like the other entries in `AGENTS.md`.
