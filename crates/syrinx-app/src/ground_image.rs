//! Draws a window ground GPUI cannot: a three-stop gradient, grain, or both, as an image.
//! GPUI's own gradients take two stops (`docs/theme-editor.md`).

use std::cell::RefCell;
use std::sync::Arc;

use gpui::{App, Image, ImageFormat, Window};
use syrinx_ui::{Color, Ground};

/// Size of the generated ground; it is stretched to the window, close to 1:1 at full screen.
const GROUND_WIDTH: usize = 1920;
const GROUND_HEIGHT: usize = 1200;

/// What the last image was made from, so a redraw reuses it instead of generating again.
#[derive(Clone, Copy, PartialEq)]
struct Recipe {
    stops: [(Color, f32); 3],
    angle_degrees: f32,
    grain: f32,
}

thread_local! {
    // GPUI draws every Syrinx window on one thread.
    static LAST: RefCell<Option<(Recipe, Arc<Image>)>> = const { RefCell::new(None) };
}

/// The image for `ground` with `grain`, or `None` when GPUI can draw it as a plain fill.
/// A see-through ground keeps no grain, since an image would cover the material.
///
/// Only the latest image is kept: when the recipe changes, the previous one's decoded copy
/// and texture are released, or every step of the editor would keep about 9 MB.
pub(crate) fn ground_image(
    ground: &Ground,
    grain: f32,
    window: &mut Window,
    cx: &mut App,
) -> Option<Arc<Image>> {
    let recipe = recipe(ground, grain)?;
    let (image, replaced) = LAST.with_borrow_mut(|last| {
        if let Some((_, image)) = last.as_ref().filter(|(made, _)| *made == recipe) {
            return (image.clone(), None);
        }
        let image = gradient_image(recipe.stops, recipe.angle_degrees, recipe.grain);
        let replaced = last.replace((recipe, image.clone())).map(|(_, old)| old);
        (image, replaced)
    });
    if let Some(old) = replaced {
        release(old, window, cx);
    }
    Some(image)
}

/// Free a replaced image's texture in every window and its decoded copy.
fn release(old: Arc<Image>, window: &mut Window, cx: &mut App) {
    if let Some(decoded) = old.clone().get_render_image(window, cx) {
        cx.drop_image(decoded, Some(window));
    }
    old.remove_asset(cx);
}

/// What `ground` and `grain` need drawn, or `None` for a plain fill.
fn recipe(ground: &Ground, grain: f32) -> Option<Recipe> {
    let recipe = match *ground {
        Ground::Gradient {
            angle_degrees,
            stops,
        } => Recipe {
            stops,
            angle_degrees,
            grain,
        },
        Ground::Solid(color) if grain > 0. => Recipe {
            stops: [(color, 0.), (color, 0.5), (color, 1.)],
            angle_degrees: 180.,
            grain,
        },
        Ground::Solid(_) | Ground::Material { .. } => return None,
    };
    Some(recipe)
}

/// A three-stop CSS-style gradient with grey grain, as a 24-bit BMP GPUI can draw.
pub(crate) fn gradient_image(
    stops: [(Color, f32); 3],
    angle_degrees: f32,
    grain: f32,
) -> Arc<Image> {
    let (width, height) = (GROUND_WIDTH, GROUND_HEIGHT);
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
    let line = GradientLine::new(width, height, angle_degrees);
    let mut pixels = Vec::with_capacity(width * height * 3);
    // BMP rows run bottom to top, blue first.
    for row in (0..height).rev() {
        for column in 0..width {
            let t = line.position(column, row);
            let [red, green, blue] = mix_stops(&stops, t);
            let speck = noise.next() * 255.;
            let grained = |channel: f32| (channel * (1. - grain) + speck * grain).round() as u8;
            pixels.extend_from_slice(&[grained(blue), grained(green), grained(red)]);
        }
    }
    Arc::new(Image::from_bytes(
        ImageFormat::Bmp,
        bmp(width, height, &pixels),
    ))
}

