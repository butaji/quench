//! `String.prototype.replace` whose result only `.length` reads.
use super::*;

script_handler! {
    /// A `Call` site whose callee was `String.prototype.replace` and whose result only the
    /// `GetField "length"` right after it reads, dead after that read
    /// (`native_call_handler`'s `length_only`): the length of the replacement without
    /// building it (`Vm::steady_string_replace_length`), in the read's result register, the
    /// read skipped. Any other callee, or a call whose proof does not hold, takes
    /// `lane_native_call`, whose result the following `GetField` then reads as itself.
    #[inline(never)]
    fn lane_call_string_replace_length<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let native = match vm.heap.function(r.get(i.b)) {
            Some(FunctionRecord { kind: FunctionKind::Native(Native::StringReplace), .. }) => Native::StringReplace,
            _ => sibling!(lane_native_call::<H>, r, ip, Carried::script(cx)),
        };
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        // SAFETY: the site's next record is the `GetField` that reads this result.
        let read = unsafe { &*ip.add(1) };
        // SAFETY: the call's window is registers of the running function, which the native
        // call copies before anything can move them.
        let args = unsafe { lane_call_window(r, i) };
        let Some(result) = vm.steady_string_replace_length(code, native, r.get(i.c), args) else {
            sibling!(lane_native_call::<H>, r, ip, Carried::script(cx))
        };
        // The call's allocations may make a collection due, which the lane's safepoints take.
        note_allocation(vm);
        match result {
            Ok(length) => {
                r.set(read.a, length);
                next!(cx, r, unsafe { ip.add(2) }, Carried::script(cx))
            }
            Err(error) => {
                vm.lane_raised = Some(error);
                exit(ip)
            }
        }
    }
}

/// The handler of a `String.prototype.replace` site whose result only `.length` reads
/// (`native_call_handler`): `lane_call_string_replace_length`.
pub(super) fn replace_length_handler<H: Host>() -> Handler {
    lane_call_string_replace_length::<H> as Handler
}

/// The record after a `Call`'s that reads its result (`length_read`).
const CALL_READ_RECORD: usize = 1;
/// The record after that read, where liveness must show the call's result dead.
const AFTER_CALL_READ: usize = CALL_READ_RECORD + 1;

/// Whether the `Call` record `i` at `ip` returns into the base of the `GetField "length"`
/// right after it, which nothing reads the string again after (`native_call_handler`'s
/// `length_only`): the site may answer the call with that length alone.
pub(super) fn length_read<H: Host>(cx: *mut LaneContext<H>, ip: Ip, i: &LaneInstruction) -> bool {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let function = lane_running_function(cx);
    let Some(body) = code.function_get(function as usize) else {
        return false;
    };
    // SAFETY: `ip` is a record of the running view, which starts at the context's base.
    let pc = unsafe { ip.offset_from((*cx).base) } as usize;
    let read = pc + CALL_READ_RECORD;
    let Some(field) = body.code.get(read).map(|packed| decoded_instruction(body, *packed)) else {
        return false;
    };
    let result = i.a & crate::bytecode::REGISTER_MASK;
    if !field.op().is_get_field()
        || field.result_flags() != 0
        || field.register_b() != result
        || field.atom_index() != vm.length_atom as u32
        || !crate::bytecode::entered_only_by_fallthrough(body, read)
    {
        return false;
    }
    // The read takes the string's place, so no other use can observe it; else liveness must
    // show nothing reads the call's result after it.
    field.result_register() == result
        || code
            .register_root_masks(function)
            .and_then(|masks| masks.get(pc + AFTER_CALL_READ))
            .is_some_and(|after| crate::compile::liveness::register_dead(after, usize::from(result)))
}
