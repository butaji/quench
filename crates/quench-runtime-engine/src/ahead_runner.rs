//! The execution-only lane runner: an image body entered at its function's first instruction,
//! and the lane view that entry runs with. Kept outside the lane dispatch's census tree
//! (`quench-lint`'s `lane_dispatch_lines`): it is the interpreter's absence, not its expansion.

use crate::engine_prelude::*;
use quench_runtime::bytecode::{FieldSite, Function, ProgramKind};
use quench_runtime::compile::liveness::RegisterMasks;
use quench_runtime::vm::engine_types::LaneView;
use quench_runtime::Value;
#[cfg(not(feature = "jit"))]
use crate::ext_traits::*;
#[cfg(not(feature = "jit"))]
use crate::dispatch_fast::lane_roots::{LaneRoot, LaneStart};
#[cfg(not(feature = "jit"))]
use quench_runtime::vm::dispatch_frame::{ExecutingProgram, PendingGeneralCall};
#[cfg(not(feature = "jit"))]
use quench_runtime::vm::engine_types::{ActivationShape, LaneInstruction, LaneOperand, Records, Registers, NO_CALL_LINK};
use quench_runtime::vm::ProgramId;

/// `function`'s view of `code`, derived with this VM's facts (`Vm::entry_lane_view`,
/// `Vm::view_any_in_lane` and `frame_state_in_lane`) and its program's solve (`program`).
#[cfg(feature = "jit")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn view_in_lane<'a>(vm: &Vm, program: ProgramId, function: u32, code: &Function, constants: &[Value], kind: ProgramKind,
    live: &dyn Fn() -> Option<&'a std::rc::Rc<RegisterMasks>>, sites: &[FieldSite], image: bool) -> LaneView {
    let facts = vm.programs.aot_facts(program);
    let planned = vm.programs.planned_call_sites(program, function, crate::dispatch_fast::planned_calls::planned_arm_admits);
    // A function's loop regions enter native code iff its predicted tier compiles it (design decision 4.9: the argmin,
    // or the AOT mode's tier for an image, `ProgramStore::compiled_tier`).
    let loop_regions = vm.programs.compiled_tier(program, function);
    let mut view = crate::dispatch_fast::derive_lane_view(function, code, constants, vm.heap_reads(), kind, live, sites,
        crate::compiled::admission::record_plan(loop_regions && quench_runtime::vm::tier_policy::tiers_run(vm.execution_mode, vm.memory_policy(), crate::compiled::admission::Tier::ANY)),
        vm.programs.reports_on_drop(), facts.as_deref(), planned);
    // Where an image's bodies are the only code, the lane takes on every record they leave at.
    if image && !vm.execution_mode.allows_dynamic_execution() { view.admit_every_record(); }
    view
}

/// Execution-only support runs no record: the view's only entry is the function's first
/// instruction, where its image body runs (`run_ahead_frame`); every other record steps in the
/// general loop.
#[cfg(not(feature = "jit"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn view_in_lane<'a>(vm: &Vm, _program: ProgramId, _function: u32, code: &Function, constants: &[Value], _kind: ProgramKind,
    _live: &dyn Fn() -> Option<&'a std::rc::Rc<RegisterMasks>>, _sites: &[FieldSite], _image: bool) -> LaneView {
    ahead_lane_view(code, constants)
}

/// The lane view of an execution-only build: the body's own records, whose only entry is the
/// function's first, where a loaded image's body runs it (`run_ahead_frame`). The records carry
/// the bytecode's operand fields (result, operands and immediate metadata) because the image's
/// helpers read them (`body_calls::ends_run`, `push_passed_in_lane`, the general step); they
/// carry no bytecode handler, since nothing dispatches them.
#[cfg(not(feature = "jit"))]
fn ahead_lane_view(function: &Function, constants: &[Value]) -> LaneView {
    let count = function.code.len();
    let records = Records::new(body_records(function, constants));
    let mut entries = vec![0u64; count.div_ceil(u64::BITS as usize)];
    if let Some(first) = entries.first_mut() { *first = 1; }
    let base = records.base();
    LaneView {
        records,
        entries: entries.into_boxed_slice(),
        memory_register: None,
        activation: ActivationShape::of(function),
        target_body: std::cell::OnceCell::new(),
        _string_case_filters: Box::new([]),
        environment_window: None,
        returns_from_entry: false,
        loops: Box::new([]),
        loop_at_backedge: Box::new([]),
        start: std::cell::Cell::new(base),
        prologue: std::cell::UnsafeCell::new(Records::blank()),
        tier_direct: std::cell::Cell::new(None),
        tier: std::cell::OnceCell::new(),
        entry: crate::compiled::entry_cell::EntryCell::new(crate::compiled::lane_body_abi::entry_stub()),
        accumulator_registers: vec![crate::dispatch_fast::NO_ACCUMULATOR; count].into_boxed_slice(),
        plain_backward_jump_sites: 0,
        osr_rejections: None,
        field_records: Box::new([]),
        name_records: Box::new([]),
        proven_counters: Box::new([]),
        planned_calls: Box::new([]),
    }
}

