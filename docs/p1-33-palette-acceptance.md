# P1-33 palette acceptance: method, results and what the owner checks

The command palette is the centre of Atlas (S5), so its speed has to be judged
the way the owner feels it: from the key press to pixels on the screen, not
only inside the search engine. Plan 1's phase 4 exit criteria say "opening the
palette paints within one frame of Ctrl K; keystroke ranking meets D48's 5 ms
at the 95th percentile for 100,000 rows, with the maximum reported; the overlay
tests from Phase 0 pass in the real app". P1-05's benchmark (`cargo xtask
bench`) measures only the search core. P1-33 measures the composed path in the
running application and re-checks the live-overlay focus and stacking
behaviour that earlier slices (P1-10, P1-11, P1-17, P1-30/31, P1-36) built.

**Short version.** All figures here come from a busy development machine,
with Atlas launched by an agent. Only the latest run had Atlas in the
foreground. The owner's quiet foreground runs are the acceptance numbers. On
that basis:

- The harness works and has run five times.
- **The harness found a product bug, and it caused real traffic.** Enter
  pressed before the current query's local reply fell back to a Google
  search. In two agent runs this sent the synthetic text "enter-target" to
  www.google.com, before any loopback guard existed. No personal data was
  involved: the profile and the text were synthetic. The palette now waits for
  the reply. The run now aborts on any destination outside its loopback pages.
  See "Product bug found by the harness" below.
- **Opening passes since DW-90.** It took about half a second, roughly 30
  frames against one, almost all of it inside GPUI's `open_window`. DW-90 now
  creates the popup once and shows, resets and hides it: the opening p95 is
  about 6 ms against 16.7 ms (see "DW-90: the kept popup" below).
- **Composed ranking passes at 1,000 and 10,000 rows but fails at 100,000:**
  key to presented rows has a p95 of about 58 ms against the owner's 33.3 ms.
  Digit-heavy queries (`row-0420`) take up to about 106 to 123 ms in the worker.
- **D48 fails in the running app** at 10,000 and 100,000 rows; that work
  stays with P1-05.
- **Overlay ownership, not-topmost, z-order and popup reuse pass.**
- **Enter activation passes** since the fix (run `1791856583`).
- **Focus return after Esc was a product defect; it is diagnosed and fixed.**
  GPUI's `activate_window` set Win32 focus to the main window from a task that
  ran after Atlas had already sent the page its focus. Esc now passes 20 of 20,
  and so does Esc pressed before the palette's first frame. See "Why Esc did
  not return focus to the page" below.

Nothing slow has been hidden or excluded.

## How to run it

From the repository in PowerShell, with no other Atlas build running:

```powershell
cargo xtask palette-acceptance
```

The command:

1. builds `atlas-app` with D38's `local` profile first, so the run never races
   a build of the same executable (see the trap about LNK1104);
2. starts a loopback-only page server on `127.0.0.1` with an ephemeral port,
   inside the xtask process. It answers exactly two synthetic pages,
   `/page/focus-page` and `/page/enter-target`, and a 404 for everything else;
3. launches `target\local\atlas-app.exe --test-palette-acceptance <report.json>
   http://127.0.0.1:<port>` **visibly**. A hidden launch can block on an
   invisible message box and cannot test focus (see the traps);
4. writes `target\p1-33\palette-acceptance-<unix time>.json` (all raw samples
   and verdicts) and `...-summary.md` (p50, p95 and maximum per stage), prints
   the summary and exits non-zero if a budget or a functional check failed.

The app flag is not part of the default gate. Like `--smoke` and
`--test-suspended-tabs`, it always uses a freshly reserved synthetic profile
under `%TEMP%`, never `%LOCALAPPDATA%\AtlasProductPreview`. Startup problems go
to stderr only, never to a message box. It refuses any page base other than
`http://127.0.0.1:<port>`, and it cannot be combined with `--smoke`,
`--test-data-dir`, the suspended-tab mode or a link. Once running, the run
also aborts when a page reports an address outside its loopback pages; see
"What the loopback guard covers" below. A 600-second watchdog
(exit code 124) bounds the whole run, as `--smoke`'s 30-second one does. The
run takes about three minutes after the build.

**For the quiet-machine numbers,** the owner should close other heavy
programs and builds, leave the laptop on mains power, and run the command
from a terminal **in the foreground**. Then leave the keyboard and mouse
alone until the window closes by itself. Run it three times and keep all
three reports; the variation between runs is part of the result.

## Method

### What drives the window

The run is scripted inside the real `BrowserView`
(`crates/atlas-app/src/ui/palette_acceptance.rs`). It waits on what the window
shows rather than on fixed delays. Input goes only to Atlas's own windows:

- **Ctrl K** is the `Palette` action. The run calls `BrowserView::shortcut`
  with `BrowserShortcut::Palette`, the same function the WebView2 accelerator
  event and the GPUI key binding call. The accelerator leg itself is not
  measured; see "Not measured".
