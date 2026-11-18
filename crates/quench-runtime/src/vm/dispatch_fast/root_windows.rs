//! The local records of a root body that runs as a window activation (an async module's body
//! resumed after its top-level `await`): a slot that stands for a binding outside the window (a
//! module import, a global lexical or var, `Vm::root_binding`) is read through the activation's
//! scope as a frame's read is; any other slot is the window's own, which the ordinary window
//! handlers serve. A root in a frame's run has no window scope and takes them too.
use super::*;
use super::scoped_names::window_scope;
use super::window_records::{is_local_load, is_local_store};
use super::super::root_bindings::RootBinding;

script_handler! {
    /// A local load of a root body (`root_local_handler`): an initialized import or global lexical
    /// the realm and the module environments hold. Any other binding of the window, a global `var`
    /// (a property read, which may run a getter) and an uninitialized binding take the general path.
    #[inline(never)]
    fn lane_load_root_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, _) = environment_window_fields(i);
        let vm = unsafe { vm(cx) };
        let Some(scope) = window_scope(cx, r, ip, false) else {
            sibling!(lane_load_window_local::<H, ENVIRONMENT>, r, ip, view)
        };
        match vm.root_binding(&scope, slot) {
            None => sibling!(lane_load_window_local::<H, ENVIRONMENT>, r, ip, view),
            Some(RootBinding::GlobalLexical(value) | RootBinding::Import(value)) if !value.is_deleted() => {
                r.set(i.a, value);
                next!(cx, r, following(ip), view)
            }
            Some(_) => exit(ip),
        }
    }
}

script_handler! {
    /// A local store of a root body (`root_local_handler`): to a slot the window owns. A store to a
    /// binding outside it (a const check, a global property, a realm binding to mirror) takes the
    /// general path.
    #[inline(never)]
    fn lane_store_root_local<ENVIRONMENT: bool>(cx, r, ip, i, view) {
        let (slot, _) = environment_window_fields(i);
        let vm = unsafe { vm(cx) };
        let Some(scope) = window_scope(cx, r, ip, false) else {
            sibling!(lane_store_window_local::<H, ENVIRONMENT>, r, ip, view)
        };
        match vm.root_binding(&scope, slot) {
            None => sibling!(lane_store_window_local::<H, ENVIRONMENT>, r, ip, view),
            Some(_) => exit(ip),
        }
    }
}

/// The handler a root body's local load or store takes: `instruction` is a load, a store or an
/// initialization of the environment window of a function laid out as a program's root, which
/// resolves its slots through `Vm::root_binding`; `None` for any other record.
pub(super) fn root_local_handler<H: Host>(instruction: WideInstruction, environment: bool) -> Option<Handler> {
    match (is_local_load(instruction), is_local_store(instruction), environment) {
        (true, _, true) => Some(lane_load_root_local::<H, true>),
        (true, _, false) => Some(lane_load_root_local::<H, false>),
        (_, true, true) => Some(lane_store_root_local::<H, true>),
        (_, true, false) => Some(lane_store_root_local::<H, false>),
        _ => None,
    }
}
