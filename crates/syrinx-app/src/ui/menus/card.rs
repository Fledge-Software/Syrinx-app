//! The card every menu shares: how one row is drawn, how big the card is, and where it
//! hangs inside the main window.

use super::*;

/// A separator, or the confirmation's question in the menu's own type.
pub(super) fn drawn_only_row(
    index: usize,
    row: &MenuRow,
    inset: bool,
    palette: &syrinx_ui::Palette,
) -> Stateful<Div> {
    if row.item == MenuItem::Separator {
        return div()
            .id(("menu-separator", index))
            .h(px(SEPARATOR_HEIGHT))
            .flex_shrink_0()
            .px(px(SEPARATOR_INSET))
            .flex()
            .items_center()
            .child(
                div()
                    .h(px(1.))
                    .w_full()
                    .bg(theme::rgba(palette.panel_divider)),
            );
    }
    // The history list's note is quieter than a question: it explains, it does not ask.
    let note = row.item == MenuItem::HistoryUnlisted;
    // A named label, so a screen reader reads the question with the menu.
    div()
        .id(("menu-prompt", index))
        .role(gpui::Role::Label)
        .aria_label(row.label.clone())
        .h(px(ITEM_HEIGHT))
        .flex_shrink_0()
        .px(px(ITEM_PAD_X))
        .flex()
        .items_center()
        .when(note, |prompt| {
            prompt.text_color(theme::rgba(palette.text_muted))
        })
        .when(!note, |prompt| {
            prompt.font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(inset, |prompt| prompt.gap_3().child(icon_space()))
        .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
}

/// One item: its name on the left and its shortcut, dimmed, on the right. A history page
/// also shows its site's placeholder icon first, as its sidebar row does, and its host on
/// the right. Rows take no Tab stop; the selected one is the focused menu's active
/// descendant for screen readers.
pub(super) fn menu_row(
    index: usize,
    row: &MenuRow,
    selected: bool,
    inset: bool,
    palette: &syrinx_ui::Palette,
) -> Stateful<Div> {
    let page = matches!(row.item, MenuItem::HistoryPage { .. });
    div()
        .id(("menu-item", index))
        .role(gpui::Role::MenuItem)
        .aria_label(row.label.clone())
        .aria_selected(selected)
        .when(selected, |row| {
            let row = row
                .aria_active_descendant()
                .bg(theme::rgba(palette.row_selected));
            crate::controls::raised(row, syrinx_ui::ShadowRole::RowSelected)
        })
        .h(px(ITEM_HEIGHT))
        .flex_shrink_0()
        .px(px(ITEM_PAD_X))
        .flex()
        .items_center()
        .gap_3()
        .rounded(px(ITEM_RADIUS))
        .cursor_pointer()
        .children(
            row.icon
                .as_deref()
                .map(|letter| super::sidebar_view::row_icon(letter, false)),
        )
        .when(inset && row.icon.is_none(), |drawn| {
            drawn.child(icon_space())
        })
        .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
        .children(row.shortcut.clone().map(|keys| {
            let detail = div()
                .text_size(px(syrinx_ui::metrics::text::SMALL))
                .text_color(theme::rgba(palette.text_muted))
                .child(keys);
            // A page's host gives way to its title; a shortcut is never cut short.
            if page {
                detail.max_w(gpui::relative(HOST_SHARE)).truncate()
            } else {
                detail.flex_shrink_0()
            }
        }))
}

/// An empty space as wide as a page's icon, so a row without one lines its text up.
fn icon_space() -> Div {
    div()
        .size(px(super::sidebar_view::MONOGRAM_SIZE))
        .flex_none()
}

/// Which corner of the menu sits at its anchor point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum Corner {
    TopLeft,
    TopRight,
}

/// Where the menu is anchored in the main window, in logical pixels.
#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct MenuAnchor {
    pub(in crate::ui) position: Point<Pixels>,
    pub(in crate::ui) corner: Corner,
}

/// Where the ☰ menu and the Downloads flyout hang from: the right edge of the title bar's own
/// controls, left of the caption buttons and the gap before them. Ctrl J and the palette
/// open the flyout with no button press to measure, so the corner follows the title bar's
/// layout constants rather than a button's painted bounds.
pub(super) fn title_bar_corner(window: &Window) -> Point<Pixels> {
    corner_for_width(f32::from(window.viewport_size().width))
}

/// The corner for a window `width` logical pixels wide.
fn corner_for_width(width: f32) -> Point<Pixels> {
    point(
        px(width - caption_buttons_width() - CAPTION_GAP),
        px(TITLE_BAR_HEIGHT),
    )
}

/// The popup's logical size for `rows` of `kind`, separators counted at their own height.
pub(super) fn kind_size(kind: MenuKind, rows: &[MenuRow]) -> gpui::Size<Pixels> {
    let separators = rows
        .iter()
        .filter(|row| row.item == MenuItem::Separator)
        .count();
    let items = rows.len() - separators;
    size(px(kind.width()), menu_size(items, separators).height)
}

