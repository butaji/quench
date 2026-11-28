//! Read memos in a lane: a site's memoized read, its first step, and the memo reads that answer it.
use super::*;

/// A memo leaf's first step (`MemoLeaf::run`): `field` of the object `base` names, read
/// from `this`'s record where the caller holds it (a forwarding body's receiver, already
/// proven a direct object).
#[inline(always)]
pub(in super::super) fn memo_first_step(
    vm: &Vm,
    field: quench_runtime::vm::FieldCache,
    base: u16,
    arguments: ReadArguments,
    this: Value,
    this_record: Option<&quench_runtime::tier_surface::Object>,
) -> Option<Value> {
    match this_record {
        Some(record) if base == quench_runtime::bytecode::FieldBase::THIS.0 => vm.record_field_formed_hit_in(field, record),
        _ => vm.record_field_formed_hit(field, read_base_in_lane(quench_runtime::bytecode::FieldBase(base), arguments, this)?),
    }
}

/// The `script_call_memo_miss` instantiation of memo shape `shape` (`MemoRead::shape`).
pub(in super::super) fn script_call_memo_handler(shape: u8) -> Handler {
    const _: () = assert!(MEMO_NESTED < MEMO_FORWARD);
    macro_rules! shapes {
        ($($shape:expr),*) => {
            match shape {
                $(shape if shape == $shape => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::FRAME_CALL | ($shape as u16) }>(),)*
                _ => unreachable!("a memo shape is a leaf step, forwarded or not"),
            }
        };
    }
    shapes!(
        MEMO_FIELD,
        MEMO_ELEMENT,
        MEMO_LENGTH,
        MEMO_NESTED,
        MEMO_FORWARD | MEMO_FIELD,
        MEMO_FORWARD | MEMO_ELEMENT,
        MEMO_FORWARD | MEMO_LENGTH,
        MEMO_FORWARD | MEMO_NESTED
    )
}

/// The memo the call record `site` made, in the slot it names (`CallOperand::memo`); `None`
/// before its first, or once the table refilled and gave the slot to another site.
#[inline(always)]
pub(in super::super) fn site_read_memo<'a>(memos: &'a [ReadMemo], site: &LaneInstruction) -> Option<&'a ReadMemo> {
    // SAFETY: a memo's site is a call record (`Op::makes_script_call`), whose operand is `call`.
    let slot = unsafe { site.operand.call.memo };
    memos.get(slot as usize).filter(|held| std::ptr::eq(held.site, site))
}

/// Records `target`'s read at the call record `site` (`memoize_read_in_lane`) unless the site's
/// own memo of this generation missed, which keeps the full form until the generation ends.
#[inline(always)]
pub(in super::super) fn note_read_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    let stack = unsafe { &vm(cx).lane_stack };
    // SAFETY: `site` is a call record of the running view.
    if site_read_memo(&stack.read_memos, unsafe { &*site }).is_none_or(|held| held.admission.generation() != stack.generation) {
        memoize_read_in_lane(cx, site, target, arguments, this);
    }
}

/// The memo of the read `target` just answered at `site` for `arguments` and `this`, and the
/// site's memo form (`script_call_memo_miss`); nothing for an arrow's own `this`, which the
/// memo does not hold, or a read whose records no longer hit.
#[cold]
#[inline(never)]
pub(in super::super) fn memoize_read_in_lane(
    cx: *mut LaneContext,
    site: Ip,
    target: &ScriptCallTarget,
    arguments: ReadArguments,
    this: Value,
) {
    // SAFETY: the context's program outlives the lane run.
    let (vm, code) = unsafe { (vm(cx), &*(*cx).code) };
    // The memo answers the site from now on, so no call of it links its record: its record
    // names the callee for the bodies that inline it (`site_targets_in_lane`).
    // SAFETY: `site` is a record of the running view.
    vm.note_site_callee(unsafe { &(*site).call_link }, crate::compiled::CallTarget {
        callee: crate::compiled::CalleeBinding::Identity(target.callee), function: target.function });
    if target.this_binding == ThisBinding::Lexical {
        return;
    }
    let read = if target.entry == CallEntry::frameless(Frameless::ForwardsRead) {
        // SAFETY: as for `ForwardRead::of`.
        let Some(forward) = (unsafe { ForwardRead::of(target) }) else {
            return;
        };
        let Some((method, _)) = forward.method(vm, code, arguments, this) else {
            return;
        };
        // SAFETY: as `Vm::script_call_entry`; nothing here resolves a target or collects.
        let Some(inner) = vm.script_call_entry(method).map(|inner| unsafe { &*inner }) else {
            return;
        };
        if !inner.entry.reads() || inner.this_binding == ThisBinding::Lexical {
            return;
        }
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of(inner) }) else {
            return;
        };
        let entry = |hit: FieldHit| match hit {
            FieldHit::Own(cache) | FieldHit::Holder(cache) => vm.field_cache_entry(cache),
        };
        let Some(leaf) = MemoLeaf::of(vm, code, read) else {
            return;
        };
        // The steps as whole-word compares of the objects this call read (`MemoLeaf::formed`).
        let (field, method_field) = (entry(forward.field), entry(forward.method));
        let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(forward.base), arguments, this);
        let receiver = base.and_then(|base| vm.record_field_entry_hit(field, base));
        let read = match receiver.and_then(|receiver| receiver_this_in_lane(vm, inner.this_binding, receiver)) {
            Some(method_this) => leaf.formed(vm, arguments.within(target.params), method_this),
            None => leaf,
        };
        MemoRead::Forward {
            base: forward.base,
            field: base.map_or(field, |base| vm.formed_entry(field, base)),
            method_field: receiver.map_or(method_field, |receiver| vm.formed_entry(method_field, receiver)),
            params: target.params,
            method,
            read,
        }
    } else {
        // SAFETY: as for `LeafRead::of`.
        let Some(read) = (unsafe { LeafRead::of(target) }) else {
            return;
        };
        match MemoLeaf::of(vm, code, read) {
            Some(read) => MemoRead::Leaf(read.formed(vm, arguments, this)),
            None => return,
        }
    };
    let handler = script_call_memo_handler(read.shape());
    let memo = ReadMemo {
        site,
        callee: target.callee,
        admission: vm.lane_stack.current_admission(),
        this_binding: target.this_binding,
        read,
    };
    let memos = &mut vm.lane_stack.read_memos;
    // SAFETY: `site` is a call record (`Op::makes_script_call`) of the running view, whose operand is
    // `call`, written as `requicken` writes it.
    unsafe {
        if site_read_memo(memos, &*site).is_some() {
            memos[(*site).operand.call.memo as usize] = memo;
        } else {
            if memos.len() == READ_MEMOS {
                memos.clear();
            }
            (*site.cast_mut()).operand.call.memo = memos.len() as u32;
            memos.push(memo);
        }
    }
    requicken(site, handler);
}

