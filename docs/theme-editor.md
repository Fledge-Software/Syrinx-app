# Theme editor

Status: **built (pull request #5), owner check pending**, from the owner's
decision of 2026-10-15 (`docs/decisions.md`).

## Why

The design shipped five fixed palettes. Graphite and Paper looked right
in the owner's window check of 2026-10-15. Tint, Haze and Glass did not,
and the owner judged that the design itself does not make transparency
look good. The theme lab (`syrinx --theme-lab`) showed what Windows can
draw: Mica, Mica Alt and Acrylic backdrops, three-stop gradients and
grain. Rather than choose new fixed palettes, the owner asked for an
editor:

- **Defaults.** Graphite with the orange accent, opaque, is the default.
  Graphite, Paper and Glass are the presets. Glass came back as a preset
  on 2026-10-21, when the owner asked for "the glass look" in the theme
  selection (#93): the canvas's Glass under Acrylic, with Transparency
  55 % (the canvas's `rgba(16,17,24,0.46)` tint, to the nearest step) and
  Glass edges 24 % (the canvas's edge). Choosing it draws the canvas look
  with nothing to edit; Reduce transparency turns it into Graphite with its
  edges.
- **Custom themes.** Everything else is a theme the owner builds and saves
  by name. Several can be saved.

## What a theme holds

A theme is a few choices; Syrinx works out every colour role from them.
This is the same way one accent colour already becomes a whole set of
accent colours.

| Choice | Values | Default for a new theme |
|---|---|---|
| Name | Up to 40 characters | "My theme" |
| Mode | Dark, Light | Dark |
| Background colour | Any `#RRGGBB` | Graphite's `#141519` |
| Background style | Solid, Gradient | Solid |
| Second colour | Any `#RRGGBB`, used by Gradient | `#2A1838` |
| Angle | 0–355°, turned 15° per press, used by Gradient | 155° |
| Material | None, Mica, Mica Alt, Acrylic | None |
| Transparency | 0–90 % in 5 % steps, used with a material | 50 % |
| Glass edges | 0–40 % in 4 % steps | 0 % |
| Grain | 0–15 % in 1 % steps | 0 % |
| Accent | As before: a preset or any `#RRGGBB`, shared by every theme | The orange `#FF612E` |

How the choices combine:

- A material replaces the fill. With Mica, Mica Alt or Acrylic chosen, the
  background colour is painted over it at the chosen transparency, so
  Fill and Grain are hidden. Grain is drawn into an opaque image, which
  would cover the material.
- Glass edges draw the canvas's glass edge (#93): an inset top highlight
  at the edge strength and a faint inner ring at 7/24 of it, on fields,
  selected rows, the Home card, plugin cards, the search pill and the
  Customize panel. At 24 % it is the canvas's Glass exactly. On a light
  theme the ring is dark ink.
- Under a material, Transparency thins the window's tint; a dark tint on
  Home's card (Haze's) thins with it, so the material shows through Home.
- Colours are worked out from a starting preset:
  - a light theme starts from Paper;
  - a dark opaque theme starts from Graphite;
  - a dark theme under Acrylic starts from the canvas's Glass: white frost
    surfaces, its drop shadows (drawn only outside a translucent surface),
    and its bright wallpaper stand-ins for the contrast check;
  - a dark theme under Mica or Mica Alt starts from the old Haze, whose
    surfaces are translucent.
  Its neutral colours then take the background's hue at the same
  lightness. A theme with Graphite's background and no material draws
  exactly Graphite.

Contrast exceptions: the canvas's Glass fails nine recorded contrast
pairs, among them muted text on the search pill and on the selected row
(about 3.2:1). Only the Glass preset keeps them, because it draws the
canvas exactly. A saved theme that starts from Glass (a dark theme under
Acrylic, including a saved "Glass") keeps none of them, so all its text
is lifted to 4.5:1 (#93 review). It does not take Haze's two either,
because one of those excuses muted text on the selected row. Where even
white cannot reach 4.5:1, as at 70 % Transparency over a bright wallpaper,
the editor reports it.

Rules that every theme keeps:

- **Readable text.** Every theme is checked against the 4.5:1 contrast
  rule (3:1 for marks), over the background's own colours or, for a
  see-through theme, over the canvas's stand-in wallpapers.
  - Text and icons that fail are lightened (dark themes) or darkened
    (light themes) two points at a time until they pass.
  - Pairs the starting preset already fails are the recorded design
    exceptions of [`docs/p1-07-design-tokens.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-07-design-tokens.md), and are left as they are.
  - Accent marks or tile letters that still fail are named in the editor;
    the theme is saved anyway.
- **Opaque pages and popups.** Web pages and the command palette stay
  opaque, as the design requires.
- **Reduce transparency.** It still turns any material off: a see-through
  theme then draws opaque over its own background colour.
- **The page card stays unpainted** (`docs/dev/trapstoknow.md`). Gradients and
  grain are drawn only around the card.

## Storage

Themes live in the existing `settings` table. Values hold at most 256
bytes, so no schema change is needed:

- `appearance.theme`: `graphite`, `paper`, `glass` or `custom:<slot>`.
- `appearance.custom.<slot>` for slots 1 to 12: one line such as
  `v1;name=…;mode=dark;bg=#141519;…`. An empty value marks a free slot,
  because the table has no delete.
- `appearance.accent` and `appearance.reduce_transparency` are unchanged.

Migration: the old `appearance.palette` key is still read when
`appearance.theme` is absent. `graphite` and `paper` map to their
presets, and since 2026-10-21 so does `glass`. `haze` and `tint` become a
saved custom theme with the same name, so a chosen look is not lost.
A theme named "Glass" that the earlier migration already saved is left
exactly as it is, and stays chosen if it was: the owner may have edited
it, so Syrinx never replaces or deletes it. It is listed under the saved
themes, beside the preset, as "Glass (saved)" (label and accessible name
only; the stored name is unchanged), and the owner can delete it if it is
no longer wanted. Any saved theme whose name matches a preset is shown
the same way.

## Customize

The Theme section replaces the palette list:

- One row each for Graphite and Paper, then the saved themes, then
  "New theme".
- Choosing a row applies it at once, as today.
- A custom theme's row has Edit. Edit opens the controls above, applied
  live to the real window as they change, with Duplicate, Delete and Done.
- Steppers (− value +) are used rather than sliders: they work from the
  keyboard and need no new component state.
- Colour fields take `#RRGGBB` and apply on Enter or Done; the other
  controls apply at once. Every change is saved as it is made.
- Duplicate copies a theme into a free slot; Delete removes it, and
  Graphite takes its place if it was shown.

## Where it lives

- `crates/syrinx-ui/src/theme.rs`: `ThemeSpec`, its stored form, the
  palette it derives, readable-text lifting and the light rims, with
  tests.
- `crates/syrinx-app/src/appearance.rs`: storage, slots and the
  carry-over of the old palette key, with tests.
- `crates/syrinx-app/src/ui/theme_editor.rs`: the Theme section and the
  editor.
- `crates/syrinx-app/src/ground_image.rs`: three-stop gradients and grain
  as an image, shared with the theme lab.
- `crates/syrinx-app/src/ui/page_card.rs`: the fill layer draws that image
  only around the page card.
- `crates/syrinx-platform/src/backdrop.rs`: Mica, Mica Alt and Acrylic
  through `DwmSetWindowAttribute`.

## Building it

1. `syrinx-ui`: `ThemeSpec`, its parser and writer, and `ThemeSpec::palette()`,
   which derives a `Palette`. Tests cover contrast and round-tripping, and
   check that Graphite and Paper are unchanged.
2. `syrinx-app`: storage, migration and the Customize editor.
3. `syrinx-app` and `syrinx-platform`: the real window's material (Acrylic
   through `syrinx_platform::backdrop`), three-stop gradients and grain
   (an image generated as in the theme lab), and glass edges on the
   chrome's surfaces.
4. Measure Acrylic's drag and resize speed, as `design/README.md` asks.
   **Not done yet**: it needs the owner's own drag on their own screen.
