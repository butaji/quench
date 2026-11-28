//! The dependency table's and the fuses' own tests.
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
    let dependency = SlotDependency { shape: 2, slot: 0, ty: Type::INT32 };
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
