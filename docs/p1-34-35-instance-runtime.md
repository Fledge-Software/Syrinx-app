# P1-34 and P1-35: one running Atlas, and the runtime-change notice

Plan 1's slices P1-34 and P1-35 prepare Atlas to be the owner's everyday
browser. Once Windows sends links to Atlas (P1-37, which the owner performs),
every click on a link in another program starts `atlas-app.exe` with that
link as its argument. Without P1-34, each click would start a second Atlas on
the same profile; the storage lease would refuse it, and the link would be
lost. P1-35 exists because Atlas uses the Evergreen WebView2 runtime, which
Windows updates silently, and Google's sign-in has been sensitive to how
WebView2 presents itself (D22, D36). The owner needs to know when the runtime
changed, so that a sign-in break is not a mystery, and needs a one-click way
to continue in Edge.

This note records what the two slices deliver, why they are built this way,
what they deliberately do not do, and the checks the owner runs on the
window, because an agent cannot see it.

## What P1-34 delivers

- **One running Atlas per profile and Windows user.** At start-up, before any
  window opens, `main` calls `atlas_platform::instance::claim`
  (`crates/atlas-platform/src/instance.rs`). The process that owns the
  profile's named mutex is the running Atlas; a *mutex* is a Windows kernel
  object that only one thread can own at a time, and that Windows hands to the
  next waiter if its owner dies. Every later launch forwards its request to
  the running Atlas and exits successfully.
- **A local channel only the same user can open.** Forwarding uses a *named
  pipe*, a Windows inter-process channel addressed by name. Its security
  descriptor grants access to the signed-in user alone (`D:P(A;;GA;;;<SID>)`,
  `instance/security.rs`), it refuses clients on other machines
  (`PIPE_REJECT_REMOTE_CLIENTS`), and it is created "first instance only", so
  if another program already holds the name, Atlas refuses to start instead of
  talking to it. A forwarding launch connects at identification level, so
  whatever holds the pipe cannot impersonate it. The alternative,
  `WM_COPYDATA` to the main window, was rejected: GPUI owns the window class,
  so finding the window would mean matching on a title, and a pipe can be
  tested without a window.
- **Only http(s) links, checked on both sides.** A link argument must pass
  `atlas_core::web_address`, the same rule navigation and import use (DW-1):
  http or https, with a host and no user name or password. Anything else
  (`javascript:`, `file:`, another protocol, text) makes the launch exit
  with an error. The running Atlas checks every request again when it
  arrives (`instance/protocol.rs`), so a program that writes to the pipe
  directly gains nothing. Nothing in a request is ever executed. Requests are
  limited to 32 KiB, and diagnostics never print the address.
- **New tab and foreground.** The running Atlas opens each forwarded link in a
  new tab, in arrival order, and calls GPUI's `activate_window`, which also
  restores a minimised window. Windows lets a program take the foreground only
  when the foreground program allows it, so the forwarding launch, which the
  owner just started, calls `AllowSetForegroundWindow` for the running Atlas
  before sending. A launch without a link only brings Atlas forward.
- **Start-up and shutdown races** (`instance/tests.rs`):
  - Six launches at once elect exactly one running Atlas; the other five
    forward and their links arrive.
  - A launch while the first Atlas holds the mutex but is not yet listening
    retries every 100 ms and is delivered once the listener starts. Links that
    arrive before the session has loaded wait in the window and open after the
    first session state (`ui/incoming_links.rs`).
  - A launch while Atlas is closing does not lose its link. The window stops
    listening as soon as closing starts; the mutex is released only after the
    store and pages have shut down, at the end of `main`. The waiting launch
    then becomes the next running Atlas and opens the link itself.
  - An Atlas that died while owning the profile leaves an abandoned mutex,
    which the next launch takes over.
  - A running Atlas that never answers ends a launch's wait after 15 seconds
    (`FORWARD_PATIENCE`) with an error.
  - A pipe name another program already holds stops the launch.
