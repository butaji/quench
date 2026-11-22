//! Proved loop kernels: a counted loop whose body is straight-line Number work over record
//! arrays runs as one record (`loop_kernel_head`, on the loop's test record) that executes
//! the body column-wise over chunks of iterations in Rust loops, instead of one dispatched
//! record per operation per iteration.
//!
//! A kernel is the loop's own semantics with every per-iteration check moved to the loop's
//! entry or to the end of a chunk (`shape` finds the loops, `run` executes them):
//! - the loop is `H: JumpBinaryFalse counter REL bound -> exit; body; Jump H`, the exit right
//!   after the jump, and the body straight-line `GetIndex`, `SetIndex`, Number operators,
//!   `IncDec`, `Move` and `LoadConst`;
//! - every register the body writes is an induction (stepped by a constant, an int32 at
//!   entry) or a value the body computes; every index is an induction or an unwritten
//!   register plus a constant; every base is an unwritten register holding a record array;
//! - so the trip count comes from the test, the bounds of every access are checked once for
//!   the whole run, and each operator's operands are Numbers (loaded elements are checked as
//!   they are decoded), which makes every operator its f64 or int32 operation, exactly.
//!
//! The body's operations form a dataflow graph. Operations that do not depend on a value the
//! previous iteration computed (a recurrence, such as `s += ...`) run column-wise over a
//! chunk, each a tight loop the compiler vectorizes; the rest run in one scalar pass over the
//! chunk in body order. A chunk commits its stores only after every element it read was a
//! Number and every element it replaces was present; otherwise it is discarded, the
//! registers are written as the previous chunk left them, the loop's own records run from
//! there, and the kernel stands down for the rest of the view's life. A store and a load of
//! the same array are ordered by their index distance, fixed at entry: a load reaching an
//! element an earlier iteration of the same chunk stores shortens the chunk below that
//! distance. Chunks also end at the run's safepoint (`script_below_floor`).
use super::*;

pub(crate) mod machine;
pub(crate) mod recur;
pub(crate) mod run;
pub(crate) mod typed;
pub(crate) mod shape;

pub(crate) use shape::Shape;

use quench_runtime::vm::engine_types::KERNELS;

use crate::compiled::tier_policy::KERNEL_SHORT_DECLINES;
pub(crate) use quench_runtime::vm::engine_types::LoopKernel;

/// The recurrence pass that runs the loop of `shape`, where the kernel is that loop's tier. A pass
/// that interprets its recurrences per iteration is not under `--b-jit`: the loop's stencils run
/// it faster (crc), while a fold or chain runs faster than they do (sum, dot).
fn pass(shape: &Shape) -> Option<recur::Recurrence> {
    recur::Recurrence::of(shape).filter(|recurrence| !(quench_runtime::jit_mode::base_only() && recurrence.interprets()))
}

/// Drops the temps (registers the body writes before it reads them) dead where the loop exits:
/// an expression's scratch registers, whose last values no one reads, so the run neither keeps
/// their columns nor writes their final values. The loop's one exit is its test's jump past
/// the backedge; a register the masks do not cover stays.
/// `false` where a register the body writes under a branch only (`Shape::guarded`) may be live
/// past the loop, or no masks say: such a loop has no kernel.
fn drop_dead_temps(shape: &mut Shape, decoded: &[WideInstruction], header: usize, backedge: usize,
    live: Option<&quench_runtime::compile::liveness::RegisterMasks>) -> bool {
    let guarded = shape.guarded.is_empty();
    let Some(masks) = live else { return guarded };
    let Some(width) = super::int_kinds::covered(masks) else { return guarded };
    let exits: Vec<usize> = (header..=backedge)
        .filter(|&pc| decoded[pc].op() == quench_runtime::bytecode::Op::JumpBinaryFalse)
        .map(|pc| decoded[pc].jump_target() as usize)
        .collect();
    let Some(&exit) = exits.first() else { return guarded };
    if exit != backedge + 1 {
        return guarded;
    }
    let Some(after) = masks.get(exit) else { return guarded };
    let live = |register: u16| usize::from(register) >= width || after.contains(usize::from(register));
    shape.temps.retain(|&(register, _)| live(register));
    !shape.guarded.iter().any(|&register| live(register))
}