- **Typing, Backspace, Enter and Esc** are posted to the palette popup's own
  window handle as `WM_CHAR`, `WM_KEYDOWN` and `WM_KEYUP` messages
  (`crates/atlas-platform/src/acceptance.rs`). GPUI handles them through its
  normal message loop: `WM_KEYDOWN` goes through `translate_accelerator` to its
  key handler, and `WM_CHAR` goes to the focused input. Before posting,
  `post_text` and `post_key` check that the handle is a live window owned by
  this Atlas process, and refuse otherwise. A unit test proves the refusal for
  the current foreground window. Nothing uses `SendInput`/SendKeys, so no key
  can reach another application.
- **Posted keys prove the palette's key handling, not Windows' routing of real
  keyboard input.** A posted message is delivered to the named window whether
  or not it has keyboard focus, so these runs do not show that a physical key
  press reaches the popup. That is part of the owner's manual check.

### How a frame is judged "presented"

`crates/atlas-app/src/acceptance/probe.rs` is a small shared timing record that
exists only in an acceptance run. The palette's `render` asks the probe
whether any measurement is waiting for a frame. If so, it schedules a
`cx.defer` callback. GPUI runs deferred callbacks when the update that drew
the frame ends, and that update also calls `Present`. The stamp is therefore
taken **after `Present` returned**. GPUI presents with sync interval 0 into a
DirectComposition swap chain, so the compositor shows the frame at the next
display refresh. That refresh is up to one more interval (16.7 ms here) and is
not included. The numbers are "submitted to the compositor", not "photons".

For an opening, the show task arms the probe and marks the palette for a
fresh frame just before it calls Windows to show the popup. GPUI draws and
presents that frame while handling the popup's `WM_SHOWWINDOW`, before the
window is visible, so the first visible frame already shows the new entry.
(Before DW-90 the probe was armed from `Window::on_next_frame` after
`open_window` returned.)

**Cost in ordinary runs.** The palette holds `Option<PresentProbe>`, which is
`None` outside an acceptance run. It costs one `Option` check per query,
reply and render, and reads no clock. The session worker's acceptance
behaviour is fixed when it starts: only a worker started with
`session::spawn_acceptance` accepts `GrowSyntheticCatalogue` and times its
queries. Any other worker refuses the command with
`SessionError::AcceptanceOnly`, and a worker test proves this. An ordinary
worker pays one boolean check per query and calls `Instant::now` only when
timing is on.

### Opening

The run measures 20 openings, each with the focus page focused. Each one runs
from just before the `Palette` action to the first presented frame of the
popup, and is broken into stages. Since DW-90 the popup already exists,
hidden, so the stages are:

- **show requested**: `open_palette` has reset the kept popup for this
  entry (text, rows, selection, scope chip and any waiting Enter), sent its
  first query and queued the task that shows it;
- **show started**: that task begins moving the hidden popup into place;
- **first render**: the palette's first render after the opening began,
  which GPUI draws as Windows shows the popup;
- **presented**: the first frame presented.

