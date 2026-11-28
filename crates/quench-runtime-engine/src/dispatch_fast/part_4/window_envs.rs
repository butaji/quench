//! Environment windows of a lane: the environment a window takes over, the slots it holds, and its scope reference.
use super::*;

/// The environment of the environment window whose entries start at `entries`
/// (`WINDOW_ENVIRONMENT`): the one already made, or a new environment of `function` of
/// `program` under `parent` that takes over the slots the window held in place, its
/// captured ones from the locals, which then hold `DELETED` for them as a frame's do. The
/// window reaches the new storage from then on. The environment binds the activation's
/// `this`, where its closures' arrows find it (`Vm::captured_lexical_this`), as a frame's
/// promotion carries its bindings; an arrow's own `this` is that same lexical value. The
/// environment of a window over a record also takes over the activation's dynamic `bindings`
/// (`FrameRecord::dynamic_bindings`, `Activation::take_record_bindings`), as a frame's
/// promotion takes its own (`Vm::promote_frame_environment`); any other passes none.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`activate_environment_in_lane`).
pub(in super::super) unsafe fn window_environment(
    vm: &mut Vm,
    code: &ResidualProgram,
    entries: *mut Value,
    function: u32,
    parent: Value,
    program: ProgramId,
    this: Value,
    bindings: Option<Vec<(quench_runtime::bytecode::Atom, Value)>>,
) -> Value {
    // SAFETY: as the caller guarantees.
    unsafe {
        let made = *entries.add(usize::from(WINDOW_ENVIRONMENT));
        if !made.is_deleted() {
            return made;
        }
        let slots = window_slots_taken(entries);
        // A non-arrow binds its `this` where its closures' arrows find it
        // (`Vm::captured_lexical_this`), as a frame does when a closure or the body itself
        // can observe its implicit bindings (`Function::activation_bindings_observable`);
        // any other scope names only its program, which needs no allocation.
        let mut bindings = bindings.unwrap_or_default();
        let body = code.function(function as usize);
        if !body.is_arrow && body.activation_bindings_observable(code) && body.maker_bindings_read(code) {
            bindings.insert(0, (vm.runtime_atoms.lexical_this, this));
        }
        let scope = window_scope_ref(program, bindings);
        let (environment, storage) = vm.allocate_environment(parent, function, slots, scope);
        // Its storage links to its parent's for its closures' outer captures
        // (`EnvironmentSlots::link`), as a capture walk would find the parent's.
        if let Some(quench_runtime::tier_surface::EnvironmentRecord { function: owner, slots: parent_slots, .. }) = vm.heap_reads().environment(parent)
            && *owner != u32::MAX
            && *owner != quench_runtime::vm::ROOT_FUNCTION_ID
            && let Some(parent_storage) = parent_slots.dense_storage()
            && let Some(own) = (*storage).dense_storage()
        {
            quench_runtime::tier_surface::EnvironmentSlots::link(own, parent_storage, vm.heap_reads().collection_epoch());
        }
        entries.add(usize::from(WINDOW_ENVIRONMENT)).write(environment);
        entries
            .add(usize::from(WINDOW_SLOTS))
            .write(std::mem::transmute::<u64, Value>(storage as u64));
        environment
    }
}

/// The slots of an environment made from the window whose entries are `entries`: the locals a
/// closure captured, each cleared in the window, which the environment owns from then on.
///
/// # Safety
/// `entries` starts the entries of a live environment window (`activate_environment_in_lane`).
pub(in super::super) unsafe fn window_slots_taken(entries: *mut Value) -> quench_runtime::tier_surface::EnvironmentSlots {
    // SAFETY: as the caller guarantees.
    unsafe {
        let captured = std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_CAPTURED)));
        let in_place = &*(std::mem::transmute::<Value, u64>(*entries.add(usize::from(WINDOW_SLOTS)))
            as *const quench_runtime::tier_surface::EnvironmentSlots);
        let locals = entries.add(usize::from(WINDOW_LOCALS));
        let window = EnvironmentWindow { locals: in_place.len() as u16, captured, rest: false, self_binding: None, parameter_registers: 0 };
        // SAFETY: the mask names locals of the window, which hold owned values.
        let slots = quench_runtime::tier_surface::EnvironmentSlots::from_captured(in_place.len(), window.captured, locals);
        // Each captured local, one per bit of the mask, which names no slot past the locals.
        debug_assert_eq!(captured.checked_shr(u32::from(window.locals)).unwrap_or(0), 0);
        let mut remaining = captured;
        while remaining != 0 {
            locals.add(remaining.trailing_zeros() as usize).write(Value::DELETED);
            remaining &= remaining - 1;
        }
        slots
    }
}

/// The scope of a window's environment: its program's alone, which needs no allocation, or
/// holding the dynamic `bindings` it takes over.
pub(in super::super) fn window_scope_ref(program: ProgramId, bindings: Vec<(quench_runtime::bytecode::Atom, Value)>) -> quench_runtime::tier_surface::EnvironmentScopeRef {
    if bindings.is_empty() {
        return quench_runtime::tier_surface::EnvironmentScopeRef::of_program(program.raw());
    }
    quench_runtime::tier_surface::EnvironmentScopeRef::new(quench_runtime::tier_surface::EnvironmentScope {
        program: Some(program.raw()),
        root_eval_scope: false,
        binding_site_pc: None,
        dynamic_bindings: bindings.into(),
        with_objects: Box::default(),
    })
}
