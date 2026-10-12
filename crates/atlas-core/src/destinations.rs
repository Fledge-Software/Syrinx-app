//! Holds the owner's Quick access tiles and sidebar Favorites as two ordered, labelled lists (D45).

use std::fmt;

/// The most tiles either surface keeps; the home grid shows eight per row.
pub const MAX_DESTINATIONS: usize = 64;

/// The longest label, in characters, so a tile caption stays a caption.
pub const MAX_LABEL_CHARS: usize = 64;

/// The longest stored web address, in bytes.
pub const MAX_ADDRESS_BYTES: usize = 8 * 1024;

/// The two separate tile surfaces. Favorites are not saved pages (D45).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DestinationSurface {
    /// The home page's Quick access grid.
    QuickAccess,
    /// The square tiles at the top of the sidebar.
    Favorites,
}

impl DestinationSurface {
    pub const ALL: [DestinationSurface; 2] = [
        DestinationSurface::QuickAccess,
        DestinationSurface::Favorites,
    ];
}

/// A built-in plugin a tile may point at before its plugin exists.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BuiltInPlugin {
    Forge,
    Notes,
    Fledge,
}

impl BuiltInPlugin {
    pub const ALL: [BuiltInPlugin; 3] = [
        BuiltInPlugin::Forge,
        BuiltInPlugin::Notes,
        BuiltInPlugin::Fledge,
    ];

    /// The stable id stored on disk.
    pub const fn id(self) -> &'static str {
        match self {
            BuiltInPlugin::Forge => "forge",
            BuiltInPlugin::Notes => "notes",
            BuiltInPlugin::Fledge => "fledge",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            BuiltInPlugin::Forge => "Forge",
            BuiltInPlugin::Notes => "Notes",
            BuiltInPlugin::Fledge => "Fledge",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|plugin| plugin.id() == id)
    }
}

/// Where a tile leads.
#[derive(Clone, Eq, Hash, PartialEq)]
pub enum DestinationTarget {
    /// An `http` or `https` address, already normalised by navigation.
    Web(String),
    /// A plugin page; it stays unavailable until plan 2's plugin lifecycle exists.
    Plugin(BuiltInPlugin),
}

/// One labelled tile.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Destination {
    pub label: String,
    pub target: DestinationTarget,
}

impl fmt::Debug for Destination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Labels and addresses are the owner's data and never enter diagnostics.
        formatter.write_str("Destination([redacted])")
    }
}

impl Destination {
    /// Trims the label and checks both fields, so stored tiles always render.
    pub fn new(label: &str, target: DestinationTarget) -> Result<Self, DestinationError> {
        let label = label.trim();
        check_label(label)?;
        check_target(&target)?;
        Ok(Self {
            label: label.to_owned(),
            target,
        })
    }

    fn validate(&self) -> Result<(), DestinationError> {
        if self.label.trim() != self.label {
            return Err(DestinationError::InvalidLabel);
        }
        check_label(&self.label)?;
        check_target(&self.target)
    }
}

/// Why a tile edit or a stored list was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DestinationError {
    InvalidLabel,
    InvalidAddress,
    TooMany,
    UnknownPosition,
    /// The tile at that position is no longer the one the edit was made against.
    Changed,
}

impl fmt::Display for DestinationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLabel => "a tile needs a short name without control characters",
            Self::InvalidAddress => "a tile needs an http or https address",
            Self::TooMany => "this list already has the most tiles it can hold",
            Self::UnknownPosition => "that tile is no longer in the list",
            Self::Changed => "that tile changed before the edit arrived, so nothing was changed",
        })
    }
}

impl std::error::Error for DestinationError {}

/// An edit to one surface. Each positional edit names the tile it was made against
/// (`expected`), so a queued edit never lands on a tile that moved into that position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DestinationEdit {
    Add(Destination),
    Replace {
        index: usize,
        expected: Destination,
        tile: Destination,
    },
    Remove {
        index: usize,
        expected: Destination,
    },
    Move {
        from: usize,
        to: usize,
        expected: Destination,
    },
}

/// Both surfaces, each in display order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuickDestinations {
    quick_access: Vec<Destination>,
    favorites: Vec<Destination>,
}

impl QuickDestinations {
    /// Accepts stored lists only if every tile is valid and each list is within bounds.
    pub fn from_lists(
        quick_access: Vec<Destination>,
        favorites: Vec<Destination>,
    ) -> Result<Self, DestinationError> {
        for list in [&quick_access, &favorites] {
            if list.len() > MAX_DESTINATIONS {
                return Err(DestinationError::TooMany);
            }
            list.iter().try_for_each(Destination::validate)?;
        }
        Ok(Self {
            quick_access,
            favorites,
        })
    }

    pub fn list(&self, surface: DestinationSurface) -> &[Destination] {
        match surface {
            DestinationSurface::QuickAccess => &self.quick_access,
            DestinationSurface::Favorites => &self.favorites,
        }
    }

