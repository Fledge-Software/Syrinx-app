//! Checks short ASCII subsequences with one bit per reachable query prefix.

use super::ascii_counterpart;

pub(crate) struct ShortAscii {
    masks: [u64; 256],
    length: usize,
    accepted: u64,
    valid_prefixes: u64,
}

impl ShortAscii {
    pub(super) fn new(query: &str, case_sensitive: bool) -> Option<Self> {
        if !query.is_ascii() || !(1..=63).contains(&query.len()) {
            return None;
        }
        let mut masks = [0; 256];
        for (position, byte) in query.bytes().enumerate() {
            let prefix = 1u64 << position;
            masks[usize::from(byte)] |= prefix;
            masks[usize::from(ascii_counterpart(byte, case_sensitive))] |= prefix;
        }
        let accepted = 1u64 << query.len();
        Some(Self {
            masks,
            length: query.len(),
            accepted,
            // This includes prefix 63 without shifting by 64.
            valid_prefixes: accepted | (accepted - 1),
        })
    }

    pub(super) fn matches(&self, field: &[u8], allow_deletion: bool) -> bool {
        let deletions = usize::from(allow_deletion);
        if self.length <= deletions {
            return true;
        }
        if self.length - deletions > field.len() {
            return false;
        }
        if allow_deletion {
            self.matches_one_deletion(field)
        } else {
            self.matches_exact(field)
        }
    }

    fn matches_exact(&self, field: &[u8]) -> bool {
        let mut exact = 1u64;
        for &byte in field {
            let matches = self.masks[usize::from(byte)];
            exact |= (exact & matches) << 1;
            if exact & self.accepted != 0 {
                return true;
            }
        }
        false
    }

    fn matches_one_deletion(&self, field: &[u8]) -> bool {
        let mut exact = 1u64;
        let mut deleted = 2u64;
        for &byte in field {
            let matches = self.masks[usize::from(byte)];
            let next_exact = exact | ((exact & matches) << 1);
            // Consume from old states, then spend the deletion after the new exact prefix.
            deleted =
                (deleted | ((deleted & matches) << 1) | (next_exact << 1)) & self.valid_prefixes;
            exact = next_exact;
            if (exact | deleted) & self.accepted != 0 {
                return true;
            }
        }
        false
    }
}
