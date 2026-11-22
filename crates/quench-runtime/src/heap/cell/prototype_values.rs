//! The code that holds a prototype's value as a constant, retired when that value changes.

use std::rc::Rc;

// The code that took a value a prototype offers its inheritors as a constant, by the
// prototype's exact shape: a Stable fact (`typed_ir::BindingTime::Stable`). A write to a slot
// of a prototype of that shape (`Object::note_value_write`) retires that code, and so does a
// change of that prototype's own layout (`note_layout_change`). The chain a fact was read
// through (an inherited read's prototypes, an add row's or a membership's chain proof, an
// `instanceof`'s way to its `@@hasInstance`) is the code's own dependency on each prototype of
// it (`code_dependencies::PrototypeChain`, `Heap::subscribe_prototype_chain`), so a change to a
// prototype off every chain the code read leaves it standing. Code whose chain names a link no
// dependency table watches holds the layouts of all prototypes instead
// (`subscribe_prototype_layouts`). The code itself checks nothing per read. Guest heaps and their
// native publications are thread-confined (Rc ownership): another thread's prototype writes never
// retire this thread's code.
//
// Interpreter caches hold their own dependency (`watch_prototype_values`), on the holder's
// values and layout alone: a cache that read the value through a chain holds the chain's cell
// as well (`heap::ChainCell`), so a layout change of another prototype leaves it standing. No
// epoch is compared per use.
thread_local! {
    static PROTOTYPE_VALUE_READERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
    /// The code whose chain could not be named (`subscribe_prototype_layouts`), retired by a
    /// layout change of any prototype; one owner.
    static PROTOTYPE_LAYOUT_READERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
    /// The watches interpreter caches hold (`watch_prototype_values`), by holder shape: retired
    /// by a write to a holder's value or a change of the holder's own layout only.
    static PROTOTYPE_VALUE_WATCH_READERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
    /// Per holder shape, the watch interpreter caches share (`watch_prototype_values`).
    static PROTOTYPE_VALUE_WATCHES: std::cell::RefCell<Vec<Option<PrototypeValueWatch>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Makes `code` retire when a prototype of exact shape `holder` changes a value or its own
/// layout, or at an event every prototype shares (`advance_prototype_layout_epoch`); `false`
/// where the reader table is full. The chain the value was read through is a dependency of its
/// own (`Heap::subscribe_prototype_chain`).
pub fn subscribe_prototype_values(holder: u32, code: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().subscribe(holder, code))
}

/// The one owner of `PROTOTYPE_LAYOUT_READERS`.
const ANY_PROTOTYPE: u32 = 0;

/// Makes `code` retire when any prototype changes its layout: the fallback for a fact read
/// through a chain with a link no dependency table watches, so no subject names it
/// (`Heap::subscribe_prototype_chain`). `false` where the reader table is full.
pub fn subscribe_prototype_layouts(code: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    PROTOTYPE_LAYOUT_READERS.with(|readers| readers.borrow_mut().subscribe(ANY_PROTOTYPE, code))
}

/// Makes `dependency` retire when a prototype of exact shape `holder` changes a value or its
/// own layout, and at an event every prototype shares (`advance_prototype_layout_epoch`);
/// `false` where the reader table is full. A layout change of another prototype leaves it.
pub fn hold_prototype_values(holder: u32, dependency: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    PROTOTYPE_VALUE_WATCH_READERS.with(|readers| readers.borrow_mut().subscribe(holder, dependency))
}

/// The dependency an interpreter cache holds on the values of prototypes of exact shape
/// `holder`: published until one changes a value or its own layout
/// (`hold_prototype_values`). A cache that read the value through other prototypes holds
/// their chain's cell too. Every cache on the holder shares it, so it subscribes once per
/// change; `None` where the reader table is full.
pub fn watch_prototype_values(holder: u32) -> Option<PrototypeValueWatch> {
    PROTOTYPE_VALUE_WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let index = holder as usize;
        let len = watches.len().max(index + 1);
        watches.resize(len, None);
        if let Some(watch) = watches[index].as_ref().filter(|watch| watch.is_published()) {
            return Some(watch.clone());
        }
        let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
        let code: Rc<dyn crate::code_dependencies::DependentCode> = watch.clone();
        hold_prototype_values(holder, &code).then(|| watches[index].insert(watch).clone())
    })
}

/// What `watch_prototype_values` gives: `is_published` while the values it watches hold.
pub type PrototypeValueWatch = Rc<crate::code_dependencies::PublishedCode<()>>;