- **Profile isolation.** The mutex and pipe names contain the user's SID and a
  64-bit FNV-1a hash of the profile directory (`instance/names.rs`), with
  spelling differences removed (slashes, letter case, a trailing separator,
  the `\\?\` prefix). A `--test-data-dir` profile and every `--smoke` profile
  therefore never forward to the owner's real Atlas. Spellings that the key
  does not unify, such as a junction or an 8.3 short name, could start a
  second Atlas on the same folder; the storage lease still refuses it.
- **Command line.** `atlas-app.exe [--test-data-dir <directory>] [link]`.
  One link at most; `--smoke` refuses a link.

What P1-34 does **not** do: it does not register Atlas as a browser, a URL
handler or the default browser, and it changes no registry setting. That is
P1-37, performed by the owner.

Known limits:

- A link that arrives in the moment between the window starting to close and
  its listener stopping is accepted but not opened. The window stops the
  listener first thing when closing starts, so this window is a few
  milliseconds.
- Error messages from a forwarding launch go to standard error, which a
  launch from Windows does not show. A refused link therefore fails silently.
- Two Windows sessions of the same user (for example console and Remote
  Desktop) each get their own mutex but share the pipe name; the second
  session's Atlas then refuses to start with "Another program already holds
  Atlas's link channel".

## What P1-35 delivers

- **The last observed runtime version is stored** in the existing `settings`
  table (schema version 3, no migration):

  | Key | Values | Default |
  | --- | --- | --- |
  | `webview2.runtime_version` | WebView2's version string, such as `141.0.3537.71` | absent until the first start |

  The page host reads the version from the shared environment's
  `BrowserVersionString` right after it is ready and sends it to the window as
  `BrowserEvent::RuntimeVersion` (`crates/atlas-platform/src/browser.rs`). No
  network call is made; this is the version already installed and running.
- **D36's notice when it changes** (`crates/atlas-app/src/ui/runtime_notice.rs`).
  When the stored version differs from the running one, a one-line strip
  appears under the toolbar: "WebView2 updated from A to B. Google sign-in
  can change after a runtime update; if it stops working, open the page in
  Edge." The page card moves down by the strip's height
  (`ui/page_card.rs`), because GPUI content must never overlap a page web
  view (see `docs/trapstoknow.md`). **Dismiss** removes it. The first start
  of a profile stores the version silently; an unchanged version shows
  nothing. The new version is saved as soon as the session accepts settings,
  so the notice appears on the first start after an update only; dismissing
  it is not stored.
- **Open in Edge, only on the owner's click.** While the active tab is an
  http(s) page, the strip offers **Open in Edge**. It re-checks the address
  with `atlas_core::web_address` and hands it to Windows as
  `microsoft-edge:<address>` (`crates/atlas-platform/src/edge.rs`). Atlas
  never opens Edge on its own, sends nothing else to Edge and imports
  nothing back. Home, `about:blank` and other non-web tabs get no button.
- **No sign-in canary.** D36 rejected an automatic sign-in test, which would
  put credentials in a test. Nothing signs in to anything.

## Files

- `crates/atlas-core/src/web_address.rs`: the shared address rule, copied
  byte for byte from the import branch (`claude/p1-22-import-commit`) so the
  two branches merge without conflict. `atlas-core` gains the owner-approved
  `url` dependency (DW-1).
- `crates/atlas-platform/src/instance.rs` and `instance/`: claim, names,
  owner-only security, the pipe and its protocol, and the race tests.
- `crates/atlas-platform/src/edge.rs`: the open-in-Edge hand-off.
- `crates/atlas-platform/Cargo.toml`: `atlas-core` becomes a normal
  dependency (it was a development dependency), and the pinned `windows`
  crate gains the `Win32_System_Pipes` and `Win32_Security_Authorization`
  modules. No package or version changes; `Cargo.lock` only records
  `atlas-core -> url`.
- `crates/atlas-app/src/main.rs`: the link argument and the claim.
- `crates/atlas-app/src/ui/incoming_links.rs` and `ui/runtime_notice.rs`: the
  window side; `ui.rs` and `ui/page_card.rs` carry only the wiring.

## Owner checks

Run these on the real window; an agent cannot inspect it. Use a test profile
for everything except the last check, so the owner's real Atlas is untouched.
Build first with `cargo build --package atlas-app --profile local --offline
--locked`; the program is `target\local\atlas-app.exe`.

1. **Forwarding.** Start
   `target\local\atlas-app.exe --test-data-dir C:\Temp\atlas-p134`. While it
   is open, run
   `target\local\atlas-app.exe --test-data-dir C:\Temp\atlas-p134 https://example.com`
   in PowerShell. Expected: the command returns at once, the open Atlas comes
   to the front with `example.com` in a new tab, and no second window opens.
2. **Minimised.** Minimise that Atlas and repeat with another address.
   Expected: the window is restored and in front.
3. **No link.** Run the command from check 1 without a link. Expected: Atlas
   comes to the front and no tab is added.
4. **Refusals.** Run it with `javascript:alert(1)`, with `file:///C:/Windows`
   and with `notepad.exe` as the link. Expected: nothing opens, nothing runs,
   and the open Atlas is unchanged.
5. **Start-up race.** Close Atlas, then run the start command and, within a
   second, the forwarding command from check 1. Expected: one window, with
   the link in a new tab once it has loaded.
6. **Shutdown race.** Close Atlas with the window's close button and
   immediately run the forwarding command. Expected: the first Atlas closes,
   and a new one starts with the link in a new tab.
7. **Isolation.** With the test profile open, start your normal Atlas
   (`cargo atlas`). Expected: a second, separate window; the test profile's
   window gets nothing.
8. **Runtime notice.** With the test profile closed, open
   `C:\Temp\atlas-p134\State\atlas.db` with a SQLite tool and run
   `UPDATE settings SET value = '1.0.0.0' WHERE key = 'webview2.runtime_version';`
   on this test copy only, never on the owner's real profile. Start Atlas
   with that profile. Expected: the strip "WebView2 updated from 1.0.0.0 to
   …" under the toolbar, with the page card moved down by the strip and not
   overlapped. Open a web page: **Open in Edge** appears and opens that page
   in Edge only when clicked. **Dismiss** removes the strip and the page card
   moves back up. Restart: no strip.
9. **Real profile, once.** After the owner's next WebView2 update, the
   strip should appear on the first start of the owner's Atlas.

Delete `C:\Temp\atlas-p134` afterwards.