Before DW-90 the stages were `open_window` called, window created (inside
GPUI's `open_window`), first render and presented; the earlier runs below
use those names.

**Budget (owner decision, 2026-10-12):** within one refresh interval at the
95th percentile, with every sample and the maximum reported. The interval comes
from Windows (`EnumDisplaySettingsW` on the monitor showing Atlas). If Windows
cannot report it, or the window is not Atlas's own, the report records why
(`display.problem`), sets `measured: false` and assumes 60 Hz.

### Ranking

Only a worker started with `spawn_acceptance` (see above) accepts the
`GrowSyntheticCatalogue { total }` command
(`crates/atlas-app/src/session/synthetic.rs`). It adds deterministic synthetic
history rows to the **in-memory** tier-1 catalogue:

- titles like `harbor project guide row-004200`; some also carry `café`,
  `東京`, `🧭` or a long fragment;
- addresses under `.test` or `www.example.com`;
- visit counts and recencies spread over 90 days.

Nothing is written to the store; a worker test proves that the rows are gone
after a restart.

At 1,000, then 10,000, then 100,000 synthetic rows (plus the run's few real
rows), the palette is opened. The words `amber`, `harbor guide`, `row-0420`,
`café`, `naviga` and `zqxv` are typed and erased in two ways.

**One key at a time** (156 keystrokes per size). The run waits for each key's
rows to be presented, then pauses 20 ms before the next key. This measures
each key cleanly, but it **hides queueing**: a real typist does not wait for
results. Each key records:

- **worker**: `Catalogue::query` inside the session worker, covering ranking,
  scope counts and highlights. This is D48's quantity.
- **query round trip**: from the browser view queueing the query to the
  palette accepting the reply (both thread hops, plus `palette::arrange`).
- **key to echo frame**: the posted key to the first presented frame showing
  the changed text.
- **key to rows frame**: the posted key to the first presented frame showing
  that text's ranked rows. This is the **composed latency**.

Keys whose rows never appear within 3 s count as timeouts.

**Back to back** (12 bursts per size). Each word is typed, then erased, at one
key every 80 ms without waiting, which is a steady fast typist. A burst
records:

- the time from its **last** key to the first presented frame showing the
  final text's rows;
- how many palette requests a newer one replaced before their rows arrived
  ("superseded").

Bursts have no budget of their own and do not decide the verdict.

**Budgets:**

- D48's 5 ms at p95 applies to the worker time at every size; any timeout
  fails the size.
- **Composed latency (owner decision, 2026-10-12):** key to presented rows
  within two refresh intervals at the 95th percentile (33.3 ms at 60 Hz),
  with the maximum reported. Two intervals is the smallest budget that allows
  for the frame wait: a reply that lands just after a frame must wait for the
  next one.

### Focus and stacking

The synthetic pages' own script writes `document.hasFocus()` into
`document.title` as `atlas-p133-<label> focus:yes|no`. Atlas injects nothing,
attaches no IPC handler and sets no user agent. It reads the title from the
ordinary `BrowserEvent::Title` it already receives for every page, as P1-11's
popup fixture does. The checks:

| check | what passes |
|---|---|
| `page_focus_baseline` | after `focus_page`, the focus page reports focus |
| `popup_owned_by_main_window` | `GetWindow(popup, GW_OWNER)` is the main window |
| `popup_not_topmost` | `WS_EX_TOPMOST` is clear, so the palette does not float over other apps |
| `popup_above_page` | the popup is visible and above the main window in the z-order |
| `palette_takes_keyboard_focus` | the page reports blur once the palette opens |
| `escape_returns_page_focus` | after each of the 20 Esc presses the palette closes and the page reports focus again |
| `openings_reuse_one_popup` | all 20 openings used the same native popup (DW-90) |
| `no_stale_refocus_after_escape` | one second later: no open entry, the kept popup hidden, two windows (main and kept popup), page still focused, same tab |
| `hidden_popup_renders_nothing` | for 30 seconds with the palette hidden, after a five-second settle, the palette view does not render and its input does not notify, so no caret blink wakes Atlas (D37's idle budget) |
| `idle_cpu_with_hidden_popup` | over those 30 seconds Atlas's own process uses under 0.5% of one core (D37); WebView2's processes are not counted |
| `rapid_open_then_escape` | Esc posted in the same turn as Ctrl K, before the popup is shown, still ends the entry, keeps the popup hidden and returns focus |
| `no_stale_refocus_after_rapid_escape` | as above, after the rapid case |
| `second_ctrl_k_reuses_popup` | Ctrl K while open keeps the same popup and entry, and two windows in total |
| `enter_activates_selected_tab` | typing the background tab's name selects its tab row; Enter switches to it and its page reports focus |
| `no_stale_refocus_after_enter` | one second later: no open entry, popup hidden, target tab still active and focused |

Each focus check's detail also records where Windows keyboard focus is
(`GetGUIThreadInfo` on the interface thread: the main window itself, inside
it, which is the page host and WebView2, the popup, or elsewhere). The Esc
check adds a trace for the first opening and the first failed one: keyboard
focus changes and the browser view's focus steps, in milliseconds after Esc.

## Results on this machine

**These numbers were taken on a busy development machine:** other agent
sessions were compiling in parallel, and Atlas was launched by an agent from
a background shell, so Windows never let it into the foreground. **Because
Atlas was inactive, GPUI capped its frames at about 30 per second** (GPUI's
default `inactive_frame_interval` of 33.3 ms). The cap applies to frames with
pending `on_next_frame` callbacks, which the opening measurement uses, so each
opening's present could be delayed by up to 33 ms. The keystroke frames are
driven by view notifications and are not capped this way. These runs show
where time goes; they are not the owner's quiet-machine acceptance numbers.

The latest run is `target/p1-33/palette-acceptance-1791855053.json`
(2026-10-12, after merging the integration branch's P1-15 follow-up). The
display is 60 Hz (16.667 ms, read from Windows). Earlier runs the same day
are `1791845563`, `1791845322` and `1791845202`.

### Opening: fails, about 30 frames

| stage (from the Ctrl K action) | n | p50 ms | p95 ms | max ms |
|---|---|---|---|---|
| open_window called | 20 | 0.01 | 0.01 | 0.06 |
| window created | 20 | 455.9 | 510.5 | 556.2 |
| first render | 20 | 458.3 | 514.0 | 558.5 |
| presented | 20 | 469.3 | 525.4 | 573.3 |

The earlier run `1791845563` gave a presented p50 of 493.0 ms and p95 of
518.4 ms; `1791845322` gave a p50 of 512.6 ms and a maximum of 622.4 ms.

`open_palette`'s own work, including closing a peek, takes about 0.01 ms.
Nearly all of the half second falls **inside GPUI's `open_window`, before
the palette view is built**: creating the Win32 window, its
DirectX/DirectComposition swap chain and its renderer. Building the view and
its first render take about 2 ms, and reaching the first present about 11 ms
(possibly lengthened by the 30 fps cap above).

This attribution is **provisional** until the owner's foreground run, which
removes both the busy machine and the inactive-window cap. Which part of
GPUI's window creation dominates is not established; GPUI's `profiler` feature
is off in the pinned build.

P1-10 opens a new native popup for every Ctrl K and removes it on close
(`BrowserView::open_palette`, `PaletteView::finish`), so every opening pays
this cost. The owner approved **DW-90** on 2026-10-12: create the popup once
and show and hide it. DW-90 has since done so; its results are in "DW-90: the
kept popup" below.

### Ranking (run `1791855053`)

| rows | stage | n | p50 ms | p95 ms | max ms | verdict |
|---|---|---|---|---|---|---|
| 1,000 | worker | 156 | 0.23 | 0.95 | 2.06 | D48 **pass** |
| | round trip | 156 | 0.37 | 1.14 | 4.94 | |
| | key to echo frame | 156 | 4.05 | 26.08 | 29.70 | |
| | key to rows frame | 156 | 19.74 | 28.75 | 31.31 | composed **pass** |
| | burst: last key to rows | 12 | 22.34 | 27.27 | 27.27 | no budget; 0 of 78 superseded |
| 10,000 | worker | 156 | 0.73 | 8.78 | 15.78 | D48 **FAIL** |
| | round trip | 156 | 0.90 | 8.92 | 16.67 | |
| | key to echo frame | 156 | 3.67 | 23.25 | 27.33 | |
| | key to rows frame | 156 | 19.47 | 28.77 | 34.33 | composed **pass** |
| | burst: last key to rows | 12 | 17.31 | 27.45 | 27.45 | no budget; 0 of 78 superseded |
| 100,000 | worker | 156 | 5.76 | 40.59 | 106.49 | D48 **FAIL** |
| | round trip | 156 | 6.10 | 40.73 | 106.67 | |
| | key to echo frame | 156 | 3.83 | 26.32 | 32.34 | |
| | key to rows frame | 156 | 24.62 | 58.14 | 121.52 | composed **FAIL** |
| | burst: last key to rows | 12 | 20.86 | 70.01 | 70.01 | no budget; 2 of 78 superseded |

There were no timeouts in any run.

The previous run (`1791845563`) agreed:

| rows | worker p95 / max (ms) | composed p95 / max (ms) |
|---|---|---|
| 1,000 | 1.01 / 1.69 | 29.9 / 60.8 |
| 10,000 | 10.1 / 13.5 | 30.8 / 36.5 |
| 100,000 | 47.9 / 123.4 | 62.0 / 148.6 |

**The slow case is the digit query.** In run `1791845563`, every 100,000-row
worker maximum except one is between 2.8 and 13.6 ms. Typing and erasing
`row-0420` reaches 123.4 ms, because every synthetic title contains `row-`
followed by six digits. At 10,000 rows the same word is the only one above
1.5 ms. This agrees with P1-05's open D48 failure (`docs/p1-05-search.md`).
The round trip exceeds the worker time by under a millisecond at p95, so the
composed latency is the worker plus the wait for a frame.

**Typing echo** (the changed text on screen) has a p95 of about 25 ms at every
size, about one and a half frames, with a p50 of 4 to 6 ms. It does not depend
on catalogue size, since the input renders before the reply arrives.

**Back-to-back typing** at 80 ms per key rarely piles up here: at 100,000 rows,
2 of 78 requests were replaced before their rows arrived. The last key's rows
appeared within 70 ms (one sample per word, 12 words). Faster cadences were
not tried.

### Focus and stacking

| check | latest run (`1791855053`) | earlier runs |
|---|---|---|
| `page_focus_baseline` | **FAIL** | passed in `1791845563` and `1791845322` |
| `popup_owned_by_main_window` | pass | pass |
| `popup_not_topmost` | pass | pass |
| `popup_above_page` | pass | pass |
| `palette_takes_keyboard_focus` | pass | pass |
| `escape_returns_page_focus` | **FAIL** 0/20 | 0/20 in every run; the palette closed every time |
| `no_stale_refocus_after_escape` | **FAIL** | no palette reopened and no extra window in any run; page focus stayed false |
| `rapid_open_then_escape` | **FAIL** | passed in two of three earlier runs |
| `no_stale_refocus_after_rapid_escape` | **FAIL** | as above |
| `second_ctrl_k_reuses_popup` | pass | pass |
| `enter_activates_selected_tab` | **FAIL** | passed in `1791845563`; failed in `1791845322` |
| `no_stale_refocus_after_enter` | **FAIL** | as above |

**In the earlier runs, focus return was not established either way.**
Windows denied Atlas foreground activation throughout those runs: `foreground_window()` never returned Atlas's
main window or popup. When the active popup is destroyed in a background
process, Windows activates some other application's window. The page's
`document.hasFocus()` is then false even though Atlas moved WebView2 focus
back (`focus_page`). That is consistent with the traps on foreground rules.

**Enter activation: diagnosed, fixed, and passing.** The earlier failures
(`1791845322` and `1791855053`) were the palette bug described in the next
section. The harness posted Enter as soon as a reply to a *partial* query had
selected the target; the last characters then started a new request, and
Enter reached it before its reply. Enter then searched the web for
"enter-target". That explains every symptom:
- the active tab stayed put and showed "The page could not be loaded";
- the catalogue held one extra history row (1,005 rather than 1,004);
- the synthetic profiles contain `https://www.google.com/search?q=enter-target`.

After the fix, run `1791856583` passed `enter_activates_selected_tab` and
`no_stale_refocus_after_enter`. The active address was the loopback
`enter-target` page, and the catalogue was back to 1,004 rows.

### Run after the fix (`1791856583`, Atlas in the foreground)

For the first time, Windows let the agent-launched Atlas into the foreground
(`page_focus_baseline` reports "Atlas in foreground: true").

| measure | result |
|---|---|
| opening presented | p50 417 ms, p95 446 ms, max 448 ms; `open_window` called at 0.01 ms, window created at p95 402 ms |
| worker p95 / max at 1k, 10k, 100k | 0.93 / 1.5 ms, 8.1 / 14.2 ms, 41.8 / 92.4 ms |
| composed p95 / max at 1k, 10k, 100k | 27.9 / 32.1 ms (pass), 28.7 / 37.1 ms (pass), 60.3 / 116.3 ms (**fail**) |
| burst last-key p95 at 1k, 10k, 100k | 29.9, 28.7, 61.6 ms; nothing superseded |
| checks passing | page focus baseline, ownership, not topmost, above page, palette takes focus, popup reuse, Enter activation, no stale refocus after Enter |
| checks failing | `escape_returns_page_focus` 0/20, both Esc stale-refocus checks (page focus false), `rapid_open_then_escape` |

**Esc focus return is now a finding, not just an environment limit.** With
Atlas in the foreground, the palette closed every time and no window
leaked, but the page never reported focus afterwards. One candidate is in
the traps: when the active popup is destroyed, Windows activates its owner
and may take keyboard focus back from the WebView2 child after `focus_page`
ran. This is not diagnosed. It belongs in its own slice before P1-33 can
pass.

## DW-90: the kept popup

**Why.** Every Ctrl K built a new native popup with its own DirectX and
DirectComposition swap chain, which took about half a second inside GPUI's
`open_window`. The owner's budget is one refresh interval at p95, which no
per-opening window could meet (`docs/decisions.md`, 2026-10-12).

**What changed** (`crates/atlas-app/src/ui/palette_popup.rs`,
`crates/atlas-app/src/palette.rs` and
`crates/atlas-platform/src/browser/overlay.rs`):

- The popup is created once, hidden (`WindowOptions { show: false }`), owned
  by the main window and not topmost, right after the first session state
  arrives, so even the first Ctrl K is fast. If it is ever lost, the next
  Ctrl K builds a new one.
- Each Ctrl K opens an *entry*: `PaletteView::begin` resets the text, rows,
  selection, scope chip, status and any waiting Enter, gives the entry its own
  event channel and sends the first query. Request IDs keep counting across
  entries, so a reply to an earlier entry can never match.
- A task outside any GPUI update then moves the popup to its place (computed in
  logical pixels from the main window, converted at the main window's scale)
  and shows it. Windows' resize and show messages must reach GPUI while its app
  state is free; inside an update GPUI drops them (see the traps). When the
  popup crosses to a monitor with another scale, Windows resizes it, so it is
  placed a second time. With Atlas in front, the popup is then activated
  synchronously; if another application has taken the foreground, the popup
  is shown without activation and Atlas does not steal the foreground back.
- Esc, Enter, a run command and Windows' close request end the entry and hide
  the popup. Hiding activates the main window first when the popup is active,
  so Windows never picks the next window itself. The entry's channel is
  dropped, which ends the browser's receiver.
- Stale-event guards are per entry: the receiver drops events from any entry
  but the current one, and the show task does nothing if Esc or Enter already
  ended its entry (the rapid-Esc case). A popup-origin Ctrl K is dispatched
  only while its entry still takes input, which keeps the queued-refocus trap
  handled.
- At shutdown the kept popup is removed with the main window.
- A hidden window schedules no frames: GPUI's VSync thread skips windows that
  are not visible (D37's patch), and the harness checks that the palette does
  not render while hidden.

**Results.** All runs are on the same busy machine as above.

| run | Atlas in front | opening presented p50 / p95 / max (ms) | Esc returns focus |
|---|---|---|---|
| `1791858770`, before (diagnostics only) | no | 501.7 / 556.5 / 566.4 | 0 of 20 |
| `1791859013`, focus fix only | no | 493.9 / 528.8 / 558.6 | 20 of 20 |
| `1791859688`, kept popup, first version | from the middle of the run | 3.9 / 9.9 / 14.4 | 3 of 20 |
| `1791859981`, with the activation follow-up | yes | 4.5 / 6.3 / 9.0 | 20 of 20 |
| `1791860064` | yes | 4.5 / 6.5 / 7.4 | 20 of 20 |
| `1791860299`, final | yes | 4.3 / 5.9 / 6.9 | 20 of 20 |
| `1791860396`, final, with a UI Automation client attached | yes | 4.2 / 6.8 / 8.1 | 20 of 20 |
| `1791861452`, code later withdrawn (see below) | no | 3.7 / 10.6 / 394.1 | 20 of 20 |
| `1791862999`, after review, 5 s idle window | yes | 3.9 / 5.4 / 10.8 | 20 of 20 |
| `1791863160`, after review, final | yes | 5.3 / 13.0 / 27.5 | 20 of 20 |

In the final run the stages were: show requested at p95 0.16 ms, show started
at 0.26 ms, first render at 2.0 ms and presented at 5.9 ms, so the opening
passes with about 11 ms to spare. Every focus and stacking check passed,
including `openings_reuse_one_popup` (one native popup across 20 openings)
and `hidden_popup_renders_nothing` (0 renders in one hidden second). Ranking
is unchanged by DW-90: worker p95 1.0, 7.6 and 37.3 ms and composed p95 28.3,
29.3 and 58.3 ms at 1k, 10k and 100k rows, so D48 and the 100,000-row
composed budget still fail and stay with P1-05.

**The 3-of-20 run.** In `1791859688` Atlas was not in front when the run
started (its baseline check failed too) and was by the end. With Atlas behind
another application, `focus_page` asked GPUI to activate the window and
waited for an activation event to focus the page; when Windows reported no
activation change, no event came and the page was never focused. This
explanation is inferred from the code and the run's checks: that run had no
focus trace yet. A follow-up that focused the page right after GPUI's
activation task made run `1791861452` pass 20 of 20 with Atlas behind another
window, but only because GPUI's activation forced Atlas to the front.

**Atlas no longer takes the foreground (review, 2026-10-12).** The owner
decided that Atlas must never take the foreground from another application
(`docs/decisions.md`), so that path is gone: `focus_page` no longer calls
`activate_window`. With Atlas behind, the page's focus is deferred, and the
main window's activation observer completes it when the owner returns to
Atlas. Ctrl K while Atlas is behind leaves the palette closed with a message,
instead of showing a popup over the other program. The harness therefore
waits up to 20 seconds for Atlas to be in front, stops with an explanation if
the palette cannot open, and counts an Esc whose focus is deferred, with
Atlas behind, as the expected outcome rather than a failure.

**The hidden popup woke Atlas twice a second (review, 2026-10-12).** Hiding
the popup did not blur its input, so the input's caret blink kept re-arming a
500 ms timer: GPUI delivers blur only during a draw, which a hidden window
never has. The earlier `hidden_popup_renders_nothing` check could not see
this, because a blink notifies the input without rendering the hidden window.
Each close now drops the used input, which cancels its blink, and creates a
fresh unfocused one; the harness counts the input's notifications over 30
hidden seconds and measures Atlas's own CPU time across them.

**After the review.** Both runs had Atlas in the foreground throughout, and
every focus, stacking and idle check passed in both; the overall verdict
still fails only on D48 and the 100,000-row composed budget, which stay with
P1-05. In `1791863160` the palette rendered 0 times and its input notified 0
times over 30 hidden seconds, and Atlas's own process used 109.4 ms of CPU in
those 30 s, 0.365% of one core against D37's 0.5%. WebView2's processes are
not included, and the machine was busy, so this is not D37's five-minute
quiet-machine measurement, which stays with P1-06. The first run used a
five-second window and read 62.5 ms, 1.25% of a core: four of Windows' 15.6 ms
accounting ticks, starting right after the openings, which is why the window
is now 30 s after a five-second settle. Esc returned focus 20 of 20 times in
both runs, all through the "activated at once" path, since Atlas was in
front; the deferred path was not exercised by these runs.

**The deferral is an owner hand check.** The harness cannot complete a
deferred focus itself: that needs the owner to bring Atlas back, and the
harness must not take the foreground either. `escape_returns_page_focus`
therefore fails if any Esc deferred in a run that began with Atlas in front,
and counts deferrals as expected only in a run that began behind. To check
the path by hand: open the palette, switch to another program, let Esc be
delivered, then click Atlas's window frame and confirm the page has the keys.
Then repeat, returning by clicking the sidebar address field: the field must
keep the keys, because a click in Atlas's chrome cancels the deferral and
the activation observer completes it only while GPUI focus is still on the
window root or nowhere. One Esc in
`1791863160` took 272 ms to reach the page, and three of its twenty closes
found keys on the main window for a moment before the page took them.

**One slow opening.** The first opening of `1791861452` took 394 ms: its show
task started 79 ms after Ctrl K and its first render came at 391 ms. The other
19 took 2 to 11 ms, and no earlier run's first opening exceeded 10 ms. Atlas
was behind another window, so the popup was shown without activation; why
that first frame was late is not established. It does not change the p95.

## Why Esc did not return focus to the page

**Symptom.** After Esc closed the palette, the page never reported
`document.hasFocus()` again: 0 of 20 in every earlier run, including run
`1791856583` with Atlas in front. Enter, which also returns focus to a page,
passed.

**Evidence.** The harness now reads which window has Windows keyboard focus
(`GetGUIThreadInfo`). In the before run `1791858770` it was the main window
itself, not the page, after all 20 Esc presses, both when the palette had
closed and three seconds later; the page never reported focus even briefly.

**Cause.** `focus_page` called GPUI's `Window::activate_window` and then sent
the page its `Focus` command. On Windows, `activate_window` does its work
later, from a spawned task: `SetActiveWindow`, `SetFocus` on the GPUI window,
a synthetic Alt press and `SetForegroundWindow`
(`gpui-pre-windows-0.3.8/src/window.rs`, `activate`). The page's `Focus`
reached WebView2 first, and the task's `SetFocus` then took the keys back to
the main window. Enter's pass is consistent with this: the target tab's
`Focus` is sent later, after a round trip to the session worker, by which
time GPUI's task had run. The hypothesis in the earlier draft, that Windows reactivating the
owner of a destroyed popup took the keys, was not the cause: the fix below
works with the old destroy-per-opening popup.

**Fix.** With Atlas already in front, `focus_page` now activates the main
window natively and synchronously (`atlas_platform::browser::
activate_if_foreground`) and then sends the page `Focus`. With another
application in front, it does not bring Atlas forward; the page takes the
keys when the owner returns to Atlas (see the review section above). Run `1791859013` changed only this,
keeping the old popup, and passed 20 of 20 Esc presses and the rapid Esc.
With the kept popup, the focus trace of the final run shows the order: at
17.3 ms after Esc the main window is activated and the page's focus sent, and
by 17.4 ms keyboard focus is inside the main window, in the page.

## Product bug found by the harness

**Enter before the local reply searched the web.** Each keystroke starts a
new local request: `PaletteChoices::begin_query` clears the rows and
`query_scoped` selects row 0. If Enter arrived before that request's reply,
`submission` (`crates/atlas-app/src/palette/choices.rs`) found no row and no
address, and fell through to `Submit { text }`, a Google search for the typed
text. A fast typist would hit this window on many queries.

The fix makes Enter wait whenever neither a visible row nor Go to is
selected:
- the palette shows "Waiting for local results…";
- when the current request's reply arrives, the normal rules apply, so its
  top row opens;
- the web is searched only if the reply has no rows and the text is not an
  address;
- new text cancels the waiting Enter;
- Alt Enter, Go to and an already visible row act at once.

Acting on the last shown rows was rejected: those belong to an older,
shorter query, so Enter could open a row the owner never meant. Unit tests
in `choices.rs` cover Enter between the query and a reply with rows, and
between the query and an empty reply.

**What it caused.** Two agent runs sent
the synthetic query "enter-target" to www.google.com. Their synthetic
profiles under `%TEMP%` (`atlas-window-smoke-33716-1791855054…`, from run
`1791855053`, and `…42036-…`, most likely `1791845322`, whose Enter check also
failed) recorded that address. No personal data was involved. The owner had
both profiles deleted on 2026-10-12. Before pressing Enter, the harness now
waits until the input holds the full target text and the palette has the
reply to its newest request.

### What the loopback guard covers

In an acceptance run, Atlas refuses any destination other than the run's
loopback pages and `about:blank` (`AcceptancePlan::refusal`, unit-tested). A
refusal fails the run and aborts it at its next step. The guard acts at three
points:

- before the session worker records an address Atlas navigates to
  (`BrowserView::navigate`);
- before WebView2 creates a page or navigates one on Atlas's command
  (`page_command`);
- when a page reports a new address on its own (`BrowserEvent::Url`, through
  `acceptance_observed`): a redirect, the page's own script or a link.

The first two stop the request before anything leaves the machine. The third
cannot: WebView2 reports the address after the navigation started, so the
first request has already been sent. It ends the run instead of letting it
continue somewhere else. The synthetic pages load nothing and contain no
links, so this has not happened in any run. The guard does not cover requests
a loopback page makes without navigating (there are none in the synthetic
pages) or WebView2's own background traffic.

The checks that do not depend on activation pass in every run: ownership, no
topmost, z-order above the page, palette reuse on a second Ctrl K, and no
reopened palette or leaked window after Esc, rapid Esc or Enter.

## Not measured, and what remains unverified

- **The accelerator leg.** With a page focused, Ctrl K travels through
  WebView2's `AcceleratorKeyPressed`, which posts to Atlas's event loop. That
  delivery is not timed. GPUI's own Ctrl K binding (chrome focused) is not
  timed either. Both precede the measured action.
- **Real keyboard routing.** Posted keys skip Windows' keyboard focus; whether a
  physical key reaches the popup is a manual check.
- **Latency under back-to-back typing faster than 80 ms per key,** and
  queueing beyond the bursts' last-key latency and superseded count.
- **Compositor latency** after `Present` (up to one refresh) and display
  latency.
- **The sidebar peek's Esc.** It goes through the same `focus_page` as the
  palette, so the fix applies to it. A harness check was attempted, but
  opening the peek from a script inside a `BrowserView` update panics in GPUI
  (a separate defect, recorded in the traps and flagged for its own slice), so
  the peek's Esc is an owner check.
- **Liveness under the palette (D7: no freeze frame).** A playing video should
  keep moving under the open palette. This is visual, so it is an owner check.
- **The owner's real catalogue shape.** Synthetic titles are deliberately
  repetitive (`row-NNNNNN`), which is the worst case for digit queries. Real
  history has fewer shared digit runs.
- **Wide and narrow palettes.** The run uses the window's default size (wide,
  with the preview pane).
- **UIA rerun (DW-90).** DW-90 changes no role, element id or focus handle,
  but it keeps the palette's handles alive across entries, so P1-36's probe
  was repeated: a read-only UI Automation client walked every Atlas window's
  tree every 100 ms through a whole acceptance run (`1791860396`, 267 sweeps,
  largest window tree 77 nodes). The palette appeared as "Command palette"
  with its "Search or command" field in 204 sweeps. Atlas did not panic and
  every focus check passed. The probe script was not committed; it sends no
  input.

## Owner checks

1. Run `cargo xtask palette-acceptance` three times in the foreground on a quiet
   machine, without touching the keyboard or mouse, and keep the three reports
   in `target\p1-33\`. Report the opening p95 and its stages, the 100,000-row
   worker and composed p95s, the burst figures and every failed check.
2. In particular: does `escape_returns_page_focus` pass 20 of 20, and does
   `enter_activates_selected_tab` pass every time? Is the opening's p95 under
   one refresh interval?
3. By hand: open a synthetic local video page, press Ctrl K and type. The video
   keeps playing under the palette (D7), the palette does not stay on top
   when you Alt Tab to another app, and Esc puts the keys back into the page
   (type in a text field). Do the same from the sidebar peek: Esc there must
   also put the keys back into the page.
4. With the window on a second monitor at a different scale, and after
   resizing the window, Ctrl K opens the palette centred 72 pixels below the
   window's top at the right size, with no flash of the previous query.

## Owner decisions (2026-10-12)

The owner answered this slice's questions, recorded in `docs/decisions.md`:

1. **Composed budget:** key to rows presented within two refresh intervals at
   p95, with the maximum reported. The harness judges every size against it.
2. **One-frame opening** is judged at p95, with every sample and the maximum
   reported. The harness does so.
3. **DW-90 is approved:** create the palette popup once and show and hide it,
   as a separate slice after P1-33 merges. Its result will be checked with this
   harness.
4. **D48 stays with P1-05, unchanged.** P1-33 stays open until P1-05's
   benchmark passes and this harness passes in the owner's foreground runs.

## Gate

`cargo xtask check` passed on 2026-10-12 with this slice's source. It ran
dependency boundaries, deferred-work markers, packaging, formatting, Clippy
with warnings denied, the tests and the application smoke run. D9's
typing-path scan stays clean with the palette and session hooks. The tests
cover:

- argument parsing and the loopback-only page base;
- the probe's stamp ordering and burst tracking;
- percentiles, frame counting, verdicts and JSON escaping;
- synthetic rows, the worker's acceptance-only, timed and non-persisted
  synthetic catalogue;
- the input refusal for other processes' windows and the page server's
  allowlist.

With the Enter fix and the loopback guard, the run passed 940 tests.

With DW-90 and the focus fix, `cargo xtask check` passed on 2026-10-12 with
946 tests. The new ones cover the palette's placement at any width, monitor
position and scale (`ui/palette_popup.rs`), a new entry forgetting the last
one while request IDs keep counting, Enter waiting through a model refresh
between its submission and the reply (`palette/choices.rs`), and the refusal
of non-window handles by the popup and focus helpers in `atlas-platform`.

## Files

- `crates/atlas-app/src/acceptance.rs` and `acceptance/{probe,report}.rs`: the
  plan, the present-time probe and the JSON report with p50/p95/max and
  verdicts (unit-tested).
- `crates/atlas-app/src/ui/palette_acceptance.rs`: the scripted run, with
  the keyboard-focus traces and the DW-90 checks.
- `crates/atlas-app/src/ui/palette_popup.rs`: DW-90's kept popup.
- `crates/atlas-app/src/session/synthetic.rs`: in-memory synthetic rows. In
  `session.rs`: `spawn_acceptance`, the `GrowSyntheticCatalogue` command and
  the `QueryTimed`/`SyntheticCatalogue` events; the worker test is in
  `worker_tests.rs`.
- `crates/atlas-app/src/palette.rs`: probe hooks in `query`, `refresh`,
  `receive` and `render`, plus the read-only accessors the run uses.
- `crates/atlas-app/src/ui.rs`: plumbing (`LaunchChecks`, the open-window
  stamp, title and query hooks).
- `crates/atlas-platform/src/acceptance.rs`: owner/topmost/z-order/foreground
  and keyboard-focus queries, the refresh interval with explicit errors, and
  input posted to Atlas's own windows only.
- `crates/atlas-platform/src/browser/overlay.rs`: showing, hiding and
  activating the kept popup.
- `crates/xtask/src/palette_acceptance.rs`: build, loopback synthetic pages,
  launch and summary.