/// The value the memo of the call record `i` answers its call with (`ReadMemo`); `None`
/// where the memo is another site's or stale, the call is another callee's or a construction's
/// (`constructing_in_lane`), or a hit misses.
///
/// # Safety
/// `cx` is the running lane's context and `i` its call record.
#[inline(always)]
pub(in super::super) unsafe fn memo_read_in_lane<const SHAPE: u8>(
    cx: *mut LaneContext,
    r: Registers,
    i: &LaneInstruction,
) -> Option<Value> {
    let vm: &Vm = unsafe { vm(cx) };
    let callee = r.get(i.b);
    let held = site_read_memo(&vm.lane_stack.read_memos, i)?;
    if held.callee != callee
        // A pending construction refuses every memo (`LaneStack::set_refused`).
        || held.admission != vm.lane_stack.admission
    {
        return None;
    }
    // A memo's steps read `this` only as a field base, which hits on a direct object alone: the
    // receiver binds itself then (`receiver_this_in_lane`), and any other receiver, whatever it
    // binds, misses the step as its binding would, so the receiver stands for `this` untested.
    if held.this_binding == ThisBinding::Lexical {
        return None;
    }
    let this = r.get(i.c);
    let arguments = ReadArguments {
        registers: r,
        base: ImmediateLayout::call_window_base(i.imm()),
        passed: ImmediateLayout::argument_count(i.imm()),
    };
    let leaf = SHAPE & !MEMO_FORWARD;
    match &held.read {
        MemoRead::Leaf(read) if SHAPE & MEMO_FORWARD == 0 => read.run(leaf, vm, arguments, this, None),
        MemoRead::Forward { base, field, method_field, params, method, read }
            if SHAPE & MEMO_FORWARD != 0 =>
        {
            let base = read_base_in_lane(quench_runtime::bytecode::FieldBase(*base), arguments, this)?;
            let receiver = vm.record_field_formed_hit(*field, base)?;
            let record = vm.heap_reads().object_record_by_id(vm.heap_reads().record_id(receiver.direct_object_index()?)?);
            if vm.record_field_formed_hit_in(*method_field, record)? != *method {
                return None;
            }
            // A memo's method binds no lexical `this` (`memoize_read_in_lane`), so the receiver, a
            // direct object, is its `this` (`receiver_this_in_lane`), and a step on `this` reads
            // its record.
            // The call passes the parameters in order, so its argument `k` is parameter `k`.
            read.run(leaf, vm, arguments.within(*params), receiver, Some(record))
        }
        _ => None,
    }
}

/// The object a read body's field record names: `this`, or the parameter its activation
/// would hold in that register; `None` for an unpassed one, whose `undefined` no read hits.
#[inline(always)]
pub(in super::super) fn read_base_in_lane(
    base: quench_runtime::bytecode::FieldBase,
    arguments: ReadArguments,
    this: Value,
) -> Option<Value> {
    match base.register_index() {
        Some(register) => arguments.get(register),
        // A local base (`FieldBase::local`) names no activation register; the call runs.
        None if base.local_index().is_some() => None,
        None => Some(this),
    }
}
