//! The unit a lane body specializes: one function with the callees its call sites' inline
//! caches name spliced in, transitively, under one budget (R3, Linear TOD-1232).
//!
//! A call whose record links one script target (`LaneInstruction::call_link`), to a callee
//! whose body the lane-body lowering runs entirely and every call of which is spliced in in
//! turn, is replaced by a call marker and the callee's renamed instructions. Its registers
//! follow the unit's; its `this` is the call's receiver register; its `Return` moves the
//! value to the call's result and jumps past the call. Each PC of the unit names its origin,
//! so a leave inside a callee makes the activations of its frame and the frames around it,
//! outermost first, through the lane's own call path, and continues at the innermost one's
//! record (`Vm::lane_body_inlined_activation`).

use crate::bytecode::{FieldLookup, ImmediateLayout, Op, Register, WideInstruction};
use crate::bytecode::Function;
use crate::value::Value;

/// Binding time of a callee: a published closure, whose identity the call compares, or a
/// static body whose actual closure and environment are admitted at the call. Function
/// bindings are accepted for verified plain bodies with explicit restart or exact-state
/// recovery. A forwarded binding is a closure the site called through a native that
/// forwards its receiver as the callee (`Function.prototype.call`, `Vm::forwarded_call`):
/// the call guards the native's identity too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CalleeBinding { Identity(Value), Function, Forwarded { through: Value, callee: Value } }

/// How a leave inside a function-bound frame recovers: restart the call, which ran no
/// effect yet, or materialize the callee's exact activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallRecovery { RestartCall, Materialize }

/// The script target a call site links, as the unit inlines it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallTarget {
    pub(crate) callee: CalleeBinding,
    pub(crate) function: u32,
}

impl CallTarget {
    pub(crate) fn identity(self) -> Option<Value> {
        match self.callee { CalleeBinding::Identity(value) | CalleeBinding::Forwarded { callee: value, .. } => Some(value), _ => None }
    }

    /// The forwarding native this site called the target through.
    pub(crate) fn through(self) -> Option<Value> {
        match self.callee { CalleeBinding::Forwarded { through, .. } => Some(through), _ => None }
    }
}

/// Where a unit PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of inlined `frame`.
    Inlined { frame: usize, pc: u32 },
}

/// An inlined call. Its registers are the unit's: the caller's are its first, and each
/// callee's start at its `base`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) recovery: CallRecovery,
    /// The frame whose function makes the call; `None` for the caller's.
    pub(crate) parent: Option<usize>,
    /// The PC of the call in its parent's function.
    pub(crate) call: u32,
    pub(crate) target: CallTarget,
    /// The register holding the callee, guarded at the call.
    pub(crate) callee_register: Register,
    /// The register holding the call's receiver, the callee's `this`.
    pub(crate) receiver: Register,
    /// The call's first argument register.
    pub(crate) window: Register,
    pub(crate) arguments: u16,
    /// The register the call's completion writes in its parent, dead until then; `None` for a returning call.
    pub(crate) result: Option<Register>,
    /// The callee's register 0.
    pub(crate) base: Register,
    pub(crate) registers: u16,
    /// The unit PC of the call's next alternative's marker, where a callee other than this
    /// frame's goes; `None` for the last, which leaves at the call.
    pub(crate) alternative: Option<usize>,
    /// For a call through a forwarding native (`CalleeBinding::Forwarded`), the register
    /// holding the native and its identity, which the call guards too; the callee register
    /// then holds the function it calls, the receiver is the first argument and the window
    /// the rest (`Planner::through_call`).
    pub(crate) function_call: Option<(Register, Value)>,
}

impl Frame {
    /// The physical parameter homes initialized by the passed arguments.
    pub(crate) fn passed_registers(self, callee: &Function) -> impl Iterator<Item = (Register, Register)> + '_ {
        (0..self.arguments.min(callee.params)).filter_map(move |argument|
            callee.parameter_registers.map(|base| base + argument)
                .or_else(|| callee.promoted_register(argument))
                .map(|register| (self.base + register, self.window + argument)))
    }

    /// The parent's register 0: the caller's, or the enclosing callee's.
    pub(crate) fn parent_base(&self, frames: &[Frame]) -> Register {
        self.parent.map_or(0, |parent| frames[parent].base)
    }
}

pub(crate) struct Unit {
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) origins: Vec<Origin>,
    pub(crate) frames: Vec<Frame>,
    /// The unit PC of each frame's call marker, where the call's guards and bindings run.
    pub(crate) markers: Vec<usize>,
    pub(crate) caller_registers: u16,
    pub(crate) registers: u16,
    /// The caller PC past its last instruction, where falling off the unit resumes.
    pub(crate) end: u32,
}

impl Unit {
    /// `frame` and the frames around it, outermost first: the activations a leave inside it
    /// makes.
    pub(crate) fn chain(&self, frame: usize) -> Vec<usize> {
        let mut chain = vec![frame];
        while let Some(parent) = self.frames[*chain.last().expect("a chain has a frame")].parent {
            chain.push(parent);
        }
        chain.reverse();
        chain
    }

    /// Maximum simultaneous inlined activations, derived from the parent graph.
    pub(crate) fn recovery_depth(&self) -> usize {
        (0..self.frames.len()).map(|index| self.chain(index).len()).max().unwrap_or(0)
    }

