//! Generates repeatable, entirely fictional page text for the full-text comparison.

pub const SEED: u64 = 0x4154_4c41_5323;
pub const NOW: i64 = 10 * 86_400;
pub const CUTOFF: i64 = NOW - 7 * 86_400;
pub const QUERIES: [&str; 12] = [
    "lantern",
    "orb*",
    "\"copper meadow\"",
    "café",
    "naive",
    "naïve",
    "星砂",
    "сад",
    "lantern AND violet",
    "copper OR orchard",
    "sigil99999",
    "absentquartzword",
];

pub struct Document {
    pub id: usize,
    pub title: String,
    pub url: String,
    pub body: String,
    pub captured: i64,
}

pub fn document(id: usize) -> Document {
    // Every tenth page repeats its neighbor's body except for the final identifier.
    let group = id - usize::from(id % 10 == 9);
    let mut state = SEED ^ group as u64;
    let words = [
        "lantern",
        "violet",
        "orbit",
        "copper meadow",
        "orchard",
        "velvet",
        "café",
        "naïve",
        "résumé",
        "jardín",
        "星砂",
        "сад",
        "月舟",
        "atelier",
    ];
    let length = if id.is_multiple_of(100) { 4096 } else { 128 };
    let mut body = String::new();
    for _ in 0..length {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        body.push_str(words[state as usize % words.len()]);
        body.push(' ');
    }
    body.push_str(&format!("sigil{id}"));
    Document {
        id,
        title: format!("Fictional observatory folio {}", group % 997),
        url: format!("https://shelf{}.example.test/folio/{id}", id % 97),
        body,
        captured: (id % 10) as i64 * 86_400,
    }
}
