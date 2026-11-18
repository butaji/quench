//! Reads that meet an accessor: answered without an activation where the getter's call runs
//! no guest code. The answers and activations are `AccessorCall`'s (`accessor_protocol.rs`).
use super::*;

script_handler! {
    /// The generated getter read (`GETTER_THIS`, `GETTER_REGISTER`) past its hint: the read the
    /// getter's answer makes (`AccessorCall::getter_answer`), whose getter's own field becomes the
    /// hint (`AccessorCall::hold_getter_hint`). A getter the answer declines takes `lane_get_field_getter`.
    #[inline(never)]
    pub(super) fn row_get_field_getter_refresh<>(cx, r, ip, i, view) {
        let base = unsafe { lane_field_base(cx, r, crate::bytecode::FieldBase(i.b)) };
        let Some(value) = base.and_then(|base| unsafe { AccessorCall::of(cx).hold_getter_hint(ip, i, base) }) else {
            requicken::<H>(ip, lane_get_field_getter::<H>);
            sibling!(lane_get_field_getter::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// The `FieldOperand::shape` of a record whose hint is a getter's own-field return
/// (`Vm::getter_hint_operands`), as `HOLDER_HINT` no object's exact shape.
const GETTER_HINT: u32 = super::super::NO_FIELD_RECEIVER - 3;

/// Makes the own field `slot` of `base`, which the record's getter returns, the hint of the
/// `GetField` record at `ip`, in place of the one it holds; whether the record holds it.
pub(super) fn set_getter_hint<H: Host>(vm: &mut Vm<H>, ip: Ip, atom: Atom, base: Value, slot: u16) -> bool {
    if holds_loop_site(ip) {
        return false;
    }
    // SAFETY: `ip` is a `GetField` record of a live view.
    let i = unsafe { &*ip };
    let owned = (unsafe { i.operand.field.shape } == GETTER_HINT).then(|| i.call_link.get());
    let Some(index) = vm.hold_getter_hint(owned, atom, base, slot, NO_CALL_LINK) else {
        return false;
    };
    set_record_hint(ip, GETTER_HINT, index);
    true
}
