//! The code that holds a prototype's value as a constant, retired when that value changes.

use std::rc::Rc;

// The code that took a value a prototype offers its inheritors as a constant, by the
// prototype's exact shape: a Stable fact (`typed_ir::BindingTime::Stable`). A write to a slot
// of a prototype of that shape (`Object::note_prototype_value_write`) retires that code, and a
// layout change retires all of it (`advance_prototype_layout_epoch`), so the code itself checks
// nothing per read. Guest heaps and their native publications are thread-confined (Rc
// ownership): another thread's prototype writes never retire this thread's code.
//
// Interpreter caches hold the same dependency (`watch_prototype_values`): no epoch is compared
// per use.
thread_local! {
    static PROTOTYPE_VALUE_READERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
    /// Per holder shape, the watch interpreter caches share (`watch_prototype_values`).
    static PROTOTYPE_VALUE_WATCHES: std::cell::RefCell<Vec<Option<PrototypeValueWatch>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Makes `code` retire when a prototype of exact shape `holder` changes a value or any
/// prototype changes its layout; `false` where the reader table is full.
pub(crate) fn subscribe_prototype_values(holder: u32, code: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().subscribe(holder, code))
}

/// The dependency an interpreter cache holds on the values of prototypes of exact shape
/// `holder`: published until one changes a value or any prototype changes its layout. Every
/// cache on the holder shares it, so it subscribes once per change; `None` where the reader
/// table is full.
pub(crate) fn watch_prototype_values(holder: u32) -> Option<PrototypeValueWatch> {
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
        subscribe_prototype_values(holder, &code).then(|| watches[index].insert(watch).clone())
    })
}

/// What `watch_prototype_values` gives: `is_published` while the values it watches hold.
pub(crate) type PrototypeValueWatch = Rc<crate::code_dependencies::PublishedCode<()>>;

pub(super) fn retire(holder: u32) {
    // Retiring drops code, which never writes a prototype, so the table is not reentered.
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().retire(holder));
}

/// Retires all of it: a layout change moves what every slot holds.
pub(super) fn reset() {
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_value_readers_retire_on_their_holder_or_any_layout_change_on_this_thread() {
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
        super::super::advance_prototype_layout_epoch();
        assert!(other_flag.is_set(), "a layout change retires every prototype value reader");
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
}
