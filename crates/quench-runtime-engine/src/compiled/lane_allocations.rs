//! The lane helpers that allocate (`LaneHelper::NewArray`, `NewRegExp`, `Binary`, the staged claim's
//! finish and cold paths): each
//! leaves for a due collection first, and the dispatch then reads the moved arena bases again. The
//! materializations of kept values (`LaneHelper::MaterializeArray`) are the exception: they allocate
//! whatever is due, since their code has no leave to take (DESIGN 4.7).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{JsStepDispatch, ResidualProgram, Value, Vm};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// A lane body's empty object literal, as the lane's own (the `MakeObject` row, `Allocation::Empty`);
/// `DELETED` where a collection is due, which the body leaves for. The allocation may move
/// the property arena, whose base and length the dispatch reads again.
pub(crate) unsafe extern "C" fn compiled_new_object(vm: u64, _residual: u64, payload: u64) -> u64 {
    // SAFETY: the body passes its own dispatch; nothing else borrows the VM while it runs.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let object = vm.object();
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    // After the refresh, which drops a stale template: the words this object holds are the template.
    dispatch.capture_empty_object(vm, object);
    object.to_bits()
}

/// A two-property object literal of literal `site`'s shape holding `first` and `second`, as
/// the lane makes it (`dispatch_fast::object_pair_in_lane`); `DELETED` where a collection is due.
pub(crate) unsafe extern "C" fn new_object_pair_in_lane(vm: u64, residual: u64, payload: u64, site: u64,
    first: Value, second: Value) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let object = vm.object_pair(residual, site as usize, first, second);
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// A binary operator one operand of which is a string or BigInt, by the interpreter's own
/// definition (`Vm::binary`); `DELETED` where an operand is an object (its coercion may run
/// guest code), the operator throws, or a collection is due: the interpreter runs it then.
pub(crate) unsafe extern "C" fn binary_in_lane(vm: u64, residual: u64, payload: u64, operator: u64, left: Value, right: Value) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() || vm.is_object_like(left) || vm.is_object_like(right) { return Value::DELETED.to_bits(); }
    let Ok(value) = vm.binary(residual, operator as u32, left, right) else { return Value::DELETED.to_bits() };
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A scalar native's call in a loop region or a body (`lane_region::native_calls::Lowering::scalar_call`): the value its
/// in-place case answers for the call of `callee` (a scalar native, `global_facts::scalar_native`) with `receiver` and the
/// `argc` (one or two) arguments `first` and `second`; `DELETED` where that case leaves (an object argument, an error, a due
/// collection), which the call's own step takes. Nothing is changed before a value is answered: the native runs no guest
/// code and changes no array.
pub(crate) unsafe extern "C" fn scalar_native_in_lane(vm: u64, residual: u64, payload: u64, callee: Value, argc: u64,
    receiver: Value, first: Value, second: Value) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    let Some(quench_runtime::tier_surface::FunctionRecord { kind: quench_runtime::tier_surface::FunctionKind::Native(native), .. }) = vm.function_record(callee) else {
        return Value::DELETED.to_bits();
    };
    let native = *native;
    // A call an in-place native's loop makes (`native_calls::IN_PLACE_CALL`) passes its receiver and up to two arguments.
    let in_place = argc & super::lane_region::IN_PLACE_CALL != 0;
    let argc = argc & !super::lane_region::IN_PLACE_CALL;
    let admitted = if in_place { crate::dispatch_fast::in_place_in_loop(native) && argc <= 2 }
        else { quench_runtime::vm::global_facts::scalar_native(native) && (1..=2).contains(&argc) };
    if !admitted || vm.collection_due() {
        return Value::DELETED.to_bits();
    }
    let arguments = [first, second];
    // A native that allocated nothing (a lookup: `Map.prototype.get`) moved no base the code holds.
    let unchanged = |vm: &Vm| (vm.heap_reads().allocated_weight(), vm.heap_reads().native_layout_epoch(), vm.heap_reads().store_watch_word(), vm.method_cache_atoms.words().len());
    let before = unchanged(vm);
    let Some(value) = vm.in_place_native_value(residual, native, callee, receiver, &arguments[..argc as usize]) else {
        return Value::DELETED.to_bits();
    };
    if unchanged(vm) != before {
        super::dispatch_fast::note_allocation(vm);
        dispatch.refresh_object_bases(vm);
    }
    value.to_bits()
}

