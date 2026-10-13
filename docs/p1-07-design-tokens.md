# P1-07 design tokens: what was built and what the owner reviews

Plan 1's slice P1-07 exists so that every later UI slice draws its colours
and sizes from one place instead of typing hex values into each view. Before
it, `crates/syrinx-app/src/ui.rs` and `palette.rs` hard-coded one dark look,
which made the other four base palettes in `design/README.md`, the
user-chosen accent and the "Reduce transparency" fallback impossible to
build without touching every view. This note records what the slice
delivers, the contrast results it found and the decisions it leaves to the
owner. The authority for every value is the design canvas snapshot in
`design/canvas/project/` (S10), amended by D43–D45.

## What the slice delivers

`crates/syrinx-ui` is a new crate with no dependencies at all, not even GPUI.
Tokens are plain data and small functions, so they are tested without a
window and any interface code can read them. `syrinx-app` converts a token
colour to a GPUI colour in `crates/syrinx-app/src/theme.rs`. The dependency
rule in `crates/xtask/src/dependency_policy.rs` would have allowed
`syrinx-ui` to depend on GPUI; keeping it free of GPUI was chosen because it
keeps the token tests fast and nothing in the tokens needs a GPUI type.

- `palette.rs`: the five base palettes (Graphite, Haze, Glass, Tint, Paper),
  transcribed field by field from the canvas's shared `tokens()` method (the
  same method appears in `Sidebar`, `Main`, `Palette` and `Browsing`; all four
  copies were checked to be identical). Each field's doc comment names the
  canvas key. Tint is computed from the accent's hue, exactly as the canvas
  does, so it is built by a function rather than stored as constants.
  Haze and Glass describe their ground as an OS material (Mica, Acrylic) with
  Syrinx's translucent tint over it, plus the canvas's stand-in wallpaper
  colours, which are used only for contrast checks and samples.
- `accent.rs`: the default accent `#FF8A65`, the four Customize presets from
  the canvas, and the derived states (`soft`, `soft_line`, `ring`, the badge
  text, and Paper's darker `ink`).
- `PaletteId::resolve(reduce_transparency)`: Reduce transparency turns Haze
  and Glass into Graphite, as `design/README.md` says.
- `metrics.rs`: spacing, radii, type sizes and weights, using the values the
  canvas repeats. The canvas loads JetBrains Mono from the web; Syrinx makes no
  network requests of its own, so the monospace hint font is Cascadia Mono
  with Consolas as fallback.
- `sidebar.rs`: the D44 width states and drag thresholds, all marked
  PROPOSED (see below).
- `contrast.rs`: a WCAG 2.x audit of every palette with every accent preset.
- Shadows are not tokens yet; they stay in the canvas until a slice draws
  them.

`syrinx-app` now takes its existing colours from Graphite. Behaviour is
unchanged: a unit test in `theme.rs` proves each token reproduces the exact
hex value it replaced. Two preview colours are not in the design and were
kept as named, unchanged values for the sidebar slices to replace:

- the opaque selected-row grey `#343037` (the canvas uses translucent
  `row_selected`, white at 9% on Graphite), and
- the palette's "Search the web" button grey `#29272A` (removed by P1-30,
  whose palette draws only from tokens; see `docs/p1-30-31-palette.md`).

`crates/syrinx-app/src/ui/recovery.rs` still has its three hex values
(including the error text `#F2B5B5`, which is not in the design). It was left
alone because another slice is changing `ui/` modules in parallel; it should
move to the tokens once that work lands. `ui.rs`'s `SIDEBAR_WIDTH` likewise
stays until P1-08 adopts `syrinx_ui::sidebar::WIDE_WIDTH`.

## Contrast results

`design/README.md` sets text contrast at 4.5:1 or more in every palette.
The audit measures:

- five text roles (`text`, `text_strong`, `text_secondary`,
  `text_secondary_bright`, `text_muted`) on every surface text is drawn on:
  the ground, the selected row, the address field, the page card, a surface
  and the search pill on the card, the palette panel, its preview side and
  its input; minimum 4.5:1;
- icons on the ground, card and panel, and the accent ink as a selection mark;
  minimum 3:1 (WCAG 1.4.11, non-text);
- the accent ink as text: the palette draws matched characters in it
  (`design/canvas/project/Palette.dc.html`, the bold `mark` spans), so it is
  also checked on the palette panel and on the selected palette row (the
  accent wash over the panel); minimum 4.5:1;
- each tile and plugin monogram on its own tile, and badge text on the accent;
  minimum 4.5:1.

Every palette is audited with each of the four accent presets, and each
result is labelled `palette/accent/foreground/background` so failures that
depend on the accent can be told apart. Translucent layers, including
Glass's translucent tiles, are blended over every colour the ground can
present and the worst result is kept. For Haze and Glass that means the canvas's stand-in
wallpaper colours, whose most saturated centres are a harsher test than an
average wallpaper. Disabled controls are exempt under WCAG and are not checked.

The canvas values miss the minimum in these pairs. They are reported, not
retuned: changing a palette value is a design decision for the owner.
Unless the table says otherwise, a pair fails with every accent preset and
the ratio is the same for each. The test's list therefore has 55 entries,
one per palette, accent and pair.

