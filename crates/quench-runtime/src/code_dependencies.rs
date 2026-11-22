//! Weak native publications indexed by the shape or slot owner they depend on.
//! These notifications do not turn observations into unchecked read permissions.

use std::{num::NonZeroU32, rc::{Rc, Weak}};
use crate::{folded_roots::{FoldedRoots, RootedCode}, typed_ir::Type};

pub trait DependentCode {
    fn is_published(&self) -> bool;
    fn retire(&self);
}

/// The word a body's code reads at its invalidation points: zero while every assumption the
/// code folded holds, nonzero once a change retired it. Code that holds an assumption (a
/// Stable fact, `typed_ir::BindingTime::Stable`) pays no check per use: it reads this word
/// once at entry and again only after a call that may have changed the assumption's subject,
/// and leaves at the exact PC where it is set.
#[derive(Clone, Default)]
pub struct Invalidation(Rc<std::cell::Cell<u64>>);

impl Invalidation {
    /// The word's address, which lives as long as any holder of this invalidation.
    pub fn address(&self) -> usize { self.0.as_ptr() as usize }
    #[cfg(test)]
    pub(crate) fn is_set(&self) -> bool { self.0.get() != 0 }
    fn set(&self) { self.0.set(1); }
}

/// A dependency with no code behind it: published until something it was subscribed to
/// retires it (`PublishedCode::retire`), for proofs held as data rather than as code.
pub fn new_dependency() -> Rc<PublishedCode<()>> { Rc::new(PublishedCode::new(())) }

/// Publication owns future entry; execution and recovery own independent leases.
/// No borrow of the publication survives acquiring a lease or retiring it.
pub struct PublishedCode<T> {
    code: std::cell::RefCell<Option<Rc<T>>>,
    /// Whether `code` still holds it: read on every cache hit, so a plain flag rather than a
    /// borrow of `code`.
    published: std::cell::Cell<bool>,
    /// Set when the code retires, for runs already inside it (`Invalidation`).
    invalidation: Invalidation,
    /// The entry words to put back (`stub`) while they still hold this code's `entry`.
    entry_links: std::cell::RefCell<smallvec::SmallVec<[EntryLink; ENTRY_WORDS]>>,
    /// The heap identities its code embeds, kept alive while it is published (`hold_roots`).
    roots: std::cell::RefCell<FoldedRoots>,
}

/// An entry word, the entry it holds and the stub it returns to: a view's cell words (`EntryCell`).
type EntryLink = (*const std::cell::Cell<usize>, usize, usize);

/// The words of a view's cell that a published body owns: its `body`, its `entry` and its `scalar` entry.
const ENTRY_WORDS: usize = 3;

impl<T> PublishedCode<T> {
    /// Names an entry word whose `entry` this code retiring takes back to `stub`.
    pub(crate) fn on_retire(&self, word: &std::cell::Cell<usize>, entry: usize, stub: usize) {
        self.entry_links.borrow_mut().push((std::ptr::from_ref(word), entry, stub));
    }
    pub fn new(code: T) -> Self { Self::with_invalidation(code, Invalidation::default()) }
    /// Code compiled to read `invalidation` at its invalidation points.
    pub(crate) fn with_invalidation(code: T, invalidation: Invalidation) -> Self {
        Self { code: std::cell::RefCell::new(Some(Rc::new(code))), published: std::cell::Cell::new(true), invalidation, entry_links: Default::default(), roots: Default::default() }
    }
    /// Keeps `roots`, the heap identities its code embeds, alive until it retires: `heap` roots
    /// them at every collection (`Heap::extend_folded_roots`).
    pub fn hold_roots(self: &Rc<Self>, roots: FoldedRoots, heap: &mut crate::heap::Heap) where T: 'static {
        if roots.is_empty() { return; }
        *self.roots.borrow_mut() = roots.settled();
        let code: Rc<dyn RootedCode> = self.clone();
        heap.hold_folded_roots(&code);
    }
    pub fn lease(&self) -> Option<Rc<T>> { self.code.borrow().clone() }
    #[inline(always)]
    pub fn is_published(&self) -> bool { self.published.get() }
    pub fn retire(&self) {
        self.invalidation.set();
        *self.roots.borrow_mut() = FoldedRoots::default();
        let links = std::mem::take(&mut *self.entry_links.borrow_mut());
        for (word, entry, stub) in links {
            // SAFETY: the word is a view's, which outlives the publications it names (the
            // view owns its body and its pins).
            let word = unsafe { &*word };
            if word.get() == entry { word.set(stub); }
        }
        self.code.borrow_mut().take();
        self.published.set(false);
    }
}

impl<T> DependentCode for PublishedCode<T> {
    fn is_published(&self) -> bool { self.is_published() }
    fn retire(&self) { self.retire(); }
}

