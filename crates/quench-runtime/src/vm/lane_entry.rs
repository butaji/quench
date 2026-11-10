//! A lane loop's entry record captures: rewritten global-name reads, `this`, inlined
//! callees' environment captures and initialized environment slots, all without guest code.
use super::*;

/// The entry record's captures: each rewritten global-name read, the activation's `this`, an
/// inlined callee's environment capture, or an initialized environment slot, resolved without
/// guest code.
pub(super) fn lane_entry_captures<H: Host>(
    cx: *mut LaneContext<H>,
    region: &super::compiled::LoopRegion,
    entry: &mut super::compiled::LoopEntry,
) -> Option<()> {
    if region.captures.is_empty() { return Some(()); }
    let slots = capture_reads(unsafe { (*cx).capture });
    for (index, slot) in region.captures.iter().enumerate() {
        lane_entry_capture::<H>(cx, region, entry, slots, index, *slot)?;
    }
    Some(())
}

/// One entry capture (`lane_entry_captures`), by `region.capture_globals` and its slot.
fn lane_entry_capture<H: Host>(
    cx: *mut LaneContext<H>,
    region: &super::compiled::LoopRegion,
    entry: &mut super::compiled::LoopEntry,
    slots: *const usize,
    index: usize,
    slot: u16,
) -> Option<()> {
    // A rewritten global-name read resolves like the lane's own global var read: an own data
    // property of the global object, else the entry declines (an accessor, lexical binding or
    // absent name stays the interpreter's).
    if let Some((atom, site)) = region.capture_globals.get(index).copied().flatten() {
        entry.push_capture(lane_entry_global::<H>(cx, atom, site)?);
        return Some(());
    }
    if slot == super::compiled::THIS_CAPTURE {
        let this = unsafe { lane_held_this(cx) };
        // An unbound `this` (a lexical or a dead-zone binding) needs the general lookup.
        if this.is_deleted() {
            return None;
        }
        entry.push_capture(this);
        return Some(());
    }
    if slot & super::compiled::CALLEE_CAPTURE != 0 {
        entry.push_capture(lane_entry_callee_capture(cx, region, slot)?);
        return Some(());
    }
    // SAFETY: the context's dense slots remain live until the next collection.
    let value = unsafe { crate::heap::EnvironmentSlots::read_storage(slots, slot) }
        .or_else(|| lane_capture_value(cx, 0, slot))
        .or_else(|| (!region.writes_fields()).then(|| lane_projected_capture(cx, 0, slot)).flatten())?;
    entry.push_capture(value);
    Some(())
}

/// An inlined callee's environment capture (`Inlining::callee_captures`): the binding its own
/// environment holds, or the global object property a top-level one projects.
fn lane_entry_callee_capture<H: Host>(
    cx: *mut LaneContext<H>,
    region: &super::compiled::LoopRegion,
    slot: u16,
) -> Option<Value> {
    let inlining = region.inlining.as_deref()?;
    let &(frame, callee_slot) = inlining.callee_captures.get(usize::from(slot & !super::compiled::CALLEE_CAPTURE))?;
    let callee = inlining.frames.get(frame)?.callee;
    let vm = unsafe { vm(cx) };
    // SAFETY: as `Vm::script_call_entry`; nothing below resolves a target.
    let env = unsafe { &*vm.script_call_entry(callee)? }.env;
    vm.environment_capture(env, callee_slot).or_else(|| {
        (!region.writes_fields())
            .then(|| vm.environment_capture_env(env, false, 0)
                .and_then(|env| vm.plain_projected_root_binding(env, callee_slot)))
            .flatten()
    })
}

