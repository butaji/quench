//! The executable's text spans a native image's init edge wrote (its lane text, `aot_image::live::link_placed_lane`):
//! private file pages made dirty in place. Whatever discards clean file pages to cut the resident set must leave these
//! (discarded, they read back as the file's unlinked text), and an image dump writes them back into the file.

use std::sync::Mutex;

/// The written spans, each page-aligned `(start, end)`.
static WRITTEN: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

/// Records the page-aligned span `[start, end)` as written in place.
pub fn note_written(start: usize, end: usize) {
    let mut spans = WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !spans.contains(&(start, end)) { spans.push((start, end)); }
}

/// The spans written in place so far.
pub fn written() -> Vec<(usize, usize)> {
    WRITTEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// `range` without the written spans: the pieces of it that still hold the file's bytes.
pub fn clean_pieces((start, end): (usize, usize)) -> Vec<(usize, usize)> {
    let mut written = written();
    written.sort_unstable();
    let mut pieces = Vec::new();
    let mut at = start;
    for (from, to) in written.into_iter().filter(|&(from, to)| from < end && to > start) {
        if from > at { pieces.push((at, from)); }
        at = at.max(to);
    }
    if at < end { pieces.push((at, end)); }
    pieces
}