/// A lane body's collection lookup (`native_calls::Inline::Lookup`): the lookup `callee` is (`native_calls::LOOKUPS`)
/// of the collection `receiver` at `key` (`Vm::pure_collection_lookup`), which allocates nothing and moves no base;
/// `DELETED` where the callee is no such native or `receiver` no collection of its kind.
pub(crate) unsafe extern "C" fn collection_lookup_in_lane(vm: u64, _residual: u64, callee: Value, receiver: Value, key: Value) -> u64 {
    // SAFETY: as `scalar_native_in_lane`; the lookup only reads the heap.
    let vm = unsafe { &*(vm as *const Vm) };
    match vm.function_record(callee) {
        Some(quench_runtime::tier_surface::FunctionRecord { kind: quench_runtime::tier_surface::FunctionKind::Native(native), .. })
            if super::lane_region::LOOKUPS.contains(native) => vm.pure_collection_lookup(*native, receiver, key),
        _ => None,
    }.map_or(Value::DELETED.to_bits(), Value::to_bits)
}

/// A lane body's array literal of the `count` program constants from `constant`, as the lane's
/// own (the `MakeConstArray` row, `Allocation::ConstArray`); `DELETED` where a collection is due or the
/// range is outside the constants.
pub(crate) unsafe extern "C" fn new_const_array_in_lane(vm: u64, _residual: u64, payload: u64, constant: u64, count: u64) -> u64 {
    // SAFETY: as `compiled_new_object`.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let Some(array) = vm.const_array_value(dispatch.program_id, constant as usize, count as usize) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    array.to_bits()
}

/// A lane body's array literal of `length` holes, or of the `elements` its definitions
/// write, as the lane's own (the `MakeArray` row and its `DefineElement` rows);
/// `DELETED` where a collection is due, as `compiled_new_object`.
pub(crate) unsafe extern "C" fn new_array_in_lane(vm: u64, residual: u64, payload: u64, length: u64, elements: u64) -> u64 {
    // SAFETY: as `compiled_new_object`.
    if unsafe { (*(vm as *const Vm)).collection_due() } { return Value::DELETED.to_bits(); }
    // SAFETY: as `materialize_array_in_lane`, whose arguments these are.
    unsafe { materialize_array_in_lane(vm, residual, payload, length, elements) }
}

/// The array `new_array_in_lane` makes, made whatever is due: a materialization of a kept array
/// (`lane_region::virtual_objects::arrays`) allocates here and never refuses. The allocation only
/// sets a collection due (DESIGN 4.7); the next safepoint collects, the lane's leave or the next
/// allocating helper's refusal or poll, so nothing is collected between the materialized arrays.
///
/// # Safety
/// As `compiled_new_object`; nonzero `elements` addresses `length` values the code wrote.
pub(crate) unsafe extern "C" fn materialize_array_in_lane(vm: u64, _residual: u64, payload: u64, length: u64, elements: u64) -> u64 {
    // SAFETY: as `compiled_new_object`; nonzero `elements` addresses `length` values the code wrote.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm), &mut *(payload as *mut JsStepDispatch)) };
    let array = if elements == 0 { vm.hole_array(length as usize) } else { vm.literal_array(length as usize, |index| Value::from_bits(unsafe { *(elements as *const u64).add(index) })) };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    // The empty array (no elements, so `hole_array`'s static empty header) is the inline template's source.
    if length == 0 && elements == 0 { dispatch.capture_empty_array(vm, array); }
    array.to_bits()
}