impl<T> RootedCode for PublishedCode<T> {
    fn is_published(&self) -> bool { self.is_published() }
    fn extend_roots(&self, roots: &mut Vec<crate::Value>) { self.roots.borrow().extend_roots(roots); }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotDependency {
    pub shape: u32,
    pub slot: u16,
    pub ty: Type,
}

/// The prototypes a Stable fact was read through: the `depth` links from the prototype the encoded
/// link `link` names (`heap::ProtoLink::bits`), nearest first, the holder last. Code holding the
/// fact holds each one's layout (`Heap::subscribe_prototype_chain`): a change to one of them (its
/// shape, its prototype link, its integrity, a descriptor its shape does not record) or its death
/// retires the code, and a change to a prototype off the chain leaves it standing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrototypeChain {
    pub(crate) link: u32,
    pub(crate) depth: u8,
}

/// What an `instanceof` of one constructor depends on when its code takes the constructor's
/// `prototype` as a Stable fact (`Vm::instanceof_fact`): the encoded link of that prototype and the
/// slot of the constructor's own `prototype` property in its exact shape (a write retires the
/// code, as does a departure of that shape, which an own `@@hasInstance` would be), the exact
/// shape of the holder the constructor inherits the native `@@hasInstance` from (a change of its
/// values retires the code), and the chain from the constructor itself to that holder, whose
/// lookup a new prototype or an `@@hasInstance` added on the way would change (a layout change of
/// one retires the code).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstanceofFact {
    pub link: u32,
    pub shape: u32,
    pub(crate) slot: u16,
    pub holder: u32,
    pub chain: PrototypeChain,
}

impl InstanceofFact {
    /// The slot dependency on the constructor's `prototype`, an object.
    pub fn slot_dependency(self) -> SlotDependency {
        SlotDependency { shape: self.shape, slot: self.slot, ty: Type::DIRECT_OBJECT_FAMILY }
    }
}

type Link = Option<NonZeroU32>;
struct Reader {
    code: Option<Weak<dyn DependentCode>>,
    next: Link,
}

#[derive(Default)]
pub struct CodeReaders {
    heads: Vec<Link>,
    readers: Vec<Reader>,
    free: Link,
}

impl CodeReaders {
    pub(crate) fn memory_bytes(&self) -> usize {
        self.heads.capacity() * size_of::<Link>() + self.readers.capacity() * size_of::<Reader>()
    }

    fn release(&mut self, link: NonZeroU32) -> Link {
        let reader = &mut self.readers[(link.get() - 1) as usize];
        let next = reader.next;
        reader.code = None;
        reader.next = self.free;
        self.free = Some(link);
        next
    }

    /// Publication is cold: prune expired readers and deduplicate the same code
    /// here, rather than accumulating dead weak handles on an unchanged owner.
    pub fn subscribe(&mut self, owner: u32, code: &Rc<dyn DependentCode>) -> bool {
        let owner = owner as usize;
        self.heads.resize(self.heads.len().max(owner + 1), None);
        let weak = Rc::downgrade(code);
        let mut cursor = self.heads[owner];
        let mut previous: Link = None;
        while let Some(link) = cursor {
            let reader = &self.readers[(link.get() - 1) as usize];
            let held = reader.code.as_ref().expect("a linked reader is occupied");
            if held.upgrade().is_some_and(|code| code.is_published()) {
                if Weak::ptr_eq(held, &weak) { return true; }
                previous = cursor;
                cursor = reader.next;
            } else {
                cursor = self.release(link);
                match previous {
                    Some(previous) => self.readers[(previous.get() - 1) as usize].next = cursor,
                    None => self.heads[owner] = cursor,
                }
            }
        }
        let link = match self.free {
            Some(link) => {
                self.free = self.readers[(link.get() - 1) as usize].next;
                link
            }
            None => {
                let Some(link) = self.readers.len().checked_add(1)
                    .and_then(|length| u32::try_from(length).ok()).and_then(NonZeroU32::new) else {
                    return false;
                };
                self.readers.push(Reader { code: None, next: None });
                link
            }
        };
        self.readers[(link.get() - 1) as usize] = Reader { code: Some(weak), next: self.heads[owner] };
        self.heads[owner] = Some(link);
        true
    }

    /// A widening visits only this owner's readers, before the incompatible effect. An owner
    /// without readers, every owner of a run without native code, is the test alone; the
    /// walk is out of line (`retire_readers`).
    /// Whether the owner had readers.
    #[inline(always)]
    pub(crate) fn retire(&mut self, owner: u32) -> bool {
        let readers = self.heads.get(owner as usize).is_some_and(Option::is_some);
        if readers {
            self.retire_readers(owner);
        }
        readers
    }

