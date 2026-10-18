//! Delivers background work to the browser view in its main window, whichever window drew it last.

use gpui::{AppContext, Context, WeakEntity, Window};

use super::BrowserView;

/// Runs `update` on the browser view inside the main window. GPUI's `update_in` would use
/// the window that drew the view last, which fails once the sidebar peek closes (traps file).
pub(super) fn update_in_main<C: AppContext, R>(
    view: &WeakEntity<BrowserView>,
    cx: &mut C,
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
        collect_rust_files(&root.join("ui"), &mut files);
        files
    }

    /// Adds every `.rs` file under `folder`, at any depth, to `files`.
    fn collect_rust_files(folder: &Path, files: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(folder).expect("the ui folder is readable");
        for entry in entries {
            let path = entry.expect("a ui folder entry is readable").path();
            if path.is_dir() {
                collect_rust_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    /// Whether a source line calls GPUI's `update_in`, ignoring comment lines.
    fn calls_update_in(line: &str) -> bool {
        !line.trim_start().starts_with("//") && line.contains(".update_in(")
    }

    /// Each `file:line` that calls GPUI's `update_in`.
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
            .filter(|(_, line)| calls_update_in(line))
            .map(|(index, _)| format!("{name}:{}", index + 1))
            .collect()
    }

    #[test]
    fn the_scan_ignores_comments_but_not_calls() {
        assert!(calls_update_in(
            "        view.update_in(cx, |view, window, cx| {"
        ));
        assert!(!calls_update_in(
            "    // GPUI's `.update_in(` would use the popup."
        ));
        assert!(!calls_update_in("/// Calls `.update_in(` nowhere."));
    }

    // Before #10's fix this listed the session, page-host, sampler, palette, import and link
    // loops; the session loop's failure is what lost the peek click's tab switch. The peek's
    // behaviour itself is checked by `cargo xtask palette-acceptance` (`peek_click_*`).
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
             `update_in`, which fails once the sidebar peek that last drew the view closes. The \
             rule covers every `update_in` under src/ui; only main_window.rs is exempt: \
             {offenders:#?}"
        );
    }
}