/// The element `index` of `array`, a hole array a materialization of a kept array made (`new_array_in_lane`
/// with no elements), defined as its literal's `DefineArrayElement` would (`Vm::define_literal_element`); `0`
/// where the define refuses, which the code traps on.
pub(crate) unsafe extern "C" fn define_kept_element_in_lane(vm: u64, _residual: u64, payload: u64, array: u64, index: u64, value: u64) -> u64 {
    // SAFETY: as `compiled_new_object`; `array` is the hole array the materialization just made, held by no other.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm), &mut *(payload as *mut JsStepDispatch)) };
    let defined = vm.define_literal_element(Value::from_bits(array), index as usize, Value::from_bits(value));
    dispatch.refresh_object_bases(vm);
    u64::from(defined)
}

/// A lane body's regexp literal at `site`, as the lane makes it (`dispatch_fast::regexp_literal_value_in_lane`):
/// a fresh object of the site's compiled template; `DELETED` where a collection is due or the pattern
/// fails to compile, which the interpreter raises at the instruction. A callee-argument site passes
/// its call's callee (`DELETED` for any other site, which is no String method): where the activation
/// has a key (`regexp_argument_key`) the site answers as the interpreter's does
/// (`Vm::regexp_literal_argument_of`), its one shared object while the callee is the realm's
/// String method and the object unobservable.
pub(crate) unsafe extern "C" fn lane_new_regexp(vm: u64, residual: u64, payload: u64, site: u64, callee: Value) -> u64 {
    // SAFETY: as `lane_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let made = match regexp_argument_key(dispatch).filter(|_| !callee.is_deleted()) {
        Some(key) => vm.regexp_literal_argument_of(residual, dispatch.program_id, key, site as usize, callee),
        None => vm.regexp_literal_of(residual, dispatch.program_id, site as usize),
    };
    let Ok(object) = made else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// The key a callee-argument regexp literal evaluated by the payload's activation shares its
/// object under (`RegExpArgumentShare::frame`): the payload's frame index where the code runs as
/// that frame (a loop region or body of it, and any callee inlined into it), the interpreter's own
/// key for it; `None` inside a lane activation, which then makes a fresh object.
///
/// Sound because no two activations live at once with a use of the object in flight hold one key,
/// which is all `Vm::regexp_literal_argument_of` asks of a frame index: a use runs builtins alone
/// unless guest code runs, and guest code that runs during it runs in a frame above the use's (an
/// index above the payload's), which continues only once that frame returned and the use completed.
/// An inlined callee shares its caller's key, so it may answer the caller's object, but only to an
/// evaluation that completes its own use before the caller's starts (between the literal and the
/// call nothing suspends), the same sequential reuse as two evaluations of one frame. A lane
/// activation (`lane_context`) has no frame and its records are reused across lane runs this
/// helper cannot tell apart.
fn regexp_argument_key(dispatch: &JsStepDispatch) -> Option<usize> {
    (dispatch.lane_context == 0).then_some(dispatch.frame)
}

/// A lane body's object literal of literal `site`'s shape over the `count` values at `parts`,
/// as the lane makes it (the `MakeObjectLiteral` row, `Allocation::ObjectLiteral`); `DELETED` where a
/// collection is due.
pub(crate) unsafe extern "C" fn new_object_literal_in_lane(vm: u64, residual: u64, payload: u64, site: u64, parts: u64, count: u64) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program, and `parts` addresses
    // `count` values the code wrote.
    let (vm, residual, dispatch, properties) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch), std::slice::from_raw_parts(parts as *const Value, count as usize)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let shape = vm.object_site_shape(residual, site as usize);
    let object = vm.allocate_object_with_properties(vm.object_proto, shape, properties);
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    object.to_bits()
}

