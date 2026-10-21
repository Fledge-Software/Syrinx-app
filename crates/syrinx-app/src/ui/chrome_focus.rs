//! Gives Windows keyboard focus back to Syrinx's chrome window, reporting a failure once (#84).

use super::BrowserView;

impl BrowserView {
    /// Moves Windows keyboard focus from the page child to the chrome window. A failure is
    /// shown in the status line; returns whether the chrome now has the keys.
    pub(super) fn move_keys_to_chrome(&mut self) -> bool {
        let Err(error) = syrinx_platform::browser::focus_chrome(self.parent_hwnd) else {
            return true;
        };
        self.message = error.to_string();
        false
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::main_window::tests::browser_view_sources;

    /// Whether a source line calls the platform's `focus_chrome`, ignoring comment lines.
    fn calls_platform_focus_chrome(line: &str) -> bool {
        !line.trim_start().starts_with("//") && line.contains("focus_chrome(")
    }

    #[test]
    fn the_scan_ignores_comments_but_not_calls() {
        assert!(calls_platform_focus_chrome(
            "        if let Err(error) = syrinx_platform::browser::focus_chrome(self.parent_hwnd) {"
        ));
        assert!(calls_platform_focus_chrome(
            "            && let Err(error) = focus_chrome(self.parent_hwnd)"
        ));
        assert!(!calls_platform_focus_chrome(
            "    // Calls `focus_chrome(` through the helper."
        ));
        assert!(!calls_platform_focus_chrome(
            "        self.move_keys_to_chrome();"
        ));
    }

    #[test]
    fn only_the_helper_calls_the_platform_focus_chrome() {
        let offenders: Vec<String> = browser_view_sources()
            .iter()
            .filter(|path| !path.ends_with("chrome_focus.rs"))
            .flat_map(|path| platform_focus_chrome_calls(path))
            .collect();
        assert!(
            offenders.is_empty(),
            "call `BrowserView::move_keys_to_chrome`, which reports a failure in the status \
             line, instead of the platform's `focus_chrome` (#84): {offenders:#?}"
        );
    }

    /// Each `file:line` that calls the platform's `focus_chrome`.
    fn platform_focus_chrome_calls(path: &Path) -> Vec<String> {
        let source = std::fs::read_to_string(path).expect("a ui source is readable");
        source
            .lines()
            .enumerate()
            .filter(|(_, line)| calls_platform_focus_chrome(line))
            .map(|(index, _)| format!("{}:{}", path.display(), index + 1))
            .collect()
    }
}