/// A register scalar of `shape` that every definition in the function loads with a non-number
/// constant (`a === "abc"` in the body): the kernel runs on numbers, so every entry would decline,
/// and an installed kernel costs the view its int kinds and proven loop step (`int_kinds`).
fn never_number_scalar(shape: &Shape, decoded: &[WideInstruction], constants: &[Value]) -> Option<u16> {
    use quench_runtime::compile::liveness::{definitions, register_in, RegisterSet};
    shape.scalars.iter().find_map(|scalar| {
        let shape::Scalar::Register(register) = *scalar else { return None };
        if usize::from(register) >= RegisterSet::BITS as usize {
            return None;
        }
        let mut defined = false;
        for &instruction in decoded {
            if !register_in(definitions(instruction, &[]), usize::from(register)) {
                continue;
            }
            let non_number = instruction.op() == quench_runtime::bytecode::Op::LoadConst
                && instruction.result_flags() == 0
                && constants.get(instruction.constant_index()).is_some_and(|constant| constant.as_number().is_none());
            if !non_number {
                return None;
            }
            defined = true;
        }
        defined.then_some(register)
    })
}

#[quench_macros::ext(LoopKernelOps1)]
impl LoopKernel {
    /// Whether the loop `header..=backedge` has a kernel: `--b-jit` leaves such a loop to it, as
    /// its stencils run it slower (`install_loop_sites`).
    pub(crate) fn accepts(decoded: &[WideInstruction], constants: &[Value], function: &Function, header: usize, backedge: usize) -> bool {
        Shape::of(decoded, constants, usize::from(function.registers), header, backedge)
            .is_some_and(|shape| shape.guarded.is_empty() && pass(&shape).is_some())
    }

    /// The registered kernel of the loop `header..=backedge`, if it has one.
    fn of(
        decoded: &[WideInstruction],
        constants: &[Value],
        function: &Function,
        header: usize,
        backedge: usize,
        original: *const (),
        live: Option<&quench_runtime::compile::liveness::RegisterMasks>,
    ) -> Option<Box<Self>> {
        let trace = quench_runtime::trace::Channel::LoopKernel.on();
        let Some(mut shape) = Shape::of(decoded, constants, usize::from(function.registers), header, backedge) else {
            if trace {
                eprintln!("loop-kernel: loop {header}..{backedge}: no shape");
            }
            return None;
        };
        if !drop_dead_temps(&mut shape, decoded, header, backedge, live) {
            if trace {
                eprintln!("loop-kernel: loop {header}..{backedge}: a branch-written register is live past the loop");
            }
            return None;
        }
        if let Some(register) = never_number_scalar(&shape, decoded, constants) {
            if trace {
                eprintln!("loop-kernel: loop {header}..{backedge}: scalar r{register} is only ever a non-number constant");
            }
            return None;
        }
        let recurrence = pass(&shape);
        if trace {
            eprintln!("loop-kernel: loop {header}..{backedge}: {}", recurrence.as_ref().map_or("no recurrence pass", |r| r.name()));
            eprintln!("loop-kernel:   nodes {:?} phis {:?} temps {:?}", shape.nodes, shape.phis, shape.temps);
        }
        let recurrence = recurrence?;
        let mut kernel = Box::new(LoopKernel {
            shape,
            recurrence,
            slot: NO_CALL_LINK,
            original: std::cell::Cell::new(original),
            declines: std::cell::Cell::new(0),
            scratch: std::cell::UnsafeCell::new(Vec::new()),
            state: std::cell::UnsafeCell::new(None),
        });
        kernel.slot = register(&kernel)?;
        Some(kernel)
    }

