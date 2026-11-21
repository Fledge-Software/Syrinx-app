//! The origin a plugin view's documents are served on, and the check that a message came from it.

/// The `https://<host>` a view's documents are served on; its owner is a parameter so the
/// host's own editor view (#37) uses the same bridge as a plugin's interface.
///
/// ⚠️ Unverified: the form follows wry's rewrite of a custom protocol to `https://<name>.<host>`
/// in the vendored source; the native fixture confirms what WebView2 reports as `Source()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewOrigin {
    origin: String,
}

impl ViewOrigin {
    /// A plugin's interface; `plugin_id` comes from a checked manifest (one lowercase segment).
    pub fn plugin(plugin_id: &str) -> Self {
        Self {
            origin: format!("https://syrinx-plugin.{plugin_id}.invalid"),
        }
    }

    /// The host's own CodeMirror editor view (D28).
    pub fn editor() -> Self {
        Self {
            origin: "https://syrinx-editor.invalid".into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.origin
    }

    /// Whether WebView2's `Source()`, the sending document's full URI, is on this origin: the
    /// origin itself or the origin followed by `/` and a path. Anything else after the host
    /// (`.evil.test`, `@`, `:8080`) is another origin, and the comparison is case-sensitive.
    pub fn matches_source(&self, source: &str) -> bool {
        let Some(rest) = source.strip_prefix(self.origin.as_str()) else {
            return false;
        };
        rest.is_empty() || rest.starts_with('/')
    }
}

#[cfg(test)]
mod tests {
    use super::ViewOrigin;

    #[test]
    fn a_plugin_and_the_editor_have_their_own_origins() {
        assert_eq!(
            ViewOrigin::plugin("hello").as_str(),
            "https://syrinx-plugin.hello.invalid"
        );
        assert_eq!(
            ViewOrigin::editor().as_str(),
            "https://syrinx-editor.invalid"
        );
    }

    #[test]
    fn a_document_on_the_origin_matches() {
        let origin = ViewOrigin::plugin("hello");
        for source in [
            "https://syrinx-plugin.hello.invalid",
            "https://syrinx-plugin.hello.invalid/",
            "https://syrinx-plugin.hello.invalid/index.html",
        ] {
            assert!(origin.matches_source(source), "{source}");
        }
    }

    #[test]
    fn any_other_origin_is_refused() {
        let origin = ViewOrigin::plugin("hello");
        for source in [
            "https://syrinx-plugin.hello.invalid.evil.test/",
            "https://syrinx-plugin.hello.invalid@example.com/",
            "https://syrinx-plugin.hello.invalid:8080/",
            "http://syrinx-plugin.hello.invalid/",
            "https://SYRINX-PLUGIN.hello.invalid/",
            "https://syrinx-plugin.other.invalid/",
            "https://syrinx-editor.invalid/",
            "https://example.com/",
            "",
        ] {
            assert!(!origin.matches_source(source), "{source}");
        }
    }
}
