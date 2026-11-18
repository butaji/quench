//! A root environment's binding slots, which `LoadCapture` and `StoreCapture` records of a
//! module's top-level declarations read and write in place, from a frame or an activation.
use super::*;

/// A frame run's depth-0 capture slots, read when its capture walk ran: dense ones become
/// `LaneContext::capture`; whether they are a root environment's, whose binding slots the
/// record may read in place (`lane_root_binding_slot`).
pub(super) fn frame_capture_at_root<H: Host>(cx: *mut LaneContext<H>, frame: usize) -> bool {
    let vm = unsafe { vm(cx) };
    match vm.current_capture_slots(frame) {
        Some((slots, None)) => {
            if let Some(slots) = unsafe { &*slots }.dense_storage() {
                unsafe { (*cx).registers.set_capture(slots) };
            }
            false
        }
        current => current.is_some(),
    }
}

/// The root environment `depth` parent links out from the running activation's closure
/// environment, while each link is an environment (a frame's: `Vm::capture_env`): no
/// capture walk, as a root binding's records read it (`lane_load_root_binding`,
/// `lane_store_root_binding`).
#[inline(always)]
pub(super) fn lane_root_environment<H: Host>(cx: *mut LaneContext<H>, depth: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let mut env = unsafe { (*cx).registers }.env();
    if env.is_deleted() {
        // A frame's closure environment, past any `with` layers, as its capture walk finds it.
        return vm.owner_capture_env(VarOwner::Frame(unsafe { (*cx).frame() }), depth);
    }
    for _ in 0..depth {
        let Some(crate::heap::EnvironmentRecord { parent, .. }) = vm.heap.environment(env) else { return None };
        env = *parent;
    }
    Some(env)
}

/// Whether the capture at `depth` and `slot` of the running activation is a root
/// environment's slot that holds its binding itself (`RootSlotRule::Binding`): no global
/// var, global lexical or module import projection reads through it, so the slot answers
/// each read and takes each store. The capture walk and the plain parent links must reach
/// the same environment. Which slots those are is fixed per program, and a record's scope
/// chain is fixed by its function, so the answer holds for the record.
pub(super) fn lane_root_binding_slot<H: Host>(cx: *mut LaneContext<H>, depth: u16, slot: u16) -> bool {
    // The parent links first: most captures end at a function's environment.
    let Some(root) = lane_root_environment(cx, depth) else { return false };
    let vm = unsafe { vm(cx) };
    let slots = match vm.heap.environment(root) {
        Some(crate::heap::EnvironmentRecord { function, slots, .. }) if *function == super::ROOT_FUNCTION_ID => std::ptr::from_ref(slots),
        _ => return false,
    };
    let env = unsafe { (*cx).registers }.env();
    let walked = if env.is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame() }];
        vm.walked_capture_slots(frame.env, frame.captured, depth)
    } else {
        vm.walked_capture_slots(env, false, depth)
    };
    let Some((walked, Some(program))) = walked else {
        return false;
    };
    if !std::ptr::eq(slots, walked) {
        return false;
    }
    let owner = program.map_or(vm.active_program, ProgramId::from_raw);
    vm.programs.root_slot(owner, slot) == crate::bytecode::RootSlotRule::Binding
}

