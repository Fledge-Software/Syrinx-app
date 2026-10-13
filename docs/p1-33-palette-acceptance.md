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
with Atlas launched by an agent and never allowed into the foreground. The
owner's quiet foreground runs are the acceptance numbers. On that basis:

- The harness works and has run four times.
- **Opening fails:** about half a second, roughly 30 frames against one.
  Nearly all of it falls between the call into GPUI's `open_window` and the
  moment the new window exists, which points at GPUI's window creation. That
  attribution stays provisional until the owner's foreground run. DW-90 is the
  approved follow-up.
- **Composed ranking passes at 1,000 and 10,000 rows but fails at 100,000:**
  key to presented rows has a p95 of about 58 ms against the owner's 33.3 ms.
  Digit-heavy queries (`row-0420`) take up to about 106 to 123 ms in the worker.
- **D48 fails in the running app** at 10,000 and 100,000 rows; that work
  stays with P1-05.
- **Overlay ownership, not-topmost, z-order and popup reuse pass.**
- **Not decided here:** focus return after Esc, because Windows kept the
  agent-launched Atlas out of the foreground. Enter activation also failed in
  two of three complete runs and remains unexplained.

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
`--test-data-dir`, the suspended-tab mode or a link. A 600-second watchdog
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

Window creation is a special case. `App::open_window` renders the new window
once without presenting it. The probe therefore arms the opening measurement
from `Window::on_next_frame`, registered right after the window is created,
and notifies the palette view so the first real frame renders and presents.
That frame re-renders the palette once more than a normal run would, so the
opening time may be slightly too high, never too low.

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
from just before the `Palette` action to the first presented frame of the new
popup, and is broken into stages:

- **open_window called**: `open_palette` has finished its own preparation
  (closing a peek, building the request and computing the popup's bounds) and
  is about to call GPUI's `open_window`;
- **window created**: inside `open_window`, the native popup, its
  DirectX/DirectComposition resources and its swap chain exist, and the
  palette view is about to be built;
- **first render**: the unpresented render inside `open_window`;
- **presented**: the first frame presented.

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
| `no_stale_refocus_after_escape` | one second later: no palette, one window, page still focused, same tab |
| `rapid_open_then_escape` | Esc posted in the same turn as Ctrl K, before the popup's first frame, still closes it and returns focus |
| `no_stale_refocus_after_rapid_escape` | as above, after the rapid case |
| `second_ctrl_k_reuses_popup` | Ctrl K while open keeps the same popup and two windows in total |
| `enter_activates_selected_tab` | typing the background tab's name selects its tab row; Enter switches to it and its page reports focus |
| `no_stale_refocus_after_enter` | one second later: no palette, target tab still active and focused |

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
and show and hide it. That is a separate slice after this one merges; P1-33
does not change how the palette opens.

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

**Focus return is not established either way.** Windows denied Atlas
foreground activation throughout: `foreground_window()` never returned Atlas's
main window or popup. When the active popup is destroyed in a background
process, Windows activates some other application's window. The page's
`document.hasFocus()` is then false even though Atlas moved WebView2 focus
back (`focus_page`). That is consistent with the traps on foreground rules.

**Enter activation is an open failure.** In the latest run the target's row
was selected, Enter was posted and the palette closed, but the focus page
stayed active. The status line read "The page could not be loaded. Try
reloading." The catalogue held one more real row than in earlier runs (1,005
rather than 1,004). This run followed the merge of P1-15's hidden-tab
suspension follow-up, and the target tab sits in the background. A connection
between the two is not established. Both P1-30's model and this slice expect
Enter on a tab row to focus that tab (`activation_command`). The owner's
foreground runs, and a diagnosis if it recurs, decide it.

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
- **Focus return with Atlas in the foreground.** This needs the owner's run.
- **Liveness under the palette (D7: no freeze frame).** A playing video should
  keep moving under the open palette. This is visual, so it is an owner check.
- **The owner's real catalogue shape.** Synthetic titles are deliberately
  repetitive (`row-NNNNNN`), which is the worst case for digit queries. Real
  history has fewer shared digit runs.
- **Wide and narrow palettes.** The run uses the window's default size (wide,
  with the preview pane).
- **No UIA rerun.** P1-33 changes no accessibility role, element id or focus
  handle, so P1-36's UI Automation probe was not repeated.

## Owner checks

1. Run `cargo xtask palette-acceptance` three times in the foreground on a quiet
   machine, without touching the keyboard or mouse, and keep the three reports
   in `target\p1-33\`. Report the opening p95 and its stages, the 100,000-row
   worker and composed p95s, the burst figures and every failed check.
2. In particular: does `escape_returns_page_focus` pass 20/20 when Atlas is in
   the foreground? Does `enter_activates_selected_tab` pass every time? Does the
   opening still spend its time between `open_window called` and
   `window created`?
3. By hand: open a synthetic local video page, press Ctrl K and type. The video
   keeps playing under the palette (D7), the palette does not stay on top
   when you Alt Tab to another app, and Esc puts the keys back into the page
   (type in a text field).

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

The run passed 937 tests.

## Files

- `crates/atlas-app/src/acceptance.rs` and `acceptance/{probe,report}.rs`: the
  plan, the present-time probe and the JSON report with p50/p95/max and
  verdicts (unit-tested).
- `crates/atlas-app/src/ui/palette_acceptance.rs`: the scripted run.
- `crates/atlas-app/src/session/synthetic.rs`: in-memory synthetic rows. In
  `session.rs`: `spawn_acceptance`, the `GrowSyntheticCatalogue` command and
  the `QueryTimed`/`SyntheticCatalogue` events; the worker test is in
  `worker_tests.rs`.
- `crates/atlas-app/src/palette.rs`: probe hooks in `query`, `refresh`,
  `receive` and `render`, plus the read-only accessors the run uses.
- `crates/atlas-app/src/ui.rs`: plumbing (`LaunchChecks`, the open-window
  stamp, title and query hooks).
- `crates/atlas-platform/src/acceptance.rs`: owner/topmost/z-order/foreground
  queries, the refresh interval with explicit errors, and input posted to
  Atlas's own windows only.
- `crates/xtask/src/palette_acceptance.rs`: build, loopback synthetic pages,
  launch and summary.
