//! Delivers background work to the browser view in its main window, whichever window drew it last.

use gpui::{AppContext, AsyncWindowContext, Context, WeakEntity, Window};

use super::BrowserView;

/// Runs `update` on the browser view inside the main window. GPUI's `update_in` would use
/// the window that drew the view last, which fails once the sidebar peek closes (traps file).
pub(super) fn update_in_main<R>(
    view: &WeakEntity<BrowserView>,
    cx: &mut AsyncWindowContext,
    update: impl FnOnce(&mut BrowserView, &mut Window, &mut Context<BrowserView>) -> R,
) -> gpui::Result<R> {
    let main = view.read_with(cx, |browser, _| browser.main_window)?;
    cx.update_window(main, |_, window, cx| {
        view.update(cx, |browser, cx| update(browser, window, cx))
    })?
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Every Rust file of the browser view: `src/ui.rs` and everything under `src/ui/`.
    fn browser_view_sources() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![root.join("ui.rs")];
        let mut folders = vec![root.join("ui")];
        while let Some(folder) = folders.pop() {
            let entries = std::fs::read_dir(&folder).expect("the ui folder is readable");
            for entry in entries {
                let path = entry.expect("a ui folder entry is readable").path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    files.push(path);
                }
            }
        }
        files
    }

    /// Each `file:line` that updates the browser view through GPUI's `update_in`.
    fn update_in_calls(path: &Path) -> Vec<String> {
        let source = std::fs::read_to_string(path).expect("a ui source is readable");
        let name = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(path)
            .display()
            .to_string();
        source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(".update_in("))
            .map(|(index, _)| format!("{name}:{}", index + 1))
            .collect()
    }

    // Before #10's fix this listed the session, page-host, sampler, palette, import and link
    // loops; the session loop's failure is what lost the peek click's tab switch.
    #[test]
    fn background_work_never_reaches_the_browser_view_through_its_last_drawn_window() {
        let offenders: Vec<String> = browser_view_sources()
            .iter()
            .filter(|path| !path.ends_with("main_window.rs"))
            .flat_map(|path| update_in_calls(path))
            .collect();
        assert!(
            offenders.is_empty(),
            "update the browser view from a task with `main_window::update_in_main`, not GPUI's \
             `update_in`, which fails once the sidebar peek that last drew the view closes; \
             another entity under src/ui needs its own reviewed exception: {offenders:#?}"
        );
    }
}
