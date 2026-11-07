//! The code that holds a prototype's value as a constant, retired when that value changes.

use std::rc::Rc;

// The code that took a value a prototype offers its inheritors as a constant, by the
// prototype's exact shape: a Stable fact (`typed_ir::BindingTime::Stable`). A write to a slot
// of a prototype of that shape (`Object::note_prototype_value_write`) retires that code, and a
// layout change retires all of it (`advance_prototype_layout_epoch`), so the code itself checks
// nothing per read. Guest heaps and their native publications are thread-confined (Rc
// ownership): another thread's prototype writes never retire this thread's code.
thread_local! {
    static PROTOTYPE_VALUE_READERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
}

/// Makes `code` retire when a prototype of exact shape `holder` changes a value or any
/// prototype changes its layout; `false` where the reader table is full.
pub(crate) fn subscribe_prototype_values(holder: u32, code: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    PROTOTYPE_VALUE_READERS.with(|readers| readers.borrow_mut().subscribe(holder, code))
}

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
}
