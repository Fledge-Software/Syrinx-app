"""Regenerates Syrinx's icon tiles, rasters, .ico files and favicon kit from the hawk logo.

Run from the repository root: `python design/brand/tools/regenerate.py`. It needs Microsoft
Edge for rasterizing; the outputs are committed, so building Syrinx needs neither Edge nor
Python. See `design/brand/README.md`.
"""

import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

import ico
import png

BRAND = pathlib.Path(__file__).resolve().parent.parent
ROOT = BRAND.parent.parent
LOGO = BRAND / "syrinx-logo.svg"
ICON_DIR = BRAND / "icon"
WEB_DIR = BRAND / "web"
# Edge gets a throwaway profile under target/, never the owner's.
WORK_DIR = ROOT / "target" / "brand-tools"
EDGE = pathlib.Path(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")

# The Graphite palette's ground (`crates/syrinx-ui/src/palette.rs`).
GROUND = "#141519"
ORANGE = (0xFF, 0x61, 0x2E)
TILE = 1024
CORNER_RADIUS = round(TILE * 0.22)
# The hawk's extent inside the logo's 1254-unit viewBox, measured from its paths.
HAWK_LEFT, HAWK_RIGHT, HAWK_TOP, HAWK_BOTTOM = 187, 1067, 280, 951

# Each variant is the share of the tile's width the hawk spans, and whether corners are round.
VARIANTS = {
    "syrinx-icon.svg": (0.70, True),
    # Small sizes give the hawk more of the tile so its head still reads at 16 px.
    "syrinx-icon-small.svg": (0.86, True),
    # Apple masks its own corners, so the touch icon is a full square.
    "syrinx-icon-square.svg": (0.64, False),
}
SMALL_LIMIT = 32
ICON_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256, 512]
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
FAVICON_ICO_SIZES = [16, 32, 48]
# Syrinx's own 30 px interface marks (Home's logo tile) draw the small variant at twice their size.
SMALL_EXTRA_SIZES = [64]


def main():
    # Start from an empty work folder so no earlier run's Edge profile is ever reused.
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    hawk = hawk_paths()
    write_tile_svgs(hawk)
    rasters = render_icon_sizes()
    pack_ico(ICON_DIR / "syrinx.ico", rasters, ICO_SIZES)
    write_web_kit(rasters)
    print("Brand assets regenerated.")


def hawk_paths():
    """The logo's path elements, copied verbatim so the tile can never drift from the logo."""
    paths = re.findall(r"<path [^>]*/>", LOGO.read_text(encoding="utf-8"))
    if len(paths) != 4:
        sys.exit(f"Expected the logo's 4 paths, found {len(paths)}; recheck the hawk's extent.")
    return paths


def write_tile_svgs(hawk):
    for name, (share, rounded) in VARIANTS.items():
        (BRAND / name).write_text(tile_svg(hawk, share, rounded), encoding="utf-8", newline="\n")


def tile_svg(hawk, share, rounded):
    # Scale the hawk to `share` of the tile's width and centre its bounding box.
    scale = share * TILE / (HAWK_RIGHT - HAWK_LEFT)
    centre_x = (HAWK_LEFT + HAWK_RIGHT) / 2
    centre_y = (HAWK_TOP + HAWK_BOTTOM) / 2
    radius = CORNER_RADIUS if rounded else 0
    transform = (
        f"translate({TILE / 2} {TILE / 2}) scale({scale:.6f}) translate({-centre_x} {-centre_y})"
    )
    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {TILE} {TILE}" '
        'role="img" aria-label="Syrinx">',
        "<title>Syrinx</title>",
        f'<rect width="{TILE}" height="{TILE}" rx="{radius}" fill="{GROUND}"/>',
        f'<g transform="{transform}">',
        *hawk,
        "</g>",
        "</svg>",
    ]
    return "\n".join(lines) + "\n"


def render_icon_sizes():
    """Renders every size, checks it, and returns {size: png path}."""
    ICON_DIR.mkdir(exist_ok=True)
    rasters = {}
    for size in ICON_SIZES:
        variant = "syrinx-icon-small.svg" if size <= SMALL_LIMIT else "syrinx-icon.svg"
        target = ICON_DIR / f"syrinx-icon-{size}.png"
        render(BRAND / variant, size, target)
        check_tile(target, size, rounded=True)
        rasters[size] = target
    render_small_extras()
    return rasters


