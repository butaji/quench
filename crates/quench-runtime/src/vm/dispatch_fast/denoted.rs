//! Lane handlers an operation's JavaScript denotation (`JsDenotation`) selects: numeric
//! binaries and unaries, and string concatenation answered where no part's conversion runs
//! guest code.
use super::*;

script_handler! {
    /// `Unary`: logical not and `typeof` of any value, and the numeric operators of a
    /// Number, run no guest code; any other operand takes the general path.
    fn lane_unary<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_unary_value(cx, r, i) }) else {
            return exit(ip);
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `StringConcat` of a window whose every part converts to a string purely
    /// (`Vm::converts_to_string_purely`), made as the general loop makes it; a window holding
    /// any other value, or a result past the string length limit, takes the general path.
    fn lane_string_concat<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (base, count) = (usize::from(i.b), usize::from(i.c));
        let part = |_: &Vm<H>, part: usize| r.get((base + part) as Register);
        let value = match vm.ascii_string_concat(count, part) {
            Some(text) => vm.heap.alloc_string(text),
            None => {
                if !(0..count).all(|index| vm.converts_to_string_purely(part(vm, index))) {
                    return exit(ip);
                }
                // SAFETY: the context's program outlives the lane run.
                let Ok(value) = vm.converted_string_concat(unsafe { &*(*cx).code }, count, part) else {
                    return exit(ip);
                };
                value
            }
        };
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The lane handler `op`'s denotation selects, if it selects one.
pub(super) const fn denoted_handler<H: Host>(op: Op) -> Option<Handler> {
    match op.js_denotation() {
        crate::bytecode::JsDenotation::NumericBinary => Some(lane_binary::<H>),
        crate::bytecode::JsDenotation::NumericUnary => Some(lane_unary::<H>),
        crate::bytecode::JsDenotation::StringConcatenation => Some(lane_string_concat::<H>),
        _ => None,
    }
}

/// The lane handler of a `Unary` record, which a view specializes further.
pub(super) const fn unary_handler<H: Host>() -> Handler {
    lane_unary::<H>
}
