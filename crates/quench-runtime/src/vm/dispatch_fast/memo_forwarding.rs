//! Forwarding read memos: a call whose body forwards to a method read, memoized as the two field reads it makes.

use super::*;

/// The memo of a call whose read target forwards to a method's read (`CallEntry::ForwardsRead`):
/// the forwarding body's field and method reads, and the method's read; `None` where it does not hold.
pub(super) fn forward_memo_read<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, target: &ScriptCallTarget,
    arguments: ReadArguments, this: Value) -> Option<MemoRead> {
    // SAFETY: as for `ForwardRead::of`.
    let forward = unsafe { ForwardRead::of::<H>(target) }?;
    let (method, _) = forward.method(vm, code, arguments, this)?;
    // SAFETY: as `Vm::script_call_entry`; nothing here resolves a target or collects.
    let inner = vm.script_call_entry(method).map(|inner| unsafe { &*inner })?;
    if !inner.entry.reads() || inner.this_binding == ThisBinding::Lexical {
        return None;
    }
    // SAFETY: as for `LeafRead::of`.
    let read = unsafe { LeafRead::of::<H>(inner) }?;
    let leaf = MemoLeaf::of(vm, code, read)?;
    let entry = |hit: FieldHit| match hit {
        FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
    };
    // The steps as whole-word compares of the objects this call read (`MemoLeaf::formed`).
    let (field, method_field) = (entry(forward.field), entry(forward.method));
    let base = lane_read_base(crate::bytecode::FieldBase(forward.base), arguments, this);
    let receiver = base.and_then(|base| vm.record_field_entry_hit(field, base));
    let read = match receiver.and_then(|receiver| lane_receiver_this(vm, inner.this_binding, receiver)) {
        Some(method_this) => leaf.formed(vm, arguments.within(target.params), method_this),
        None => leaf,
    };
    Some(MemoRead::Forward {
        base: forward.base,
        field: base.map_or(field, |base| vm.formed_entry(field, base)),
        method_field: receiver.map_or(method_field, |receiver| vm.formed_entry(method_field, receiver)),
        params: target.params,
        method,
        read,
    })
}

/// The memo a read `target` answers with: a forwarding read's chain, else the target's own read
/// as a leaf. `None` where no memo holds.
fn memo_read_of<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, target: &ScriptCallTarget,
    arguments: ReadArguments, this: Value) -> Option<MemoRead> {
    if target.entry == CallEntry::ForwardsRead {
        return forward_memo_read::<H>(vm, code, target, arguments, this);
    }
    // SAFETY: as for `LeafRead::of`.
    let read = unsafe { LeafRead::of::<H>(target) }?;
    MemoLeaf::of(vm, code, read).map(|read| MemoRead::Leaf(read.formed(vm, arguments, this)))
}

/// The memo of the read `target` just answered at `site` for `arguments` and `this`, and the
/// site's memo form (`lane_script_call_memo`); nothing for an arrow's own `this`, which the
/// memo does not hold, or a read whose records no longer hit.
#[cold]
#[inline(never)]
pub(super) fn lane_memoize_read<H: Host>(
    cx: *mut LaneContext<H>,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // The memo answers the site from now on, so no call of it links its record: its record
    // names the callee for the bodies that inline it (`lane_site_targets`).
    // SAFETY: `site` is a record of the running view.
    vm.note_site_callee(unsafe { &(*site).call_link }, super::compiled::CallTarget {
        callee: super::compiled::CalleeBinding::Identity(target.callee), function: target.function });
    if target.this_binding == ThisBinding::Lexical {
        return;
    }
    let Some(read) = memo_read_of(vm, code, target, arguments, this) else {
        return;
    };
    let handler = script_call_memo_handler::<H>(read.shape());
    let memo = ReadMemo {
        site,
        callee: target.callee,
        admission: vm.lane_stack.current_admission(),
        this_binding: target.this_binding,
        read,
    };
    store_read_memo(vm, site, memo);
    requicken::<H>(site, handler);
}