    fn original(&self) -> Handler {
        // SAFETY: `original` is the record's handler for this VM type (`install_loop_kernels`).
        unsafe { std::mem::transmute::<*const (), Handler>(self.original.get()) }
    }

    /// Gives the record at `ip` back its own handler for good.
    fn stand_down(&self, ip: Ip) {
        // SAFETY: as `requicken`; the record holds this kernel.
        unsafe { (*ip.cast_mut()).handler = self.original.get() };
    }
}



/// Registers `kernel`, whose box keeps its address, in a free slot.
fn register(kernel: &LoopKernel) -> Option<u16> {
    // SAFETY: the registry is this thread's; no reference into it is held.
    let kernels = unsafe { &mut *KERNELS.get() };
    let slot = kernels.iter().position(|entry| entry.is_null()).unwrap_or_else(|| {
        kernels.push(std::ptr::null());
        kernels.len() - 1
    });
    let slot = u16::try_from(slot).ok().filter(|&slot| slot < NO_CALL_LINK)?;
    kernels[usize::from(slot)] = kernel;
    Some(slot)
}

/// Gives each loop that has a kernel its kernel record (`loop_kernel_head`) on the loop's test
/// record. Runs after every other pass: the kernel keeps the record's final handler and
/// leaves the record's operand to it.
pub(crate) fn install_loop_kernels(
    decoded: &[WideInstruction],
    constants: &[Value],
    function: &Function,
    exits: &[bool],
    records: &mut [LaneInstruction],
    live: Option<&quench_runtime::compile::liveness::RegisterMasks>,
) -> Box<[Box<LoopKernel>]> {
    let mut kernels = Vec::new();
    for backedge in 0..decoded.len() {
        let jump = decoded[backedge];
        if !matches!(quench_runtime::staged::Operation::<(), ()>::decode(jump, quench_runtime::staged::DecodeContext::EMPTY), Some(quench_runtime::staged::Operation::Jump)) {
            continue;
        }
        let header = jump.jump_target() as usize;
        if header >= backedge {
            continue;
        }
        if exits[header] || records[header].call_link.get() != NO_CALL_LINK {
            continue;
        }
        let Some(kernel) = LoopKernel::of(decoded, constants, function, header, backedge, records[header].handler, live) else {
            continue;
        };
        records[header].handler = loop_kernel_head as Handler as *const ();
        records[header].call_link.set(kernel.slot);
        kernels.push(kernel);
    }
    kernels.into_boxed_slice()
}

/// `requicken` of a record holding a kernel: the record's own handler (which the kernel runs
/// after itself) becomes `handler`, and the record keeps the kernel. `false` for any other
/// record.
///
/// # Safety
/// `ip` is a live record of the running view.
#[inline(always)]
pub(crate) unsafe fn requicken_kernel(ip: Ip, handler: Handler) -> bool {
    // SAFETY: the caller's contract.
    let record = unsafe { &*ip };
    if record.handler != loop_kernel_head as Handler as *const () {
        return false;
    }
    // SAFETY: the record names a live kernel of its view (`install_loop_kernels`).
    let kernel = unsafe { &*(&*KERNELS.get())[usize::from(record.call_link.get())] };
    kernel.original.set(handler as *const ());
    true
}

script_handler! {
    /// A loop's head record holding its kernel (`LoopKernel`): runs the loop as far as the
    /// kernel can, then the record's own handler.
    fn loop_kernel_head<>(cx, r, ip, i, view) {
        // SAFETY: the record names a live kernel of its view (`install_loop_kernels`).
        let kernel = unsafe { &*(&*KERNELS.get())[usize::from(i.call_link.get())] };
        // The loop's own records continue from where the kernel left the registers: its
        // head runs, then its test, which fails where the kernel ran the whole loop.
        unsafe { kernel.run(cx, r, ip) };
        if kernel.declines.get() >= KERNEL_SHORT_DECLINES {
            kernel.stand_down(ip);
        }
        sibling!(kernel.original(), r, ip, Carried::script_in(cx, view))
    }
}
