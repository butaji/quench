//! The field stores a loop region writes natively get their slots' addresses at entry.
use super::*;

/// The slot addresses of `region`'s field stores, for `entry` (`lane_loop_entry`).
pub(super) fn lane_loop_field_stores<H: Host>(
    cx: *mut LaneContext<H>,
    r: Registers,
    region: &super::compiled::LoopRegion,
    entry: &mut super::compiled::LoopEntry,
) -> Option<()> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the running program outlives this lane's OSR entry setup.
    let code = unsafe { &*(*cx).code };
    // A raw field slot stays valid for this native run because OSR regions make no calls or
    // allocations, and the receiver is unchanged and rooted in its register. Every miss exits before the
    // store through the interpreter's exact SetField PC.
    // SAFETY: the runtime cache probe proves a primary own-data slot. `osr_loop::admit_region`
    // rejects receiver rewrites and ambiguous same-name aliases. A Number cell's
    // stores remain Numbers; a store-only field may also receive a rooted entry value.
    let receiver = |receiver: super::compiled::FieldReceiver| match receiver {
        super::compiled::FieldReceiver::This => Some(unsafe { lane_held_this(cx) }),
        super::compiled::FieldReceiver::Register(register) => Some(r.get(register)),
        super::compiled::FieldReceiver::Field(_) | super::compiled::FieldReceiver::Capture(_) => None,
    };
    // Every receiver's slots get addresses before the first is taken (`spill_inline_properties`).
    for field in &region.field_store_sites {
        vm.heap.spill_inline_properties(receiver(field.receiver)?);
    }
    for field in &region.field_store_sites {
        let object = receiver(field.receiver)?;
        let writes = region.field_store_type(field);
        let pointer = unsafe {
            vm.cached_field_store_pointer(code, object, field.atom, field.cache_site, writes)
        }?;
        // Numeric cells cannot create heap edges. Reference-capable stores log their
        // owner once at admission; nothing collects before the run ends.
        if !writes.is_subtype(crate::typed_ir::Type::NUMBER) {
            vm.heap.object_store_mut(object, object)?;
        }
        entry.push_field_store(pointer);
    }
    Some(())
}
