//! Syrinx's interface icons: Segoe MDL2 Assets glyphs on Windows, where the font ships with the
//! system, and Syrinx's own stroke SVGs (`assets/icons`) elsewhere.

use gpui::{ParentElement, Styled};

/// Every icon Syrinx's own interface draws; each control names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Back,
    Forward,
    Sidebar,
    Menu,
    More,
    Bookmarks,
    Passwords,
    Settings,
    Reload,
    Search,
    Downloads,
    History,
    Add,
    Remove,
    Mute,
    Pause,
    Resume,
    Cancel,
    Warning,
    Info,
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[cfg(not(windows))]
impl Icon {
    const ALL: [Icon; 24] = [
        Icon::Back,
        Icon::Forward,
        Icon::Sidebar,
        Icon::Menu,
        Icon::More,
        Icon::Bookmarks,
        Icon::Passwords,
        Icon::Settings,
        Icon::Reload,
        Icon::Search,
        Icon::Downloads,
        Icon::History,
        Icon::Add,
        Icon::Remove,
        Icon::Mute,
        Icon::Pause,
        Icon::Resume,
        Icon::Cancel,
        Icon::Warning,
        Icon::Info,
        Icon::Minimize,
        Icon::Maximize,
        Icon::Restore,
        Icon::Close,
    ];
}

/// Adds `icon` to `element`, `size` pixels tall, in the element's inherited text colour.
#[cfg(windows)]
pub(crate) fn with_icon<E: ParentElement + Styled>(element: E, icon: Icon, size: f32) -> E {
    element
        .font_family(segoe::FONT)
        .text_size(gpui::px(size))
        .child(icon.glyph())
}

/// Adds `icon` to `element`, `size` pixels tall, in the element's inherited text colour.
#[cfg(not(windows))]
pub(crate) fn with_icon<E: ParentElement + Styled>(element: E, icon: Icon, size: f32) -> E {
    element.child(svg::icon(icon, size))
}

/// Segoe MDL2 Assets, which ships with Windows, so no icon files are bundled there.
#[cfg(windows)]
mod segoe {
    use super::Icon;

    pub(super) const FONT: &str = "Segoe MDL2 Assets";

    impl Icon {
        /// The icon's code point in Segoe MDL2 Assets.
        pub(super) const fn glyph(self) -> &'static str {
            match self {
                Icon::Back => "\u{E76B}",
                Icon::Forward => "\u{E76C}",
                Icon::Sidebar => "\u{E89F}",
                Icon::Menu => "\u{E700}",
                Icon::More => "\u{E712}",
                Icon::Bookmarks => "\u{E734}",
                Icon::Passwords => "\u{E8D7}",
                Icon::Settings => "\u{E713}",
                Icon::Reload => "\u{E72C}",
                Icon::Search => "\u{E721}",
                Icon::Downloads => "\u{E896}",
                Icon::History => "\u{E81C}",
                Icon::Add => "\u{E710}",
                Icon::Remove => "\u{E738}",
                Icon::Mute => "\u{E74F}",
                Icon::Pause => "\u{E769}",
                Icon::Resume => "\u{E768}",
                Icon::Cancel => "\u{E711}",
                Icon::Warning => "\u{E7BA}",
                Icon::Info => "\u{E946}",
                Icon::Minimize => "\u{E921}",
                Icon::Maximize => "\u{E922}",
                Icon::Restore => "\u{E923}",
                Icon::Close => "\u{E8BB}",
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn caption_icons_keep_the_windows_caption_glyphs() {
            assert_eq!(Icon::Minimize.glyph(), "\u{E921}");
            assert_eq!(Icon::Maximize.glyph(), "\u{E922}");
            assert_eq!(Icon::Restore.glyph(), "\u{E923}");
            assert_eq!(Icon::Close.glyph(), "\u{E8BB}");
        }
    }
}

/// Syrinx's own icons, embedded in the executable and served to GPUI's SVG renderer.
#[cfg(not(windows))]
pub(crate) mod svg {
    use std::borrow::Cow;

    use gpui::{AssetSource, IntoElement, SharedString, Styled, TransformationMatrix, canvas, px};

    use super::Icon;

    impl Icon {
        /// The icon's asset path and its SVG source.
        fn file(self) -> (&'static str, &'static [u8]) {
            macro_rules! icon {
                ($name:literal) => {
                    (
                        concat!("icons/", $name, ".svg"),
                        include_bytes!(concat!("../assets/icons/", $name, ".svg")),
                    )
                };
            }
            match self {
                Icon::Back => icon!("back"),
                Icon::Forward => icon!("forward"),
                Icon::Sidebar => icon!("sidebar"),
                Icon::Menu => icon!("menu"),
                Icon::More => icon!("more"),
                Icon::Bookmarks => icon!("bookmarks"),
                Icon::Passwords => icon!("passwords"),
                Icon::Settings => icon!("settings"),
                Icon::Reload => icon!("reload"),
                Icon::Search => icon!("search"),
                Icon::Downloads => icon!("downloads"),
                Icon::History => icon!("history"),
                Icon::Add => icon!("add"),
                Icon::Remove => icon!("remove"),
                Icon::Mute => icon!("mute"),
                Icon::Pause => icon!("pause"),
                Icon::Resume => icon!("resume"),
                Icon::Cancel => icon!("cancel"),
                Icon::Warning => icon!("warning"),
                Icon::Info => icon!("info"),
                Icon::Minimize => icon!("minimize"),
                Icon::Maximize => icon!("maximize"),
                Icon::Restore => icon!("restore"),
                Icon::Close => icon!("close"),
            }
        }
    }

    /// Serves the embedded icons by their `icons/<name>.svg` path.
    pub(crate) struct Assets;

    impl AssetSource for Assets {
        fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
            Ok(Icon::ALL
                .into_iter()
                .map(Icon::file)
                .find(|(file, _)| *file == path)
                .map(|(_, source)| Cow::Borrowed(source)))
        }

        fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
            Ok(Icon::ALL
                .into_iter()
                .map(|icon| icon.file().0)
                .filter(|file| file.starts_with(path))
                .map(SharedString::from)
                .collect())
        }
    }

    /// The icon painted in the text colour it inherits, as a glyph would be, so a parent's
    /// colour and hover colour reach it; GPUI's `svg()` reads only its own colour.
    pub(super) fn icon(icon: Icon, size: f32) -> impl IntoElement {
        let path = SharedString::from(icon.file().0);
        canvas(
            |_, _, _| {},
            move |bounds, (), window, cx| {
                let color = window.text_style().color;
                let painted = window.paint_svg(
                    bounds,
                    path.clone(),
                    None,
                    TransformationMatrix::unit(),
                    color,
                    cx,
                );
                if let Err(error) = painted {
                    eprintln!("Cannot draw the {icon:?} icon: {error}");
                }
            },
        )
        .size(px(size))
        .flex_none()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_icon_is_served_from_its_own_svg() {
            for icon in Icon::ALL {
                let (path, source) = icon.file();
                let served = Assets.load(path).expect("loading never fails");
                assert_eq!(served.as_deref(), Some(source), "{icon:?}");
                assert!(source.starts_with(b"<svg "), "{icon:?}");
            }
            let listed = Assets.list("icons/").expect("listing never fails");
            assert_eq!(listed.len(), Icon::ALL.len(), "two icons share a file");
            assert_eq!(Assets.load("icons/unknown.svg").expect("no error"), None);
        }
    }
}
