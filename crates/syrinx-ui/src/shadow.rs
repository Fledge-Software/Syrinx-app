//! Soft shadows as plain data, so a light palette can lift its white surfaces off the card (#94).
//!
//! Values are kept as the canvas writes CSS `box-shadow`, so they compare line by line;
//! [`ShadowLayer::gaussian_sigma`] converts the blur for a renderer.

use crate::color::Color;

/// The most layers one shadow role holds; the canvas uses at most three (Glass's edge).
pub const MAX_LAYERS: usize = 3;

/// The surfaces that carry a shadow, one per canvas `…Shadow` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShadowRole {
    /// `rowOnShadow`: the selected sidebar or menu row.
    RowSelected,
    /// `surfShadow`: plugin cards and other cards on Home.
    Surface,
    /// `pillShadow`: the home page's search pill.
    Pill,
    /// `panelShadow`: the Customize panel and other near-opaque popups.
    Panel,
}

impl ShadowRole {
    pub const ALL: [ShadowRole; 4] = [
        ShadowRole::RowSelected,
        ShadowRole::Surface,
        ShadowRole::Pill,
        ShadowRole::Panel,
    ];
}

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
}

/// The layers one role draws, bottom first; empty draws nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    layers: [Option<ShadowLayer>; MAX_LAYERS],
}

impl Shadow {
    pub const NONE: Shadow = Shadow {
        layers: [None; MAX_LAYERS],
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
            layers: [Some(layer), None, None],
        }
    }

    pub fn layers(&self) -> impl Iterator<Item = ShadowLayer> + '_ {
        self.layers.iter().flatten().copied()
    }

    pub fn is_none(&self) -> bool {
        self.layers().next().is_none()
    }
}

/// Every shadow role of one palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadows {
    pub row_selected: Shadow,
    pub surface: Shadow,
    pub pill: Shadow,
    pub panel: Shadow,
}

impl Shadows {
    /// No shadows: the dark palettes' current look.
    pub const NONE: Shadows = Shadows {
        row_selected: Shadow::NONE,
        surface: Shadow::NONE,
        pill: Shadow::NONE,
        panel: Shadow::NONE,
    };

    pub const fn role(&self, role: ShadowRole) -> Shadow {
        match role {
            ShadowRole::RowSelected => self.row_selected,
            ShadowRole::Surface => self.surface,
            ShadowRole::Pill => self.pill,
            ShadowRole::Panel => self.panel,
        }
    }
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
    }

    #[test]
    fn each_role_reads_its_own_field() {
        let marked = |offset| Shadow::drop(offset, 0., Color::hex(0));
        let shadows = Shadows {
            row_selected: marked(1.),
            surface: marked(2.),
            pill: marked(3.),
            panel: marked(4.),
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
        assert_eq!(offsets, [1., 2., 3., 4.]);
    }
}