/// A lane body's closure of function `id` over the frame's own environment, as the lane makes
/// it (`dispatch_fast::closure_value_of_in_lane`): only a function that is not a module's root and
/// has no binding sites, whose closures share the frame's environment; `DELETED` for any other,
/// inside a lane activation, or with a collection due, which the exact step makes.
/// Promotion moves the frame's captured locals into its environment, so the dispatch takes its
/// views again.
pub(crate) unsafe extern "C" fn new_closure_in_lane(vm: u64, residual: u64, payload: u64, id: u64) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program.
    let (vm, program, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram),
        &mut *(payload as *mut JsStepDispatch)) };
    // A lane activation has no frame of the payload's to read.
    if dispatch.lane_context != 0 || vm.collection_due() {
        return Value::DELETED.to_bits();
    }
    let function = vm.frames[dispatch.frame].function;
    let shares_frame_environment = function != quench_runtime::vm::ROOT_FUNCTION_ID
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
    if captured { dispatch.refresh_object_bases(vm) } else { dispatch.refresh_heap_bases(vm) }
    closure.to_bits()
}

/// A lane body's literal template `template` in its frame, as the interpreter builds it
/// (`Vm::literal_template`); `DELETED` inside a lane activation, whose frame is
/// not the payload's, where the template fails, or where a collection is due.
pub(crate) unsafe extern "C" fn new_template_in_lane(vm: u64, residual: u64, payload: u64, template: u64) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() || dispatch.lane_context != 0 { return Value::DELETED.to_bits(); }
    let Ok(value) = vm.literal_template(residual, dispatch.frame, template as usize) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A lane body's string concatenation of the `count` parts at `parts`, where each converts to a
/// string without guest code (`Vm::pure_string_concat`, the lane's `StringConcat` row);
/// `DELETED` for any other part, a result past the string limit, or a due collection.
pub(crate) unsafe extern "C" fn new_concat_in_lane(vm: u64, residual: u64, payload: u64, parts: u64, count: u64) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program, and `parts` addresses
    // `count` values the code wrote.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    // SAFETY: as above.
    let part = |_: &Vm, index: usize| Value::from_bits(unsafe { *(parts as *const u64).add(index) });
    let Some(value) = vm.pure_string_concat(residual, count as usize, part) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A rope's flat form as a new string value, for a loop that holds the string's flat facts
/// (`Lowering::hoist_strings`): the same string in a record that holds its units flat, which the
/// loop's entry proves. `DELETED` where the value is no string, its text has no record form a row
/// reads (a wide rope's units are held beside its record), or a collection is due; nothing has
/// changed then, so the loop's entry leaves.
pub(crate) unsafe extern "C" fn flatten_string_in_lane(vm: u64, _residual: u64, payload: u64, string: Value) -> u64 {
    // SAFETY: as `concat_pair_in_lane`; the flattening allocates a record and runs no guest code.
    let (vm, dispatch) = unsafe { (&mut *(vm as *mut Vm), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let Some(flat) = vm.heap_reads().string(string).map(|text| text.unkept_flat()).filter(|flat| flat.holds_record_form())
        else { return Value::DELETED.to_bits() };
    let value = vm.allocate_string(flat);
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}

/// A lane body's `+` of two operands that become strings without guest code (`Vm::primitive_concatenation`,
/// the interpreter's rule for `+` once a string operand is present): the joined string, which
/// appends in place where the left operand is the accumulator its buffer ends at. `DELETED` where
/// the pair is another, a result is past the string limit, or a collection is due; nothing has
/// changed then, so the interpreter takes the operator.
pub(crate) unsafe extern "C" fn concat_pair_in_lane(vm: u64, residual: u64, payload: u64, left: Value, right: Value) -> u64 {
    // SAFETY: as `compiled_new_object`; the residual is the running program.
    let (vm, residual, dispatch) = unsafe { (&mut *(vm as *mut Vm), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch)) };
    if vm.collection_due() { return Value::DELETED.to_bits(); }
    let Some(value) = vm.primitive_concatenation(residual, left, right) else { return Value::DELETED.to_bits() };
    super::dispatch_fast::note_allocation(vm);
    dispatch.refresh_object_bases(vm);
    value.to_bits()
}