    /// Project the original functions' recovery liveness onto unit registers. A suspended
    /// frame retains both its call inputs and continuation state, and so do the frames
    /// around it; callee facts include implicit observations such as legacy f.arguments.
    /// The unit's own CFG then refines the projection, so uses the splice introduces (the
    /// result move after a returning operation, for example) stay live. `None` for a unit
    /// wider than one register set, whose lowering projects each frame's own masks per
    /// query instead (`Lowering::is_live`).
    pub(crate) fn register_masks<'a>(
        &self, program: &'a crate::ResidualProgram, caller: u32,
    ) -> Option<std::borrow::Cow<'a, crate::compile::liveness::RegisterMasks>> {
        use crate::compile::liveness::{ROOT_MAP_REGISTERS, RegisterMasks, RegisterSet, register_bit};
        let caller_masks = program.register_root_masks(caller)?;
        if self.frames.is_empty() && self.origins.iter().enumerate().all(|(pc, origin)|
            *origin == Origin::Caller(pc as u32)) {
            return Some(std::borrow::Cow::Borrowed(caller_masks));
        }
        if self.registers > ROOT_MAP_REGISTERS { return None; }
        // Frames follow their parents (`Emitter::emit`).
        let mut suspended: Vec<RegisterSet> = Vec::with_capacity(self.frames.len());
        let mut callees = Vec::with_capacity(self.frames.len());
        for frame in &self.frames {
            let (parent, outer) = match frame.parent {
                None => (caller_masks, 0),
                Some(parent) => (callees[parent], suspended[parent]),
            };
            let call = frame.call as usize;
            let around = parent.get(call)? | (parent.get(call + 1).unwrap_or(0) & !frame.result.map_or(0, |result| register_bit(result - frame.parent_base(&self.frames))));
            suspended.push(outer | around.checked_shl(u32::from(frame.parent_base(&self.frames)))?);
            callees.push(program.register_root_masks(frame.target.function)?);
        }
        let mut masks = self.origins.iter().map(|origin| match *origin {
            Origin::Caller(pc) => caller_masks.get(pc as usize),
            Origin::Inlined { frame, pc } => Some(suspended[frame]
                | callees[frame].get(pc as usize)?.checked_shl(u32::from(self.frames[frame].base))?),
        }).collect::<Option<Vec<_>>>()?;
        masks.push(caller_masks.get(self.end as usize)?);
        let mut transfers: Vec<_> = self.instructions.iter().map(|instruction|
            crate::compile::liveness::transfer(*instruction, program,
                &program.field_sites, &program.superinstructions)).collect();
        // A marker defines its callee's registers: call admission binds every one.
        for (frame, pc) in self.frames.iter().zip(&self.markers) {
            transfers[*pc].1 = (0..frame.registers).fold(0, |mask, register| mask | register_bit(frame.base + register));
        }
        // A returning instruction continues at the function's terminal `Return` where it has
        // one, whose register then holds its value; otherwise it completes the activation.
        let terminal = self.instructions.len().checked_sub(1)
            .filter(|terminal| self.instructions[*terminal].op() == Op::Return);
        let mut starts = Vec::with_capacity(self.instructions.len() + 1);
        let mut successors = Vec::new();
        for (pc, instruction) in self.instructions.iter().enumerate() {
            starts.push(successors.len());
            if instruction.returns_from_frame() {
                if let Some(terminal) = terminal {
                    transfers[pc].1 |= register_bit(self.instructions[terminal].register_a());
                    successors.push(terminal);
                }
                continue;
            }
            if let Some(frame) = self.markers.iter().position(|marker| *marker == pc) {
                successors.extend(self.frames[frame].alternative);
            }
            match instruction.op().control_flow_layout() {
                crate::bytecode::ControlFlowLayout::Terminal => {},
                crate::bytecode::ControlFlowLayout::Jump => successors.push(instruction.jump_target() as usize),
                crate::bytecode::ControlFlowLayout::ConditionalJump => {
                    successors.push(instruction.jump_target() as usize);
                    successors.push(pc + 1);
                }
                _ => successors.push(pc + 1),
            }
        }
        starts.push(successors.len());
        let live = crate::compile::liveness::analyze_graph(&transfers, &starts, &successors, |pc| masks[pc]);
        Some(std::borrow::Cow::Owned(RegisterMasks::new(live)))
    }
}

#[cfg(feature = "aot")]
/// Every register output retires the previous hypothesis before its new value is recorded.
fn clear_written_hints<T>(hints: &mut [Option<T>], instruction: WideInstruction) {
    for register in instruction.written_registers() { hints[usize::from(register)] = None; }
}

/// The bodies a call site's callee may name statically: a superset of the callees it meets,
/// which the unit's inlining budgets choose among, each behind its own body guard. A name,
/// slot or register defined with more names none.
#[cfg(feature = "aot")]
pub(crate) type StaticTargets = smallvec::SmallVec<[u32; super::tier_policy::POLYMORPHISM]>;

