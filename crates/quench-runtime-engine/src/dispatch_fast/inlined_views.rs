//! The inlined views of the lane (N7): a function's second record array, derived from its inlining
//! unit (`compiled::lane_unit::unit`), whose records run the callees its call sites link in place.
//! The function's own view stays: it owns every per-PC table, and a run in the inlined view leaves
//! to it at any caller origin (`record_pc`). This registry maps an inlined record to its origin by
//! the record's address, so a slow path's PC is the original's whichever view the run is in.

use crate::compiled::lane_unit::{InlinedCall, Origin};
use quench_runtime::vm::engine_types::{Ip, LaneView};
use std::cell::{Cell, RefCell};

/// One inlined view: its records' address range, the origin of each record, the frames it inlines,
/// and the function's own view.
pub(crate) struct InlinedView {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) origins: Box<[Origin]>,
    pub(crate) frames: Box<[InlinedCall]>,
    pub(crate) caller_registers: u16,
    pub(crate) view: *const LaneView,
    pub(crate) original: *const LaneView,
}

thread_local! {
    /// The lane's inlined views, by the address of their first record.
    static INLINED: RefCell<Vec<InlinedView>> = const { RefCell::new(Vec::new()) };
    /// The span every inlined view's records lie in (`(usize::MAX, 0)` while there are none): one
    /// test tells a record of no inlined view.
    static SPAN: Cell<(usize, usize)> = const { Cell::new((usize::MAX, 0)) };
}

/// Registers `inlined`, whose records the program store keeps alive with the original's.
pub(crate) fn register(inlined: InlinedView) {
    SPAN.with(|span| {
        let (low, high) = span.get();
        span.set((low.min(inlined.start), high.max(inlined.end)));
    });
    INLINED.with(|views| {
        let mut views = views.borrow_mut();
        let at = views.partition_point(|view| view.start < inlined.start);
        views.insert(at, inlined);
    });
}

/// Whether `ip` may be a record of an inlined view (one test against the span).
#[inline(always)]
pub(crate) fn may_be_inlined(ip: Ip) -> bool {
    let address = ip.addr();
    SPAN.with(|span| {
        let (low, high) = span.get();
        address >= low && address < high
    })
}

/// `f` of the inlined view `ip` is a record of, and the record's index there; `None` for a record of
/// no inlined view.
#[cold]
pub(crate) fn with_inlined<R>(ip: Ip, f: impl FnOnce(&InlinedView, usize) -> R) -> Option<R> {
    let address = ip.addr();
    INLINED.with(|views| {
        let views = views.borrow();
        let at = views.partition_point(|view| view.start <= address).checked_sub(1)?;
        let view = &views[at];
        (address < view.end).then(|| f(view, (address - view.start) / size_of::<quench_runtime::vm::engine_types::LaneInstruction>()))
    })
}

/// The original PC of the record at `ip` of an inlined view: a caller origin's own PC, an inlined
/// origin's outermost call (the activation the original view would be at). `None` for a record of no
/// inlined view.
#[cold]
pub(crate) fn original_pc(ip: Ip) -> Option<usize> {
    with_inlined(ip, |view, index| match view.origins[index] {
        Origin::Caller(pc) => pc as usize,
        Origin::Inlined { frame, .. } => {
            let mut frame = frame;
            while let Some(parent) = view.frames[frame].parent { frame = parent; }
            view.frames[frame].call as usize
        }
    })
}

/// The inlined view a function's own view `original` runs new activations in, if one is made.
pub(crate) fn inlined_of(original: *const LaneView) -> Option<*const LaneView> {
    INLINED.with(|views| views.borrow().iter().find(|view| std::ptr::eq(view.original, original)).map(|view| view.view))
}

/// Whether `function` of `code` is a callee N7a inlines: a leaf (no call), straight-line (no
/// backward jump), short, of no handler, generator or async body.
fn leaf_callee(code: &quench_runtime::ResidualProgram, function: u32) -> bool {
    const MAX_LEAF_INSTRUCTIONS: usize = 16;
    let Some(body) = code.function_get(function as usize) else { return false };
    body.handlers.is_empty() && !body.is_generator && !body.is_async && body.code.len() <= MAX_LEAF_INSTRUCTIONS
        && body.instructions().enumerate().all(|(pc, instruction)| !quench_runtime::staged::calls(instruction.op())
            && (instruction.op().immediate_role() != quench_runtime::bytecode::ImmediateRole::JumpTarget || instruction.imm() as usize > pc))
}

/// Makes and registers the inlined view of `function` of the running program, whose own view is
/// `original` (N7a): its unit (`lane_unit::unit`) splicing in the leaf callees its call sites link,
/// derived as the function's second record array. `None`, registering nothing, where it inlines no
/// call or the function is one N7a leaves (handlers, generators, async, dynamic scopes).
pub(crate) fn make_inlined_view(cx: *mut super::LaneContext, function: u32, original: *const LaneView) -> Option<*const LaneView> {
    // SAFETY: the running lane's context; its program outlives the run.
    let (vm, code, program) = unsafe { (super::vm(cx), &*(*cx).code, (*cx).program) };
    let body = code.function_get(function as usize)?;
    if !body.handlers.is_empty() || body.is_generator || body.is_async || body.has_dynamic_scope_access() { return None; }
    let targets = |caller: u32, pc: u32| -> Vec<crate::compiled::CallTarget> {
        if caller != function { return Vec::new(); }
        super::unit_targets_in_lane(cx, caller, pc).into_iter()
            .filter(|target| matches!(target.callee, crate::compiled::CalleeBinding::Identity(_)) && leaf_callee(code, target.function))
            .take(1)
            .collect()
    };
    let unit = crate::compiled::lane_unit::unit(code, function, body, &targets, &|callee| code.function_get(callee as usize))?;
    if unit.frames.is_empty() { return None; }
    let masks = unit.register_masks(code, function).map(|masks| std::rc::Rc::new(masks.into_owned()));
    let synthetic = body.with_unit_code(&unit.instructions, unit.registers)?;
    let proper_tail_calls = vm.host.proper_tail_calls();
    let (heap, tiers) = (vm.heap_reads(), vm.programs.tiers());
    let view = vm.programs.store_extra_lane_view(program, function, |_, constants, kind, sites| {
        Some(super::derive_lane_view(function, &synthetic, constants, heap, kind, &|| masks.as_ref(), sites,
            crate::compiled::admission::record_plan(false), false, proper_tail_calls, tiers))
    })?;
    // SAFETY: the program store keeps the view alive with its program.
    let derived = unsafe { &*view };
    let start = derived.records().addr();
    let end = start + derived.records.len() * size_of::<quench_runtime::vm::engine_types::LaneInstruction>();
    register(InlinedView {
        start, end, origins: unit.origins.into_boxed_slice(), frames: unit.frames.into_boxed_slice(),
        caller_registers: unit.caller_registers, view, original,
    });
    Some(view)
}
