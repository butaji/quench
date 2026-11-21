//! What the interpreter's staged frame reads beyond its registers: the constants, local slots and
//! capture slots a jump record or a capture row names. A record's operands are data decoded once
//! (`RecordOperand`); the frame answers each from the running lane, so a handler states which
//! operands its record holds and the staged step decides the rest.

use super::*;
use crate::bytecode::{Operand, OperandKind};

/// The constant index a staged operand names where the record holds its constant as an immediate
/// (`RecordOperand::Immediate`): the frame answers that index with the immediate itself.
const IMMEDIATE_CONSTANT: u32 = 0;

/// A comparison jump's operand as its record holds it.
#[derive(Clone, Copy)]
pub(super) enum RecordOperand {
    Register(u16),
    /// A plain local slot of the running function.
    Local(u16),
    /// A constant of the program, by index.
    Constant(u16),
    /// A constant the record carries itself (a small integer, a Boolean).
    Immediate(Value),
}

impl RecordOperand {
    /// The operand an encoded record field names; `None` for a field operand, which only the
    /// general path reads.
    #[inline(always)] pub(super) fn encoded(encoded: u16) -> Option<Self> {
        let operand = Operand(encoded);
        Some(match operand.kind()? {
            OperandKind::Register => Self::Register(operand.payload()),
            OperandKind::Local => Self::Local(operand.payload()),
            OperandKind::Constant => Self::Constant(operand.payload()),
            OperandKind::Field => return None,
        })
    }

    /// The staged operand, and the immediate the frame answers its constant index with; inlined so
    /// the encoding folds into the reader, which decodes it again (`Reads::word`).
    #[inline(always)]
    fn staged(self) -> (Operand, Option<Value>) {
        let constant = |index| Operand::constant(index).expect("a record's constant operand was a constant operand");
        match self {
            Self::Register(register) => (Operand::register(register), None),
            Self::Local(slot) => (Operand::local(slot), None),
            Self::Constant(index) => (constant(u32::from(index)), None),
            Self::Immediate(value) => (constant(IMMEDIATE_CONSTANT), Some(value)),
        }
    }

    /// The value the operand holds in `r`, as the frame reads it.
    #[inline(always)] pub(super) fn value<H: Host>(self, cx: *mut LaneContext<H>, r: Registers) -> Option<Value> {
        match self {
            Self::Register(register) => Some(r.get(register)),
            operand => {
                let (staged, immediate) = operand.staged();
                Reads::of_jump::<H>(cx, immediate).word(staged)
            }
        }
    }

    /// The staged operands of a jump over `operands`, and the reads that answer them.
    #[inline(always)]
    pub(super) fn staged_pair<H: Host>(cx: *mut LaneContext<H>, operands: [Self; 2]) -> ([Operand; 2], Reads) {
        // Destructured, not `array::map`, whose out-of-line drain would keep the pair in memory.
        let [left, right] = operands;
        let ((left, left_immediate), (right, right_immediate)) = (left.staged(), right.staged());
        ([left, right], Reads::of_jump::<H>(cx, left_immediate.or(right_immediate)))
    }
}

/// The reads a staged frame makes outside its registers, erased of the lane's host so the frame
/// names no `H`.
#[derive(Clone, Copy)]
pub(super) struct Reads {
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
    pub(super) const NONE: Self = Self { constants: &[], immediate: None, locals: None, captures: None };

    /// The reads of a comparison jump of the running lane's `cx`, whose record holds `immediate`.
    #[inline(always)]
    pub(super) fn of_jump<H: Host>(cx: *mut LaneContext<H>, immediate: Option<Value>) -> Self {
        let context: *mut () = cx.cast();
        Self { constants: unsafe { (*cx).constants }, immediate,
            locals: Some(LocalReads { context, read: |context, slot|
                plain_local_operand(context.cast::<LaneContext<H>>(), slot) }),
            captures: None }
    }

    /// The reads of a capture access of the running lane's `cx`.
    #[inline(always)]
    pub(super) fn of_captures<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { captures: Some(CaptureReads { context: cx.cast(), read: |context, depth, slot, root|
            capture_slot_read(context.cast::<LaneContext<H>>(), depth, slot, root) }), ..Self::NONE }
    }

    /// The value of a constant or local operand. Inlined, as `capture`: a call here would take the
    /// address of the frame that holds these reads, which keeps the whole frame in memory and
    /// every handler that builds one would initialize it in full before its fast path.
    #[inline(always)]
    pub(super) fn word(&self, operand: Operand) -> Option<Value> {
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
    pub(super) fn capture(&self, depth: u16, slot: u16, root: bool) -> Option<Value> {
        let CaptureReads { context, read } = self.captures?;
        read(context, depth, slot, root)
    }
}

/// The capture `slot` past the running function's own scope, by the lane's own lookups, which run
/// no guest code: a frame run's remembered walk, an activation's walk memo (`LaneContext::outer`)
/// or a new walk it then remembers; with `root`, the root environment's projected binding
/// (`projected_capture`). A read at the function's own scope is the walk handler's, which
/// also finds the function's dense slots (`LaneContext::capture`).
fn capture_slot_read<H: Host>(cx: *mut LaneContext<H>, depth: u16, slot: u16, root: bool) -> Option<Value> {
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
