//! Soft shadows and glass edges as plain data, so a palette can lift its surfaces off the card
//! (#94) and draw the canvas's light-edged glass (#93).
//!
//! Values are kept as the canvas writes CSS `box-shadow`, so they compare line by line;
//! [`ShadowLayer::gaussian_sigma`] converts the blur for a renderer.

use crate::color::Color;

/// The surfaces that carry a shadow, one per canvas `…Shadow` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadowRole {
    /// `rowOnShadow`: the selected sidebar or menu row.
    RowSelected,
    /// `fieldShadow`: the sidebar's address field.
    Field,
    /// `cardShadow`: the page card around Syrinx's own pages, such as Home.
    Card,
    /// `surfShadow`: plugin cards and other cards on Home.
    Surface,
    /// `pillShadow`: the home page's search pill.
    Pill,
    /// `panelShadow`: the Customize panel and other near-opaque popups.
    Panel,
}

impl ShadowRole {
    pub const ALL: [ShadowRole; 6] = [
        ShadowRole::RowSelected,
        ShadowRole::Field,
        ShadowRole::Card,
        ShadowRole::Surface,
        ShadowRole::Pill,
        ShadowRole::Panel,
    ];

    /// The canvas's glass edge on this role as (top highlight, inner ring) opacities at the
    /// canvas's own strength. From the `glass` tokens in `Main.dc.html`: `edge` is
    /// `inset 0 1px 0 rgba(255,255,255,0.24), inset 0 0 0 1px rgba(255,255,255,0.07)` on
    /// fields, selected rows, surfaces and the pill; the card has only a 0.18 highlight and
    /// the panel a 0.14 one.
    const fn canvas_edge(self) -> (f32, f32) {
        match self {
            ShadowRole::RowSelected
            | ShadowRole::Field
            | ShadowRole::Surface
            | ShadowRole::Pill => (0.24, 0.07),
            ShadowRole::Card => (0.18, 0.),
            ShadowRole::Panel => (0.14, 0.),
        }
    }
}

/// The canvas Glass palette's edge strength: its top highlight's opacity on a field.
pub const CANVAS_EDGE_STRENGTH: f32 = 0.24;

/// One CSS `box-shadow` layer. The canvas never offsets a shadow sideways, so there is no x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowLayer {
    pub offset_y: f32,
    /// The CSS blur radius, in pixels.
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
    /// Drawn inside the element's bounds, as Glass's top highlight is.
    pub inset: bool,
}

impl ShadowLayer {
    /// The Gaussian's standard deviation: CSS defines it as half the blur radius, and GPUI's
    /// `blur_radius` is that deviation, so passing the CSS value would blur twice as far.
    pub fn gaussian_sigma(&self) -> f32 {
        self.blur / 2.
    }

    /// How far an outer layer can reach past the element's edge; an inset layer reaches 0.
    pub fn reach(&self) -> f32 {
        if self.inset {
            return 0.;
        }
        self.offset_y.abs() + 3. * self.gaussian_sigma() + self.spread
    }
}

/// The layers one role draws: at most one drop shadow under it, and the glass edge's top
/// highlight and inner ring inside it. That is everything the canvas gives any role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    drop: Option<ShadowLayer>,
    highlight: Option<ShadowLayer>,
    ring: Option<ShadowLayer>,
}

impl Shadow {
    pub const NONE: Shadow = Shadow {
        drop: None,
        highlight: None,
        ring: None,
    };

    /// A single outer shadow: CSS `0 {offset_y}px {blur}px {color}`.
    pub const fn drop(offset_y: f32, blur: f32, color: Color) -> Shadow {
        let layer = ShadowLayer {
            offset_y,
            blur,
            spread: 0.,
            color,
            inset: false,
        };
        Shadow {
            drop: Some(layer),
            ..Shadow::NONE
        }
    }