/// Static method-name, local-closure and captured-slot hints, resolved into flat per-PC targets.
/// A hint is never a proof: replacement/aliasing is handled by call-edge admission, which
/// tries each target a site names behind its own body guard.
#[cfg(feature = "aot")]
pub(crate) fn static_call_targets(program: &crate::ResidualProgram) -> Vec<Vec<StaticTargets>> {
    #[derive(Clone, Copy)]
    enum Definition { Closure(u32), Name(crate::bytecode::Atom) }
    // Derive a flat constant-index projection once, without allocating lookup
    // nodes or rescanning all names at each method definition.
    let constant_names: Vec<_> = {
        let mut names: Vec<_> = program.atoms.iter().enumerate().map(|(atom, name)|
            (name, crate::bytecode::Atom::try_from(atom).expect("validated atom index"))).collect();
        names.sort_unstable_by_key(|&(name, _)| name);
        program.constants.iter().map(|constant| match constant {
            crate::bytecode::Constant::String(name) => names.binary_search_by(|(candidate, _)|
                candidate.cmp(&name.as_ref())).ok().map(|index| names[index].1),
            _ => None,
        }).collect()
    };
    // `None` once more definitions than a site admits have been observed.
    let mut methods = vec![Some(StaticTargets::new()); program.atoms.len()];
    let mut captured = Vec::new();
    let mut offsets = Vec::with_capacity(program.function_count() + 1);
    let mut parents = Vec::with_capacity(program.function_count());
    let observe = |slot: &mut Option<StaticTargets>, target| {
        if let Some(targets) = slot && !targets.contains(&target) {
            if targets.len() == targets.inline_size() { *slot = None } else { targets.push(target) }
        }
    };
    for id in 0..program.function_count() {
        program.with_record(id, |function| {
            let start = captured.len();
            offsets.push(start);
            parents.push(function.parent);
            captured.resize(start + usize::from(function.locals), Some(StaticTargets::new()));
            let mut definitions = vec![None; usize::from(function.registers)];
            for instruction in function.instructions() {
                // A stored or literal-defined data property holding a closure names its body.
                if instruction.op().field_store().map_or_else(|| instruction.op() == Op::DefineField, |store| store.receiver == crate::bytecode::StoreReceiver::Register)
                    && let Some(Definition::Closure(id)) = definitions.get(usize::from(instruction.register_a())).copied().flatten() {
                        observe(&mut methods[instruction.atom_index() as usize], id);
                    }
                if instruction.op() == Op::DefinePropertyRecord
                    && matches!(instruction.property_definition_mode(),
                        Some(crate::bytecode::PropertyDefinitionMode::Method | crate::bytecode::PropertyDefinitionMode::ReadonlyMethod))
                    && let Some(Definition::Closure(id)) = definitions[usize::from(instruction.register_a())]
                    && let Some(Definition::Name(atom)) = definitions[usize::from(instruction.register_c())] {
                        // Class definitions use an explicit key register. Only
                        // an admitted literal name contributes a body hint;
                        // computed effects/accessor results remain dynamic.
                        observe(&mut methods[atom as usize], id);
                    }
                if let Some(slot) = function.stored_slot(instruction).and_then(|slot| captured[start..].get_mut(slot))
                    && let Some(Definition::Closure(target)) = definitions.get(usize::from(instruction.register_a())).copied().flatten() {
                        observe(slot, target);
                    }
                let value = match instruction.op() {
                    Op::MakeClosure => Some(Definition::Closure(instruction.closure_function_index())),
                    Op::LoadConst => constant_names.get(instruction.constant_index()).copied().flatten().map(Definition::Name),
                    Op::Move => definitions.get(usize::from(instruction.register_b())).copied().flatten(),
                    _ => None,
                };
                clear_written_hints(&mut definitions, instruction);
                if let Some(register) = instruction.defined_register() { definitions[usize::from(register)] = value; }
                if instruction.op().control_flow_layout() != crate::bytecode::ControlFlowLayout::Fallthrough { definitions.fill(None); }
            }
        });
    }
    offsets.push(captured.len());
    (0..program.function_count()).map(|id| program.with_record(id, |function| {
        let mut methods_by_register = vec![None::<StaticTargets>; usize::from(function.registers)];
        function.instructions().map(|instruction| {
            let target = (instruction.op() == Op::Call).then(||
                methods_by_register[usize::from(instruction.register_b())].clone()).flatten().unwrap_or_default();
            let method = match instruction.op() {
                Op::GetField => match instruction.field_lookup() {
                    Some(FieldLookup::Atom { atom, .. }) => methods.get(atom as usize).cloned().flatten(),
                    _ => None,
                },
                Op::MakeClosure => Some(smallvec::smallvec![instruction.closure_function_index()]),
                Op::LoadCapture => {
                    // The lexical owner and source slot come from the same layout as
                    // capture execution. This identifies a possible body, never the
                    // closure: admission checks its body, and captures read its actual environment.
                    let owner = (0..=instruction.capture_depth()).try_fold(id, |owner, _|
                        parents.get(owner).copied().flatten().map(|parent| parent as usize));
                    owner.and_then(|owner| captured[offsets[owner]..offsets[owner + 1]]
                        .get(usize::from(instruction.capture_slot())).cloned().flatten())
                }
                Op::Move => methods_by_register[usize::from(instruction.register_b())].clone(),
                _ => None,
            };
            clear_written_hints(&mut methods_by_register, instruction);
            if let Some(register) = instruction.defined_register() { methods_by_register[usize::from(register)] = method; }
            // Hints survive control-flow edges, including loops around a local
            // closure. They are hypotheses only: inline admission checks the
            // actual closure/program/body before any callee effect.
            target
        }).collect()
    })).collect()
}

/// The longest callee body the unit inlines: each copy of it costs code and compile time at
/// every call site, and short bodies (accessors, field updates) are where a call costs most
/// relative to its work.
const MAX_INLINED_INSTRUCTIONS: usize = 96;
/// The callee instructions one unit inlines in all, the region's budget: a hot loop's call
/// graph, inlined, is the region an optimizing tier compiles as one function. Its compile
/// cost grows with it (Richards' 600-instruction units cost ~63M instructions to compile),
/// so it is TurboFan's cumulative inlining budget (`max_inlined_bytecode_size_cumulative`).
pub(super) const MAX_UNIT_INLINED_INSTRUCTIONS: usize = 920;
/// How deep calls nest inside a unit: JSC's inlining depth (`maximumInliningDepth`).
const MAX_INLINING_DEPTH: usize = 5;