/// The body's records without handlers: the bytecode's result, operands and immediate metadata,
/// which its helpers read (`body_calls::ends_run`, `push_passed_in_lane`, the general step).
#[cfg(not(feature = "jit"))]
fn body_records(function: &Function, constants: &[Value]) -> Box<[LaneInstruction]> {
    function.instructions().enumerate()
        .map(|(pc, instruction)| {
            let operand = crate::dispatch_fast::operand_in_lane(instruction, pc, constants)
                .unwrap_or(LaneOperand { imm: 0 });
            LaneInstruction {
                handler: std::ptr::null(),
                operand,
                a: instruction.a(),
                b: instruction.b(),
                c: if instruction.op() as u16 == quench_runtime::staged::ARGUMENT_GET_ROW {
                    function.registers
                } else {
                    instruction.c()
                },
                call_link: std::cell::Cell::new(NO_CALL_LINK),
                mode: 0,
            }
        })
        .collect()
}

/// The frame's run in an execution-only build (`run_fast_lane` without a code generator): the
/// image's body at the frame's first instruction is the lane's only work, and it leaves at the
/// record the general loop continues at. Kept out of line as `run_fast_lane` is.
#[cfg(not(feature = "jit"))]
#[quench_macros::ext(AheadRunnerVm)]
impl Vm {
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_ahead_frame(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        view: *const LaneView,
        frame: &mut usize,
        pc: usize,
        pending: &mut Vec<PendingGeneralCall>,
        executing: &mut ExecutingProgram,
        inline_calls: bool,
    ) -> usize {
        // Only a function's first instruction has an entry (`ahead_lane_view`): every other
        // record steps in the general loop.
        if pc != 0 { return pc; }
        // SAFETY: the cursor's view outlives the run.
        let view = unsafe { &*view };
        let registers = Registers(self.frames[*frame].registers.as_mut_ptr());
        let function = self.frames[*frame].function;
        let entered = self.enter_lane_floor();
        let start = frame_start(self, code, program, *frame, view, pending, executing, inline_calls, entered.cell);
        let mut context = self.context_for_start(start);
        let ip = self.drive_lane(&mut context, entered, |cx| {
            // SAFETY: `cx` is the only access path to the context during the run; `view` has one
            // record per instruction, so its first is this PC's.
            unsafe { (*cx).memory = crate::dispatch_fast::frame_memory_view(cx, view, registers) };
            crate::dispatch_fast::body_calls::enter_frame_body(cx, function).unwrap_or(unsafe { view.records().add(pc) })
        });
        *frame = context.frame();
        unsafe { ip.offset_from(context.base) as usize }
    }
}

/// The frame's lane start (`run_ahead_frame`): its registers, receiver, view and floor.
#[cfg(not(feature = "jit"))]
#[allow(clippy::too_many_arguments)]
fn frame_start<'a>(vm: &mut Vm, code: &'a ResidualProgram, program: ProgramId, frame: usize, view: &LaneView,
    pending: &'a mut Vec<PendingGeneralCall>, executing: &'a mut ExecutingProgram, inline_calls: bool,
    floor: *const std::sync::atomic::AtomicUsize) -> LaneStart<'a> {
    LaneStart {
        code,
        program,
        root: LaneRoot::Frame(frame),
        registers: Registers(vm.frames[frame].registers.as_mut_ptr()),
        base: view.records(),
        this: vm.frame_lane_this(frame),
        pending,
        executing,
        inline_calls,
        script_calls: inline_calls
            && code.kind != quench_runtime::bytecode::ProgramKind::Wasm
            && frame + 1 == vm.frames.len(),
        script_constructs: code
            .function_get(vm.frames[frame].function as usize)
            .is_some_and(|function| {
                function.dispatch == quench_runtime::bytecode::DispatchClass::General
            }),
        floor,
    }
}
