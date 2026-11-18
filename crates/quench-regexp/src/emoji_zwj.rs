//! The RGI emoji ZWJ sequences (`RGI_Emoji_ZWJ_Sequence`, Unicode 17.0). `emoji_zwj.txt`
//! holds one per line, sorted by code point; generated from Unicode's
//! emoji-zwj-sequences.txt by way of test262's `RGI_Emoji_ZWJ_Sequence.js`. Regenerate it
//! rather than edit it.

use std::sync::OnceLock;

/// The sequences, one per line, in code point order (which UTF-8 byte order is).
const SEQUENCES: &str = include_str!("emoji_zwj.txt");

/// Whether the code points `units` are one RGI emoji ZWJ sequence: a binary search of the
/// list's lines, split once.
pub(crate) fn is_rgi_zwj_sequence(units: impl Iterator<Item = u32> + Clone) -> bool {
    static LINES: OnceLock<Box<[&'static str]>> = OnceLock::new();
    let lines = LINES.get_or_init(|| SEQUENCES.lines().collect());
    lines.binary_search_by(|line| line.chars().map(u32::from).cmp(units.clone())).is_ok()
}
