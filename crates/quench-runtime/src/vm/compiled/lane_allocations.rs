//! The lane helpers that allocate (`LaneHelper::NewArray`, `NewRegExp`, `Binary`, the staged claim's
//! finish and cold paths): each
//! leaves for a due collection first, and the dispatch then reads the moved arena bases again.

use super::{Host, JsStepDispatch, ResidualProgram, Value, Vm};

/// A lane body's empty object literal, as the lane's own (`dispatch_fast::lane_make_object`);
/// `DELETED` where a collection is due, which the body leaves for. The allocation may move
/// the property arena, whose base and length the dispatch reads again.
pub(super) unsafe extern "C" fn lane_new_object<H: Host>(vm: u64, _residual: u64, payload: u64) -> u64 {
    // SAFETY: the body passes its own dispatch; nothing else borrows the VM while it runs.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let object = vm.object();
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    // After the refresh, which drops a stale template: the words this object holds are the template.
    dispatch.capture_empty_object(vm, object);
    object.to_bits()
}

/// A two-property object literal of literal `site`'s shape holding `first` and `second`, as
/// the lane makes it (`dispatch_fast::lane_object_pair`); `DELETED` where a collection is due.
pub(super) unsafe extern "C" fn lane_new_object_pair<H: Host>(vm: u64, residual: u64, payload: u64, site: u64,
    first: u64, second: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let object = vm.object_pair(residual, site as usize, Value::from_bits(first), Value::from_bits(second));
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// A binary operator one operand of which is a string or BigInt, by the interpreter's own
/// definition (`Vm::binary`); `DELETED` where an operand is an object (its coercion may run
/// guest code), the operator throws, or a collection is due: the interpreter runs it then.
pub(super) unsafe extern "C" fn lane_binary<H: Host>(vm: u64, residual: u64, payload: u64, operator: u64, left: u64, right: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    let (left, right) = (Value::from_bits(left), Value::from_bits(right));
    if vm.heap.should_collect() || vm.is_object_like(left) || vm.is_object_like(right) { return Value::DELETED.to_bits(); }
    let Ok(value) = vm.binary(residual, operator as u32, left, right) else { return Value::DELETED.to_bits() };
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A lane body's array literal of the `count` program constants from `constant`, as the lane's
/// own (`dispatch_fast::lane_make_const_array`); `DELETED` where a collection is due or the
/// range is outside the constants.
pub(super) unsafe extern "C" fn lane_new_const_array<H: Host>(vm: u64, _residual: u64, payload: u64, constant: u64, count: u64) -> u64 {
    // SAFETY: as `lane_new_object`.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let Some(array) = vm.const_array_value(dispatch.program_id, constant as usize, count as usize) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    array.to_bits()
}

/// A lane body's array literal of `length` holes, or of the `elements` its definitions
/// write, as the lane's own (`dispatch_fast::lane_make_array`, `lane_make_array_elements`);
/// `DELETED` where a collection is due, as `lane_new_object`.
pub(super) unsafe extern "C" fn lane_new_array<H: Host>(vm: u64, _residual: u64, payload: u64, length: u64, elements: u64) -> u64 {
    // SAFETY: as `lane_new_object`; nonzero `elements` addresses `length` values the code wrote.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let array = if elements == 0 { vm.hole_array(length as usize) } else { vm.literal_array(length as usize, |index| Value::from_bits(unsafe { *(elements as *const u64).add(index) })) };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    // The empty array (no elements, so `hole_array`'s static empty header) is the inline template's source.
    if length == 0 && elements == 0 { dispatch.capture_empty_array(vm, array); }
    array.to_bits()
}

/// A lane body's regexp literal at `site`, as the lane makes it (`dispatch_fast::lane_regexp_literal_value`):
/// a fresh object of the site's compiled template; `DELETED` where a collection is due or the pattern
/// fails to compile, which the interpreter raises at the instruction.
pub(super) unsafe extern "C" fn lane_new_regexp<H: Host>(vm: u64, residual: u64, payload: u64, site: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let Ok(object) = vm.regexp_literal_of(residual, dispatch.program_id, site as usize) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// A lane body's object literal of literal `site`'s shape over the `count` values at `parts`,
/// as the lane makes it (`dispatch_fast::lane_make_object_literal`); `DELETED` where a
/// collection is due.
pub(super) unsafe extern "C" fn lane_new_object_literal<H: Host>(vm: u64, residual: u64, payload: u64, site: u64, parts: u64, count: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program, and `parts` addresses
    // `count` values the code wrote.
    let (vm, residual, dispatch, properties) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch), std::slice::from_raw_parts(parts as *const Value, count as usize)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    let shape = vm.object_site_shape(residual, site as usize);
    let object = vm.heap.alloc_object_with_properties(vm.object_proto, shape, properties);
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// A lane body's closure of function `id` over the frame's own environment, as the lane makes
/// it (`dispatch_fast::lane_closure_value_of`): only a function that is not a module's root and
/// has no binding sites, whose closures share the frame's environment; `DELETED` for any other,
/// inside a frameless or lane activation, or with a collection due, which the exact step makes.
/// Promotion moves the frame's captured locals into its environment, so the dispatch takes its
/// views again.
pub(super) unsafe extern "C" fn lane_new_closure<H: Host>(vm: u64, residual: u64, payload: u64, id: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch)) };
    // A lane or frameless activation has no frame of the payload's to read.
    if dispatch.lane_context != 0 || vm.frameless.inside(dispatch) || vm.heap.should_collect() {
        return Value::DELETED.to_bits();
    }
    let function = vm.frames[dispatch.frame].function;
    let shares_frame_environment = function != crate::vm::ROOT_FUNCTION_ID
        && program.function_get(function as usize).is_some_and(|body| body.binding_sites.is_empty());
    if !shares_frame_environment {
        return Value::DELETED.to_bits();
    }
    let captured = vm.frames[dispatch.frame].captured;
    // A closure that reads nothing of its environment takes none, and the frame stays unpromoted.
    let env = if program.closure_takes_no_environment(id as u32) {
        Value::NULL
    } else {
        vm.promote_frame_environment(dispatch.frame)
    };
    let closure = vm.closure(program, id as u32, env).unwrap_or(Value::DELETED);
    super::dispatch_fast::note_allocation(vm);
    // The allocation moves only the record bases; the frame's views change where this closure first captured it.
    if captured { dispatch.refresh_object_bases(vm) } else { dispatch.refresh_heap_bases(vm, program) }
    closure.to_bits()
}