    /// Applies one edit; a refused edit leaves the list unchanged.
    pub fn apply(
        &mut self,
        surface: DestinationSurface,
        edit: DestinationEdit,
    ) -> Result<(), DestinationError> {
        let list = match surface {
            DestinationSurface::QuickAccess => &mut self.quick_access,
            DestinationSurface::Favorites => &mut self.favorites,
        };
        match edit {
            DestinationEdit::Add(tile) => add(list, tile),
            DestinationEdit::Replace {
                index,
                expected,
                tile,
            } => {
                check_expected(list, index, &expected)?;
                replace(list, index, tile)
            }
            DestinationEdit::Remove { index, expected } => {
                check_expected(list, index, &expected)?;
                remove(list, index)
            }
            DestinationEdit::Move { from, to, expected } => {
                check_expected(list, from, &expected)?;
                move_tile(list, from, to)
            }
        }
    }
}

/// Refuses an edit whose tile is gone or was replaced by another one at that position.
fn check_expected(
    list: &[Destination],
    index: usize,
    expected: &Destination,
) -> Result<(), DestinationError> {
    match list.get(index) {
        None => Err(DestinationError::UnknownPosition),
        Some(current) if current != expected => Err(DestinationError::Changed),
        Some(_) => Ok(()),
    }
}

fn add(list: &mut Vec<Destination>, tile: Destination) -> Result<(), DestinationError> {
    tile.validate()?;
    if list.len() >= MAX_DESTINATIONS {
        return Err(DestinationError::TooMany);
    }
    list.push(tile);
    Ok(())
}

fn replace(
    list: &mut [Destination],
    index: usize,
    tile: Destination,
) -> Result<(), DestinationError> {
    tile.validate()?;
    let slot = list
        .get_mut(index)
        .ok_or(DestinationError::UnknownPosition)?;
    *slot = tile;
    Ok(())
}

fn remove(list: &mut Vec<Destination>, index: usize) -> Result<(), DestinationError> {
    if index >= list.len() {
        return Err(DestinationError::UnknownPosition);
    }
    list.remove(index);
    Ok(())
}

fn move_tile(list: &mut Vec<Destination>, from: usize, to: usize) -> Result<(), DestinationError> {
    if from >= list.len() || to >= list.len() {
        return Err(DestinationError::UnknownPosition);
    }
    let tile = list.remove(from);
    list.insert(to, tile);
    Ok(())
}

fn check_label(label: &str) -> Result<(), DestinationError> {
    let length = label.chars().count();
    if length == 0 || length > MAX_LABEL_CHARS || label.chars().any(char::is_control) {
        return Err(DestinationError::InvalidLabel);
    }
    Ok(())
}