/// Whether the lane-body lowering runs `instruction` of an inlined callee: a capture of its
/// own environment only, no fusion but a returning one, which splits into the
/// computation and the return (`osr_loop::inlined_returning`). A call is the plan's to admit:
/// only one it inlines in turn.
fn inlinable(instruction: WideInstruction) -> bool {
    let plain = instruction.result_flags() == 0 || super::osr_loop::inlined_returning(instruction);
    match instruction.op() {
        Op::Return | Op::Nop | Op::LoadConst | Op::Move | Op::LoadThis | Op::IncDec | Op::Jump | Op::JumpFalse | Op::JumpTrue
        | Op::JumpBinaryFalse | Op::GetIndex | Op::SetIndex => plain,
        // The callee's own captures, which its frame reads from its environment
        // (`Lowering::frame_captures`).
        Op::LoadCapture => plain && ImmediateLayout::capture_depth(instruction.imm()) == 0,
        // A nested lookup's chain names its base by the function's registers; the lowering
        // renames it through the frame (`Lowering::get_field`), and its sink stores to `this`.
        Op::GetField => match instruction.field_lookup() {
            Some(FieldLookup::Atom { .. }) => plain,
            Some(FieldLookup::Site(_)) => plain || instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER,
            None => false,
        },
        op if op.is_field_store() => plain,
        // A fused `typeof` test (`Lowering::typeof_test`), a `typeof` switch's index
        // (`Lowering::typeof_index`); any other unary leaves where the lowering has none.
        Op::Unary => plain,
        // Operations native code leaves at, exactly, through the inlined frame's recovery: a
        // global name's read, a construction and a throw, typically a callee's error path.
        Op::LoadName | Op::Construct | Op::Throw => instruction.result_flags() == 0,
        Op::Call => returns_or_keeps(instruction) && !instruction.direct_eval() && !instruction.parameter_eval(),
        op => plain && op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary,
    }
}

/// The shared lowering performs only guarded, non-guest operations for an inlinable pure
/// leaf. A miss may restart that call without replaying effects.
fn restartable(program: &crate::ResidualProgram, function: &Function, code: &[WideInstruction]) -> bool {
    if super::lane_region::restartable_numeric_function(program, function, code) { return true; }
    code.iter().enumerate().all(|(pc, instruction)| match instruction.op() {
        Op::SetIndex | Op::Call => false,
        op if op.is_field_store() => false,
        Op::IncDec => instruction.numeric_local_target().is_none(),
        Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse => instruction.jump_target() as usize > pc,
        _ => true, // `inlinable` owns the supported instruction/effect set.
    })
}

/// Whether a call keeps its result in its register or returns it: the two a unit inlines.
fn returns_or_keeps(instruction: WideInstruction) -> bool {
    matches!(instruction.result_flags(), 0 | crate::bytecode::RETURN_REGISTER)
}

/// The unit PCs occupied by a call's alternatives, including each admission marker.
/// Returning calls continue at an existing completion; they add no synthetic `Return`.
fn call_length(alternatives: &[InlinedPlan]) -> u32 {
    alternatives.iter().map(|callee| 1 + callee.plan.length(false)).sum()
}

/// The canonical caller completion a terminal call may use without replaying the call.
fn terminal_return(code: &[WideInstruction]) -> Option<usize> {
    code.len().checked_sub(1).filter(|pc|
        code[*pc].op() == Op::Return && code[*pc].result_flags() == 0)
}

/// Whether an inlined callee's `instruction` renames to the unit's registers
/// (`osr_loop::renamed`): its operands are registers or constants. The plan decides it, so a
/// unit it plans always emits.
fn renamable(instruction: WideInstruction) -> bool {
    match instruction.op() {
        Op::Call | Op::Return | Op::LoadThis | Op::LoadName | Op::Construct | Op::Throw => true,
        Op::GetField if matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) => true,
        _ => {
            let plain = if super::osr_loop::inlined_returning(instruction) {
                WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm())
            } else {
                instruction
            };
            super::osr_loop::renamed(plain, &|register| register, 0, &|target| Some(target)).is_some()
        }
    }
}

/// The unit PCs an inlined callee instruction takes: a completion of the call (a `Return`, or
/// an instruction that computes and returns) adds the result's move and the jump past the call.
fn completion_length(instruction: WideInstruction) -> u32 {
    if instruction.op() == Op::Return {
        2
    } else if super::osr_loop::inlined_returning(instruction) {
        3
    } else {
        1
    }
}

fn decoded(function: &Function) -> Vec<WideInstruction> {
    function.code.iter().map(|packed| super::super::dispatch_fast::decoded_instruction(function, *packed)).collect()
}

/// A function's code as the unit splices it: per PC, the call it inlines there, if any.
struct Plan {
    code: Vec<WideInstruction>,
    /// Per PC, the callees its call inlines, as a switch on the callee; empty for none.
    calls: Vec<Vec<InlinedPlan>>,
    /// Whether a call the plan does not inline has run (its site linked a target): an
    /// inlined callee's never-run call is an exit of the unit, one that runs is not.
    blocked: bool,
    /// Per PC, the register and identity of the forwarding native an inlined call through it
    /// guards; its instruction in `code` is then the call of the function it calls.
    through_call: Vec<Option<(Register, Value)>>,
}

struct InlinedPlan {
    target: CallTarget,
    recovery: CallRecovery,
    /// The callee's register 0 in the unit.
    base: u32,
    registers: u16,
    plan: Plan,
}

impl Plan {
    /// The unit PCs the function takes where `root` keeps its own instructions; an inlined
    /// callee's completions add their moves and jumps.
    fn length(&self, root: bool) -> u32 {
        self.lengths(root).sum()
    }

    /// The unit PCs each instruction takes, in order.
    fn lengths(&self, root: bool) -> impl Iterator<Item = u32> + '_ {
        self.code.iter().zip(&self.calls).map(move |(instruction, calls)| match calls.as_slice() {
            [] if root => 1,
            [] => completion_length(*instruction),
            alternatives => call_length(alternatives),
        })
    }
}

struct Planner<'t, 'f> {
    program: &'t crate::ResidualProgram,
    targets: &'t dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &'t dyn Fn(u32) -> Option<&'f Function>,
    registers: u32,
    budget: usize,
}

