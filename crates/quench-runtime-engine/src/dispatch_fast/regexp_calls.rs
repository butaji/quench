//! `String.prototype.replace` whose result only `.length` reads.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

script_handler! {
    /// A `Call` site whose callee was `String.prototype.replace` and whose result only the
    /// `GetField "length"` right after it reads, dead after that read (`native_call_handler`'s
    /// `length_only`): the length of the replacement without building it
    /// (`Vm::steady_string_replace_length`), in the read's result register, the read skipped.
    /// Any other callee, or a call whose proof does not hold, takes `native_call_miss`, whose
    /// result the following `GetField` then reads as itself.
    #[inline(never)]
    fn string_replace_length_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        if !matches!(vm.function_record(r.get(i.b)), Some(FunctionRecord { kind: FunctionKind::Native(Native::StringReplace), .. })) {
            sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        // SAFETY: the call's window is registers of the running function, which the native
        // call copies before anything can move them.
        let args = unsafe { call_window_in_lane(r, i) };
        let Some(outcome) = vm.steady_string_replace_length(code, Native::StringReplace, r.get(i.c), args) else {
            sibling!(native_call_miss, r, ip, Carried::script_in(cx, view))
        };
        // The call's allocations may make a collection due, which the lane's safepoints take.
        note_allocation(vm);
        match outcome {
            Ok(length) => {
                // SAFETY: the site's next record is the `GetField` that reads this result.
                let read = unsafe { &*following(ip) };
                r.set(read.a, length);
                next!(cx, r, following(following(ip)), Carried::script_in(cx, view))
            }
            Err(error) => {
                vm.lane_completion.raise(error);
                exit(ip)
            }
        }
    }
}

/// The handler of a `String.prototype.replace` site whose result only `.length` reads
/// (`native_call_handler`): `string_replace_length_miss`.
pub(crate) fn replace_length_handler() -> Handler {
    string_replace_length_miss as Handler
}

/// The record after a `Call`'s that reads its result (`length_read`).
const CALL_READ_RECORD: usize = 1;
/// The record after that read, where liveness must show the call's result dead.
const AFTER_CALL_READ: usize = CALL_READ_RECORD + 1;

/// Whether the `Call` record `i` at `ip` returns into the base of the `GetField "length"`
/// right after it, which nothing reads the string again after (`native_call_handler`'s
/// `length_only`): the site may answer the call with that length alone. Proved once, when the
/// site resolves its native.
pub(crate) fn length_read(cx: *mut LaneContext, ip: Ip, i: &LaneInstruction) -> bool {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let function = running_function_in_lane(cx);
    let body = code.function(function as usize);
    // SAFETY: `ip` is a record of the running view, which starts at the context's base.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let read = pc + CALL_READ_RECORD;
    let Some(packed) = body.code.get(read) else {
        return false;
    };
    let field = body.widened(*packed);
    let result = i.a & quench_runtime::bytecode::REGISTER_MASK;
    let reads_length = match field.field_lookup() {
        Some(quench_runtime::bytecode::FieldLookup::Atom { atom, base, .. }) => atom == vm.length_atom && base.register_index() == Some(result),
        _ => false,
    };
    if !quench_runtime::staged::reads_named_field(field.op())
        || field.result_flags() != 0
        || !reads_length
        || !quench_runtime::bytecode::entered_only_by_fallthrough(body, read)
    {
        return false;
    }
    // The read takes the string's place, so no other use can observe it; else liveness must
    // show nothing reads the call's result after it.
    field.result_register() == result
        || code
            .register_root_masks(function)
            .and_then(|masks| masks.get(pc + AFTER_CALL_READ))
            .is_some_and(|after| quench_runtime::compile::liveness::register_dead(after, usize::from(result)))
}

/// `RegExp.prototype.exec` or `test`, or `String.prototype.replace` or `match` of a RegExp, made in
/// place when it runs no guest code and raises nothing, so it observes no frame
/// (`Vm::steady_regexp_call`, `Vm::steady_string_replace`); any other such call goes on as
/// `string_or_native_call_in_lane` makes it.
#[inline(never)]
pub(super) fn regexp_call_in_lane(
    cx: *mut LaneContext,
    ip: Ip,
    i: &LaneInstruction,
    native: Native,
    callee: Value,
    receiver: Value,
    args: &[Value],
) -> Option<Value> {
    let Some(result) = steady_regexp_in_lane(cx, i, native, receiver, args) else {
        return string_or_native_call_in_lane(cx, ip, native, callee, receiver, args);
    };
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            unsafe { vm(cx) }.lane_completion.raise(error);
            None
        }
    }
}

/// The steady call of `regexp_call_in_lane` at the call record `i` (`Vm::steady_regexp_call`, `Vm::steady_string_replace`,
/// `Vm::steady_string_match`), which a compiled body's native call makes too (`body_native_call_in_lane`); `None` for any other.
#[inline]
pub(crate) fn steady_regexp_in_lane(cx: *mut LaneContext, i: &LaneInstruction, native: Native, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let result_used = ImmediateLayout::call_result_used_in_lane(i.imm());
    let steady = match (native, args) {
        (Native::StringReplace, _) => vm.steady_string_replace(code, native, receiver, args, result_used),
        (Native::StringMatch, [search, ..]) => vm.steady_string_match(code, receiver, *search, result_used),
        (_, [subject, ..]) => vm.steady_regexp_call(code, native, receiver, *subject, result_used),
        _ => None,
    }?;
    let result = vm.regexp_search_outcome(steady);
    // The call's allocations may make a collection due, which the lane's safepoints take.
    note_allocation(vm);
    Some(result)
}