/// A CSS `linear-gradient(<angle>deg)` line across an image, worked out once per image.
struct GradientLine {
    dx: f32,
    dy: f32,
    length: f32,
    half_width: f32,
    half_height: f32,
}

impl GradientLine {
    fn new(width: usize, height: usize, angle: f32) -> Self {
        let radians = angle.to_radians();
        let (dx, dy) = (radians.sin(), -radians.cos());
        let (w, h) = (width as f32, height as f32);
        Self {
            dx,
            dy,
            length: (w * dx).abs() + (h * dy).abs(),
            half_width: w / 2.,
            half_height: h / 2.,
        }
    }

    /// Where a pixel falls along the line, from 0 to 1.
    fn position(&self, column: usize, row: usize) -> f32 {
        let x = column as f32 + 0.5 - self.half_width;
        let y = row as f32 + 0.5 - self.half_height;
        ((x * self.dx + y * self.dy) / self.length + 0.5).clamp(0., 1.)
    }
}

fn mix_stops(stops: &[(Color, f32); 3], t: f32) -> [f32; 3] {
    let channels = |color: Color| [color.red, color.green, color.blue].map(f32::from);
    let pair = if t <= stops[1].1 {
        (stops[0], stops[1])
    } else {
        (stops[1], stops[2])
    };
    let ((from, from_at), (to, to_at)) = pair;
    let span = (to_at - from_at).max(f32::EPSILON);
    let share = ((t - from_at) / span).clamp(0., 1.);
    let (from, to) = (channels(from), channels(to));
    [0, 1, 2].map(|index| from[index] + (to[index] - from[index]) * share)
}

/// A minimal uncompressed 24-bit BMP; `pixels` holds rows bottom to top, three bytes each.
fn bmp(width: usize, height: usize, pixels: &[u8]) -> Vec<u8> {
    let row_bytes = width * 3;
    let padding = (4 - row_bytes % 4) % 4;
    let image_bytes = (row_bytes + padding) * height;
    let mut file = Vec::with_capacity(54 + image_bytes);
    let u32_le = |value: usize| (value as u32).to_le_bytes();
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&u32_le(54 + image_bytes));
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&u32_le(54));
    file.extend_from_slice(&u32_le(40));
    file.extend_from_slice(&u32_le(width));
    file.extend_from_slice(&u32_le(height));
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&24u16.to_le_bytes());
    file.extend_from_slice(&[0; 24]);
    for row in pixels.chunks(row_bytes) {
        file.extend_from_slice(row);
        file.extend_from_slice(&[0; 3][..padding]);
    }
    file
}

/// A small deterministic noise source, so the grain looks the same on every run.
struct Noise(u64);

impl Noise {
    /// The next value in 0..1 (xorshift64*).
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        value as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gradient_runs_from_its_first_stop_to_its_last() {
        // At 155° the top-left corner is the start and the bottom-right the end.
        let line = GradientLine::new(100, 60, 155.);
        assert!(line.position(0, 0) < 0.01);
        assert!(line.position(99, 59) > 0.99);
        let black = Color::rgba(0, 0, 0, 1.);
        let grey = Color::rgba(100, 100, 100, 1.);
        let stops = [(black, 0.), (grey, 0.5), (black, 1.)];
        assert_eq!(mix_stops(&stops, 0.5), [100., 100., 100.]);
        assert_eq!(mix_stops(&stops, 0.25), [50., 50., 50.]);
    }

    #[test]
    fn the_bitmap_header_matches_its_rows() {
        let file = bmp(3, 2, &[7; 18]);
        // Each 9-byte row is padded to 12, after the 54-byte header.
        assert_eq!(file.len(), 54 + 24);
        assert_eq!(&file[..2], b"BM");
        assert_eq!(u32::from_le_bytes(file[2..6].try_into().expect("size")), 78);
    }

    #[test]
    fn a_plain_ground_needs_no_image() {
        let solid = Ground::Solid(Color::hex(0x141519));
        assert!(recipe(&solid, 0.).is_none());
        assert!(recipe(&solid, 0.05).is_some());
    }
}
