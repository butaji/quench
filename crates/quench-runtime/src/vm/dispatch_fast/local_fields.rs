//! `GetField` of a local slot (`FieldBase::local`): the generated field row (`generated_row`) reads
//! the plain local through its staged `Property` decode, and a site whose own-field hint is unbound
//! or misses takes `local_field_miss`, the cached read of the site's tier.
use super::*;

/// The handler a `GetField` of a local slot takes once its site's entries name a tier a single
/// entry answers (`field_site_tier_in_lane`): its generated row of the local-field site. `None` for any
/// other record, tier or receiver, which the general read keeps: a name read through function
/// information, a private name, a primitive or a receiver outside the object records.
pub(super) fn local_field_handler<H: Host>(cx: *mut LaneContext<H>, i: &LaneInstruction) -> Option<Handler> {
    let slot = crate::bytecode::FieldBase(i.b).local_index()?;
    // SAFETY: the context outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    if vm.is_private_name(i.imm()) || vm.reads_function_info(i.imm()) {
        return None;
    }
    plain_local_operand(cx, slot)?.direct_object_index()?;
    match field_site_tier_in_lane(vm, code, i) {
        FieldSiteTier::Own => Some(generated_row!(H, crate::staged::GET_FIELD_ROW, ACC_NONE, LOCAL_FIELD_SITE) as Handler),
        // The local row plans only the own read; a held or inherited read is its miss arm's cached
        // read, entered directly rather than through a row that always misses.
        FieldSiteTier::Holder | FieldSiteTier::Inherited => Some(local_field_miss::<H> as Handler),
        FieldSiteTier::Nested | FieldSiteTier::General | FieldSiteTier::Getter | FieldSiteTier::Polymorphic => None,
    }
}

script_handler! {
    /// The miss arm of a `GetField` of a local slot's row (`local_field_handler`): the cached read of
    /// the site's tier of the plain local, which binds the record's own-field hint at the primary
    /// own entry so the row reads it next time. Any other read goes through `row_get_field_miss`,
    /// which may move the record to a fitting handler and makes the general read.
    pub(super) fn local_field_miss<>(cx, r, ip, i, view) {
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*cx).code };
        let tier = field_site_tier_in_lane(unsafe { vm(cx) }, code, i);
        let object = crate::bytecode::FieldBase(i.b).local_index().and_then(|slot| plain_local_operand(cx, slot));
        let value = object.and_then(|object| match tier {
            FieldSiteTier::Own => returning_field_hit::<H, RETURNING_FIELD_OWN>(unsafe { vm(cx) }, code, object, i.c),
            FieldSiteTier::Holder => returning_field_hit::<H, RETURNING_FIELD_HOLDER>(unsafe { vm(cx) }, code, object, i.c),
            FieldSiteTier::Inherited => returning_field_hit::<H, RETURNING_FIELD_INHERITED>(unsafe { vm(cx) }, code, object, i.c),
            FieldSiteTier::Nested | FieldSiteTier::General | FieldSiteTier::Getter | FieldSiteTier::Polymorphic => None,
        });
        let Some(value) = value else {
            sibling!(row_get_field_miss::<H>, r, ip, Carried::script(cx))
        };
        if let (FieldSiteTier::Own, Some(object)) = (tier, object) {
            bind_local_field_hint::<H>(cx, ip, object, i.c);
        }
        r.set(i.a, value);
        next!(cx, r, following(ip), Carried::script(cx))
    }
}

/// Binds the own-field hint of the local `GetField` record at `ip` to the primary own entry of `site`
/// as `object` holds it, so the local-field row reads that field (`set_field_hint`). Only a direct
/// object whose primary entry is an own one binds; the object records are bound on the first bind.
fn bind_local_field_hint<H: Host>(cx: *mut LaneContext<H>, ip: Ip, object: Value, site: u16) {
    // SAFETY: the context's program outlives the lane run.
    let vm = unsafe { vm(cx) };
    let entry = vm.field_cache_entry(site);
    if entry.holder != super::super::NO_FIELD_HOLDER || object.direct_object_index().is_none() {
        return;
    }
    if unsafe { (*cx).binding.object_base }.is_null() {
        let Some(base) = vm.heap.object_record_base() else { return };
        // SAFETY: the context is the running lane's, and the object records it binds are live for the run.
        unsafe { (*cx).binding.object_base = base };
    }
    set_field_hint(ip, vm.form_hint(entry, object));
    requicken::<H>(ip, generated_row!(H, crate::staged::GET_FIELD_ROW, ACC_NONE, LOCAL_FIELD_SITE) as Handler);
}
