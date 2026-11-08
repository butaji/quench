//! Weak native publications indexed by the shape or slot owner they depend on.
//! These notifications do not turn observations into unchecked read permissions.

use std::{num::NonZeroU32, rc::{Rc, Weak}};
use crate::typed_ir::Type;

pub(crate) trait DependentCode {
    fn is_published(&self) -> bool;
    fn retire(&self);
}

/// The word a body's code reads at its invalidation points: zero while every assumption the
/// code folded holds, nonzero once a change retired it. Code that holds an assumption (a
/// Stable fact, `typed_ir::BindingTime::Stable`) pays no check per use: it reads this word
/// once at entry and again only after a call that may have changed the assumption's subject,
/// and leaves at the exact PC where it is set.
#[derive(Clone, Default)]
pub(crate) struct Invalidation(Rc<std::cell::Cell<u64>>);

impl Invalidation {
    /// The word's address, which lives as long as any holder of this invalidation.
    pub(crate) fn address(&self) -> usize { self.0.as_ptr() as usize }
    #[cfg(test)]
    pub(crate) fn is_set(&self) -> bool { self.0.get() != 0 }
    fn set(&self) { self.0.set(1); }
}

/// Publication owns future entry; execution and recovery own independent leases.
/// No borrow of the publication survives acquiring a lease or retiring it.
pub(crate) struct PublishedCode<T> {
    code: std::cell::RefCell<Option<Rc<T>>>,
    /// Set when the code retires, for runs already inside it (`Invalidation`).
    invalidation: Invalidation,
}

impl<T> PublishedCode<T> {
    pub(crate) fn new(code: T) -> Self { Self::with_invalidation(code, Invalidation::default()) }
    /// Code compiled to read `invalidation` at its invalidation points.
    pub(crate) fn with_invalidation(code: T, invalidation: Invalidation) -> Self {
        Self { code: std::cell::RefCell::new(Some(Rc::new(code))), invalidation }
    }
    pub(crate) fn lease(&self) -> Option<Rc<T>> { self.code.borrow().clone() }
    pub(crate) fn is_published(&self) -> bool { self.code.borrow().is_some() }
    pub(crate) fn retire(&self) {
        self.invalidation.set();
        self.code.borrow_mut().take();
    }
}

impl<T> DependentCode for PublishedCode<T> {
    fn is_published(&self) -> bool { self.is_published() }
    fn retire(&self) { self.retire(); }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotDependency {
    pub(crate) shape: u32,
    pub(crate) slot: u16,
    pub(crate) ty: Type,
    pub(crate) target_shape: Option<u32>,
}

type Link = Option<NonZeroU32>;
struct Reader {
    code: Option<Weak<dyn DependentCode>>,
    next: Link,
}

#[derive(Default)]
pub(crate) struct CodeReaders {
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
    pub(crate) fn subscribe(&mut self, owner: u32, code: &Rc<dyn DependentCode>) -> bool {
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
    #[inline(always)]
    pub(crate) fn retire(&mut self, owner: u32) {
        if self.heads.get(owner as usize).is_some_and(Option::is_some) {
            self.retire_readers(owner);
        }
    }

    #[inline(never)]
    fn retire_readers(&mut self, owner: u32) {
        let Some(head) = self.heads.get_mut(owner as usize) else { return };
        let mut cursor = head.take();
        while let Some(link) = cursor {
            if let Some(code) = self.readers[(link.get() - 1) as usize].code.as_ref().and_then(Weak::upgrade) {
                code.retire();
            }
            cursor = self.release(link);
        }
    }

    pub(crate) fn reset(&mut self) {
        for reader in &mut self.readers {
            if let Some(code) = reader.code.take().and_then(|code| code.upgrade()) { code.retire(); }
        }
        self.heads.clear();
        self.readers.clear();
        self.free = None;
    }
}

impl Drop for CodeReaders {
    fn drop(&mut self) { self.reset(); }
}

/// A fact about one realm's intrinsic method that fast paths rely on with no check per use
/// (a fuse): the realm's holder keeps the fuse's key as an own data property holding the
/// realm's intrinsic. A lookup proves it once (`Vm::fuse_intact`); a write to that slot of the
/// holder pops it for good (`Fuses::note_slot_write`), and a change of the holder's layout,
/// which may move the slot, only sends the next use back to one proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum IntactFuse {
    /// `Array.prototype[@@iterator]` is the realm's `Array.prototype.values`.
    ArrayIterator,
    /// `%ArrayIteratorPrototype%.next` is the realm's intrinsic `next`.
    ArrayIteratorNext,
    StringCharCodeAt,
    StringIncludes,
    RegExpTest,
    RegExpExec,
}

impl IntactFuse {
    const COUNT: usize = 6;
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
pub(crate) struct Fuses([FuseRecord; IntactFuse::COUNT]);

impl Fuses {
    /// Whether `fuse` holds for `holder`; `None` where no proof for `holder` is held.
    #[inline(always)]
    pub(crate) fn intact(&self, fuse: IntactFuse, holder: crate::Value) -> Option<bool> {
        let record = &self.0[fuse as usize];
        match record.state {
            _ if record.holder != holder => None,
            FuseState::Intact => Some(true),
            FuseState::Popped => Some(false),
            FuseState::Unproved => None,
        }
    }

