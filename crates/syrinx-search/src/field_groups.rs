//! Shares bounded canonical fields for membership while releasing deleted field references.

use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

const MAX_FIELD_BYTES: usize = 1_024;
const MAX_GROUPS: usize = 2_048;
const MAX_KEY_BYTES: usize = 2 * 1_024 * 1_024;

pub(crate) type GroupId = NonZeroU32;

struct Group {
    text: Arc<str>,
    references: usize,
}

#[derive(Default)]
pub(crate) struct FieldGroups {
    keys: HashMap<Arc<str>, GroupId>,
    groups: Vec<Option<Group>>,
    free: Vec<usize>,
    key_bytes: usize,
}

impl FieldGroups {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn intern(&mut self, field: &str) -> Option<GroupId> {
        if field.len() > MAX_FIELD_BYTES {
            return None;
        }
        // ASCII digit substitution preserves all UTF-8 boundaries and every other character.
        let text: String = field
            .chars()
            .map(|character| {
                if character.is_ascii_digit() {
                    '0'
                } else {
                    character
                }
            })
            .collect();
        if let Some(&id) = self.keys.get(text.as_str()) {
            let group = self.groups[index(id)]
                .as_mut()
                .expect("live group key has a slot");
            group.references = group.references.checked_add(1)?;
            return Some(id);
        }
        // Existing groups remain available at capacity; new fields use the caller's fallback.
        if self.keys.len() == MAX_GROUPS || text.len() > MAX_KEY_BYTES - self.key_bytes {
            return None;
        }
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.groups.push(None);
                self.groups.len() - 1
            }
        };
        let id = GroupId::new((slot + 1) as u32).expect("bounded group slots have nonzero IDs");
        let text: Arc<str> = Arc::from(text);
        self.key_bytes += text.len();
        self.keys.insert(text.clone(), id);
        self.groups[slot] = Some(Group {
            text,
            references: 1,
        });
        Some(id)
    }

    /// IDs are valid only while retained; callers must discard query results before reusing slots.
    pub(crate) fn release(&mut self, id: GroupId) -> bool {
        let slot = index(id);
        let Some(group) = self.groups.get_mut(slot).and_then(Option::as_mut) else {
            return false;
        };
        if group.references > 1 {
            group.references -= 1;
            return true;
        }

        // Remove the lookup before making the slot reusable, and account for shared text once.
        let group = self.groups[slot]
            .take()
            .expect("released live group has a slot");
        assert_eq!(self.keys.remove(group.text.as_ref()), Some(id));
        self.key_bytes -= group.text.len();
        self.free.push(slot);
        true
    }

    pub(crate) fn get(&self, id: GroupId) -> Option<&str> {
        self.groups
            .get(index(id))?
            .as_ref()
            .map(|group| group.text.as_ref())
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }

    pub(crate) fn slots(&self) -> usize {
        self.groups.len()
    }

    /// Unique UTF-8 key payload; Arc, map, slot and free-list overhead are separately bounded.
    #[cfg(test)]
    pub(crate) fn storage_bytes(&self) -> usize {
        self.key_bytes
    }
}

pub(crate) fn query_safe(query: &str) -> bool {
    !query.bytes().any(|byte| byte.is_ascii_digit())
}

fn index(id: GroupId) -> usize {
    (id.get() - 1) as usize
}

#[cfg(test)]
mod tests {
    use frizbee::{Config, Matcher};

    use super::*;

    fn unique_field(mut number: usize, length: usize) -> String {
        let mut suffix = String::new();
        loop {
            suffix.push(char::from(b'a' + (number % 26) as u8));
            number /= 26;
            if number == 0 {
                break;
            }
        }
        format!("{suffix}{}", ".".repeat(length - suffix.len()))
    }

    #[test]
    fn equivalent_fields_share_one_allocation_until_the_final_release() {
        let mut groups = FieldGroups::new();
        let first = groups.intern("a012z").expect("first synthetic field");
        let second = groups.intern("a987z").expect("equivalent synthetic field");
        assert_eq!(first, second);
        assert_eq!(groups.get(first), Some("a000z"));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.slots(), 1);
        assert_eq!(groups.storage_bytes(), 5);
        let (key, _) = groups.keys.iter().next().expect("interned key");
        let slot = groups.groups[index(first)].as_ref().expect("interned slot");
        assert!(Arc::ptr_eq(key, &slot.text));

