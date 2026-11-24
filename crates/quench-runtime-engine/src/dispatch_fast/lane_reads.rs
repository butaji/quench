//! What the interpreter's staged frame reads beyond its registers: the constants, local slots and
//! capture slots a jump record or a capture row names; the frame answers each from the running lane.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::{Operand, OperandKind};

/// The reads a staged frame makes outside its registers, erased of the lane's host so the frame
/// names no `H`.
#[derive(Clone, Copy)]
pub(crate) struct Reads {
    /// The program's constants, which constant operands index (`LaneContext::constants`).
    constants: *const [Value],
    /// The record's own constant, which answers every constant operand where there is one.
    immediate: Option<Value>,
    locals: Option<LocalReads>,
    captures: Option<CaptureReads>,
}

/// The running function's plain local slots, read as the general loop reads them.
#[derive(Clone, Copy)]
struct LocalReads {
    context: *mut (),
    read: fn(*mut (), u16) -> Option<Value>,
}

/// The capture slots past the running function's own, read by the lane's lookups.
#[derive(Clone, Copy)]
struct CaptureReads {
    context: *mut (),
    read: fn(*mut (), u16, u16, bool) -> Option<Value>,
}

impl Reads {
    /// The reads of a frame that makes none: its operands are registers.
    pub(crate) const NONE: Self = Self { constants: &[], immediate: None, locals: None, captures: None };

    /// The reads of a comparison jump of the running lane's `cx`, whose record holds `immediate`.
    #[inline(always)]
    pub(crate) fn of_jump(cx: *mut LaneContext, immediate: Option<Value>) -> Self {
        let context: *mut () = cx.cast();
        Self { constants: unsafe { (*cx).constants }, immediate,
            locals: Some(LocalReads { context, read: |context, slot|
                plain_local_operand(context.cast::<LaneContext>(), slot) }),
            captures: None }
    }

    /// These reads with every constant operand answered by `immediate`: a record whose one constant
    /// the view folded into the record (`specialize_plain_operators`' int32 form).
    #[inline(always)]
    pub(crate) fn with_immediate(self, immediate: Value) -> Self {
        Self { immediate: Some(immediate), ..self }
    }

    /// The reads of a capture access of the running lane's `cx`.
    #[inline(always)]
    pub(crate) fn of_captures(cx: *mut LaneContext) -> Self {
        Self { captures: Some(CaptureReads { context: cx.cast(), read: |context, depth, slot, root|
            capture_slot_read(context.cast::<LaneContext>(), depth, slot, root) }), ..Self::NONE }
    }

    /// The value of a constant or local operand. Inlined, as `capture`: a call here would take the
    /// address of the frame that holds these reads, which keeps the whole frame in memory and
    /// every handler that builds one would initialize it in full before its fast path.
    #[inline(always)]
    pub(crate) fn word(&self, operand: Operand) -> Option<Value> {
        match operand.kind()? {
            // SAFETY: the run's program keeps its constants (`LaneContext::constants`).
            OperandKind::Constant => self.immediate.or_else(|| unsafe { &*self.constants }
                .get(usize::from(operand.payload())).copied()),
            OperandKind::Local => {
                let LocalReads { context, read } = self.locals?;
                read(context, operand.payload())
            }
            OperandKind::Register | OperandKind::Field => None,
        }
    }

    /// The capture `slot`, `depth` scopes out, or the root's projected binding where `root`.
    #[inline(always)]
    pub(crate) fn capture(&self, depth: u16, slot: u16, root: bool) -> Option<Value> {
        let CaptureReads { context, read } = self.captures?;
        read(context, depth, slot, root)
    }
}

/// The capture `slot` past the running function's own scope, by the lane's own lookups, which run
/// no guest code: a frame run's remembered walk, an activation's walk memo (`LaneContext::outer`)
/// or a new walk it then remembers; with `root`, the root environment's projected binding
/// (`projected_capture`). A read at the function's own scope is the walk handler's, which
/// also finds the function's dense slots (`LaneContext::capture`).
fn capture_slot_read(cx: *mut LaneContext, depth: u16, slot: u16, root: bool) -> Option<Value> {
    if root {
        return projected_capture(cx, depth, slot);
    }
    if depth == 0 {
        return None;
    }
    let vm = unsafe { vm(cx) };
    let env = unsafe { (*cx).registers }.env();
    if env.is_deleted() {
        let frame = &vm.frames[unsafe { (*cx).frame() }];
        return vm.remembered_capture(frame.env, frame.captured, depth, slot);
    }
    let memo = unsafe { (*cx).outer };
    let slots = if memo.env == env && memo.depth == depth {
        memo.slots
    } else {
        vm.link_capture_chain(env, depth);
        let (slots, root) = vm.walked_capture_slots(env, false, depth)?;
        root.is_none().then_some(slots)?
    };
    // SAFETY: as `OuterWalk`; no collection runs in the lane.
    let value = unsafe { &*slots }.dense().and_then(|dense| dense.dense_plain(usize::from(slot)))?;
    unsafe { (*cx).outer = OuterWalk { env, depth, slots } };
    Some(value)
}