def render_small_extras():
    # The small tile at sizes above SMALL_LIMIT, named apart so the normal icon set is unchanged.
    for size in SMALL_EXTRA_SIZES:
        target = ICON_DIR / f"syrinx-icon-small-{size}.png"
        render(BRAND / "syrinx-icon-small.svg", size, target)
        check_tile(target, size, rounded=True)


def render(svg, size, target):
    # Edge draws the SVG at exactly `size` CSS pixels on a transparent page at scale 1.
    WORK_DIR.mkdir(parents=True, exist_ok=True)
    wrapper = WORK_DIR / "render.html"
    wrapper.write_text(
        "<!doctype html><html><head><style>html,body{margin:0;background:transparent}"
        "img{display:block}</style></head><body>"
        f'<img src="{svg.as_uri()}" width="{size}" height="{size}" alt=""></body></html>',
        encoding="utf-8",
    )
    shot = WORK_DIR / "shot.png"
    shot.unlink(missing_ok=True)
    arguments = [
        str(EDGE),
        "--headless",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-sync",
        "--disable-background-networking",
        "--disable-component-update",
        # A new empty profile per shot: a reused one can hand off to a still-exiting Edge and hang.
        f"--user-data-dir={tempfile.mkdtemp(prefix='edge-profile-', dir=WORK_DIR)}",
        "--force-device-scale-factor=1",
        "--hide-scrollbars",
        "--default-background-color=00000000",
        f"--window-size={size},{size}",
        f"--screenshot={shot}",
        wrapper.as_uri(),
    ]
    result = subprocess.run(arguments, capture_output=True, text=True, timeout=60)
    if result.returncode != 0 or not shot.exists():
        sys.exit(f"Edge could not render {svg.name} at {size} px:\n{result.stderr}")
    # Re-encode so only the pixels, not Edge's chunks, decide the committed bytes.
    png.write(target, png.read(shot))


def check_tile(path, size, rounded):
    """Fails unless the raster is the right size, has the tile's corners and shows the hawk."""
    image = png.read(path)
    if (image.width, image.height) != (size, size):
        sys.exit(f"{path.name} is {image.width}x{image.height}, not {size}x{size}")
    corner_alpha = image.pixel(0, 0)[3]
    if rounded and corner_alpha > 96:
        sys.exit(f"{path.name}: the corner is opaque ({corner_alpha}); the tile is not rounded")
    if not rounded and corner_alpha != 255:
        sys.exit(f"{path.name}: the square tile has a transparent corner")
    if not any(near(pixel, ORANGE) for pixel in image.pixels):
        sys.exit(f"{path.name}: no pixel is the logo's orange")


def near(pixel, colour, tolerance=40):
    return pixel[3] == 255 and all(abs(a - b) <= tolerance for a, b in zip(pixel, colour))


def pack_ico(target, rasters, sizes):
    ico.pack(target, [(size, rasters[size].read_bytes()) for size in sizes])
    found = ico.sizes(target)
    if found != [(size, size) for size in sizes]:
        sys.exit(f"{target.name} holds {found}, not {sizes}")


def write_web_kit(rasters):
    # The website's favicons: an SVG, a small .ico, Apple's touch icon and manifest sizes.
    WEB_DIR.mkdir(exist_ok=True)
    shutil.copyfile(BRAND / "syrinx-icon-small.svg", WEB_DIR / "favicon.svg")
    pack_ico(WEB_DIR / "favicon.ico", rasters, FAVICON_ICO_SIZES)
    touch = WEB_DIR / "apple-touch-icon.png"
    render(BRAND / "syrinx-icon-square.svg", 180, touch)
    check_tile(touch, 180, rounded=False)
    manifest_192 = WEB_DIR / "icon-192.png"
    render(BRAND / "syrinx-icon.svg", 192, manifest_192)
    check_tile(manifest_192, 192, rounded=True)
    shutil.copyfile(rasters[512], WEB_DIR / "icon-512.png")


if __name__ == "__main__":
    main()
