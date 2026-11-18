//! Regular expression literals whose early errors were already checked. A script evaluated
//! again (Octane code-load's cache-busted jQuery and Closure, each eval) holds the same
//! literals, and their validation parses each pattern in full: 12.5K instructions a literal,
//! 1,441 literals over twelve iterations. V8 keys its RegExp boilerplate the same way, by
//! pattern and flags.
use rustc_hash::FxHashSet;
use std::cell::RefCell;

/// Literal texts kept before the set starts over, bounding what a program generating
/// patterns can make it hold.
const VALID_LITERALS_CAPACITY: usize = 4096;

thread_local! {
    static VALID_LITERALS: RefCell<FxHashSet<Box<str>>> = RefCell::default();
}

/// `error`'s early error of `literal`, a `/pattern/flags` text, unless an earlier literal of
/// the same text validated without one.
pub(super) fn checked<E>(literal: &str, error: impl FnOnce(&str) -> Option<E>) -> Option<E> {
    if VALID_LITERALS.with(|valid| valid.borrow().contains(literal)) {
        return None;
    }
    let found = error(literal);
    if found.is_none() {
        note_valid(literal);
    }
    found
}

/// Records that `literal` validated without an early error.
fn note_valid(literal: &str) {
    VALID_LITERALS.with(|valid| {
        let mut valid = valid.borrow_mut();
        if valid.len() >= VALID_LITERALS_CAPACITY {
            valid.clear();
        }
        valid.insert(literal.into());
    });
}
