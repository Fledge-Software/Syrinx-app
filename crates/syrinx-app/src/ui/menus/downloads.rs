//! The Downloads flyout behind the title bar's ↓: its rows, their Pause, Resume and Cancel
//! controls, and the keys that press them (#143).

use super::*;

/// On a download row, Enter or Space presses its first button when that is Pause or Resume,
/// never Cancel (the only button where a download cannot pause), and Delete cancels it.
pub(super) fn download_key(key: &str, row: &MenuRow) -> Option<MenuAction> {
    let MenuItem::Download(download) = row.item else {
        return None;
    };
    let control = match key {
        "enter" | "space" => row
            .controls
            .first()
            .copied()
            .filter(|control| *control != DownloadControl::Cancel),
        "delete" => row
            .controls
            .contains(&DownloadControl::Cancel)
            .then_some(DownloadControl::Cancel),
        _ => None,
    }?;
    Some(MenuAction::Download(download, control))
}

/// A download button's element id, icon and spoken verb.
pub(super) fn control_face(control: DownloadControl) -> (&'static str, Icon, &'static str) {
    match control {
        DownloadControl::Pause => ("download-pause", Icon::Pause, "Pause"),
        DownloadControl::Resume => ("download-resume", Icon::Resume, "Resume"),
        DownloadControl::Cancel => ("download-cancel", Icon::Cancel, "Cancel"),
    }
}

/// The Downloads flyout's rows: the newest downloads, or a line saying there are none, then
/// Clear when any has finished. Without `pauses` (Linux) no row offers Pause or Resume.
pub(super) fn download_rows(list: &syrinx_core::DownloadList, pauses: bool) -> Vec<MenuRow> {
    let mut rows: Vec<MenuRow> = list
        .downloads()
        .iter()
        .take(MAX_DOWNLOAD_ROWS)
        .map(|download| MenuRow {
            item: MenuItem::Download(download.id),
            label: download.file_name.clone(),
            shortcut: Some(download.detail()),
            controls: download.offered_controls(pauses),
            icon: None,
        })
        .collect();
    if rows.is_empty() {
        rows.push(fixed_row(MenuItem::NoDownloads));
    }
    if list.has_finished() {
        rows.push(fixed_row(MenuItem::ClearDownloads));
    }
    rows
}

impl BrowserView {
    /// Opens the Downloads flyout under the title bar's ↓ corner, or closes it as
    /// `open_page_menu` does. It works from any tab, Home included.
    pub(in crate::ui) fn toggle_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggled_closed(MenuKind::Downloads, cx) {
            return;
        }
        self.show_downloads(window, cx);
    }

    /// Opens the Downloads flyout at its corner, replacing any open menu.
    pub(super) fn show_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let position = title_bar_corner(window);
        self.show_menu(MenuKind::Downloads, position, Corner::TopRight, window, cx);
    }

    /// The session's downloads changed: an open flyout shows the new rows, and reopens in
    /// place when their number changed, since its window is sized to its rows.
    pub(in crate::ui) fn menu_downloads_changed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.menu else {
            return;
        };
        if menu.kind != MenuKind::Downloads {
            return;
        }
        let Some(view) = menu.view.clone() else {
            return; // Still opening; its rows were read when it was requested.
        };
        let rows = self.menu_rows(MenuKind::Downloads);
        let reopen = view.update(cx, |menu, cx| {
            if menu.rows.len() != rows.len() {
                return true;
            }
            menu.rows = rows;
            cx.notify();
            false
        });
        match reopen {
            Ok(false) => {}
            // Opening afresh gives the window its new size.
            Ok(true) => self.show_downloads(window, cx),
            Err(_) => eprintln!("The downloads flyout closed before its rows changed."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_rows_carry_their_controls_and_keys_press_them() {
        let mut list = syrinx_core::DownloadList::default();
        list.add(3, 1, "synthetic.zip", None);
        let rows = download_rows(&list, true);
        assert_eq!(rows.len(), 1, "no Clear row while nothing has finished");
        let row = &rows[0];
        assert_eq!(
            row.controls,
            [DownloadControl::Pause, DownloadControl::Cancel]
        );
        assert!(matches!(
            download_key("enter", row),
            Some(MenuAction::Download(3, DownloadControl::Pause))
        ));
        assert!(matches!(
            download_key("delete", row),
            Some(MenuAction::Download(3, DownloadControl::Cancel))
        ));
        assert!(download_key("left", row).is_none());
        list.fail(
            3,
            syrinx_core::DownloadInterruption::Blocked(syrinx_core::BlockReason::Unsafe),
            0,
        );
        let rows = download_rows(&list, true);
        assert!(rows[0].controls.is_empty(), "a blocked file is never kept");
        assert!(download_key("enter", &rows[0]).is_none());
        assert_eq!(rows[1].item, MenuItem::ClearDownloads);
    }

    #[test]
    fn an_engine_that_cannot_pause_offers_cancel_and_enter_never_presses_it() {
        let mut list = syrinx_core::DownloadList::default();
        list.add(3, 1, "synthetic.zip", None);
        let rows = download_rows(&list, false);
        assert_eq!(rows[0].controls, [DownloadControl::Cancel]);
        assert!(download_key("enter", &rows[0]).is_none());
        assert!(download_key("space", &rows[0]).is_none());
        assert!(matches!(
            download_key("delete", &rows[0]),
            Some(MenuAction::Download(3, DownloadControl::Cancel))
        ));
    }
}