    /// Records the proof of `fuse`: `holder`, watched as `subject`, holds the intrinsic at
    /// `slot`, or (`None`) does not.
    pub(crate) fn prove(&mut self, fuse: IntactFuse, holder: crate::Value, subject: u32, slot: Option<u32>) {
        let state = if slot.is_some() { FuseState::Intact } else { FuseState::Popped };
        self.0[fuse as usize] = FuseRecord { holder, subject, slot: slot.unwrap_or(0), state };
    }

    /// A write to `slot` of watched prototype `subject`: each fuse it held there pops.
    #[inline(always)]
    pub(crate) fn note_slot_write(&mut self, subject: u32, slot: usize) {
        for record in &mut self.0 {
            if record.subject == subject && record.slot as usize == slot && record.state == FuseState::Intact {
                record.state = FuseState::Popped;
            }
        }
    }

    /// A change of watched prototype `subject`'s layout or elements: the slots its fuses name
    /// may move, so each is proved again on its next use.
    pub(crate) fn note_layout_change(&mut self, subject: u32) {
        for record in &mut self.0 {
            if record.subject == subject && record.state == FuseState::Intact {
                record.state = FuseState::Unproved;
            }
        }
    }

    /// Watched prototype `subject` died: no proof names it.
    pub(crate) fn forget(&mut self, subject: u32) {
        for record in &mut self.0 {
            if record.subject == subject {
                *record = FuseRecord::default();
            }
        }
    }

