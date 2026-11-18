//! Lane handlers of the `Unary` row's general arm, and the miss arm of a `StringConcat` row
//! (`Miss::Concat`): the parts the row's pure concatenation declines.
use super::*;

script_handler! {
    /// `Unary`: logical not and `typeof` of any value, and the numeric operators of a
    /// Number, run no guest code; any other operand takes the general operation in place
    /// (`lane_unary_general`).
    pub(super) fn lane_unary<>(cx, r, ip, i, view) {
        let Some(value) = (unsafe { lane_unary_value(cx, r, i) }) else {
            sibling!(lane_unary_general::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `lane_unary` of an operand its value declines: the general operation made in place
    /// (`lane_in_place`), which may reach guest code (a `ToPrimitive` hook of an object
    /// concatenated or negated) or throw. A throw takes the general path, which raises it.
    #[inline(never)]
    fn lane_unary_general<>(cx, r, ip, i, view) {
        let (operator, operand) = (i.imm(), r.get(i.b));
        let Some(value) = lane_in_place(cx, ip, |vm, code| vm.unary(code, operator, operand)) else {
            return exit(ip);
        };
        // Guest code the operator ran made the activations frames, whose registers these are.
        let r = unsafe { (*cx).registers };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

script_handler! {
    /// `StringConcat` (`Miss::Concat`) of a window whose parts the row's pure concatenation declined
    /// (`concat_value_in_lane`): a string of parts, or the parts that convert with no guest code
    /// (`Vm::pure_string_concat_apart`), made as the general loop makes it. A window holding any
    /// other value, or a result past the string length limit, takes the general path.
    #[inline(never)]
    pub(super) fn string_concat_miss<>(cx, r, ip, i, view) {
        let vm = unsafe { vm(cx) };
        let (base, count) = (usize::from(i.b), usize::from(i.c));
        let part = |_: &Vm<H>, part: usize| r.get((base + part) as Register);
        let value = match vm.string_of_parts(count, part) {
            Some(value) => value,
            // SAFETY: the context's program outlives the lane run.
            None => match vm.pure_string_concat_apart(unsafe { &*(*cx).code }, count, part) {
                Some(value) => value,
                None => return exit(ip),
            },
        };
        note_allocation(vm);
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}
