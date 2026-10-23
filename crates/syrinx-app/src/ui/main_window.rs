//! Delivers background work and field events to the browser view in its main window, whichever
//! window drew it last.

use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription, WeakEntity, Window};

use super::BrowserView;

/// Handles `emitter`'s events on the browser view inside the main window. GPUI's
/// `subscribe_in` would hand over the sidebar peek while it is open and drop the event once
/// it closes, as `update_in` did (#81, traps file). The handler always receives the main
/// window, even for a field drawn in the peek, so it must not use `window` for that field's
/// focus.
pub(super) fn subscribe_in_main<Emitter, Event>(
    emitter: &Entity<Emitter>,
    cx: &mut Context<BrowserView>,
    mut on_event: impl FnMut(&mut BrowserView, &Event, &mut Window, &mut Context<BrowserView>) + 'static,
) -> Subscription
where
    Emitter: EventEmitter<Event>,
    Event: 'static,
{
    let view = cx.weak_entity();
    App::subscribe(cx, emitter, move |_, event, cx| {
        let handled = update_in_main(&view, cx, |browser, window, cx| {
            on_event(browser, event, window, cx)
        });
        if let Err(error) = handled {
            eprintln!("A field's event could not reach the browser window: {error:#}");
        }
    })
}

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

    /// GPUI's helpers that find the browser view's window by the window that drew it last.
    const LAST_DRAWN_WINDOW_HELPERS: [&str; 4] = [
        ".update_in(",
        ".subscribe_in(",
        ".observe_in(",
        ".defer_in(",
    ];

    /// Whether a source line calls one of those helpers, ignoring comment lines.
    fn calls_last_drawn_window_helper(line: &str) -> bool {
        !line.trim_start().starts_with("//")
            && LAST_DRAWN_WINDOW_HELPERS
                .iter()
                .any(|helper| line.contains(helper))
    }

    /// Each `file:line` that calls one of those helpers.
    fn last_drawn_window_calls(path: &Path) -> Vec<String> {
        let source = std::fs::read_to_string(path).expect("a ui source is readable");
        let name = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(path)
            .display()
            .to_string();
        source
            .lines()
            .enumerate()
            .filter(|(_, line)| calls_last_drawn_window_helper(line))
            .map(|(index, _)| format!("{name}:{}", index + 1))
            .collect()
    }

    #[test]
    fn the_scan_ignores_comments_but_not_calls() {
        assert!(calls_last_drawn_window_helper(
            "        view.update_in(cx, |view, window, cx| {"
        ));
        assert!(calls_last_drawn_window_helper(
            "        let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {"
        ));
        assert!(calls_last_drawn_window_helper(
            "        cx.observe_in(&input, window, |this, _, window, cx| {});"
        ));
        assert!(calls_last_drawn_window_helper(
            "        cx.defer_in(window, |this, window, cx| {});"
        ));
        assert!(!calls_last_drawn_window_helper(
            "    // GPUI's `.update_in(` would use the popup."
        ));
        assert!(!calls_last_drawn_window_helper(
            "/// Calls `.subscribe_in(` nowhere."
        ));
        assert!(!calls_last_drawn_window_helper(
            "        cx.spawn_in(window, async move |view, cx| {"
        ));
    }

    // Before #10's fix this listed the session, page-host, sampler, palette, import and link
    // loops; the session loop's failure is what lost the peek click's tab switch. Before #81's
    // fix it listed the find, folder, tile, accent and theme fields' Enter handlers. The peek's
    // behaviour itself is checked by `cargo xtask palette-acceptance` (`peek_click_*`).
    #[test]
    fn nothing_reaches_the_browser_view_through_its_last_drawn_window() {
        let offenders: Vec<String> = browser_view_sources()
            .iter()
            .filter(|path| !path.ends_with("main_window.rs"))
            .flat_map(|path| last_drawn_window_calls(path))
            .collect();
        assert!(
            offenders.is_empty(),
            "reach the browser view through `main_window::update_in_main` or \
             `main_window::subscribe_in_main`, not GPUI's `update_in`, `subscribe_in`, \
             `observe_in` or `defer_in`: they use the window that drew the view last, which is \
             the sidebar peek while it is open and nothing once it closes. The rule covers \
             src/ui; only main_window.rs is exempt: {offenders:#?}"
        );
    }
}
