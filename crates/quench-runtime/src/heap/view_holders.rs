//! The code that holds the typed views it proved (`staged::HeldViews`, `StableFacts::views`), and
//! the one event that retires it: a live view changed (`live_view_changed`).

use std::rc::Rc;

thread_local! {
    /// The code that holds the typed views it proved (`staged::HeldViews`, `StableFacts::views`).
    static VIEW_HOLDERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
    /// How many times a change of a live view retired holding code (`typed_views_stable`).
    static VIEW_RETIREMENTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The one owner of `VIEW_HOLDERS`: every live view is one subject.
const ANY_VIEW: u32 = 0;

/// How many retirements of view-holding code the VM takes before it holds views no more: a
/// program that keeps moving live bytes would otherwise recompile its loops at every move.
const VIEW_RETIREMENT_LIMIT: u32 = 8;

/// Makes `code` retire at the first change of a live typed array's view: its bytes moved, its
/// length changed, its buffer detached or shared (`live_view_changed`); `false` where the
/// reader table is full.
pub fn subscribe_typed_views(code: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    VIEW_HOLDERS.with(|holders| holders.borrow_mut().subscribe(ANY_VIEW, code))
}

/// Whether code may hold its typed views under `subscribe_typed_views`: until changes of live
/// views have retired holding code `VIEW_RETIREMENT_LIMIT` times.
pub fn typed_views_stable() -> bool {
    VIEW_RETIREMENTS.with(|count| count.get() < VIEW_RETIREMENT_LIMIT)
}

/// A live typed array's view changed (bytes moved, unshared or shared, resized, detached): its
/// relocation count advances, so every proof of a view tests stale, and the code holding views
/// retires, so none reads through a held one again.
pub(crate) fn live_view_changed() {
    super::buffer_bytes::note_relocation();
    let holders = VIEW_HOLDERS.with(|holders| holders.borrow_mut().take(ANY_VIEW));
    if holders.is_empty() { return; }
    VIEW_RETIREMENTS.with(|count| count.set(count.get().saturating_add(1)));
    holders.into_iter().for_each(|code| code.retire());
}

#[cfg(test)]
mod view_holder_tests {
    use super::subscribe_typed_views;
    use crate::heap::BufferBytes;
    use crate::code_dependencies::{new_dependency, DependentCode};
    use std::rc::Rc;

    fn holder() -> (Rc<crate::code_dependencies::PublishedCode<()>>, Rc<dyn DependentCode>) {
        let dependency = new_dependency();
        let code: Rc<dyn DependentCode> = dependency.clone();
        assert!(subscribe_typed_views(&code));
        (dependency, code)
    }

    #[test]
    fn code_holding_views_retires_when_live_bytes_move_or_become_shared() {
        let mut bytes = BufferBytes::from(vec![0; 64]);
        let (resized, _code) = holder();
        bytes.owned_mut().resize(4096, 0);
        assert!(!resized.is_published(), "a resize moves the bytes");
        let (shared, _code) = holder();
        let second = bytes.clone();
        assert!(!shared.is_published(), "a second handle shares the bytes");
        let (unshared, _code) = holder();
        bytes.make_mut()[0] = 1;
        assert!(!unshared.is_published(), "a write copies the shared bytes");
        drop(second);
    }

    #[test]
    fn code_holding_views_stands_while_only_unreachable_bytes_are_freed() {
        let (held, _code) = holder();
        drop(BufferBytes::from(vec![0; 64]));
        let mut bytes = BufferBytes::from(vec![0; 64]);
        bytes.make_mut()[0] = 1;
        assert!(held.is_published(), "freeing or writing bytes no view shares moves no live view");
    }
}
