# Syrinx brand

The owner chose the hawk emblem as Syrinx's logo on 2026-10-13
(`docs/decisions.md`). This folder holds the logo, the app-icon tile made from
it, the rasters Windows and the website use, and the tools that regenerate
them.

## The logo and the icon tile

- **`syrinx-logo.svg`** is the logo: an orange hawk with a cream head on a
  transparent background, in a 1254-unit square viewBox. It is the source for
  everything else here.
- **`syrinx-icon.svg`** is the app icon: the same hawk centred on a
  near-black rounded square. The tile is the Graphite palette's ground,
  `#141519`, with corners rounded to 22% of its side. The hawk spans 70% of the
  tile's width.
- **`syrinx-icon-small.svg`** is the same tile with the hawk at 86% of the
  width, used for 32 px and smaller so the head and wing still read at 16 px.
- **`syrinx-icon-square.svg`** is a full square with no rounded corners, used
  only for Apple's touch icon, because iOS rounds the corners itself.

The tile SVGs are generated: their hawk paths are copied from the logo, so
change the logo and regenerate rather than editing a tile by hand.

## Colours

| Colour | Hex | Where |
| --- | --- | --- |
| Hawk orange | `#FF612E` | The wing; also Syrinx's default accent |
| Hawk cream | `#FFFDF7` | The head |
| Tile ground | `#141519` | The icon tile; the Graphite palette's ground |

## Tile or transparent logo?

**Use the tile everywhere an icon appears**: the executable, the taskbar, the
Start menu, Installed apps, the installer and browser favicons. The cream
head disappears on light backgrounds, such as a light taskbar, the Paper
palette or a light browser tab, and the tile keeps it visible on all of them.

**Use the transparent logo only where the background is known to be dark**,
such as a dark hero section on the website or Syrinx's own Graphite chrome.

## The files

- `icon/syrinx-icon-<size>.png`: the tile at 16, 20, 24, 32, 40, 48, 64, 128,
  256 and 512 px, with a transparent background outside the rounded corners.
  Sizes up to 32 use the small variant.
- `icon/syrinx-icon-small-64.png`: the small variant at 64 px, for Syrinx's own 30 px
  logo tile on Home (twice its size, for sharp HiDPI). Made by `tools/regenerate.py`.
- `icon/syrinx.ico`: every size from 16 to 256 as PNG-compressed entries.
  `crates/syrinx-app/build.rs` compiles it into `syrinx.exe` as icon resource 1,
  which the window, taskbar, Start menu and Installed apps all show.
- `web/`: the future website's favicon kit: `favicon.svg` (the small tile),
  `favicon.ico` (16, 32 and 48 px), `apple-touch-icon.png` (180 px, full
  square), and `icon-192.png` and `icon-512.png` for a web app manifest.

## Regenerating

The rasters and `.ico` files are generated once and committed, so building
Syrinx needs neither a rasterizer nor Python. To regenerate them after the
logo changes, run this from the repository root on Windows:

```
python design/brand/tools/regenerate.py
```

It needs Python 3 (standard library only) and Microsoft Edge in its default
location. It rewrites the tile SVGs, renders each size with headless Edge
using a fresh profile under `target/brand-tools/` (never the owner's Edge
profile), checks each raster's size, corners and colours, and packs the
`.ico` files. The same inputs give byte-identical outputs, so a regeneration
that changes nothing shows no diff. Look at the new rasters before
committing them, especially the 16 px one.

- `tools/regenerate.py`: the steps above.
- `tools/png.py`: a small PNG reader and writer.
- `tools/ico.py`: packs PNGs into an `.ico` and reads its entries back.