/// A lane body's literal template `template` in its frame, as the interpreter builds it
/// (`Vm::literal_template`); `DELETED` inside a frameless or lane activation, whose frame is
/// not the payload's, where the template fails, or where a collection is due.
pub(super) unsafe extern "C" fn lane_new_template<H: Host>(vm: u64, residual: u64, payload: u64, template: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() || vm.frameless.inside(dispatch) || dispatch.lane_context != 0 { return Value::DELETED.to_bits(); }
    let Ok(value) = vm.literal_template(residual, dispatch.frame, template as usize) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A lane body's string concatenation of the `count` parts at `parts`, where each converts to a
/// string without guest code (`Vm::pure_string_concat`, the lane's `lane_string_concat`);
/// `DELETED` for any other part, a result past the string limit, or a due collection.
pub(super) unsafe extern "C" fn lane_new_concat<H: Host>(vm: u64, residual: u64, payload: u64, parts: u64, count: u64) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program, and `parts` addresses
    // `count` values the code wrote.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.heap.should_collect() { return Value::DELETED.to_bits(); }
    // SAFETY: as above.
    let part = |_: &Vm<H>, index: usize| Value::from_bits(unsafe { *(parts as *const u64).add(index) });
    let Some(value) = vm.pure_string_concat(residual, count as usize, part) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}
