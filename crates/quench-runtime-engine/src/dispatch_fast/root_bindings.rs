//! A root environment's binding slots, which `LoadCapture` and `StoreCapture` records of a
//! module's top-level declarations read and write in place, from a frame or an activation.
use super::*;

/// A frame run's depth-0 capture slots, read when its capture walk ran: dense ones become
/// `LaneContext::capture`; whether they are a root environment's, whose binding slots the
/// record may read in place (`root_binding_slot_in_lane`).
pub(super) fn frame_capture_at_root(cx: *mut LaneContext, frame: usize) -> bool {
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

// merge: ours (HEAD) moved root-binding reads into the staged `LoadCapture { root }` operation
// and dropped these helpers; kept because theirs' root-binding lane records call them. Drop
// them with those records if the dispatch_fast.rs resolution does not keep those records.
/// The root environment `depth` parent links out from the running activation's closure
/// environment, while each link is an environment (a frame's: `Vm::capture_env`): no
/// capture walk, as a root binding's records read it (`lane_load_root_binding`,
/// `lane_store_root_binding`).
#[inline(always)]
pub(super) fn root_environment_in_lane(cx: *mut LaneContext, depth: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let mut env = unsafe { (*cx).registers }.env();
    if env.is_deleted() {
        return frame_root_environment(cx, depth);
    }
    for _ in 0..depth {
        let Some(quench_runtime::heap::EnvironmentRecord { parent, .. }) = vm.heap.reachable_environment(env) else { return None };
        env = *parent;
    }
    Some(env)
}

/// `root_environment_in_lane` of a frame run: the frame's closure environment, past any `with`
/// layers, as its capture walk finds it (`owner_capture_env` of the frame), read from the
/// frame's fields without the caller view's dynamic calls.
#[inline(always)]
fn frame_root_environment(cx: *mut LaneContext, depth: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let frame = &vm.frames[unsafe { (*cx).frame() }];
    // The closure environment itself, at depth 0 of a frame that made none of its own: every
    // caller reads it only as a root environment (`ROOT_FUNCTION_ID`), which a function-less
    // `with` layer never is, so no layer needs skipping to answer.
    if depth == 0 && !frame.captured {
        return Some(frame.env);
    }
    vm.environment_capture_env(frame.env, frame.captured, depth)
}

/// Whether the capture at `depth` and `slot` of the running activation is a root
/// environment's slot that holds its binding itself (`RootSlotRule::Binding`): no global
/// var, global lexical or module import projection reads through it, so the slot answers
/// each read and takes each store. The capture walk and the plain parent links must reach
/// the same environment. Which slots those are is fixed per program, and a record's scope
/// chain is fixed by its function, so the answer holds for the record.
pub(super) fn root_binding_slot_in_lane(cx: *mut LaneContext, depth: u16, slot: u16) -> bool {
    lane_root_binding_owner(cx, depth, slot).is_some()
}

/// `root_binding_slot_in_lane`, answered with the program whose root environment holds the slot.
pub(super) fn lane_root_binding_owner(cx: *mut LaneContext, depth: u16, slot: u16) -> Option<ProgramId> {
    // The parent links first: most captures end at a function's environment.
    let root = root_environment_in_lane(cx, depth)?;
    let vm = unsafe { vm(cx) };
    let slots = match vm.heap.environment(root) {
        Some(quench_runtime::heap::EnvironmentRecord { function, slots, .. }) if *function == super::ROOT_FUNCTION_ID => std::ptr::from_ref(slots),
        _ => return None,
    };
    let env = unsafe { (*cx).registers }.env();
    let walked = if env.is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame() }];
        vm.walked_capture_slots(frame.env, frame.captured, depth)
    } else {
        vm.walked_capture_slots(env, false, depth)
    };
    let Some((walked, Some(program))) = walked else {
        return None;
    };
    if !std::ptr::eq(slots, walked) {
        return None;
    }
    let owner = program.map_or(vm.active_program, ProgramId::from_raw);
    (vm.programs.root_slot(owner, slot) == quench_runtime::bytecode::RootSlotRule::Binding).then_some(owner)
}