    /// Every proof is made again: no subject names the change (a restored heap).
    pub(crate) fn forget_all(&mut self) {
        self.0 = Default::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn retirement_sets_the_invalidation_word_running_code_reads() {
        let invalidation = Invalidation::default();
        let publication = Rc::new(PublishedCode::with_invalidation((), invalidation.clone()));
        let reader: Rc<dyn DependentCode> = publication;
        let mut readers = CodeReaders::default();
        assert!(readers.subscribe(3, &reader));
        assert!(!invalidation.is_set());
        // SAFETY: the word is live while `invalidation` holds it.
        assert_eq!(unsafe { *(invalidation.address() as *const u64) }, 0);
        readers.retire(3);
        assert!(invalidation.is_set(), "a run already inside the code leaves at its next invalidation point");
        assert_ne!(unsafe { *(invalidation.address() as *const u64) }, 0);
    }

    #[test]
    fn retirement_unlinks_entry_but_preserves_execution_and_recovery_leases() {
        struct Body(Rc<Cell<usize>>);
        impl Drop for Body { fn drop(&mut self) { self.0.set(self.0.get() + 1); } }
        let drops = Rc::new(Cell::new(0));
        let publication = Rc::new(PublishedCode::new(Body(drops.clone())));
        let execution = publication.lease().unwrap();
        let recovery = execution.clone();
        let reader: Rc<dyn DependentCode> = publication.clone();
        let mut readers = CodeReaders::default();
        assert!(readers.subscribe(1, &reader));
        readers.retire(1);
        assert!(!publication.is_published());
        assert!(publication.lease().is_none());
        assert_eq!(drops.get(), 0);
        let replacement = PublishedCode::new(Body(drops.clone()));
        publication.retire();
        assert!(replacement.is_published(), "retiring old code does not unlink a replacement");
        drop(execution);
        assert_eq!(drops.get(), 0, "recovery still owns the old code");
        drop(recovery);
        assert_eq!(drops.get(), 1);
        replacement.retire();
        assert_eq!(drops.get(), 2);
    }
    struct Code(Cell<bool>);
    impl DependentCode for Code {
        fn is_published(&self) -> bool { self.0.get() }
        fn retire(&self) { self.0.set(false); }
    }

    #[test]
    fn readers_are_weak_selective_deduplicated_and_recycled() {
        let mut readers = CodeReaders::default();
        let first: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        let second: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        assert!(readers.subscribe(1, &first));
        assert!(readers.subscribe(1, &first));
        assert_eq!(readers.readers.len(), 1);
        assert_eq!(Rc::strong_count(&first), 1);
        assert!(readers.subscribe(2, &second));
        readers.retire(1);
        assert!(!first.is_published());
        assert!(second.is_published());
        let third: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        assert!(readers.subscribe(2, &third));
        assert_eq!(readers.readers.len(), 2, "reuse the retired owner's node");
        let bytes = readers.memory_bytes();
        drop(second);
        let fourth: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        assert!(readers.subscribe(2, &fourth));
        assert_eq!(readers.readers.len(), 2, "prune and reuse a dead non-head reader");
        assert_eq!(readers.memory_bytes(), bytes);
        readers.reset();
        assert!(!third.is_published() && !fourth.is_published());
    }

    #[test]
    fn slot_widening_and_census_reset_retire_owner_readers() {
        use crate::{value_vec::ValueArena, Value};
        let mut arena = ValueArena::default();
        arena.track();
        arena.register_derived_shape(1, 1, None);
        arena.register_derived_shape(2, 2, Some((1, 1)));
        arena.note_slot_value(1, 0, Value::integer(1));
        let code: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        let dependency = SlotDependency { shape: 2, slot: 0, ty: Type::INT32, target_shape: None };
        assert!(arena.subscribe_slot(dependency, &code));
        arena.note_slot_value(1, 0, Value::integer(2));
        assert!(code.is_published(), "compatible parent-owner writes retain code");
        arena.note_slot_value(2, 0, Value::number(0.5));
        assert!(!code.is_published(), "child writes retire the shared owner");
        let newer: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        assert!(!arena.subscribe_slot(dependency, &newer), "revalidate before publication");
        assert!(arena.subscribe_slot(SlotDependency { ty: Type::NUMBER, ..dependency }, &newer));
        arena.note_slot_type(1, 0, Type::ANY);
        assert!(!newer.is_published(), "raw-writer admission retires before exposing storage");
        let reset: Rc<dyn DependentCode> = Rc::new(Code(Cell::new(true)));
        assert!(arena.subscribe_slot(SlotDependency { ty: Type::ANY, ..dependency }, &reset));
        arena.track();
        assert!(!reset.is_published(), "a fresh census cannot retain old observations' readers");
    }

    #[test]
    fn fuses_pop_on_their_slot_write_and_are_proved_again_after_a_layout_change() {
        use crate::Value;
        let (holder, other) = (Value::integer(1), Value::integer(2));
        let mut fuses = Fuses::default();
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), None);
        fuses.prove(IntactFuse::ArrayIterator, holder, 4, Some(3));
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), Some(true));
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, other), None, "another realm's holder proves its own");
        fuses.note_slot_write(4, 2);
        fuses.note_slot_write(5, 3);
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), Some(true), "other slots and holders leave it");
        fuses.note_layout_change(4);
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), None, "a layout change asks for one proof");
        fuses.prove(IntactFuse::ArrayIterator, holder, 4, Some(3));
        fuses.note_slot_write(4, 3);
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), Some(false));
        fuses.note_layout_change(4);
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), Some(false), "a popped fuse stays popped");
        fuses.forget(4);
        assert_eq!(fuses.intact(IntactFuse::ArrayIterator, holder), None, "a dead holder's record is reused");
    }
}