/// The popup's logical size for `items` rows and `separators` lines between groups.
fn menu_size(items: usize, separators: usize) -> gpui::Size<Pixels> {
    let height = items as f32 * ITEM_HEIGHT
        + separators as f32 * SEPARATOR_HEIGHT
        + 2. * (MENU_PADDING + MENU_BORDER);
    size(px(MENU_WIDTH), px(height))
}

/// The menu's place on screen in physical pixels, kept inside the main window's margin.
pub(super) fn menu_rect(
    main: Bounds<Pixels>,
    scale: f32,
    anchor: MenuAnchor,
    menu: gpui::Size<Pixels>,
) -> OverlayRect {
    let (width, height) = (f32::from(menu.width), f32::from(menu.height));
    let (main_width, main_height) = (f32::from(main.size.width), f32::from(main.size.height));
    let x = f32::from(anchor.position.x);
    let left = match anchor.corner {
        Corner::TopLeft => x,
        Corner::TopRight => x - width,
    };
    let left = keep_inside(left, width, main_width);
    let top = keep_inside(f32::from(anchor.position.y), height, main_height);
    let physical = |logical: f32| (logical * scale).round() as i32;
    OverlayRect {
        left: physical(f32::from(main.origin.x) + left),
        top: physical(f32::from(main.origin.y) + top),
        width: physical(width),
        height: physical(height),
    }
}

/// Clamps a span's start so it stays `WINDOW_MARGIN` inside `extent`, or at the margin when
/// it cannot fit.
fn keep_inside(start: f32, span: f32, extent: f32) -> f32 {
    let latest = (extent - WINDOW_MARGIN - span).max(WINDOW_MARGIN);
    start.clamp(WINDOW_MARGIN, latest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_window(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn anchor(x: f32, y: f32, corner: Corner) -> MenuAnchor {
        MenuAnchor {
            position: point(px(x), px(y)),
            corner,
        }
    }

    #[test]
    fn the_corner_sits_left_of_the_caption_buttons_and_their_gap_at_any_width() {
        for width in [800., 1280., 1920.] {
            let corner = corner_for_width(width);
            assert_eq!(
                f32::from(corner.x) + CAPTION_GAP + caption_buttons_width(),
                width,
                "{width}"
            );
            assert_eq!(f32::from(corner.y), TITLE_BAR_HEIGHT);
        }
        // Today's layout: three 36 px buttons and a 4 px gap.
        assert_eq!(f32::from(corner_for_width(1280.).x), 1168.);
    }

    #[test]
    fn the_popup_fits_its_rows_exactly() {
        assert_eq!(menu_size(PAGE_ITEMS.len(), 0), size(px(220.), px(220.)));
        assert_eq!(menu_size(8, 0), size(px(220.), px(250.)));
        assert_eq!(menu_size(3, 0), size(px(220.), px(100.)));
    }

    #[test]
    fn separators_count_at_their_own_height() {
        // The ⋯ menu: four items and one separator.
        let rows: Vec<MenuRow> = SIDEBAR_ITEMS.iter().map(|&item| fixed_row(item)).collect();
        assert_eq!(
            kind_size(MenuKind::Sidebar, &rows),
            size(px(220.), px(4. * 30. + 9. + 10.))
        );
        assert_eq!(
            menu_size(9, 3),
            size(px(220.), px(9. * 30. + 3. * 9. + 10.))
        );
    }

    #[test]
    fn the_page_menu_hangs_from_its_right_corner_in_screen_pixels() {
        // A 1280-wide window at (100, 50); the ☰ corner sits at x 1176, under the title bar.
        let rect = menu_rect(
            main_window(100., 50., 1280., 800.),
            1.,
            anchor(1176., 36., Corner::TopRight),
            menu_size(8, 0),
        );
        assert_eq!(
            rect,
            OverlayRect {
                left: 1056,
                top: 86,
                width: 220,
                height: 250
            }
        );
    }

    #[test]
    fn a_right_click_near_an_edge_keeps_the_menu_inside_the_window() {
        let main = main_window(0., 0., 1000., 600.);
        let rect = menu_rect(
            main,
            1.,
            anchor(990., 590., Corner::TopLeft),
            menu_size(8, 0),
        );
        assert_eq!((rect.left, rect.top), (772, 342));
        let rect = menu_rect(
            main,
            1.,
            anchor(-20., -5., Corner::TopLeft),
            menu_size(3, 0),
        );
        assert_eq!((rect.left, rect.top), (8, 8));
    }

    #[test]
    fn physical_pixels_follow_the_main_windows_scale_on_any_monitor() {
        let rect = menu_rect(
            main_window(-1600., 40., 1280., 800.),
            1.5,
            anchor(20., 100., Corner::TopLeft),
            menu_size(3, 0),
        );
        assert_eq!(
            rect,
            OverlayRect {
                left: -2370,
                top: 210,
                width: 330,
                height: 150
            }
        );
    }
}
