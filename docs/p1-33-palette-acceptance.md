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

**Short version.** The harness works and has run on this machine. Most of the
focus and stacking behaviour passes. Opening the palette takes about half a
second, about 30 frames against a budget of one, and the time is spent creating
the popup's native window. Composed ranking is under two frames at 1,000 and
10,000 rows. At 100,000 rows it is not: digit-heavy queries such as `row-04`
take up to 123 ms in the worker. D48 fails in the running app at 10,000 and
100,000 rows. Focus return after Esc could not be judged here, because Windows
refused to bring the agent-launched Atlas to the front. The owner's
foreground run decides it. Nothing slow has been hidden or excluded.

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
   the summary and exits non-zero if a stated budget or a functional check
   failed.

The app flag is not part of the default gate. Like `--smoke` and
`--test-suspended-tabs`, it always uses a freshly reserved synthetic profile
under `%TEMP%`, never `%LOCALAPPDATA%\AtlasProductPreview`. It refuses any page
base other than `http://127.0.0.1:<port>`, and it cannot be combined with
`--smoke`, `--test-data-dir`, the suspended-tab mode or a link. A 600-second
watchdog (exit code 124) bounds the whole run, as `--smoke`'s 30-second one
does. The run takes about two minutes after the build.

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
  event and the GPUI key binding call. The accelerator leg itself (WebView2's
  `AcceleratorKeyPressed` posting to Atlas's event loop) is not in the
  measurement; see "Not measured".
- **Typing, Backspace, Enter and Esc** are posted to the palette popup's own
  window handle as `WM_CHAR`, `WM_KEYDOWN` and `WM_KEYUP` messages
  (`crates/atlas-platform/src/acceptance.rs`). GPUI handles them through its
  normal message loop: `WM_KEYDOWN` goes through `translate_accelerator` to its
  key handler, and `WM_CHAR` goes to the focused input. Before posting,
  `post_text` and `post_key` check that the handle is a live window owned by
  this Atlas process, and refuse otherwise. A unit test proves the refusal for
  the current foreground window. Nothing uses `SendInput`/SendKeys, so no key
  can reach another application.

### How a frame is judged "presented"

`crates/atlas-app/src/acceptance/probe.rs` is a small shared timing record that
exists only in an acceptance run (`Option<PresentProbe>` is `None` otherwise;
the cost in normal runs is one `Option` check per palette render, query and
reply). The palette's `render` asks the probe whether any measurement is
waiting for a frame. If so, it schedules a `cx.defer` callback. GPUI runs
deferred callbacks when the update that drew the frame ends, and that update
also calls `Present`. The stamp is therefore taken **after `Present`
returned**. GPUI presents with sync interval 0 into a DirectComposition swap
chain, so the compositor shows the frame at the next display refresh. That
refresh is up to one more interval (16.7 ms here) and is not included. The
numbers are "submitted to the compositor", not "photons".

Window creation is a special case. `App::open_window` renders the new window
once without presenting it. The probe therefore arms the opening measurement
from `Window::on_next_frame`, registered right after the window is created,
and notifies the palette view so the first real frame renders and presents.
That frame re-renders the palette once more than a normal run would, so the
opening time may be slightly too high, never too low.

### Opening

The run measures 20 openings, each with the focus page focused. Each one runs
from just before the `Palette` action to the first presented frame of the new
popup, and is broken into stages:

- **window created**: GPUI's `Window::new` has returned, so the native popup,
  its DirectX resources and its swap chain exist, and the view is about to be
  built;
- **first render**: the unpresented render inside `open_window`;
- **presented**: the first frame presented.

**Budget:** within one refresh interval, judged at the 95th percentile with
the maximum and every sample reported. The interval comes from Windows
(`EnumDisplaySettingsW` on the monitor showing Atlas). If Windows cannot report
it, 60 Hz is assumed and the report says so. Using p95 follows D48's reasoning
about scheduling interruptions. The plan's text has no percentile, which is an
owner question below.

### Ranking

The session worker gained one acceptance-only command,
`GrowSyntheticCatalogue { total }` (`crates/atlas-app/src/session/synthetic.rs`).
It adds deterministic synthetic history rows to the **in-memory** tier-1
catalogue. They have titles like `harbor project guide row-004200`. Some carry
`café`, `東京`, `🧭` or a long fragment. Addresses are under `.test` or
`www.example.com`, and visit counts and recencies are spread over 90 days.
Nothing is written to the store; a worker test proves that the rows are gone
after a restart. The same command turns on per-query timing in the worker
(`SessionEvent::QueryTimed`), so the worker's own catalogue time can be
separated from queueing.

At 1,000, then 10,000, then 100,000 synthetic rows (plus the four real rows:
two tabs, two history entries), the palette is opened. Then `amber`,
`harbor guide`, `row-0420`, `café`, `naviga` and `zqxv` are typed one key at a
time and erased one Backspace at a time, twice: 156 keystrokes per size. The
run waits for each key's rows to be presented before the next key, then pauses
20 ms. Each key records:

- **worker**: `Catalogue::query` inside the session worker, covering ranking,
  scope counts and highlights. This is D48's quantity.
- **query round trip**: from the browser view queueing the query to the
  palette accepting the reply (both thread hops, plus `palette::arrange`).
- **key to echo frame**: the posted key to the first presented frame showing
  the changed text.
- **key to rows frame**: the posted key to the first presented frame showing
  that text's ranked rows. This is the **composed latency**.

Keys whose rows never appear within 3 s count as timeouts. Keys whose request
a model refresh replaced are counted as "superseded".

**Budgets:** D48's 5 ms at p95 is applied to the worker time at every size,
because the plan's exit criterion states it for keystroke ranking. Any timeout
fails the size. The plan states **no budget for the composed latency**. The
report therefore proposes one, p95 within two refresh intervals (33.3 ms at
60 Hz), and marks it "proposed, not approved"; it does not decide the run's
verdict. Two intervals is the smallest budget that allows for the frame wait:
a reply that lands just after a frame must wait for the next one.

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
a background shell, so Windows never let it into the foreground. They show
where time goes; they are not the owner's quiet-machine acceptance numbers.
The run is `target/p1-33/palette-acceptance-1791845563.json` (2026-10-12),
with a 60 Hz display (16.667 ms, read from Windows).

### Opening: fails, about 30 frames

| stage (from the Ctrl K action) | n | p50 ms | p95 ms | max ms |
|---|---|---|---|---|
| window created | 20 | 480.1 | 504.1 | 523.7 |
| first render | 20 | 482.0 | 506.1 | 530.2 |
| presented | 20 | 493.0 | 518.4 | 541.8 |

Almost all of the half second passes **before the popup's view is built**,
inside GPUI's `Window::new`. That call creates the Win32 window, its
DirectX/DirectComposition swap chain and its renderer. Building the palette
view and its first render take about 2 ms, and waiting for the first present
about 10 ms. Earlier runs on the same day agree: p50 512.6 ms, max 622.4 ms
in run `1791845322`, and 529.8 ms for a single opening in run `1791845202`.
P1-10 opens a new native popup for every Ctrl K and removes it on close
(`BrowserView::open_palette`, `PaletteView::finish`), so every opening pays
this cost. A hidden, pre-created popup that is shown and focused on Ctrl K
would avoid it. That changes P1-10's popup lifecycle, so it is recorded as
**DW-90** and an owner question below; nothing in P1-33 changes how the
palette opens.

Not established: which part of `Window::new` dominates. A profiler run is the
next step; GPUI's `profiler` feature is off in the pinned build.

### Ranking

| rows (catalogue) | stage | n | p50 ms | p95 ms | max ms | verdict |
|---|---|---|---|---|---|---|
| 1,000 (1,004) | worker | 156 | 0.24 | 1.01 | 1.69 | D48 **pass** |
| | round trip | 156 | 0.40 | 1.51 | 4.39 | |
| | key to echo frame | 156 | 5.83 | 25.33 | 60.82 | |
| | key to rows frame | 156 | 20.33 | 29.93 | 60.82 | proposed **pass** |
| 10,000 (10,004) | worker | 156 | 0.76 | 10.11 | 13.49 | D48 **FAIL** |
| | round trip | 156 | 0.93 | 10.96 | 13.97 | |
| | key to echo frame | 156 | 3.76 | 25.38 | 31.89 | |
| | key to rows frame | 156 | 21.38 | 30.82 | 36.51 | proposed **pass** |
| 100,000 (100,004) | worker | 156 | 5.99 | 47.91 | 123.38 | D48 **FAIL** |
| | round trip | 156 | 6.56 | 48.03 | 123.68 | |
| | key to echo frame | 156 | 3.93 | 25.96 | 29.63 | |
| | key to rows frame | 156 | 24.67 | 61.98 | 148.60 | proposed **FAIL** |

There were no timeouts and no superseded keys. Seeding took 3 ms, 40 ms and
274 ms.

**The slow case is the digit query.** Broken down by word, every 100,000-row
worker maximum except one is between 2.8 and 13.6 ms. Typing and erasing
`row-0420` reaches 123.4 ms (composed 148.6 ms), because every synthetic title
contains `row-` followed by six digits. At 10,000 rows the same word is the
only one above 1.5 ms (13.5 ms). This agrees with P1-05's open D48 failure
(`docs/p1-05-search.md`): the in-app measurement adds little to the worker
time (the round trip exceeds the worker time by under a millisecond at p95).
The composed latency is dominated by waiting for a frame plus the worker.

**Typing echo** (the changed text on screen) has a p95 of about 25 ms at every
size, about one and a half frames. Its p50 is 4 to 6 ms. It does not depend on
catalogue size, since the input renders before the reply arrives.

### Focus and stacking

| check | this run | note |
|---|---|---|
| `page_focus_baseline` | pass | Atlas was not in the foreground |
| `popup_owned_by_main_window` | pass | |
| `popup_not_topmost` | pass | |
| `popup_above_page` | pass | |
| `palette_takes_keyboard_focus` | pass | the popup was not foreground either |
| `escape_returns_page_focus` | **FAIL** 0/20 | the palette closed every time; the page did not report focus |
| `no_stale_refocus_after_escape` | **FAIL** | no palette reopened and no extra window; page focus stayed false |
| `rapid_open_then_escape` | **FAIL** | passed in both earlier runs |
| `no_stale_refocus_after_rapid_escape` | **FAIL** | passed in both earlier runs |
| `second_ctrl_k_reuses_popup` | pass | |
| `enter_activates_selected_tab` | pass | **failed in run `1791845322`**: the row was selected and the palette closed, but the target tab was not active after 5 s |
| `no_stale_refocus_after_enter` | pass | failed in that run with the same cause |

The focus failures are **not established as product failures**. Windows
denied Atlas foreground activation throughout: `foreground_window()` never
returned Atlas's main window or popup. When the active popup is destroyed in a
background process, Windows activates some other application's window. The
page's `document.hasFocus()` is then false even though Atlas moved WebView2
focus back (`focus_page`). That is consistent with the traps on foreground
rules. It also explains why the cases that never activated the popup
(rapid Esc) passed in two of three runs.

The Enter failure in run `1791845322` is unexplained. That run recorded no
active-tab diagnostics, so the run now reports which tab is active and the
status line. Both P1-30's model and this slice expect Enter on a tab row to
focus that tab (`activation_command`). The owner's foreground runs decide
whether it recurs. Until then it stays an open failure, not a fluke.

The checks that do not depend on activation pass: ownership, no topmost, z-order
above the page, palette reuse on a second Ctrl K, and no reopened palette or
leaked window after Esc, rapid Esc or Enter.

## Not measured, and what remains unverified

- **The accelerator leg.** With a page focused, Ctrl K travels through
  WebView2's `AcceleratorKeyPressed`, which posts to Atlas's event loop. That
  delivery is not timed. GPUI's own Ctrl K binding (chrome focused) is not
  timed either. Both precede the measured action.
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
   in `target\p1-33\`. Report the opening p95, the 100,000-row worker and
   composed p95s, and every failed check.
2. In particular: does `escape_returns_page_focus` pass 20/20 when Atlas is in
   the foreground? Does `enter_activates_selected_tab` pass every time?
3. By hand: open a synthetic local video page, press Ctrl K and type. The video
   keeps playing under the palette (D7), the palette does not stay on top
   when you Alt Tab to another app, and Esc puts the keys back into the page
   (type in a text field).
4. Notice how long Ctrl K takes to show the palette. Is half a second
   acceptable until DW-90 is decided?

## Owner questions (each with a recommendation)

1. **Composed ranking budget.** The plan states D48 for the search core only.
   *Recommendation:* approve "key to rows presented within two refresh
   intervals at the 95th percentile, maximum reported" (33.3 ms at 60 Hz) as
   the composed budget for P1-33.
2. **Percentile for one-frame opening.** "Within one frame" has no percentile
   in the plan. *Recommendation:* use p95 with every sample and the maximum
   reported, as D48 does, because single Windows scheduling stalls should not
   decide the verdict.
3. **DW-90: keep a hidden palette popup.** Opening costs about 0.5 s because
   every Ctrl K creates a new native window. *Recommendation:* approve a
   follow-up slice that creates the owned popup once (hidden, not topmost) and
   shows, focuses and hides it. That would put the opening within one or two
   frames. It changes P1-10's lifecycle, the stale-popup guards in `ui.rs` and
   the Home palette intent, so it needs its own review. Its result is checked
   with this harness.
4. **Digit-heavy queries at 100,000 rows** stay with P1-05's open D48 work.
   *Recommendation:* keep P1-33 open until P1-05's benchmark passes, then rerun
   this harness; do not loosen D48.

## Gate

`cargo xtask check` passed on 2026-10-12 with this slice's source: dependency
boundaries (including D9's typing-path scan, which the palette and session
hooks keep clean), deferred-work markers, packaging, formatting, Clippy with
warnings denied, 919 tests and the application smoke run. New tests cover
argument parsing, the loopback-only page base, the probe's stamp ordering,
percentiles, frame counting and verdicts, JSON escaping, synthetic rows, the
worker's timed and non-persisted synthetic catalogue, the input refusal for
other processes' windows and the page server's allowlist.

## Files

- `crates/atlas-app/src/acceptance.rs` and `acceptance/{probe,report}.rs`: the
  plan, the present-time probe and the JSON report with p50/p95/max and
  verdicts (unit-tested).
- `crates/atlas-app/src/ui/palette_acceptance.rs`: the scripted run.
- `crates/atlas-app/src/session/synthetic.rs`: in-memory synthetic rows; the
  `GrowSyntheticCatalogue` command and `QueryTimed`/`SyntheticCatalogue` events
  in `session.rs` (worker test in `worker_tests.rs`).
- `crates/atlas-app/src/palette.rs`: probe hooks in `query`, `refresh`,
  `receive` and `render`, and `selected_target`. `ui.rs`: plumbing
  (`LaunchChecks`, title and query hooks).
- `crates/atlas-platform/src/acceptance.rs`: owner/topmost/z-order/foreground
  queries, the refresh interval and input posted to Atlas's own windows only.
- `crates/xtask/src/palette_acceptance.rs`: build, loopback synthetic pages,
  launch and summary.
