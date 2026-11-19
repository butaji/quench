//! The compact records a function slot and a method site keep: an unlowered record's cell,
//! a deferred record's place in its program's encoding, and a method site's receiver path.
//! Each holds its fields in the fewest words they take, so a slot of a large bundle, most of
//! whose functions never run, pays for these words alone.

use super::{Atom, NO_WORD, Unlowered};

/// A slot's `Option<Unlowered>`, read by taking it out and putting it back, so the slot pays
/// for the option's sixteen bytes alone: a `RefCell`'s borrow flag would add eight to every
/// slot, and boxing the option would add an allocation to each unlowered function (most of a
/// large bundle's functions). Single-threaded, and no read reaches the cell again.
pub(super) struct UnloweredCell(std::cell::Cell<Option<Unlowered>>);

impl UnloweredCell {
    pub(super) fn new(unlowered: Option<Unlowered>) -> Self {
        Self(std::cell::Cell::new(unlowered))
    }

    /// `read` applied to the held record, or `None`.
    pub(super) fn with<R>(&self, read: impl FnOnce(Option<&Unlowered>) -> R) -> R {
        let held = self.0.take();
        let result = read(held.as_ref());
        self.0.set(held);
        result
    }
}

impl std::ops::Deref for UnloweredCell {
    type Target = std::cell::Cell<Option<Unlowered>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Clone for UnloweredCell {
    fn clone(&self) -> Self {
        Self(std::cell::Cell::new(self.with(|held| held.cloned())))
    }
}

impl std::fmt::Debug for UnloweredCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|held| std::fmt::Debug::fmt(&held, formatter))
    }
}

/// A method site's receiver path (`MethodSite::receiver_path`): the atom and inline cache of
/// the field its receiver is read from, or none. An `Option<(Atom, u16)>` takes twelve bytes;
/// this takes eight, its atom's `NO_WORD` standing for none.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ReceiverPath {
    atom: Atom,
    cache: u16,
}

impl ReceiverPath {
    pub(crate) const NONE: Self = Self { atom: NO_WORD, cache: 0 };

    pub(crate) const fn at(atom: Atom, cache: u16) -> Self {
        assert!(atom != NO_WORD, "a receiver path's atom reaches u32::MAX");
        Self { atom, cache }
    }

    pub(crate) fn get(self) -> Option<(Atom, u16)> {
        (self.atom != NO_WORD).then_some((self.atom, self.cache))
    }
}

impl From<Option<(Atom, u16)>> for ReceiverPath {
    fn from(path: Option<(Atom, u16)>) -> Self {
        path.map_or(Self::NONE, |(atom, cache)| Self::at(atom, cache))
    }
}

/// Where a deferred record lies in its program's encoding, and its checksum, which decoding
/// it checks: the load checked only the encoding's body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DeferredRecord {
    pub(crate) start: u32,
    /// The length above two flags, in a word that is never zero (`PRESENT`), so an
    /// `Option<DeferredRecord>` takes no word of its own.
    word: std::num::NonZeroU32,
    pub(crate) checksum: u64,
}

impl DeferredRecord {
    /// Whether the record is encoded unlowered, which its first read lowers.
    const UNLOWERED: u32 = 1 << 0;
    /// Whether its rewrites wait for that first read too (`RewriteRanges::deferred`), so the
    /// read settles them before it lowers the record.
    const REWRITES_PENDING: u32 = 1 << 1;
    const PRESENT: u32 = 1 << 2;
    const LENGTH_SHIFT: u32 = 3;
    /// The longest record: the length takes the word's bits above the flags.
    pub(crate) const MAX_LENGTH: u32 = u32::MAX >> Self::LENGTH_SHIFT;

    /// A record of `length` bytes (at most `MAX_LENGTH`) at `start`, or `None` when it is longer.
    pub(crate) fn new(start: u32, length: u32, checksum: u64, unlowered: bool, rewrites_pending: bool) -> Option<Self> {
        if length > Self::MAX_LENGTH {
            return None;
        }
        let flag = |set: bool, bit: u32| if set { bit } else { 0 };
        let flags = flag(unlowered, Self::UNLOWERED) | flag(rewrites_pending, Self::REWRITES_PENDING) | Self::PRESENT;
        let word = std::num::NonZeroU32::new(flags | length << Self::LENGTH_SHIFT)?;
        Some(Self { start, word, checksum })
    }

    pub(crate) fn length(self) -> usize {
        (self.word.get() >> Self::LENGTH_SHIFT) as usize
    }

    pub(crate) fn unlowered(self) -> bool {
        self.word.get() & Self::UNLOWERED != 0
    }

    pub(crate) fn rewrites_pending(self) -> bool {
        self.word.get() & Self::REWRITES_PENDING != 0
    }
}

const _: () = assert!(size_of::<Option<DeferredRecord>>() == 16);
const _: () = assert!(size_of::<ReceiverPath>() == 8);
