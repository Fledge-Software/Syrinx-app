//! Tests compact query-local digit transitions against the scalar membership recurrence.

use super::{Needle, State};

const WIDTH: usize = 16;
const STATES: usize = WIDTH * WIDTH;
const EXACT_ACCEPTED: u8 = 0x80;
const PAIR_ACCEPTED: u16 = 0x8000;

pub(super) enum DigitTransitions {
    Exact {
        table: Box<[u8; 10 * WIDTH]>,
        steps: usize,
    },
    One {
        table: Box<[u16; 10 * STATES]>,
        steps: usize,
    },
}

impl DigitTransitions {
    pub(super) fn new(needle: &Needle) -> Option<Self> {
        if needle.length() > 15 {
            return None;
        }
        Some(if needle.deletion {
            one_deletion_table(needle)
        } else {
            exact_table(needle)
        })
    }

    pub(super) fn bytes(&self) -> usize {
        match self {
            Self::Exact { table, .. } => std::mem::size_of_val(table.as_ref()),
            Self::One { table, .. } => std::mem::size_of_val(table.as_ref()),
        }
    }

    pub(super) fn steps(&self) -> usize {
        match self {
            Self::Exact { steps, .. } | Self::One { steps, .. } => *steps,
        }
    }

    /// Consumes an original digit run; None means accepted, Some retains the unfinished state.
    /// The live canonical group guarantees these run offsets contain ASCII digits.
    pub(super) fn fold(&self, state: State, bytes: &[u8]) -> Option<State> {
        match self {
            Self::Exact { table, .. } => {
                let mut exact = state.exact;
                for &byte in bytes {
                    exact = table[usize::from(byte - b'0') * WIDTH + usize::from(exact)];
                    if exact & EXACT_ACCEPTED != 0 {
                        return None;
                    }
                }
                Some(State { exact, deleted: 0 })
            }
            Self::One { table, .. } => {
                let mut pair = pack(state);
                for &byte in bytes {
                    pair = table[usize::from(byte - b'0') * STATES + usize::from(pair)];
                    if pair & PAIR_ACCEPTED != 0 {
                        return None;
                    }
                }
                Some(unpack(pair))
            }
        }
    }
}

fn exact_table(needle: &Needle) -> DigitTransitions {
    let mut table = Box::new([0; 10 * WIDTH]);
    for digit in 0..10 {
        for exact in 0..=needle.length() {
            let output = needle.step(State { exact, deleted: 0 }, b'0' + digit);
            table[usize::from(digit) * WIDTH + usize::from(exact)] = output.exact
                | if needle.accepted(output) {
                    EXACT_ACCEPTED
                } else {
                    0
                };
        }
    }
    DigitTransitions::Exact {
        table,
        steps: 10 * (needle.pairs.len() + 1),
    }
}

fn one_deletion_table(needle: &Needle) -> DigitTransitions {
    let mut table = Box::new([0; 10 * STATES]);
    for digit in 0..10 {
        let start = usize::from(digit) * STATES;
        fill_digit_pairs(needle, digit, &mut table[start..start + STATES]);
    }
    DigitTransitions::One {
        table,
        steps: 10 * (needle.pairs.len() + 1).pow(2),
    }
}

fn fill_digit_pairs(needle: &Needle, digit: u8, table: &mut [u16]) {
    for exact in 0..=needle.length() {
        for deleted in 0..=needle.length() {
            let input = State { exact, deleted };
            let output = needle.step(input, b'0' + digit);
            table[usize::from(pack(input))] = pack(output)
                | if needle.accepted(output) {
                    PAIR_ACCEPTED
                } else {
                    0
                };
        }
    }
}

fn pack(state: State) -> u16 {
    u16::from(state.exact) | (u16::from(state.deleted) << 4)
}

fn unpack(pair: u16) -> State {
    State {
        exact: (pair & 15) as u8,
        deleted: ((pair >> 4) & 15) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_endpoint(needle: &Needle, transitions: &DigitTransitions, input: State) {
        for byte in b'0'..=b'9' {
            let expected = needle.step(input, byte);
            let result = transitions.fold(input, &[byte]);
            if needle.accepted(expected) {
                assert!(result.is_none());
            } else {
                let actual = result.expect("not yet accepted");
                assert_eq!(
                    (actual.exact, actual.deleted),
                    (expected.exact, expected.deleted)
                );
            }
        }
    }

    fn assert_endpoints(needle: &Needle, transitions: &DigitTransitions) {
        for exact in 0..=needle.length() {
            for deleted in 0..=needle.length() {
                assert_endpoint(needle, transitions, State { exact, deleted });
            }
        }
    }

    #[test]
    fn every_endpoint_and_digit_agrees_with_scalar_transitions() {
        for length in 1..=15 {
            let query: String = "a1B2\0z9087654321"
                .chars()
                .cycle()
                .take(length - 1)
                .chain(['1'])
                .collect();
            for budget in [0, 1] {
                let needle = Needle::new(query.as_str(), budget).expect("supported query");
                let transitions = DigitTransitions::new(&needle).expect("short query");
                assert_eq!(transitions.bytes(), if budget == 0 { 160 } else { 5_120 });
                assert_eq!(
                    transitions.steps(),
                    10 * (length + 1).pow(if budget == 0 { 1 } else { 2 })
                );
                assert_endpoints(&needle, &transitions);
            }
        }
        let long = Needle::new("1234567890123456", 1).expect("supported scalar query");
        assert!(DigitTransitions::new(&long).is_none());
    }
}