    /// This shadow with a glass edge: CSS `inset 0 1px 0 {highlight}` and
    /// `inset 0 0 0 1px {ring}`. A fully transparent colour adds no layer.
    pub const fn with_edge(self, highlight: Color, ring: Color) -> Shadow {
        Shadow {
            highlight: inset_layer(1., 0., highlight),
            ring: inset_layer(0., 1., ring),
            ..self
        }
    }

    /// Every layer, bottom first: the drop, then the highlight, then the ring.
    pub fn layers(&self) -> impl Iterator<Item = ShadowLayer> + '_ {
        [self.drop, self.highlight, self.ring].into_iter().flatten()
    }

    /// The layer drawn under and around the element, if any.
    pub fn drop_layer(&self) -> Option<ShadowLayer> {
        self.drop
    }

    /// The layers drawn inside the element, over its fill.
    pub fn inset_layers(&self) -> impl Iterator<Item = ShadowLayer> + '_ {
        [self.highlight, self.ring].into_iter().flatten()
    }

    pub fn is_none(&self) -> bool {
        self.layers().next().is_none()
    }
}

const fn inset_layer(offset_y: f32, spread: f32, color: Color) -> Option<ShadowLayer> {
    if color.alpha == 0 {
        return None;
    }
    Some(ShadowLayer {
        offset_y,
        blur: 0.,
        spread,
        color,
        inset: true,
    })
}

/// Every shadow role of one palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadows {
    pub row_selected: Shadow,
    pub field: Shadow,
    pub card: Shadow,
    pub surface: Shadow,
    pub pill: Shadow,
    pub panel: Shadow,
}

impl Shadows {
    /// No shadows: Graphite's, Haze's and Tint's look.
    pub const NONE: Shadows = Shadows {
        row_selected: Shadow::NONE,
        field: Shadow::NONE,
        card: Shadow::NONE,
        surface: Shadow::NONE,
        pill: Shadow::NONE,
        panel: Shadow::NONE,
    };

    pub const fn role(&self, role: ShadowRole) -> Shadow {
        match role {
            ShadowRole::RowSelected => self.row_selected,
            ShadowRole::Field => self.field,
            ShadowRole::Card => self.card,
            ShadowRole::Surface => self.surface,
            ShadowRole::Pill => self.pill,
            ShadowRole::Panel => self.panel,
        }
    }

    fn role_mut(&mut self, role: ShadowRole) -> &mut Shadow {
        match role {
            ShadowRole::RowSelected => &mut self.row_selected,
            ShadowRole::Field => &mut self.field,
            ShadowRole::Card => &mut self.card,
            ShadowRole::Surface => &mut self.surface,
            ShadowRole::Pill => &mut self.pill,
            ShadowRole::Panel => &mut self.panel,
        }
    }

    /// These drop shadows with the canvas's glass edge on every role, scaled so `strength`
    /// is a field's top-highlight opacity (the canvas's Glass is 0.24; the editor's "Glass
    /// edges" reaches 0.40). On a light theme the inner ring is dark ink, because a white
    /// ring vanishes on white surfaces.
    pub fn with_glass_edges(mut self, strength: f32, light: bool) -> Shadows {
        let scale = strength.max(0.) / CANVAS_EDGE_STRENGTH;
        for role in ShadowRole::ALL {
            let (highlight, ring) = role.canvas_edge();
            let ring_color = if light {
                Color::rgba(20, 22, 30, ring * scale)
            } else {
                Color::rgba(255, 255, 255, ring * scale)
            };
            let shadow = self.role_mut(role);
            *shadow = shadow.with_edge(Color::rgba(255, 255, 255, highlight * scale), ring_color);
        }
        self
    }
}