impl Planner<'_, '_> {
    /// The plan of `function`'s `code`, its calls inlined where the budget allows. A callee
    /// is inlined only whole: every call of its own inlined too.
    fn plan(&mut self, function: u32, mut code: Vec<WideInstruction>, strict: bool, depth: usize) -> Plan {
        let mut blocked = false;
        let mut through_call = vec![None; code.len()];
        let mut called = Vec::new();
        let calls = code
            .iter()
            .enumerate()
            .map(|(pc, instruction)| {
                // A call through a forwarding native inlines as the call of the function it calls.
                let through = self.through_call(function, pc as u32, *instruction);
                // Strict terminal calls retain the VM's proper-tail-call policy.
                // A root completion must name a real Return, never the completed Call.
                let (calls, ran) = if instruction.op() == Op::Call && instruction.returns_from_frame()
                    && (strict || (depth == 0 && terminal_return(&code).is_none())) {
                    (Vec::new(), true)
                } else { self.inline(function, pc as u32, through.map_or(*instruction, |(call, _)| call), depth, through.is_some()) };
                blocked |= calls.is_empty() && ran;
                if let Some((call, native)) = through.filter(|_| !calls.is_empty()) {
                    called.push((pc, call));
                    through_call[pc] = Some(native);
                }
                calls
            })
            .collect();
        for (pc, call) in called {
            code[pc] = call;
        }
        Plan { code, calls, blocked, through_call }
    }

    /// The call at `pc` of `function` as the call of the function it forwards to, where its
    /// site called its targets through a forwarding native (`CalleeBinding::Forwarded`): the
    /// receiver is the callee and the first argument the receiver. With the native's register
    /// and identity, which the inlined call guards.
    fn through_call(&self, function: u32, pc: u32, instruction: WideInstruction) -> Option<(WideInstruction, (Register, Value))> {
        let imm = instruction.imm();
        let (base, count) = (ImmediateLayout::call_window_base(imm), ImmediateLayout::argument_count(imm));
        (instruction.op() == Op::Call && count > 0 && !instruction.direct_eval() && !instruction.parameter_eval()).then_some(())?;
        let through = (self.targets)(function, pc).into_iter().find_map(CallTarget::through)?;
        let call = WideInstruction::new(Op::Call, instruction.a(), instruction.register_c(), base,
            ImmediateLayout::call_immediate(base + 1, count - 1, false, false));
        Some((call, (instruction.register_b(), through)))
    }

    /// The callees the call at `pc` of `function` inlines: each target its site linked that
    /// inlines within the budget, as the call's switch; any other callee takes the call.
    /// With whether the call has run: a call the plan may not inline counts as run.
    fn inline(&mut self, function: u32, pc: u32, instruction: WideInstruction, depth: usize, forwarded: bool) -> (Vec<InlinedPlan>, bool) {
        if instruction.op() != Op::Call {
            return (Vec::new(), false);
        }
        if !returns_or_keeps(instruction) || instruction.direct_eval()
            || instruction.parameter_eval() || depth >= MAX_INLINING_DEPTH
        {
            return (Vec::new(), true);
        }
        // A forwarded call inlines the targets reached through the native; a plain call, the others.
        let found: Vec<_> = (self.targets)(function, pc).into_iter().filter_map(|target| match target.callee {
            CalleeBinding::Forwarded { callee, .. } => forwarded.then_some(CallTarget { callee: CalleeBinding::Identity(callee), ..target }),
            _ => (!forwarded).then_some(target),
        }).collect();
        let ran = !found.is_empty();
        // A target that does not inline gives back what it reserved (`inline_target`).
        let alternatives = found.into_iter().filter_map(|target| self.inline_target(target, instruction, depth)).collect();
        (alternatives, ran)
    }

    fn inline_target(&mut self, target: CallTarget, instruction: WideInstruction, depth: usize) -> Option<InlinedPlan> {
        let callee = (self.body)(target.function)?;
        let code = decoded(callee);
        if code.len() > MAX_INLINED_INSTRUCTIONS
            || code.len() > self.budget
            || ImmediateLayout::argument_count(instruction.imm()) > callee.registers
            || !code.iter().all(|instruction| inlinable(*instruction) && renamable(*instruction))
            || !code.iter().any(|instruction| completion_length(*instruction) > 1
                || (instruction.op() == Op::Call && instruction.returns_from_frame()))
            || (target.callee == CalleeBinding::Function
                && (!callee.plain_activation(self.program) || !super::super::js_compiled::eligible(callee, self.program)))
        {
            return None;
        }
        let recovery = if super::lane_region::restartable_numeric_function(self.program, callee, &code)
            || (target.callee == CalleeBinding::Function && restartable(self.program, callee, &code)) {
            CallRecovery::RestartCall
        } else { CallRecovery::Materialize };
        // The callee's registers and budget are reserved before its own calls plan, and given
        // back if one of them is not inlined.
        let (registers, budget) = (self.registers, self.budget);
        let base = self.registers;
        self.registers += u32::from(callee.registers);
        self.budget -= code.len();
        let plan = self.plan(target.function, code, callee.strict, depth + 1);
        if plan.blocked {
            (self.registers, self.budget) = (registers, budget);
            return None;
        }
        Some(InlinedPlan { target, recovery, base, registers: callee.registers, plan })
    }
}

/// The unit's instructions, origins and frames as a plan lays them out.
struct Emitter {
    unit: Unit,
}

