//! `GetField` of a local slot (`FieldBase::local`): the site's cached entry read inline, as the
//! register and `this` handlers read theirs, where the generic probe (`lane_field_value`) would
//! run its full ladder of checks on every read.
use super::*;

/// The handler a `GetField` of a local slot takes once its site's entries name a tier a single
/// entry answers (`lane_field_site_tier`): its `KIND` of cached read. `None` for any other
/// record, tier or receiver, which the general read keeps: a name read through function
/// information, a private name, a primitive or a receiver outside the object records.
pub(super) fn local_field_handler<H: Host>(cx: *mut LaneContext<H>, i: &LaneInstruction) -> Option<Handler> {
    let slot = crate::bytecode::FieldBase(i.b).local_index()?;
    // SAFETY: the context outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    if vm.is_private_name(i.imm()) || vm.reads_function_info(i.imm()) {
        return None;
    }
    lane_plain_local_operand(cx, slot)?.direct_object_index()?;
    match lane_field_site_tier(vm, code, i) {
        FieldSiteTier::Own => Some(lane_get_field_local::<H, RETURNING_FIELD_OWN>),
        FieldSiteTier::Holder => Some(lane_get_field_local::<H, RETURNING_FIELD_HOLDER>),
        FieldSiteTier::Inherited => Some(lane_get_field_local::<H, RETURNING_FIELD_INHERITED>),
        FieldSiteTier::Nested | FieldSiteTier::General | FieldSiteTier::Getter | FieldSiteTier::Polymorphic => None,
    }
}

script_handler! {
    /// `GetField` of a local slot whose site's `KIND` entry answers it
    /// (`local_field_handler`); any other read goes through `lane_get_field_miss`, which may
    /// move the record to a fitting handler and makes the general read.
    fn lane_get_field_local<KIND: u8>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let value = crate::bytecode::FieldBase(i.b)
            .local_index()
            .and_then(|slot| lane_plain_local_operand(cx, slot))
            .and_then(|object| returning_field_hit::<H, KIND>(unsafe { vm(cx) }, code, object, i.c));
        let Some(value) = value else {
            sibling!(lane_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}
