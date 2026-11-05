//! Weak native publications indexed by the shape or slot owner they depend on.
//! These notifications do not turn observations into unchecked read permissions.

use std::{num::NonZeroU32, rc::{Rc, Weak}};
use crate::typed_ir::Type;

pub(crate) trait DependentCode {
    fn is_published(&self) -> bool;
    fn retire(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotDependency {
    pub(crate) shape: u32,
    pub(crate) slot: u16,
    pub(crate) ty: Type,
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

    /// A widening visits only this owner's readers, before the incompatible effect.
    pub(crate) fn retire(&mut self, owner: u32) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
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
}