    #[inline(never)]
    fn retire_readers(&mut self, owner: u32) {
        self.take(owner).into_iter().for_each(|code| code.retire());
    }

    pub(crate) fn reset(&mut self) {
        self.take_all().into_iter().for_each(|code| code.retire());
    }

    /// Unlinks `owner`'s readers and gives the code still alive among them, for a caller to
    /// retire once it no longer holds the table these readers live in (`heap::Dependencies`).
    pub(crate) fn take(&mut self, owner: u32) -> Vec<Rc<dyn DependentCode>> {
        let Some(head) = self.heads.get_mut(owner as usize) else { return Vec::new() };
        let mut cursor = head.take();
        let mut taken = Vec::new();
        while let Some(link) = cursor {
            taken.extend(self.readers[(link.get() - 1) as usize].code.as_ref().and_then(Weak::upgrade));
            cursor = self.release(link);
        }
        taken
    }

    /// `take` of every owner: an event no owner names.
    pub(crate) fn take_all(&mut self) -> Vec<Rc<dyn DependentCode>> {
        let taken = self.readers.iter_mut().filter_map(|reader| reader.code.take().and_then(|code| code.upgrade())).collect();
        self.heads.clear();
        self.readers.clear();
        self.free = None;
        taken
    }
}

impl Drop for CodeReaders {
    fn drop(&mut self) { self.reset(); }
}

/// How a fuse's holder names its method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FuseKey {
    IteratorSymbol,
    /// `next`, of an iterator prototype.
    Next,
    Named(&'static str),
    /// The method a row names: its key is the row's name (`native_rows::row_name`).
    Method,
    /// `@@species`, a getter-only accessor whose slot holds the getter.
    SpeciesSymbol,
}