| Palette | Foreground on background | Ratio | Minimum |
|---|---|---|---|
| Graphite | muted text on the selected row | 4.38 | 4.5 |
| Haze | muted text on the selected row | 3.97 | 4.5 |
| Haze | muted text on the address field | 4.48 | 4.5 |
| Glass | muted text on the ground | 4.45 | 4.5 |
| Glass | muted text on the selected row | 2.93 | 4.5 |
| Glass | muted text on the address field | 3.42 | 4.5 |
| Glass | muted text on the page card | 3.87 | 4.5 |
| Glass | muted text on a surface | 3.21 | 4.5 |
| Glass | muted text on the search pill | 3.02 | 4.5 |
| Glass | secondary text on the selected row | 4.16 | 4.5 |
| Glass | secondary text on the search pill | 4.28 | 4.5 |
| Glass | bright secondary text on the selected row | 4.39 | 4.5 |
| Glass | accent ink on the page card (blue preset `#6EA8FF` only) | 2.94 | 3.0 |
| Tint | muted text on the selected row (green and gold presets only) | 4.33–4.36 | 4.5 |
| Paper | accent ink as matched text on the palette panel (green `#6FD3A8` and gold `#E8C16A` only) | 4.17, 4.21 | 4.5 |
| Paper | accent ink as matched text on the selected palette row (green and gold only) | 3.85, 3.91 | 4.5 |

Paper's darker ink, `hsl(h, min(85, s)%, 36%)`, is still too light for the
green and gold hues at 36% lightness; the default and blue accents pass. Every
monogram tile (Glass's included, over every stand-in wallpaper) and every
badge passes. The muted grey
on a selected row is the pattern that recurs: the canvas shows a plugin's open
item ("Forge · fledge-core") in muted text on the selected row. Glass's
failures come from its brightest stand-in wallpaper disc (`#C4623B`), which
`design/README.md` already anticipates ("A bright wallpaper can still fail").

The test `contrast_failures_match_the_recorded_owner_review_list` in
`crates/syrinx-ui/src/contrast.rs` holds exactly this list. It fails if a new
pair falls below its minimum or a recorded one starts passing, so the list
cannot drift silently. The palette sample window shows the same failures
per palette.

## How the owner reviews the slice

Run from the repository root on this machine:

```powershell
cargo run --package syrinx-app --profile local --offline --locked -- --palette-sample
```

The window opens no profile, page or worker; closing it exits. Look for:

1. **Five columns, one per palette,** each drawn on its ground. Haze and
   Glass show their tint over the first canvas stand-in wallpaper colour,
   because the sample cannot show the real Mica or Acrylic. Tint shows its
   first gradient stop. Compare each column with the canvas boards
   "Base palettes" (`Theme*Home`, `Theme*Web`).
2. In each column: a section label and two sidebar rows (the second selected,
   with a muted subtitle and an accent badge), icons and a disabled label, a
   page card with the search pill and a surface, the palette panel with its
   input and an accent-marked default row, eight monogram tiles, and that
   palette's contrast failures.
3. **The Reduce transparency line,** which should read Haze → Graphite and
   Glass → Graphite and leave the others unchanged.
4. **Four miniature windows at true pixel widths:** wide, favicon-only and
   hidden sidebars beside a page card, and a hidden sidebar peeking over the
   page without moving it.

Then decide:

- **The contrast list above.** Either accept these pairs, or say which colours
  should change (most likely the muted grey on a selected row in every dark
  palette, and Glass's muted grey generally). The fix belongs in the design
  canvas first, then in `palette.rs`.
- **The D44 values, all PROPOSED in `crates/syrinx-ui/src/sidebar.rs`:**
  - wide width **264 px** (from the spec; settled);
  - favicon-only width **52 px**: the canvas sidebar's 12 px left and 10 px
    right padding around one 30 px icon button;
  - hidden **0 px**; the peek overlays at the full **264 px**;
  - while dragging, the sidebar follows the pointer between 0 and 264 px;
  - releasing at **158 px or wider** settles on wide (halfway between
    favicon-only and wide), and labels are drawn exactly when a release would
    settle on wide;
  - releasing at **26 px or wider** (half the favicon-only width) settles on
    favicon-only; anything narrower hides the sidebar;
  - a hidden sidebar peeks when the pointer is within **8 px** of the
    window's left edge (the page card's inset).
  The alternative considered was fixed pixel breakpoints unrelated to the
  widths; halfway points were chosen because they make the drag feel
  symmetric in both directions and follow automatically if a width changes.
- **The two preview greys** (`#343037`, `#29272A`) and the recovery error
  colour (`#F2B5B5`) are not in the design. The recommendation is to replace
  them with `row_selected`, `field` and a new canvas error colour when their
  slices are next touched.

## Verification

On 2026-10-11 the full local gate, `cargo xtask check`, passed on branch
`claude/p1-07-design-tokens` (dependency boundaries, formatting, Clippy with
warnings denied, every workspace test including syrinx-ui's 27 and the
application smoke run); the log is `target/p1-07-gate.log`. No Application
Control (os error 4551) block occurred. A normal launch of
`--palette-sample` opened a window titled "Syrinx · design token review
(P1-07)" and stayed running until it was closed. The agent did not inspect
the window's pixels, so the visual review above is still the owner's.
`Cargo.lock` changed only by adding the dependency-free `syrinx-ui` package.
