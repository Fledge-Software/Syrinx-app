//! Gives chrome and focused pages the same native browser controls and visible zoom level.

use super::*;
use atlas_platform::browser::PageAction;

// Find, Downloads and Print take their keys from the command registry (`ui::bind_keys`).
actions!(
    essentials,
    [Find, Downloads, Print, ZoomIn, ZoomOut, ZoomReset]
);

/// Zoom in's chrome keys; the registry's shortcut syntax cannot spell `=` or `+`.
const ZOOM_IN_KEYS: [&str; 2] = ["ctrl-=", "ctrl-+"];
/// Zoom out's chrome key; the registry's shortcut syntax cannot spell `-`.
const ZOOM_OUT_KEY: &str = "ctrl--";

/// Binds the zoom keys the registry cannot hold, while Atlas chrome has focus. Reset zoom's
/// Ctrl 0 comes from the registry. A focused page keeps these keys first (owner, 2026-10-12).
pub(super) fn bind_zoom_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(ZOOM_IN_KEYS[0], ZoomIn, None),
        KeyBinding::new(ZOOM_IN_KEYS[1], ZoomIn, None),
        KeyBinding::new(ZOOM_OUT_KEY, ZoomOut, None),
    ]);
}

impl BrowserView {
    pub(super) fn essential_shortcut(&mut self, shortcut: BrowserShortcut) {
        if self.closing {
            return;
        }
        if let Some(command) = page_action(self.shown.map(TabId::get), shortcut) {
            self.page_command(command);
        }
    }

    pub(super) fn essential_actions(&self, element: Div, cx: &mut Context<Self>) -> Div {
        element
            .on_action(cx.listener(|this, _: &Find, window, cx| {
                this.open_find(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Downloads, _, cx| {
                this.essential_shortcut(BrowserShortcut::Downloads);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Print, _, cx| {
                this.essential_shortcut(BrowserShortcut::Print);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, _| {
                this.essential_shortcut(BrowserShortcut::ZoomIn)
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _, _| {
                this.essential_shortcut(BrowserShortcut::ZoomOut)
            }))
            .on_action(cx.listener(|this, _: &ZoomReset, _, _| {
                this.essential_shortcut(BrowserShortcut::ZoomReset)
            }))
    }

    pub(super) fn essential_controls(&self, cx: &mut Context<Self>) -> Div {
        let mut controls = div().flex().items_center().gap_1();
        for (id, label, shortcut) in [
            ("find", "Find", BrowserShortcut::Find),
            ("downloads", "Downloads", BrowserShortcut::Downloads),
            ("print", "Print", BrowserShortcut::Print),
            ("zoom-out", "−", BrowserShortcut::ZoomOut),
        ] {
            controls = controls.child(button(id, label).on_click(cx.listener(
                move |this, _, window, cx| {
                    if shortcut == BrowserShortcut::Find {
                        this.open_find(window, cx);
                    } else {
                        this.essential_shortcut(shortcut);
                    }
                    cx.notify();
                },
            )));
        }
        let factor = self
            .shown
            .and_then(|id| self.zoom_levels.get(&id.get()).copied())
            .unwrap_or(1.);
        controls
            .child(
                div()
                    .id("zoom-reset")
                    .cursor_pointer()
                    .px_1()
                    .child(format!("{:.0}%", factor * 100.))
                    .on_click(cx.listener(|this, _, _, _| {
                        this.essential_shortcut(BrowserShortcut::ZoomReset)
                    })),
            )
            .child(button("zoom-in", "+").on_click(
                cx.listener(|this, _, _, _| this.essential_shortcut(BrowserShortcut::ZoomIn)),
            ))
    }
}

fn page_action(id: Option<u64>, shortcut: BrowserShortcut) -> Option<BrowserCommand> {
    let id = id?;
    let action = match shortcut {
        BrowserShortcut::Downloads => PageAction::Downloads,
        BrowserShortcut::Print => PageAction::Print,
        BrowserShortcut::ZoomIn => PageAction::ZoomIn,
        BrowserShortcut::ZoomOut => PageAction::ZoomOut,
        BrowserShortcut::ZoomReset => PageAction::ZoomReset,
        _ => return None,
    };
    Some(BrowserCommand::PageAction { id, action })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_only_zoom_keys_never_shadow_a_registry_shortcut() {
        let registry = CommandRegistry::browser();
        let registered: Vec<&str> = registry
            .iter()
            .filter_map(|(_, command)| command.shortcut.as_ref())
            .map(atlas_core::Shortcut::keystroke)
            .collect();
        for key in ZOOM_IN_KEYS.into_iter().chain([ZOOM_OUT_KEY]) {
            assert!(!registered.contains(&key), "{key} is also a registry key");
        }
        assert!(
            registered.contains(&"ctrl-0"),
            "Reset zoom uses the registry"
        );
    }

    #[test]
    fn page_focused_actions_target_only_the_shown_live_view() {
        for shortcut in [
            BrowserShortcut::Downloads,
            BrowserShortcut::Print,
            BrowserShortcut::ZoomIn,
            BrowserShortcut::ZoomOut,
            BrowserShortcut::ZoomReset,
        ] {
            assert!(matches!(
                page_action(Some(7), shortcut),
                Some(BrowserCommand::PageAction { id: 7, .. })
            ));
            assert!(matches!(
                page_action(Some(8), shortcut),
                Some(BrowserCommand::PageAction { id: 8, .. })
            ));
            assert!(page_action(None, shortcut).is_none());
        }
        assert!(matches!(
            page_action(Some(7), BrowserShortcut::Downloads),
            Some(BrowserCommand::PageAction {
                action: PageAction::Downloads,
                ..
            })
        ));
        assert!(page_action(Some(7), BrowserShortcut::Find).is_none());
    }
}