/// The one list of fuses: each variant with its holder's key and the realm's intrinsic. The
/// enum, its count and its key/intrinsic table (`IntactFuse::SPEC`) are derived from it.
macro_rules! intact_fuses {
    ($($(#[$doc:meta])* $name:ident => ($key:expr, $native:ident),)*) => {
        /// A fact about one realm's intrinsic method that fast paths rely on with no check per use
        /// (a fuse): the realm's holder keeps the fuse's key as an own data property holding the
        /// realm's intrinsic. A lookup proves it once (`Vm::fuse_intact`); a write to that slot of the
        /// holder pops it for good (`Fuses::note_slot_write`), and a change of the holder's layout,
        /// which may move the slot, only sends the next use back to one proof.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub(crate) enum IntactFuse {
            $($(#[$doc])* $name,)*
        }

        impl IntactFuse {
            pub(crate) const COUNT: usize = [$(IntactFuse::$name),*].len();
            /// Each fuse's key and intrinsic, in variant order.
            pub(crate) const SPEC: [(FuseKey, crate::heap::Native); IntactFuse::COUNT] =
                [$(($key, crate::heap::Native::$native)),*];
        }
    };
}

intact_fuses! {
    /// `Array.prototype[@@iterator]` is the realm's `Array.prototype.values`.
    ArrayIterator => (FuseKey::IteratorSymbol, ArrayValues),
    /// `%ArrayIteratorPrototype%.next` is the realm's intrinsic `next`.
    ArrayIteratorNext => (FuseKey::Next, ArrayIteratorNext),
    /// `Set.prototype[@@iterator]` is the realm's `Set.prototype.values`.
    SetIterator => (FuseKey::IteratorSymbol, SetValues),
    /// `%SetIteratorPrototype%.next` is the realm's intrinsic `next`.
    SetIteratorNext => (FuseKey::Next, IteratorNext),
    /// `Map.prototype[@@iterator]` is the realm's `Map.prototype.entries`.
    MapIterator => (FuseKey::IteratorSymbol, MapEntries),
    /// `%MapIteratorPrototype%.next` is the realm's intrinsic `next`.
    MapIteratorNext => (FuseKey::Next, IteratorNext),
    /// `String.prototype[@@iterator]` is the realm's intrinsic.
    StringIterator => (FuseKey::IteratorSymbol, StringValues),
    /// `%StringIteratorPrototype%.next` is the realm's intrinsic `next`.
    StringIteratorNext => (FuseKey::Next, StringIteratorNext),
    StringCharCodeAt => (FuseKey::Method, StringCharCodeAt),
    StringIncludes => (FuseKey::Method, StringIncludes),
    RegExpTest => (FuseKey::Named("test"), RegExpTest),
    RegExpExec => (FuseKey::Named("exec"), RegExpExec),
    /// `Array.prototype.constructor` is the realm's `Array`.
    ArrayConstructor => (FuseKey::Named("constructor"), Array),
    /// `Array[@@species]` is the intrinsic getter, a getter-only accessor.
    ArraySpecies => (FuseKey::SpeciesSymbol, ArraySpecies),
    /// `RegExp[@@species]` is the intrinsic getter, a getter-only accessor: the splitter
    /// `@@split` constructs is the realm's RegExp (`Vm::regexp_literal_argument_of`).
    RegExpSpecies => (FuseKey::SpeciesSymbol, RegExpSpecies),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum FuseState {
    #[default]
    Unproved,
    Intact,
    Popped,
}

/// One fuse's proof: of `holder`, a watched prototype, whose `slot` held the intrinsic.
#[derive(Clone, Copy)]
struct FuseRecord {
    holder: crate::Value,
    subject: u32,
    slot: u32,
    state: FuseState,
}

impl Default for FuseRecord {
    fn default() -> Self {
        Self { holder: crate::Value::UNDEFINED, subject: u32::MAX, slot: 0, state: FuseState::Unproved }
    }
}

/// Every fuse's proof, kept by the heap's dependency table, which hears the writes and layout
/// changes of the watched prototypes they name (`heap::dependencies::Watch`).
#[derive(Default)]
pub(crate) struct Fuses {
    records: [FuseRecord; IntactFuse::COUNT],
    /// Per fuse, whether its holder changed after a proof (a value it guarded may have reached
    /// guest code, `Vm::regexp_literal_receiver`), and whether a cached value relies on it.
    changed: u16,
    guarded: u16,
}

impl Fuses {
    /// Whether `fuse` holds for `holder`; `None` where no proof for `holder` is held.
    #[inline(always)]
    pub(crate) fn intact(&self, fuse: IntactFuse, holder: crate::Value) -> Option<bool> {
        let record = &self.records[fuse as usize];
        match record.state {
            _ if record.holder != holder => None,
            FuseState::Intact => Some(true),
            FuseState::Popped => Some(false),
            FuseState::Unproved => None,
        }
    }

    /// The slot a proof of `fuse` names: where its holder keeps the intrinsic while it is
    /// intact.
    #[inline(always)]
    pub(crate) fn slot(&self, fuse: IntactFuse) -> usize {
        self.records[fuse as usize].slot as usize
    }

    /// Whether `fuse`'s holder ever changed the slot or layout a proof of it named.
    #[inline(always)]
    pub(crate) fn ever_changed(&self, fuse: IntactFuse) -> bool {
        self.changed >> fuse as usize & 1 != 0
    }

    /// Notes that a cached value relies on `fuse` never changing: dropping every proof
    /// (`forget_all`) counts as a change of it.
    pub(crate) fn guard(&mut self, fuse: IntactFuse) {
        self.guarded |= 1 << fuse as usize;
    }

    /// Records the proof of `fuse`: `holder`, watched as `subject`, holds the intrinsic at
    /// `slot`, or (`None`) does not.
    pub(crate) fn prove(&mut self, fuse: IntactFuse, holder: crate::Value, subject: u32, slot: Option<u32>) {
        let state = if slot.is_some() { FuseState::Intact } else { FuseState::Popped };
        self.records[fuse as usize] = FuseRecord { holder, subject, slot: slot.unwrap_or(0), state };
    }

    /// A write to `slot` of watched prototype `subject`: each fuse it held there pops.
    #[inline(always)]
    pub(crate) fn note_slot_write(&mut self, subject: u32, slot: usize) {
        for (fuse, record) in self.records.iter_mut().enumerate() {
            if record.subject == subject && record.slot as usize == slot && record.state == FuseState::Intact {
                record.state = FuseState::Popped;
                self.changed |= 1 << fuse;
                crate::heap::note_intrinsic_change();
            }
        }
    }

    /// A change of watched prototype `subject`'s layout or elements: the slots its fuses name
    /// may move, so each is proved again on its next use.
    pub(crate) fn note_layout_change(&mut self, subject: u32) {
        for (fuse, record) in self.records.iter_mut().enumerate() {
            if record.subject == subject && record.state == FuseState::Intact {
                record.state = FuseState::Unproved;
                self.changed |= 1 << fuse;
                crate::heap::note_intrinsic_change();
            }
        }
    }

    /// Watched prototype `subject` died: no proof names it.
    pub(crate) fn forget(&mut self, subject: u32) {
        for (fuse, record) in self.records.iter_mut().enumerate() {
            if record.subject == subject {
                if record.state == FuseState::Intact {
                    self.changed |= 1 << fuse;
                    crate::heap::note_intrinsic_change();
                }
                *record = FuseRecord::default();
            }
        }
    }

    /// Every proof is made again: no subject names the change (a restored heap).
    pub(crate) fn forget_all(&mut self) {
        let changed = self.changed | self.guarded;
        *self = Self { changed, guarded: self.guarded, ..Self::default() };
        crate::heap::note_intrinsic_change();
    }
}

#[cfg(test)]
mod tests;