impl Emitter {
    /// Emits `plan` at the unit's end. The caller's (`frame` `None`) keeps its registers and
    /// its own `Return`s; an inlined callee's are renamed to its `base`, its `this` is
    /// `receiver`, and its completions move to `result` and jump to `continuation`.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        plan: &Plan,
        frame: Option<usize>,
        base: Register,
        receiver: Register,
        result: Register,
        continuation: u32,
    ) -> Option<()> {
        // Unit PCs of each instruction of the function, and past its last.
        let root = frame.is_none();
        let mut flat = Vec::with_capacity(plan.code.len() + 1);
        let mut at = self.unit.instructions.len() as u32;
        for length in plan.lengths(root) {
            flat.push(at);
            at += length;
        }
        flat.push(at);
        let target_of = |target: u32| flat.get(target as usize).copied();
        let register = |register: Register| base + register;
        for (pc, (instruction, calls)) in plan.code.iter().copied().zip(&plan.calls).enumerate() {
            let origin = match frame {
                None => Origin::Caller(pc as u32),
                Some(frame) => Origin::Inlined { frame, pc: pc as u32 },
            };
            if !calls.is_empty() {
                // A returning call's callee completes what makes the call: an inlined callee
                // completes the call that inlined it, so its result and continuation pass
                // through; the caller uses its original final Return so SourcePc recovery
                // can never execute an already-completed Call again.
                let returns = instruction.returns_from_frame();
                let (call_result, call_continuation) = match (returns, root) {
                    (true, false) => (result, continuation),
                    (true, true) => {
                        let terminal = terminal_return(&plan.code)?;
                        (register(plan.code[terminal].register_a()), flat[terminal])
                    }
                    (false, _) => (register(instruction.result_register()), flat[pc + 1]),
                };
                for (alternative, call) in calls.iter().enumerate() {
                    let index = self.unit.frames.len();
                    let marker = self.unit.instructions.len();
                    self.unit.markers.push(marker);
                    let callee_base = Register::try_from(call.base).ok()?;
                    let next = (alternative + 1 < calls.len()).then(|| marker + 1 + call.plan.length(false) as usize);
                    let frame_record = Frame {
                        recovery: call.recovery,
                        parent: frame,
                        call: pc as u32,
                        target: call.target,
                        callee_register: register(instruction.register_b()),
                        receiver: register(instruction.register_c()),
                        window: register(ImmediateLayout::call_window_base(instruction.imm())),
                        arguments: ImmediateLayout::argument_count(instruction.imm()), result: (!returns).then_some(call_result),
                        base: callee_base,
                        registers: call.registers,
                        alternative: next,
                        function_call: plan.through_call[pc].map(|(native, through)| (register(native), through)),
                    };
                    self.unit.frames.push(frame_record);
                    // The marker: the call's guards and bindings (`Lowering::inline_call`); a
                    // callee other than this frame's goes to the next alternative, and past the
                    // last leaves at the call itself, before any of its effects.
                    self.unit.instructions.push(WideInstruction::new(Op::Nop, 0, 0, 0, 0));
                    self.unit.origins.push(origin);
                    self.emit(&call.plan, Some(index), callee_base, frame_record.receiver, call_result, call_continuation)?;
                }
                continue;
            }
            if root {
                // A caller instruction keeps its registers; a jump goes to its target's unit PC.
                let retargeted = if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
                    WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target_of(instruction.imm())?)
                } else {
                    instruction
                };
                self.unit.instructions.push(retargeted);
                self.unit.origins.push(origin);
                continue;
            }
            if super::osr_loop::inlined_returning(instruction) {
                let computed = if instruction.op() == Op::GetField && instruction.b() == crate::bytecode::FieldBase::NESTED {
                    // A nested lookup keeps its site, whose base the lowering renames.
                    WideInstruction::new(instruction.op(), register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
                } else {
                    let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
                    super::osr_loop::renamed(plain, &register, receiver, &target_of)?
                };
                self.unit.instructions.push(computed);
                self.unit.instructions.push(WideInstruction::new(Op::Move, result, register(instruction.result_register()), 0, 0));
                self.unit.instructions.push(WideInstruction::new(Op::Jump, 0, 0, 0, continuation));
                self.unit.origins.extend([origin; 3]);
                continue;
            }
            let rewritten = match instruction.op() {
                Op::Return => {
                    self.unit.instructions.push(WideInstruction::new(Op::Move, result, register(instruction.register_a()), 0, 0));
                    self.unit.origins.push(origin);
                    WideInstruction::new(Op::Jump, 0, 0, 0, continuation)
                }
                Op::LoadThis => WideInstruction::new(Op::Move, register(instruction.result_register()), receiver, 0, 0),
                // Exit-only operations (`inlinable`): their registers, renamed for the analyses;
                // the lowering leaves at each, and recovery runs the callee's own instruction.
                Op::LoadName | Op::Throw => WideInstruction::new(instruction.op(), register(instruction.a()), instruction.b(), instruction.c(), instruction.imm()),
                Op::Construct => WideInstruction::new(Op::Construct, register(instruction.a()), register(instruction.b()),
                    register(instruction.c()), instruction.imm()),
                Op::GetField if matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) => {
                    let flags = instruction.result_flags();
                    WideInstruction::new(Op::GetField, flags | register(instruction.result_register()), instruction.b(), instruction.c(), instruction.imm())
                }
                _ => super::osr_loop::renamed(instruction, &register, receiver, &target_of)?,
            };
            self.unit.instructions.push(rewritten);
            self.unit.origins.push(origin);
        }
        Some(())
    }
}

/// `function` (`function_id`) with each call `targets` names (by function and PC), whose
/// callee `body` gives and qualifies, spliced in transitively under the default inlining
/// budget. A unit with no inlined call is the function itself.
pub(crate) fn unit<'f>(
    program: &crate::ResidualProgram,
    function_id: u32,
    function: &Function,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    with_budget(program, function_id, function, MAX_UNIT_INLINED_INSTRUCTIONS, targets, body)
}

