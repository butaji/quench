//! The code that folds every activation's `this` as a direct object (`StableFacts::this_trust`),
//! the lane's one `this` fact (`Vm::this_trust`), and the one event that retires it: the lane
//! bound a `this` that is no direct object (`retire_this_holders`).

use std::rc::Rc;

thread_local! {
    /// The code that folds the trusted `this` (`StableFacts::this_trust`).
    static THIS_HOLDERS: std::cell::RefCell<crate::code_dependencies::CodeReaders> =
        std::cell::RefCell::new(crate::code_dependencies::CodeReaders::default());
    /// Whether the trust retired for good: code subscribing after takes nothing.
    static THIS_RETIRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The one owner of `THIS_HOLDERS`: every activation's `this` is one subject.
const ANY_THIS: u32 = 0;

/// Makes `code` retire when the lane's `this` trust retires; `false` where it already has or the
/// reader table is full, so the code must not fold it.
pub fn subscribe_this_trust(code: &Rc<dyn crate::code_dependencies::DependentCode>) -> bool {
    !THIS_RETIRED.with(std::cell::Cell::get) && THIS_HOLDERS.with(|holders| holders.borrow_mut().subscribe(ANY_THIS, code))
}

/// The lane's `this` trust retired: every holder retires, and no code holds it again.
pub fn retire_this_holders() {
    THIS_RETIRED.with(|retired| retired.set(true));
    let holders = THIS_HOLDERS.with(|holders| holders.borrow_mut().take(ANY_THIS));
    holders.into_iter().for_each(|code| code.retire());
}