        assert!(groups.release(first));
        assert_eq!(groups.get(second), Some("a000z"));
        assert_eq!(groups.storage_bytes(), 5);
        assert!(groups.release(second));
        assert_eq!(groups.get(first), None);
        assert_eq!(groups.len(), 0);
        assert_eq!(groups.storage_bytes(), 0);
        assert!(!groups.release(first));
        let replacement = groups.intern("different").expect("reuse reclaimed slot");
        assert_eq!(replacement, first);
        assert_eq!(groups.get(replacement), Some("different"));
        assert_eq!(groups.slots(), 1);
    }

    #[test]
    fn unicode_bytes_and_non_ascii_digits_are_preserved() {
        let mut groups = FieldGroups::new();
        let original = "café12/東京34/😀56/١٢";
        let id = groups.intern(original).expect("synthetic Unicode field");
        assert_eq!(groups.get(id), Some("café00/東京00/😀00/١٢"));
        assert_eq!(groups.storage_bytes(), original.len());
        let other = groups
            .intern("café98/東京76/😀54/١٢")
            .expect("same Unicode group");
        assert_eq!(id, other);
        let changed = groups
            .intern("café98/東京76/😀54/٣٤")
            .expect("distinct Unicode digits");
        assert_ne!(id, changed);
        for query in ["", "café", "Éa", "東京", "😀", "١٢", "\0"] {
            assert!(query_safe(query));
        }
        for query in ["0", "café1", "東京9", "١٢3"] {
            assert!(!query_safe(query));
        }
    }

    #[test]
    fn byte_length_limit_and_empty_fields_preserve_fallback_state() {
        let mut groups = FieldGroups::new();
        let empty = groups.intern("").expect("empty synthetic field");
        assert_eq!(groups.get(empty), Some(""));
        assert_eq!(groups.storage_bytes(), 0);
        let boundary = format!("é{}", "1".repeat(MAX_FIELD_BYTES - 2));
        let id = groups.intern(&boundary).expect("field at byte limit");
        assert_eq!(
            groups.get(id).expect("canonical boundary field").len(),
            MAX_FIELD_BYTES
        );
        assert_eq!(groups.intern(&format!("{boundary}1")), None);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups.storage_bytes(), MAX_FIELD_BYTES);
        assert_eq!(
            groups.get(GroupId::new(u32::MAX).expect("nonzero missing ID")),
            None
        );
        assert!(!groups.release(GroupId::new(u32::MAX).expect("nonzero missing ID")));
    }

    #[test]
    fn reference_overflow_refuses_an_extra_lease_without_changing_the_group() {
        let mut groups = FieldGroups::new();
        let id = groups.intern("a1z").expect("synthetic group");
        groups.groups[index(id)]
            .as_mut()
            .expect("live group")
            .references = usize::MAX;
        assert_eq!(groups.intern("a9z"), None);
        assert_eq!(
            groups.groups[index(id)]
                .as_ref()
                .expect("live group")
                .references,
            usize::MAX
        );
        assert_eq!(groups.get(id), Some("a0z"));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.storage_bytes(), 3);
    }

    #[test]
    fn full_capacity_still_interns_existing_keys_and_reclaims_storage() {
        let mut groups = FieldGroups::new();
        let ids: Vec<_> = (0..MAX_GROUPS)
            .map(|number| {
                groups
                    .intern(&unique_field(number, MAX_FIELD_BYTES))
                    .expect("bounded field")
            })
            .collect();
        assert_eq!(groups.len(), MAX_GROUPS);
        assert_eq!(groups.slots(), MAX_GROUPS);
        assert_eq!(groups.storage_bytes(), MAX_KEY_BYTES);
        assert_eq!(
            groups.intern(&unique_field(MAX_GROUPS, MAX_FIELD_BYTES)),
            None
        );
        let repeated = groups
            .intern(&unique_field(0, MAX_FIELD_BYTES))
            .expect("existing key at cap");
        assert_eq!(repeated, ids[0]);
        assert!(groups.release(repeated));
        assert_eq!(groups.storage_bytes(), MAX_KEY_BYTES);
        assert!(groups.release(ids[0]));
        assert_eq!(groups.len(), MAX_GROUPS - 1);
        assert_eq!(groups.storage_bytes(), MAX_KEY_BYTES - MAX_FIELD_BYTES);
        let replacement = groups
            .intern(&unique_field(MAX_GROUPS, MAX_FIELD_BYTES))
            .expect("reclaimed capacity");
        assert_eq!(replacement, ids[0]);
        assert_eq!(groups.storage_bytes(), MAX_KEY_BYTES);
        assert_eq!(groups.slots(), MAX_GROUPS);
    }

    #[test]
    fn repeated_unique_replacements_do_not_grow_the_slot_table() {
        let mut groups = FieldGroups::new();
        for number in 0..MAX_GROUPS * 2 {
            let id = groups
                .intern(&unique_field(number, 20))
                .expect("transient synthetic field");
            assert_eq!(groups.slots(), 1);
            assert!(groups.release(id));
            assert_eq!(groups.len(), 0);
            assert_eq!(groups.storage_bytes(), 0);
        }
    }

    #[test]
    fn canonical_membership_matches_original_for_digit_free_ascii_and_unicode_queries() {
        let fields = [
            "",
            "a012z",
            "a987z",
            "0aB9",
            "9aB0",
            "café012",
            "café987",
            "É1a",
            "É9a",
            "東京12😀",
            "東京98😀",
            "١٢1",
            "١٢9",
            "a\0b012",
            "a\0b987",
            "\0",
            "012",
            "987",
        ];
        let mut groups = FieldGroups::new();
        let ids: Vec<_> = fields
            .iter()
            .map(|field| groups.intern(field).expect("synthetic field"))
            .collect();
        for query in ["a", "az", "aB", "café", "Éa", "東京😀", "١٢", "\0", "a\0b"] {
            assert!(query_safe(query));
            for max_typos in [0, 1] {
                let config = Config::default().max_typos(Some(max_typos));
                let mut matcher = Matcher::new(query, &config);
                for (original, id) in fields.iter().zip(&ids) {
                    let canonical = groups.get(*id).expect("retained synthetic group");
                    assert_eq!(original.len(), canonical.len());
                    assert_eq!(original.is_ascii(), canonical.is_ascii());
                    assert_eq!(
                        matcher.match_one(*original, 0).is_some(),
                        matcher.match_one(canonical, 0).is_some(),
                        "synthetic query {query:?}, field {original:?}, typos {max_typos}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod membership_tests {
    use frizbee::{Config, Matcher};

    use super::{FieldGroups, query_safe};
    use crate::membership::Membership;

    fn compare(query: &str, original: &str, canonical: &str) {
        let membership = Membership::new(query);
        for budget in [0, 1] {
            let config = Config::default().max_typos(Some(budget));
            let mut matcher = Matcher::new(query, &config);
            let expected = matcher.match_one(original, 0).is_some();
            assert_eq!(
                matcher.match_one(canonical, 0).is_some(),
                expected,
                "canonical public membership: {query:?}, {original:?}, budget {budget}"
            );
            for field in [original, canonical] {
                assert_eq!(
                    membership.matches(field, budget),
                    Some(expected),
                    "scalar membership: {query:?}, {field:?}, budget {budget}"
                );
                if field.is_ascii() {
                    assert_eq!(
                        membership.matches_ascii(field, budget),
                        Some(expected),
                        "ASCII field membership: {query:?}, {field:?}, budget {budget}"
                    );
                }
            }
        }
        assert_eq!(membership.matches(canonical, 2), None);
    }

    #[test]
    fn canonical_fields_preserve_membership_paths_and_deletion_boundaries() {
        let mut queries: Vec<String> = [
            "",
            "a",
            "aB",
            "Éa",
            "éa",
            "éé",
            "ſa",
            "ßa",
            "σa",
            "ıa",
            "İa",
            "Ka",
            "e\u{301}a",
            "東京😀",
            "١٢",
            "\0",
            "a\0b",
            "\0\0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for length in [62, 63, 64, 65, 96] {
            queries.push("aB_\0".chars().cycle().take(length).collect());
        }

        for query in queries {
            assert!(query_safe(&query));
            let scalars: Vec<char> = query.chars().collect();
            let mut shapes = vec![
                String::new(),
                query.clone(),
                query.to_lowercase(),
                query.to_uppercase(),
                "aA\0éÉßſσΣıİK東京😀١٢".to_owned(),
                query.chars().filter(char::is_ascii).collect(),
            ];
            for removed in [1, 2] {
                shapes.push(scalars.iter().skip(removed).collect());
                shapes.push(
                    scalars
                        .iter()
                        .take(scalars.len().saturating_sub(removed))
                        .collect(),
                );
            }
            for shape in shapes {
                let mut groups = FieldGroups::new();
                let mut shared = None;
                for digit in ['0', '1', '9'] {
                    let original = format!(
                        "{digit}{}{digit}",
                        shape
                            .chars()
                            .map(|character| format!("{character}{digit}"))
                            .collect::<String>()
                    );
                    let id = groups.intern(&original).expect("bounded synthetic field");
                    if let Some(previous) = shared {
                        assert_eq!(id, previous);
                    }
                    shared = Some(id);
                    compare(&query, &original, groups.get(id).expect("retained group"));
                }
            }
        }
    }

    #[test]
    fn digit_query_guard_prevents_a_demonstrably_incorrect_group_reuse() {
        let mut groups = FieldGroups::new();
        let original = "a9b";
        let id = groups.intern(original).expect("synthetic field");
        let canonical = groups.get(id).expect("retained group");
        for query in ["0", "a0b", "é0"] {
            assert!(!query_safe(query));
        }
        let membership = Membership::new("a0b");
        assert_eq!(membership.matches(original, 0), Some(false));
        assert_eq!(membership.matches(canonical, 0), Some(true));
    }
}
