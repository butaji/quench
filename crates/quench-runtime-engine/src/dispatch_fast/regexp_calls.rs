//! `String.prototype.replace` whose result only `.length` reads.
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
        if !matches!(vm.heap.function(r.get(i.b)), Some(FunctionRecord { kind: FunctionKind::Native(Native::StringReplace), .. })) {
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
pub(super) fn replace_length_handler() -> Handler {
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
pub(super) fn length_read(cx: *mut LaneContext, ip: Ip, i: &LaneInstruction) -> bool {
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
    let field = decoded_instruction(body, *packed);
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
