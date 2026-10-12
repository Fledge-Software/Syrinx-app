//! Supplies browser actions to the preview's local palette until the command registry is integrated.

use std::num::NonZeroU64;

use atlas_platform::browser::BrowserShortcut;
use atlas_search::{Entry, Target, Usage};

const COMMANDS: [(u64, &str, BrowserShortcut); 7] = [
    (1, "Find in page", BrowserShortcut::Find),
    (2, "Downloads", BrowserShortcut::Downloads),
    (3, "Print", BrowserShortcut::Print),
    (4, "History", BrowserShortcut::History),
    (5, "Zoom in", BrowserShortcut::ZoomIn),
    (6, "Zoom out", BrowserShortcut::ZoomOut),
    (7, "Reset zoom", BrowserShortcut::ZoomReset),
];

pub fn entries() -> impl Iterator<Item = Entry> {
    COMMANDS.into_iter().map(|(id, title, _)| Entry {
        target: Target::Command(NonZeroU64::new(id).expect("nonzero browser command")),
        title: title.into(),
        address: String::new(),
        usage: Usage {
            visits: 0,
            last_used: None,
        },
        open: false,
    })
}

pub fn shortcut(id: NonZeroU64) -> Option<BrowserShortcut> {
    COMMANDS
        .into_iter()
        .find_map(|(command, _, shortcut)| (command == id.get()).then_some(shortcut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_control_routes_to_its_browser_action() {
        for (title, expected) in [
            ("Find in page", BrowserShortcut::Find),
            ("Downloads", BrowserShortcut::Downloads),
            ("Print", BrowserShortcut::Print),
            ("History", BrowserShortcut::History),
            ("Zoom in", BrowserShortcut::ZoomIn),
            ("Zoom out", BrowserShortcut::ZoomOut),
            ("Reset zoom", BrowserShortcut::ZoomReset),
        ] {
            let entry = entries()
                .find(|entry| entry.title == title)
                .expect("palette command");
            let Target::Command(id) = entry.target else {
                panic!("command target expected")
            };
            assert_eq!(shortcut(id), Some(expected));
            assert!(entry.address.is_empty());
        }
        assert_eq!(
            shortcut(NonZeroU64::new(u64::MAX).expect("unknown command")),
            None
        );
    }
}