/// A write to a value of a prototype of exact shape `holder`: its readers retire, and where it
/// had any, the intrinsic epoch advances (`note_intrinsic_change`), so a memo checked by epoch
/// instead of by its watch retires with them.
pub(super) fn retire(holder: u32) {
    // Retiring drops code, which never writes a prototype, so the table is not reentered.
    let code = PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().retire(holder));
    let watches = PROTOTYPE_VALUE_WATCH_READERS.with(|readers| readers.borrow_mut().retire(holder));
    if code || watches {
        super::note_intrinsic_change();
    }
}

/// A layout change of a prototype of exact shape `holder`, before it takes effect: the
/// watches and code on its values retire, as its slots may move, and the code whose chain
/// names no subject (`subscribe_prototype_layouts`). Code that read through the prototype
/// retires through its subject (`Heap::subscribe_prototype_chain`).
pub(super) fn note_layout_change(holder: u32) {
    // Retiring drops code, which never changes a prototype, so no table is reentered.
    PROTOTYPE_VALUE_WATCH_READERS.with(|readers| readers.borrow_mut().retire(holder));
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().retire(holder));
    PROTOTYPE_LAYOUT_READERS.with(|readers| readers.borrow_mut().retire(ANY_PROTOTYPE));
}

/// Retires all of it: an event every prototype shares (`advance_prototype_layout_epoch`).
pub(super) fn reset() {
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().reset());
    PROTOTYPE_VALUE_WATCH_READERS.with(|readers| readers.borrow_mut().reset());
    PROTOTYPE_LAYOUT_READERS.with(|readers| readers.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_value_readers_retire_on_their_holder_or_a_global_event_on_this_thread() {
        use crate::code_dependencies::{DependentCode, Invalidation, PublishedCode};
        let publish = || {
            let invalidation = Invalidation::default();
            let code = Rc::new(PublishedCode::with_invalidation((), invalidation.clone()));
            (code, invalidation)
        };
        let (holder_code, holder_flag) = publish();
        let (other_code, other_flag) = publish();
        let as_reader = |code: &Rc<PublishedCode<()>>| -> Rc<dyn DependentCode> { code.clone() };
        assert!(subscribe_prototype_values(5, &as_reader(&holder_code)));
        assert!(subscribe_prototype_values(6, &as_reader(&other_code)));
        std::thread::spawn(super::super::advance_prototype_layout_epoch).join().unwrap();
        assert!(!holder_flag.is_set() && !other_flag.is_set(), "another VM thread retires none of this thread's code");
        retire(5);
        assert!(holder_flag.is_set() && !holder_code.is_published());
        assert!(!other_flag.is_set(), "a write to another holder shape retires nothing else");
        note_layout_change(5);
        assert!(!other_flag.is_set(), "another holder's layout change retires no other reader");
        super::super::advance_prototype_layout_epoch();
        assert!(other_flag.is_set(), "an event every prototype shares retires every prototype value reader");
    }

    #[test]
    fn caches_on_one_holder_share_a_watch_until_its_values_change() {
        let first = watch_prototype_values(7).unwrap();
        let second = watch_prototype_values(7).unwrap();
        assert!(Rc::ptr_eq(&first, &second), "one subscription serves every cache on the holder");
        retire(7);
        assert!(!first.is_published());
        let third = watch_prototype_values(7).unwrap();
        assert!(third.is_published() && !Rc::ptr_eq(&first, &third));
    }

    #[test]
    fn watches_and_code_retire_on_their_own_holders_layout_change_and_unnamed_chains_on_any() {
        use crate::code_dependencies::PublishedCode;
        let watch = watch_prototype_values(9).unwrap();
        let code = Rc::new(PublishedCode::new(()));
        let reader: Rc<dyn crate::code_dependencies::DependentCode> = code.clone();
        assert!(subscribe_prototype_values(9, &reader));
        let unnamed = Rc::new(PublishedCode::new(()));
        let unnamed_reader: Rc<dyn crate::code_dependencies::DependentCode> = unnamed.clone();
        assert!(subscribe_prototype_layouts(&unnamed_reader));
        note_layout_change(10);
        assert!(watch.is_published() && code.is_published(), "another prototype's layout change leaves the holder's readers");
        assert!(!unnamed.is_published(), "code whose chain names no subject retires at any prototype's layout change");
        note_layout_change(9);
        assert!(!watch.is_published() && !code.is_published(), "the holder's own layout change retires its readers");
    }
}