fn check_target(target: &DestinationTarget) -> Result<(), DestinationError> {
    let DestinationTarget::Web(address) = target else {
        return Ok(());
    };
    // The application normalises addresses; the model only refuses what can never open.
    let web = address.starts_with("https://") || address.starts_with("http://");
    let unprintable = address
        .chars()
        .any(|character| character.is_whitespace() || character.is_control());
    if !web || address.len() > MAX_ADDRESS_BYTES || unprintable {
        return Err(DestinationError::InvalidAddress);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web(label: &str, number: u32) -> Destination {
        Destination::new(
            label,
            DestinationTarget::Web(format!("https://tile-{number}.example.test/")),
        )
        .expect("synthetic tile is valid")
    }

    fn labels(destinations: &QuickDestinations, surface: DestinationSurface) -> Vec<&str> {
        destinations
            .list(surface)
            .iter()
            .map(|tile| tile.label.as_str())
            .collect()
    }

    #[test]
    fn surfaces_are_independent_and_keep_their_order() {
        let mut destinations = QuickDestinations::default();
        for (label, number) in [("Alpha", 1), ("Bravo", 2), ("Charlie", 3)] {
            destinations
                .apply(
                    DestinationSurface::QuickAccess,
                    DestinationEdit::Add(web(label, number)),
                )
                .expect("add quick tile");
        }
        destinations
            .apply(
                DestinationSurface::Favorites,
                DestinationEdit::Add(
                    Destination::new(
                        "Synthetic notes",
                        DestinationTarget::Plugin(BuiltInPlugin::Notes),
                    )
                    .expect("plugin tile"),
                ),
            )
            .expect("add favorite");
        destinations
            .apply(
                DestinationSurface::QuickAccess,
                DestinationEdit::Move {
                    from: 2,
                    to: 0,
                    expected: web("Charlie", 3),
                },
            )
            .expect("move tile");
        assert_eq!(
            labels(&destinations, DestinationSurface::QuickAccess),
            ["Charlie", "Alpha", "Bravo"]
        );
        assert_eq!(
            labels(&destinations, DestinationSurface::Favorites),
            ["Synthetic notes"]
        );
    }

    #[test]
    fn replace_and_remove_change_only_the_named_tile() {
        let mut destinations =
            QuickDestinations::from_lists(vec![web("One", 1), web("Two", 2)], Vec::new())
                .expect("valid lists");
        destinations
            .apply(
                DestinationSurface::QuickAccess,
                DestinationEdit::Replace {
                    index: 1,
                    expected: web("Two", 2),
                    tile: web("Renamed", 9),
                },
            )
            .expect("rename tile");
        destinations
            .apply(
                DestinationSurface::QuickAccess,
                DestinationEdit::Remove {
                    index: 0,
                    expected: web("One", 1),
                },
            )
            .expect("remove tile");
        assert_eq!(
            destinations.list(DestinationSurface::QuickAccess),
            [web("Renamed", 9)]
        );
    }

    #[test]
    fn refused_edits_leave_the_list_unchanged() {
        let mut destinations =
            QuickDestinations::from_lists(vec![web("One", 1)], Vec::new()).expect("valid lists");
        let before = destinations.clone();
        for edit in [
            DestinationEdit::Remove {
                index: 1,
                expected: web("One", 1),
            },
            DestinationEdit::Move {
                from: 0,
                to: 1,
                expected: web("One", 1),
            },
            DestinationEdit::Replace {
                index: 3,
                expected: web("One", 1),
                tile: web("Late", 3),
            },
        ] {
            assert_eq!(
                destinations.apply(DestinationSurface::QuickAccess, edit),
                Err(DestinationError::UnknownPosition)
            );
        }
        assert_eq!(destinations, before);
    }

    #[test]
    fn a_rename_queued_behind_a_removal_never_overwrites_the_next_tile() {
        // The editor was opened on "Three" (index 2); "One" is removed before Save arrives.
        let mut destinations = QuickDestinations::from_lists(
            vec![
                web("One", 1),
                web("Two", 2),
                web("Three", 3),
                web("Four", 4),
            ],
            Vec::new(),
        )
        .expect("valid lists");
        destinations
            .apply(
                DestinationSurface::QuickAccess,
                DestinationEdit::Remove {
                    index: 0,
                    expected: web("One", 1),
                },
            )
            .expect("remove first tile");
        let before = destinations.clone();
        assert_eq!(
            destinations.apply(
                DestinationSurface::QuickAccess,
                DestinationEdit::Replace {
                    index: 2,
                    expected: web("Three", 3),
                    tile: web("Renamed", 9),
                },
            ),
            Err(DestinationError::Changed)
        );
        assert_eq!(destinations, before);
        assert_eq!(
            labels(&destinations, DestinationSurface::QuickAccess),
            ["Two", "Three", "Four"]
        );
    }

    #[test]
    fn a_repeated_remove_removes_only_one_tile() {
        // A double click queues the same removal twice before the first is acknowledged.
        let mut destinations = QuickDestinations::from_lists(
            vec![web("One", 1), web("Two", 2), web("Three", 3)],
            Vec::new(),
        )
        .expect("valid lists");
        let remove = DestinationEdit::Remove {
            index: 0,
            expected: web("One", 1),
        };
        destinations
            .apply(DestinationSurface::QuickAccess, remove.clone())
            .expect("first removal");
        assert_eq!(
            destinations.apply(DestinationSurface::QuickAccess, remove),
            Err(DestinationError::Changed)
        );
        assert_eq!(
            labels(&destinations, DestinationSurface::QuickAccess),
            ["Two", "Three"]
        );
    }

    #[test]
    fn labels_and_addresses_are_checked() {
        let address = DestinationTarget::Web("https://checked.example.test/".to_owned());
        assert_eq!(
            Destination::new("  Trimmed  ", address.clone()).map(|tile| tile.label),
            Ok("Trimmed".to_owned())
        );
        for label in ["", "   ", "line\nbreak", &"x".repeat(MAX_LABEL_CHARS + 1)] {
            assert_eq!(
                Destination::new(label, address.clone()),
                Err(DestinationError::InvalidLabel)
            );
        }
        for address in [
            "example.test",
            "javascript:void(0)",
            "https://spaced example.test/",
            "https://control.example.test/\u{7}bell",
            "https://control.example.test/\u{0}nul",
        ] {
            assert_eq!(
                Destination::new("Synthetic", DestinationTarget::Web(address.to_owned())),
                Err(DestinationError::InvalidAddress)
            );
        }
    }

    #[test]
    fn lists_are_bounded() {
        let full: Vec<_> = (0..MAX_DESTINATIONS as u32)
            .map(|number| web("Tile", number))
            .collect();
        let mut destinations =
            QuickDestinations::from_lists(Vec::new(), full.clone()).expect("full list loads");
        assert_eq!(
            destinations.apply(
                DestinationSurface::Favorites,
                DestinationEdit::Add(web("Extra", 999))
            ),
            Err(DestinationError::TooMany)
        );
        let mut over = full;
        over.push(web("Extra", 999));
        assert_eq!(
            QuickDestinations::from_lists(over, Vec::new()),
            Err(DestinationError::TooMany)
        );
    }

    #[test]
    fn plugin_ids_roundtrip_and_debug_output_is_redacted() {
        for plugin in BuiltInPlugin::ALL {
            assert_eq!(BuiltInPlugin::from_id(plugin.id()), Some(plugin));
        }
        assert_eq!(BuiltInPlugin::from_id("unknown"), None);
        assert_eq!(
            format!("{:?}", web("Private", 1)),
            "Destination([redacted])"
        );
    }
}