/// A rectangle in an element's own coordinates, its origin at the element's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Band {
    fn new(left: f32, top: f32, right: f32, bottom: f32) -> Option<Band> {
        (right > left && bottom > top).then_some(Band {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// Rectangles that tile everything within `reach` of a `width` × `height` rounded rectangle
/// with corner `radius`, but outside it. A drop shadow clipped to them shows only outside
/// the element, as CSS draws it; GPUI paints a drop under the whole element, which darkens a
/// translucent glass fill from inside. Corners are stepped in one-pixel rows, each stopping
/// where the curve is widest in that row, so no band reaches into the shape.
pub fn bands_outside(width: f32, height: f32, radius: f32, reach: f32) -> Vec<Band> {
    let radius = radius.clamp(0., width.min(height) / 2.);
    let (left, right) = (-reach, width + reach);
    let mut bands = Vec::new();
    // Above and below the element, full width.
    bands.extend(Band::new(left, -reach, right, 0.));
    bands.extend(Band::new(left, height, right, height + reach));
    // Beside the straight part of each side.
    bands.extend(Band::new(left, radius, 0., height - radius));
    bands.extend(Band::new(width, radius, right, height - radius));
    // Beside each corner row, up to the curve.
    for row in 0..radius.ceil() as usize {
        let top = row as f32;
        let bottom = (top + 1.).min(radius);
        let inset = corner_inset(radius, radius - bottom);
        for (row_top, row_bottom) in [(top, bottom), (height - bottom, height - top)] {
            bands.extend(Band::new(left, row_top, inset, row_bottom));
            bands.extend(Band::new(width - inset, row_top, right, row_bottom));
        }
    }
    bands
}

/// How far in from the side a corner's curve lies, `rise` away from the corner's centre row.
fn corner_inset(radius: f32, rise: f32) -> f32 {
    radius - (radius * radius - rise * rise).max(0.).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_shadow_draws_no_layer() {
        assert!(Shadow::NONE.is_none());
        for role in ShadowRole::ALL {
            assert!(Shadows::NONE.role(role).is_none());
        }
    }

    #[test]
    fn a_drop_shadow_is_one_outer_layer() {
        let color = Color::rgba(20, 22, 30, 0.08);
        let layers: Vec<_> = Shadow::drop(8., 24., color).layers().collect();
        assert_eq!(
            layers,
            [ShadowLayer {
                offset_y: 8.,
                blur: 24.,
                spread: 0.,
                color,
                inset: false,
            }]
        );
    }

    #[test]
    fn the_css_blur_becomes_half_as_wide_a_gaussian() {
        let layer = Shadow::drop(30., 80., Color::rgba(20, 22, 30, 0.22))
            .layers()
            .next()
            .expect("a drop shadow has a layer");
        assert_eq!(layer.gaussian_sigma(), 40.);
        assert_eq!(layer.reach(), 150.);
    }

    #[test]
    fn each_role_reads_its_own_field() {
        let marked = |offset| Shadow::drop(offset, 0., Color::hex(0));
        let shadows = Shadows {
            row_selected: marked(1.),
            field: marked(2.),
            card: marked(3.),
            surface: marked(4.),
            pill: marked(5.),
            panel: marked(6.),
        };
        let offsets: Vec<f32> = ShadowRole::ALL
            .iter()
            .map(|role| {
                shadows
                    .role(*role)
                    .layers()
                    .next()
                    .map_or(0., |layer| layer.offset_y)
            })
            .collect();
        assert_eq!(offsets, [1., 2., 3., 4., 5., 6.]);
    }

    #[test]
    fn the_canvas_strength_reproduces_the_canvas_edge() {
        let shadows = Shadows::NONE.with_glass_edges(CANVAS_EDGE_STRENGTH, false);
        let white = |alpha| Color::rgba(255, 255, 255, alpha);
        let field: Vec<_> = shadows.field.inset_layers().collect();
        assert_eq!(field.len(), 2);
        assert_eq!((field[0].offset_y, field[0].spread), (1., 0.));
        assert_eq!(field[0].color, white(0.24));
        assert_eq!((field[1].offset_y, field[1].spread), (0., 1.));
        assert_eq!(field[1].color, white(0.07));
        let card: Vec<_> = shadows.card.inset_layers().collect();
        assert_eq!(
            card,
            [field[0]].map(|layer| ShadowLayer {
                color: white(0.18),
                ..layer
            })
        );
        let panel = shadows.panel.inset_layers().next().expect("highlight");
        assert_eq!(panel.color, white(0.14));
        for role in ShadowRole::ALL {
            assert!(
                shadows.role(role).layers().all(|layer| layer.inset),
                "{role:?}"
            );
        }
    }

    #[test]
    fn edges_scale_with_their_strength_and_vanish_at_zero() {
        assert_eq!(Shadows::NONE.with_glass_edges(0., false), Shadows::NONE);
        for strength in [0.12, 0.40] {
            let edged = Shadows::NONE.with_glass_edges(strength, false);
            let highlight = edged.pill.inset_layers().next().expect("highlight");
            assert_eq!(highlight.color, Color::rgba(255, 255, 255, strength));
        }
    }

    #[test]
    fn edges_keep_the_drop_beneath_them() {
        let drop = Shadow::drop(10., 28., Color::rgba(0, 0, 0, 0.18));
        let shadows = Shadows {
            surface: drop,
            ..Shadows::NONE
        }
        .with_glass_edges(CANVAS_EDGE_STRENGTH, false);
        let layers: Vec<_> = shadows.surface.layers().collect();
        assert_eq!(layers.len(), 3);
        assert_eq!(shadows.surface.drop_layer(), drop.drop_layer());
        assert!(!layers[0].inset && layers[1].inset && layers[2].inset);
    }

    #[test]
    fn a_light_theme_rings_its_glass_in_ink() {
        let ring = Shadows::NONE
            .with_glass_edges(CANVAS_EDGE_STRENGTH, true)
            .field
            .inset_layers()
            .nth(1)
            .expect("ring");
        assert_eq!(ring.color, Color::rgba(20, 22, 30, 0.07));
    }

    /// Whether a point lies inside a rounded rectangle whose top left is at the origin.
    fn inside(width: f32, height: f32, radius: f32, x: f32, y: f32) -> bool {
        let nearest_x = x.clamp(radius, width - radius);
        let nearest_y = y.clamp(radius, height - radius);
        let (dx, dy) = (x - nearest_x, y - nearest_y);
        (0. ..=width).contains(&x)
            && (0. ..=height).contains(&y)
            && dx * dx + dy * dy < radius * radius
    }

    fn covered(bands: &[Band], x: f32, y: f32) -> bool {
        bands.iter().any(|band| {
            x >= band.x && x <= band.x + band.width && y >= band.y && y <= band.y + band.height
        })
    }

    #[test]
    fn outside_bands_stay_out_of_the_shape_and_cover_around_it() {
        // The search pill, whose ends are fully round, and a plugin card.
        for (width, height, radius) in [(760., 54., 27.), (240., 92., 14.)] {
            let reach = 60.;
            let bands = bands_outside(width, height, radius, reach);
            for band in &bands {
                for step_x in 1..20 {
                    for step_y in 1..4 {
                        let x = band.x + band.width * step_x as f32 / 20.;
                        let y = band.y + band.height * step_y as f32 / 4.;
                        assert!(!inside(width, height, radius, x, y), "{band:?} at {x},{y}");
                    }
                }
            }
            // Every point more than a pixel outside the shape, within reach, is covered.
            let mut y = -reach + 0.5;
            while y < height + reach {
                let mut x = -reach + 0.5;
                while x < width + reach {
                    let near = inside(width + 2., height + 2., radius + 1., x + 1., y + 1.);
                    assert!(near || covered(&bands, x, y), "{width}x{height}: {x},{y}");
                    x += 3.;
                }
                y += 1.;
            }
        }
    }
}