/// Plan the same unit under a smaller inlining budget after code-size rejection.
pub(crate) fn with_budget<'f>(
    program: &crate::ResidualProgram,
    function_id: u32,
    function: &Function,
    budget: usize,
    targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
    body: &dyn Fn(u32) -> Option<&'f Function>,
) -> Option<Unit> {
    let caller_registers = function.registers;
    let mut planner = Planner { program, targets, body, registers: u32::from(caller_registers), budget };
    let plan = planner.plan(function_id, decoded(function), function.strict, 0);
    let registers = u16::try_from(planner.registers).ok()?;
    let mut emitter = Emitter {
        unit: Unit {
            instructions: Vec::with_capacity(plan.length(true) as usize),
            origins: Vec::with_capacity(plan.length(true) as usize),
            frames: Vec::new(),
            markers: Vec::new(),
            caller_registers,
            registers,
            end: plan.code.len() as u32,
        },
    };
    emitter.emit(&plan, None, 0, 0, 0, 0)?;
    Some(emitter.unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "aot")]
    #[test]
    fn static_hints_keep_numeric_local_indices_out_of_register_rows() {
        // Captured prefix bindings separate the local and register index spaces;
        // the loop closure keeps the caught-result local in its frame slot.
        let names: Vec<_> = (0..16).map(|index| format!("prefix{index}")).collect();
        let bindings = names.iter().map(|name| format!("{name}=0")).collect::<Vec<_>>().join(",");
        let source = format!("function check(marker){{let {bindings};function observe(){{return [{}];}}for(const item of [1]){{let capture=()=>item;let caught=false;try{{throw item;}}catch(error){{caught=error===marker;}}consume(caught,capture);}}return observe;}}", names.join(","));
        for typescript in [false, true] {
            let program = if typescript { crate::Engine::specialize_typescript(&source, "local-hints.cts") }
                else { crate::Engine::specialize(&source, "local-hints.cjs") }.unwrap();
            program.validate().unwrap();
            assert!(program.functions.iter().any(|function| function.instructions().any(|instruction|
                instruction.numeric_local_target().is_some_and(|local|
                    local >= function.registers && local < function.locals))),
                "the regression must exercise a valid local outside the register row:\n{}", program.disassemble());
            let targets = static_call_targets(&program);
            assert_eq!(targets.len(), program.function_count());
            for (function, targets) in program.functions.iter().zip(targets) {
                assert_eq!(targets.len(), function.code.len());
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn method_hints_follow_definition_records_and_bound_conflicting_bodies() {
        // Each case names how many bodies the call site is hinted with: every distinct
        // definition of the name, each guarded at the call, up to `tier_policy::POLYMORPHISM`.
        for (definition, expected) in [
            ("class Thing { probe(){return 1;} }", 1),
            ("class Thing { static probe(){return 1;} }", 1),
            ("let mask=4;class Thing { probe(){return (this.bits&mask)!==0||this.bits===2;} }", 1),
            // Computed keys travel through ToPropertyKey and environment
            // homes; this literal-register proof deliberately declines them.
            ("class Thing { ['probe'](){return 1;} }", 0),
            ("function make(mask){return class Thing { probe(){return mask|0;} };}", 1),
            ("class Thing { probe(){return 1;} probe(){return 2;} }", 2),
            ("class First { probe(){return 1;} } class Second { probe(){return 2;} }", 2),
            ("class Thing { probe(){return 1;} } Thing.prototype.probe=function replacement(){return 2;};", 2),
            (&(0..=crate::vm::compiled::tier_policy::POLYMORPHISM).map(|n| format!("class C{n} {{ probe(){{return {n};}} }}")).collect::<String>(), 0),
            ("class Thing { get probe(){return function(){return 1;};} }", 0),
            ("class Thing { set probe(value){} }", 0),
            ("class Thing { get probe(){return 1;} set probe(value){} }", 0),
            ("class Thing { [unknownName](){return 1;} }", 0),
        ] {
            let source = format!("{definition} function caller(receiver){{let result=receiver.probe();return result;}}");
            for typescript in [false, true] {
                let admitted = if typescript { crate::Engine::specialize_typescript(&source, "method-hints.cts") }
                    else { crate::Engine::specialize(&source, "method-hints.cjs") };
                let program = match admitted {
                    Ok(program) => program,
                    Err(errors) if typescript && expected != 1 => {
                        // OXC rejects duplicate TypeScript implementations;
                        // JavaScript's accepted duplicate still exercises the
                        // conflicting-definition path below.
                        assert!(errors.iter().any(|error|
                            error.syntax_message().contains("Identifier `probe` has already been declared")), "{definition}: {errors:?}");
                        continue;
                    }
                    Err(errors) => panic!("{definition}: {errors:?}"),
                };
                let caller = program.functions.iter().position(|body|
                    body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "caller")).unwrap();
                let targets = static_call_targets(&program);
                assert_eq!(targets[caller].iter().map(|site| site.len()).sum::<usize>(), expected,
                    "{definition}\n{}", program.disassemble());
                if expected == 1 {
                    let target = targets[caller].iter().flatten().copied().next().unwrap();
                    assert!(program.function(target as usize).instructions().any(|instruction| instruction.op() == Op::Return
                        || instruction.returns_from_frame()));
                    let unit = unit(&program, caller as u32, program.function(caller), &|function, pc|
                        targets[function as usize].get(pc as usize).into_iter().flatten().copied().next()
                            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
                        &|id| program.function_get(id as usize)).unwrap();
                    assert_eq!(unit.frames.len(), 1, "method hint must reach the shared guarded inliner: {definition}");
                    assert_eq!(unit.frames[0].target, CallTarget { function: target, callee: CalleeBinding::Function });
                }
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn captured_hints_resolve_lexical_depth_and_bound_conflicting_bodies() {
        for (binding, expected) in [
            ("let target = function leaf(x) {return x + 1;};", &["leaf"][..]),
            ("let target; if (flag) target = function first(x) {return x + 1;}; else target = function second(x) {return x + 2;};",
                &["first", "second"][..]),
        ] {
            let source = format!("function outer(flag) {{{binding} function middle() {{function caller(x) {{return target(x);}} return caller;}} return middle();}}");
            let program = crate::Engine::specialize(&source, "capture-depth.js").unwrap();
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "caller")).unwrap();
            assert!(program.function(caller).instructions().any(|instruction|
                instruction.op() == Op::LoadCapture && instruction.capture_depth() == 1));
            let targets = static_call_targets(&program);
            let names: Vec<&str> = targets[caller].iter().flatten()
                .map(|target| &program.atoms[program.function(*target as usize).name.unwrap() as usize]).collect();
            assert_eq!(names, expected);
        }
    }


    #[cfg(feature = "aot")]
    #[test]
    fn terminal_leaf_uses_the_existing_caller_return_and_strict_calls_residualize() {
        for strict in [false, true] {
            let directive = if strict { "'use strict';" } else { "" };
            let source = format!("let p={{}};p.leaf=function(x){{this.count=this.count+1;return x+this.count;}};function run(o,x){{{directive}return o.leaf(x);}}");
            let program = crate::Engine::specialize(&source, "terminal-inline.js").unwrap();
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
            let targets = static_call_targets(&program);
            let original_terminal = program.function(caller).code.len() - 1;
            let unit = unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
                .get(pc as usize).into_iter().flatten().copied().next()
                .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
                &|id| program.function_get(id as usize)).unwrap();
            if strict {
                assert!(unit.frames.is_empty(), "strict terminal calls preserve VM tail-call policy");
                continue;
            }
            assert_eq!(unit.frames.len(), 1);
            assert!(!unit.instructions.iter().any(|instruction| instruction.op() == Op::Call));
            let terminal = unit.instructions.len() - 1;
            let returned = unit.instructions[terminal];
            assert_eq!(returned.op(), Op::Return);
            assert_eq!(unit.origins[terminal], Origin::Caller(original_terminal as u32),
                "completion must recover at the original Return, never the Call");
            assert!(unit.instructions.iter().any(|instruction| instruction.op() == Op::Move
                && instruction.result_register() == returned.register_a()));
            assert!(unit.instructions.iter().any(|instruction| instruction.op() == Op::Jump
                && instruction.jump_target() as usize == terminal));
            let live = unit.register_masks(&program, caller as u32).expect("terminal splice liveness");
            assert!(live.is_live(terminal, returned.register_a()));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn nested_strict_terminal_calls_residualize_in_their_own_caller() {
        for strict in [false, true] {
            let directive = if strict { "'use strict';" } else { "" };
            let source = format!("let p={{}};p.leaf=function(x){{return x+1;}};p.middle=function(o,x){{{directive}return o.leaf(x);}};function run(o,x){{let r=o.middle(o,x);return r+1;}}");
            let program = crate::Engine::specialize(&source, "nested-terminal-inline.js").unwrap();
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
            let targets = static_call_targets(&program);
            let unit = unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
                .get(pc as usize).into_iter().flatten().copied().next()
                .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
                &|id| program.function_get(id as usize)).unwrap();
            if strict {
                assert!(unit.frames.is_empty(), "a nested strict terminal call must keep its VM tail-call policy");
            } else {
                assert_eq!(unit.frames.len(), 2, "ordinary nested terminal calls retain recursive expansion");
                assert_eq!(unit.frames[0].parent, None);
                assert_eq!(unit.frames[1].parent, Some(0));
                assert!(!unit.instructions.iter().any(|instruction| instruction.op() == Op::Call));
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn function_binding_distinguishes_effectful_prefixes_from_pure_numeric_loops() {
        for (body, recovery) in [
            ("this.state=1;return this.next+1;", CallRecovery::Materialize),
            ("let x=0;while(x<4)x++;return x;", CallRecovery::RestartCall),
            ("let x=0;while(x<4)x++;this.state=x;return this.next+1;", CallRecovery::Materialize),
            ("this.next.value=1;return this.other+1;", CallRecovery::Materialize)] {
            let source = format!("let o={{}};o.unsafeProbe=function(){{{body}}};function run(o){{let r=o.unsafeProbe();if(r)return 11;return 12;}}");
            let program = crate::Engine::specialize(&source, "non-restartable.js").unwrap();
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
            let targets = static_call_targets(&program);
            assert!(targets[caller].iter().any(|site| !site.is_empty()), "the hint must reach admission");
            let unit = unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
                .get(pc as usize).into_iter().flatten().copied().next()
                .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
                &|id| program.function_get(id as usize)).unwrap();
            assert_eq!(unit.frames.len(), 1, "exact recovery admits {body}");
            assert_eq!(unit.frames[0].recovery, recovery, "{body}");
        }
    }

    #[test]
    fn remapped_liveness_covers_synthesized_reads_without_loading_callee_registers_at_entry() {
        let program = crate::Engine::specialize(
            "function leaf(x) { return x + 1; } function caller(x) { return this.leaf(x) + 2; }",
            "inline-liveness.js",
        ).unwrap();
        let id = |name: &str| program.functions.iter().position(|function|
            function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap() as u32;
        let caller = id("caller");
        let leaf = id("leaf");
        let body = program.function(caller as usize);
        let call = body.instructions().position(|instruction| instruction.op() == Op::Call).unwrap();
        let mut targets = vec![None; body.code.len()];
        targets[call] = Some(CallTarget { callee: CalleeBinding::Identity(Value::UNDEFINED), function: leaf });
        let mut unit = unit(&program, caller, body, &|function, pc| if function == caller {
            targets.get(pc as usize).into_iter().flatten().copied().next().into_iter().collect()
        } else { Vec::new() }, &|function| program.function_get(function as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1);
        let masks = unit.register_masks(&program, caller).expect("covered remapped graph");
        for register in unit.caller_registers..unit.registers {
            assert!(!masks.is_live(0, register), "callee register {register} is defined by call admission");
        }
        let (pc, instruction) = unit.instructions.iter().enumerate().find(|(pc, instruction)|
            instruction.op() == Op::Move && matches!(unit.origins[*pc], Origin::Inlined { .. })
                && instruction.result_register() < unit.caller_registers).expect("synthesized result move");
        assert!(masks.is_live(pc, instruction.register_b()), "return projection must retain its produced result");
        drop(masks);
        unit.registers = crate::compile::liveness::ROOT_MAP_REGISTERS + 1;
        assert!(unit.register_masks(&program, caller).is_none(), "uncovered widths retain conservative recovery");
    }
}
