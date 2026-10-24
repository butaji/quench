//! The compiled execution mode: the fast lane specialized to one program.
//!
//! In Ershov's terms the lane's per-instruction semantics have two readings.
//! The lane handlers interpret them; this module is their generating
//! extension: for each instruction it emits the same meaning as Cranelift IR,
//! with every operand the residual already fixes (register numbers,
//! immediates, constants, jump targets, callees) folded in. Running it over a
//! function's residual bytecode is Futamura's first projection, the
//! interpreter specialized to that function; what specialization removes is
//! exactly what the program fixed: dispatch, operand decoding, value tags
//! (i32 and i64 stay unboxed), the register file (registers become SSA
//! variables) and the per-run memory-view resolution.
//!
//! Each operator keeps one semantic authority. Integer operators come from
//! the integer, float and conversion selectors of `crate::wasm` (an i32
//! operator is its i64 namesake at the narrower width), memory accesses from
//! `MemoryLoad`/`MemoryStore`, traps from `WasmTrap`. A function compiles only
//! when every instruction it can reach has a compiled denotation and every
//! function it calls compiles; anything else stays interpreted, so the mode
//! never changes behavior, only speed. Compiled code never allocates or
//! collects itself and holds no heap roots: values cross its boundary in
//! their Wasm encoding, a trap leaves as a status that becomes the
//! interpreter's `WasmTrap`, and an imported call is a callout through which
//! the VM makes the call exactly as the interpreter would (`import_callout`).

use super::*;
use crate::bytecode::{ControlFlowLayout, Function, WideInstruction};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{
    F32BinaryOperator, F32UnaryOperator, F64BinaryOperator, F64UnaryOperator,
};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator,
};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName,
    condcodes::{FloatCC, IntCC},
    immediates::{Ieee32, Ieee64},
    types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};

/// How a VM executes Wasm functions. JavaScript always runs on the shared
/// interpreter; both modes run in the same VM with the same heap, frames and
/// instances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExecutionMode {
    /// The shared interpreter: general loop and fast lane.
    #[default]
    Interpreted,
    /// Wasm functions with a compiled denotation run as native code.
    Compiled,
}

/// A value's width inside compiled code. Each register has an integer and a
/// float view per width (`View`); a write defines both, so the analysis
/// tracks only the width and a reinterpretation is free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    I32,
    I64,
}

impl Kind {
    fn of(ty: crate::WasmType) -> Option<Self> {
        match ty {
            crate::WasmType::I32 | crate::WasmType::F32 => Some(Self::I32),
            crate::WasmType::I64 | crate::WasmType::F64 => Some(Self::I64),
            _ => None,
        }
    }

    fn ir(self) -> ir::Type {
        match self {
            Self::I32 => types::I32,
            Self::I64 => types::I64,
        }
    }

    fn integer(self) -> crate::WasmType {
        match self {
            Self::I32 => crate::WasmType::I32,
            Self::I64 => crate::WasmType::I64,
        }
    }

    fn of_ir(ty: ir::Type) -> Self {
        if ty.bits() == types::I32.bits() {
            Self::I32
        } else {
            Self::I64
        }
    }
}

/// The views of a register's value, integer and float per width, in the
/// order of a register's variables.
const VIEW_TYPES: [crate::WasmType; 4] = [
    crate::WasmType::I32,
    crate::WasmType::I64,
    crate::WasmType::F32,
    crate::WasmType::F64,
];

/// The variable index of a register's view as Wasm type `ty`.
fn view(ty: crate::WasmType) -> usize {
    VIEW_TYPES
        .iter()
        .position(|candidate| *candidate == ty)
        .expect("compiled values are numeric scalars")
}

/// The machine type of a scalar Wasm type.
fn scalar_ir(ty: crate::WasmType) -> ir::Type {
    match ty {
        crate::WasmType::I32 => types::I32,
        crate::WasmType::I64 => types::I64,
        crate::WasmType::F32 => types::F32,
        crate::WasmType::F64 => types::F64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// The type of the other view of the same bits.
fn counterpart(ty: crate::WasmType) -> crate::WasmType {
    match ty {
        crate::WasmType::I32 => crate::WasmType::F32,
        crate::WasmType::I64 => crate::WasmType::F64,
        crate::WasmType::F32 => crate::WasmType::I32,
        crate::WasmType::F64 => crate::WasmType::I64,
        _ => unreachable!("compiled values are numeric scalars"),
    }
}

/// An i32 integer operator is its i64 namesake: the i64 selectors extend the
/// shared declaration (`crate::wasm::integer`), so tags agree.
fn widen_binary(operator: I32BinaryOperator) -> I64BinaryOperator {
    I64BinaryOperator::from_tag(operator as u32).expect("i64 selectors extend i32's")
}

fn widen_unary(operator: I32UnaryOperator) -> I64UnaryOperator {
    I64UnaryOperator::from_tag(operator as u32).expect("i64 selectors extend i32's")
}

/// A float operator and its operand type; both widths share one declaration
/// (`crate::wasm::float`), so tags agree.
fn float_binary_operator(op: Op, tag: u32) -> Option<(F64BinaryOperator, crate::WasmType)> {
    match op {
        Op::WasmF32Binary => Some((
            F64BinaryOperator::from_tag(F32BinaryOperator::from_tag(tag)? as u32)?,
            crate::WasmType::F32,
        )),
        Op::WasmF64Binary => Some((F64BinaryOperator::from_tag(tag)?, crate::WasmType::F64)),
        _ => None,
    }
}

fn float_unary_operator(op: Op, tag: u32) -> Option<(F64UnaryOperator, crate::WasmType)> {
    match op {
        Op::WasmF32Unary => Some((
            F64UnaryOperator::from_tag(F32UnaryOperator::from_tag(tag)? as u32)?,
            crate::WasmType::F32,
        )),
        Op::WasmF64Unary => Some((F64UnaryOperator::from_tag(tag)?, crate::WasmType::F64)),
        _ => None,
    }
}

/// What a register holds at an instruction, as the analysis derives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Value(Kind),
    /// An instance binding: the environment slot it was loaded from.
    Binding(u32),
    /// The function's memory binding; only direct accesses read it.
    Memory,
    /// A reference to imported function `index`; only a call reads it.
    Import(u32),
    /// Paths disagree; no compiled instruction may read it.
    Conflict,
}

impl Slot {
    fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Conflict }
    }
}

/// Traps compiled code can raise, by status (index + 1); status 0 is a
/// normal return.
const TRAPS: [crate::WasmTrap; 5] = [
    crate::WasmTrap::OutOfBoundsMemory,
    crate::WasmTrap::IntegerDivideByZero,
    crate::WasmTrap::IntegerOverflow,
    crate::WasmTrap::CallStackExhausted,
    crate::WasmTrap::InvalidConversionToInteger,
];

/// The status of a callout that raised: the VM's error (or panic) waits in
/// the context.
const RAISED_STATUS: i64 = TRAPS.len() as i64 + 1;
const TERMINATED_STATUS: i64 = RAISED_STATUS + 1;

fn trap_status(trap: crate::WasmTrap) -> i64 {
    TRAPS
        .iter()
        .position(|candidate| *candidate == trap)
        .map(|index| index as i64 + 1)
        .expect("compiled code raises only listed traps")
}

/// Stack compiled code may use below its entry before a call traps as
/// exhausted; the interpreter's own guards bound the remainder.
const COMPILED_STACK_BUDGET: usize = 512 * 1024;

/// The state every compiled function receives. Field offsets are the ABI
/// compiled code loads from.
#[repr(C)]
struct CallContext {
    stack_limit: usize,
    interrupt: *const std::sync::atomic::AtomicBool,
    table: *const usize,
    vm: *mut (),
    env: Value,
    /// The entry trampoline's stack pointer after it saved the caller's
    /// registers; a guard fault resumes there.
    entry_stack: usize,
    /// The guarded memory reservation compiled code may fault in.
    guard_start: usize,
    guard_end: usize,
    /// Per global the program accesses (`CompiledProgram::globals`), the
    /// address of the global's value in this instance. Heap cells never move,
    /// so the addresses outlive callouts.
    globals: *const *mut Value,
    /// `import_callout` for the VM's host type.
    callout: usize,
    // Fields below are the VM's, never read by compiled code.
    program: ProgramId,
    /// What a callout that returned `RAISED_STATUS` raised.
    raised: Option<Raised>,
}

/// An error, or a panic to resume once compiled frames are gone.
enum Raised {
    Error(JsError),
    Panic(Box<dyn std::any::Any + Send>),
}

const CONTEXT_INTERRUPT: i32 = std::mem::offset_of!(CallContext, interrupt) as i32;
const CONTEXT_STACK_LIMIT: i32 = std::mem::offset_of!(CallContext, stack_limit) as i32;
const CONTEXT_TABLE: i32 = std::mem::offset_of!(CallContext, table) as i32;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const CONTEXT_ENTRY_STACK: u8 = std::mem::offset_of!(CallContext, entry_stack) as u8;
const CONTEXT_GLOBALS: i32 = std::mem::offset_of!(CallContext, globals) as i32;
const CONTEXT_CALLOUT: i32 = std::mem::offset_of!(CallContext, callout) as i32;

/// The callout's signature: (context, function index, arguments, result) ->
/// status; arguments and result are Wasm bits, one word each.
type Callout = unsafe extern "C" fn(*mut CallContext, u64, *const u64, *mut u64) -> i64;

/// Make imported call `function` for compiled code, as the interpreter's
/// `Op::Call` does.
unsafe extern "C" fn import_callout<H: Host>(
    context: *mut CallContext,
    function: u64,
    args: *const u64,
    out: *mut u64,
) -> i64 {
    // SAFETY: compiled code passes the context `run_compiled` made, whose
    // VM is suspended in that call.
    let context = unsafe { &mut *context };
    let vm = unsafe { &mut *context.vm.cast::<Vm<H>>() };
    // The VM's own code runs outside compiled code: no guard fault is ours.
    let active = ACTIVE_CONTEXT.with(|current| current.replace(std::ptr::null_mut()));
    let program = context.program;
    let env = context.env;
    let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: compiled code passes one word per parameter and a result
        // word.
        unsafe { vm.call_import(program, env, function as u32, args, out) }
    }));
    ACTIVE_CONTEXT.with(|current| current.set(active));
    match called {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            context.raised = Some(Raised::Error(error));
            RAISED_STATUS
        }
        Err(panic) => {
            context.raised = Some(Raised::Panic(panic));
            RAISED_STATUS
        }
    }
}

/// A compiled function's entry: (context, memory base, memory length,
/// arguments, result) -> status.
type Entry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64) -> u32;

pub(super) struct CompiledFunction {
    entry: Entry,
    params: Vec<crate::WasmType>,
    result: Option<crate::WasmType>,
}

/// A program's compiled functions, sharing one code mapping and one call
/// table.
pub(super) struct CompiledProgram {
    _code: CodeMemory,
    table: Box<[usize]>,
    functions: Vec<Option<CompiledFunction>>,
    /// The environment slot of the memory compiled code accesses.
    memory_slot: Option<u32>,
    /// Memory accesses rely on the memory's guard reservation instead of
    /// bounds checks.
    guarded: bool,
    /// The guarded memory's base, when code addresses it as a constant
    /// displacement (see `displacement_base`); calls with another base fall
    /// back to the interpreter.
    fixed_base: Option<i32>,
    /// The guarded memory repeats itself above 2^32, so an i32 sum may be
    /// formed in 64 bits (see `WRAP_ALIAS`); calls with a memory that does
    /// not fall back to the interpreter.
    wraps: bool,
    /// The environment slots of the globals compiled code accesses, in the
    /// order of the context's global addresses.
    globals: Vec<u32>,
}

/// Executable memory holding a program's machine code.
struct CodeMemory {
    pointer: *mut u8,
    length: usize,
}

impl CodeMemory {
    fn new(bytes: &[u8]) -> Option<Self> {
        let length = bytes.len().max(1);
        // SAFETY: an anonymous private mapping; the result is checked.
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if pointer == libc::MAP_FAILED {
            return None;
        }
        let pointer = pointer.cast::<u8>();
        // SAFETY: the mapping is `length` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
        // SAFETY: the same mapping, now read-only and executable.
        if unsafe { libc::mprotect(pointer.cast(), length, libc::PROT_READ | libc::PROT_EXEC) } != 0
        {
            // SAFETY: unmapping the mapping created above.
            unsafe { libc::munmap(pointer.cast(), length) };
            return None;
        }
        Some(Self { pointer, length })
    }
}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        // SAFETY: the mapping this value owns.
        unsafe { libc::munmap(self.pointer.cast(), self.length) };
    }
}

/// A function's derived compilation facts.
struct Plan {
    params: Vec<crate::WasmType>,
    result: Option<crate::WasmType>,
    /// Per instruction, `None` when unreachable, else what its `b` register
    /// holds on entry: the one analysis fact a denotation reads (a moved or
    /// selected value's kind, a global's binding, an import reference). The
    /// full register states exist only while planning.
    operands: Vec<Option<Slot>>,
}

impl Plan {
    /// The plan as its callers see it: only the function being compiled
    /// holds its per-instruction facts.
    fn interface(self) -> Self {
        Self {
            operands: Vec::new(),
            ..self
        }
    }

    fn reachable(&self, pc: usize) -> bool {
        self.operands[pc].is_some()
    }

    /// The analysis fact for the `b` register of reachable instruction `pc`.
    fn operand(&self, pc: usize) -> Slot {
        self.operands[pc].expect("reachable")
    }
}

/// The register a direct access of `instruction` reads its memory through.
fn direct_access(op: Op) -> bool {
    MemoryLoad::from_direct_op(op).is_some() || MemoryStore::from_direct_op(op).is_some()
}

fn successors(function: &Function, pc: usize, instruction: WideInstruction) -> Vec<usize> {
    match instruction.op().control_flow_layout() {
        ControlFlowLayout::Jump => vec![instruction.jump_target() as usize],
        ControlFlowLayout::ConditionalJump => vec![instruction.jump_target() as usize, pc + 1],
        ControlFlowLayout::BranchTable => crate::bytecode::branch_table_entries(pc, instruction)
            .filter(|entry| *entry < function.code.len())
            .collect(),
        ControlFlowLayout::Terminal => vec![],
        ControlFlowLayout::Call | ControlFlowLayout::Fallthrough => {
            if instruction.returns_from_frame() {
                vec![]
            } else {
                vec![pc + 1]
            }
        }
    }
}

/// The analysis context shared by every function of a program.
struct Analysis<'a, H: Host> {
    vm: &'a Vm<H>,
    program: ProgramId,
    code: &'a ResidualProgram,
    env: Value,
    /// Whether imported calls may compile: a callee may grow memory, which
    /// moves an unguarded memory's bytes.
    callouts: bool,
}

impl<H: Host> Analysis<'_, H> {
    fn constant(&self, index: usize) -> Option<Value> {
        self.vm.programs.constant(self.program, index)
    }

    /// The machine kind of a constant: i32-encoded scalars, boxed 64-bit
    /// scalars, and any other constant (the void result) as an inert i32.
    fn constant_kind(&self, value: Value) -> Kind {
        match self.vm.decode_bits64(value) {
            Some(_) => Kind::I64,
            None => Kind::I32,
        }
    }

    fn constant_bits(&self, value: Value) -> i64 {
        match self.vm.decode_bits64(value) {
            Some(bits) => bits as i64,
            None => i64::from(value.wasm_bits32() as i32),
        }
    }

    /// The type of the global bound at environment slot `slot`.
    fn global_type(&self, slot: u32) -> Option<crate::WasmType> {
        let binding = self.vm.heap.environment_slot(self.env, slot as usize)?;
        match self.vm.heap.get(binding)? {
            Cell::WasmGlobal { ty, .. } => Some(*ty),
            _ => None,
        }
    }

    /// Whether `slot` binds an unshared memory compiled code can view.
    fn binds_unshared_memory(&self, slot: u32) -> bool {
        let Some(binding) = self.vm.heap.environment_slot(self.env, slot as usize) else {
            return false;
        };
        matches!(
            self.vm.heap.get(binding),
            Some(Cell::WasmMemory { bytes, .. })
                if matches!(**bytes, crate::wasm::memory::MemoryStorage::Unshared(_))
        )
    }

    fn signature(&self, index: u32) -> Option<&crate::WasmSignature> {
        self.vm
            .programs
            .wasm_function_signature(self.program, index)
    }

    /// The signature of `function` when it is imported.
    fn import_signature(&self, function: u32) -> Option<&crate::WasmSignature> {
        let signatures = self.vm.programs.wasm_signatures(self.program)?;
        signatures
            .defined_index(function)
            .is_none()
            .then(|| signatures.get(function as usize))?
    }

    /// The function's plan, assuming every function in `callable` compiles.
    fn plan(&self, index: usize, callable: &[bool], memory_slot: &mut Option<u32>) -> Option<Plan> {
        let function = &self.code.functions[index];
        let signature = self.signature(index as u32)?;
        if function.locals != 0 || signature.results.len() > 1 {
            return None;
        }
        let params = signature.params.clone();
        let result = signature.results.first().copied();
        if params
            .iter()
            .chain(result.iter())
            .any(|ty| Kind::of(*ty).is_none())
        {
            return None;
        }
        let parameter_base = usize::from(function.parameter_registers?);
        let registers = usize::from(function.registers);
        if parameter_base + params.len() > registers {
            return None;
        }
        let memory_register = super::dispatch_fast::memory_register(function);
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        let mut entry = vec![Slot::Value(Kind::I32); registers];
        for (offset, ty) in params.iter().enumerate() {
            entry[parameter_base + offset] = Slot::Value(Kind::of(*ty)?);
        }
        let mut states: Vec<Option<Vec<Slot>>> = vec![None; instructions.len()];
        states[0] = Some(entry);
        let mut work = vec![0usize];
        while let Some(pc) = work.pop() {
            let instruction = instructions[pc];
            let mut state = states[pc]
                .clone()
                .expect("queued instructions have a state");
            self.transfer(
                function,
                instruction,
                &mut state,
                callable,
                memory_register,
                memory_slot,
                result,
            )?;
            for successor in successors(function, pc, instruction) {
                if successor >= instructions.len() {
                    return None;
                }
                let changed = match &mut states[successor] {
                    Some(existing) => {
                        let mut changed = false;
                        for (slot, incoming) in existing.iter_mut().zip(&state) {
                            let joined = slot.join(*incoming);
                            changed |= joined != *slot;
                            *slot = joined;
                        }
                        changed
                    }
                    target @ None => {
                        *target = Some(state.clone());
                        true
                    }
                };
                if changed {
                    work.push(successor);
                }
            }
        }
        let operands = states
            .iter()
            .zip(&instructions)
            .map(|(state, instruction)| {
                let state = state.as_ref()?;
                Some(
                    state
                        .get(usize::from(instruction.b()))
                        .copied()
                        .unwrap_or(Slot::Conflict),
                )
            })
            .collect();
        Some(Plan {
            params,
            result,
            operands,
        })
    }

    /// Check `instruction` has a compiled denotation in `state`, and apply its
    /// register writes.
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut [Slot],
        callable: &[bool],
        memory_register: Option<u16>,
        memory_slot: &mut Option<u32>,
        result: Option<crate::WasmType>,
    ) -> Option<()> {
        let op = instruction.op();
        if op != Op::Return && instruction.returns_from_frame() {
            return None;
        }
        if !super::dispatch_fast::lane_fields_fit(instruction, function) {
            return None;
        }
        let value = |state: &[Slot], register: u16| match state.get(usize::from(register))? {
            Slot::Value(kind) => Some(*kind),
            _ => None,
        };
        let i32_value =
            |state: &[Slot], register: u16| (value(state, register)? == Kind::I32).then_some(());
        let set = |state: &mut [Slot], register: u16, slot: Slot| -> Option<()> {
            *state.get_mut(usize::from(register))? = slot;
            Some(())
        };
        // The memory register is read only by direct accesses through it.
        if memory_register.is_some_and(|memory| {
            !direct_access(op)
                && op != Op::WasmInstanceBinding
                && crate::bytecode::InstructionField::ALL.iter().any(|field| {
                    op.field_layout(*field).reads_register()
                        && instruction.field_value(*field) == memory
                })
        }) {
            return None;
        }
        if let Some(_operator) = I32BinaryOperator::from_register_op(op) {
            i32_value(state, instruction.b())?;
            i32_value(state, instruction.c())?;
            return set(state, instruction.a(), Slot::Value(Kind::I32));
        }
        if I32BinaryOperator::from_immediate_op(op).is_some() {
            i32_value(state, instruction.b())?;
            return set(state, instruction.a(), Slot::Value(Kind::I32));
        }
        if I32BinaryOperator::from_jump_op(op).is_some() {
            i32_value(state, instruction.a())?;
            return i32_value(state, instruction.b());
        }
        if I32BinaryOperator::from_immediate_jump_op(op).is_some() {
            return i32_value(state, instruction.a());
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            if Some(instruction.b()) != memory_register
                || state.get(usize::from(instruction.b())) != Some(&Slot::Memory)
            {
                return None;
            }
            i32_value(state, instruction.c())?;
            return set(
                state,
                instruction.a(),
                Slot::Value(Kind::of(load_type(load)?)?),
            );
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            if Some(instruction.b()) != memory_register
                || state.get(usize::from(instruction.b())) != Some(&Slot::Memory)
            {
                return None;
            }
            i32_value(state, instruction.c())?;
            return (value(state, instruction.a())? == Kind::of(store.value_type())?).then_some(());
        }
        // Result widths are read off the operators' own semantics:
        // comparisons produce an i32.
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let kind = Kind::of(ty)?;
            (value(state, instruction.b())? == kind).then_some(())?;
            (value(state, instruction.c())? == kind).then_some(())?;
            let comparison = operator.apply(0.0, 0.0).ty() == crate::WasmType::I32;
            let result = if comparison { Kind::I32 } else { kind };
            return set(state, instruction.a(), Slot::Value(result));
        }
        if let Some((_, ty)) = float_unary_operator(op, instruction.imm()) {
            let kind = Kind::of(ty)?;
            (value(state, instruction.b())? == kind).then_some(())?;
            return set(state, instruction.a(), Slot::Value(kind));
        }
        match op {
            Op::WasmI64Binary => {
                let operator = I64BinaryOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::I64).then_some(())?;
                (value(state, instruction.c())? == Kind::I64).then_some(())?;
                let result = Kind::of(operator.apply(1, 1).ok()?.ty())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::I64).then_some(())?;
                let result = Kind::of(operator.apply(0).ty())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::WasmScalarConvert => {
                let operator = ScalarConversionOperator::from_tag(instruction.imm())?;
                (value(state, instruction.b())? == Kind::of(operator.source_type())?)
                    .then_some(())?;
                let result = Kind::of(operator.result_type())?;
                set(state, instruction.a(), Slot::Value(result))
            }
            Op::Move => {
                let kind = value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(kind))
            }
            Op::LoadConst => {
                let constant = self.constant(instruction.constant_index())?;
                set(
                    state,
                    instruction.a(),
                    Slot::Value(self.constant_kind(constant)),
                )
            }
            Op::WasmFillRegisters => {
                let constant = self.constant(instruction.constant_index())?;
                let kind = self.constant_kind(constant);
                let window = instruction.register_window();
                for offset in 0..window.count {
                    set(state, window.base + offset, Slot::Value(kind))?;
                }
                Some(())
            }
            Op::WasmI32Unary => {
                I32UnaryOperator::from_tag(instruction.imm())?;
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                i32_value(state, instruction.b())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmJumpI32Zero | Op::WasmJumpI32NonZero => i32_value(state, instruction.a()),
            Op::Jump => Some(()),
            Op::WasmBranchTable => {
                i32_value(state, instruction.a())?;
                let pc_entries: Vec<_> =
                    crate::bytecode::branch_table_entries(0, instruction).collect();
                // Entries are `Jump`s; validation guarantees it for
                // reachable tables.
                (!pc_entries.is_empty()).then_some(())
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                i32_value(state, condition)?;
                let kind = value(state, instruction.b())?;
                (value(state, instruction.c())? == kind).then_some(())?;
                set(state, instruction.a(), Slot::Value(kind))
            }
            Op::CallKnown => {
                let callee = u32::from(instruction.known_function_index());
                if !callable.get(callee as usize).copied().unwrap_or(false) {
                    return None;
                }
                let signature = self.signature(callee)?;
                let window = instruction.call_window();
                if usize::from(window.count) != signature.params.len() {
                    return None;
                }
                for (offset, ty) in signature.params.iter().enumerate() {
                    (value(state, window.base + offset as u16)? == Kind::of(*ty)?).then_some(())?;
                }
                let kind = match signature.results.first() {
                    Some(ty) => Kind::of(*ty)?,
                    None => Kind::I32,
                };
                set(state, instruction.result_register(), Slot::Value(kind))
            }
            Op::WasmRefFunc if self.callouts => {
                let function = instruction.imm();
                self.import_signature(function)?;
                set(state, instruction.a(), Slot::Import(function))
            }
            Op::Call => {
                let Slot::Import(function) = *state.get(usize::from(instruction.b()))? else {
                    return None;
                };
                let signature = self.import_signature(function)?;
                let window = instruction.call_window();
                if usize::from(window.count) != signature.params.len()
                    || signature.results.len() > 1
                {
                    return None;
                }
                for (offset, ty) in signature.params.iter().enumerate() {
                    (value(state, window.base + offset as u16)? == Kind::of(*ty)?).then_some(())?;
                }
                let kind = match signature.results.first() {
                    Some(ty) => Kind::of(*ty)?,
                    None => Kind::I32,
                };
                set(state, instruction.result_register(), Slot::Value(kind))
            }
            Op::Return => match result {
                Some(ty) => (value(state, instruction.a())? == Kind::of(ty)?).then_some(()),
                None => Some(()),
            },
            Op::WasmInstanceBinding => {
                let slot = instruction.imm();
                if Some(instruction.a()) == memory_register {
                    if !self.binds_unshared_memory(slot) || memory_slot.is_some_and(|s| s != slot) {
                        return None;
                    }
                    *memory_slot = Some(slot);
                    return set(state, instruction.a(), Slot::Memory);
                }
                set(state, instruction.a(), Slot::Binding(slot))
            }
            // Compiled code reads and writes a 32-bit global's encoded value
            // in place; a 64-bit global's value is a box, so it stays
            // interpreted.
            Op::WasmGlobalGet => {
                let Slot::Binding(slot) = *state.get(usize::from(instruction.b()))? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                set(state, instruction.a(), Slot::Value(Kind::I32))
            }
            Op::WasmGlobalSet => {
                let Slot::Binding(slot) = *state.get(usize::from(instruction.b()))? else {
                    return None;
                };
                (Kind::of(self.global_type(slot)?)? == Kind::I32).then_some(())?;
                (value(state, instruction.a())? == Kind::I32).then_some(())
            }
            _ => None,
        }
    }
}

/// The Wasm type a direct load produces.
fn load_type(load: MemoryLoad) -> Option<crate::WasmType> {
    Some(match load {
        MemoryLoad::I32Load
        | MemoryLoad::I32Load8S
        | MemoryLoad::I32Load8U
        | MemoryLoad::I32Load16S
        | MemoryLoad::I32Load16U => crate::WasmType::I32,
        MemoryLoad::F32Load => crate::WasmType::F32,
        MemoryLoad::I64Load
        | MemoryLoad::I64Load8S
        | MemoryLoad::I64Load8U
        | MemoryLoad::I64Load16S
        | MemoryLoad::I64Load16U
        | MemoryLoad::I64Load32S
        | MemoryLoad::I64Load32U => crate::WasmType::I64,
        MemoryLoad::F64Load => crate::WasmType::F64,
        _ => return None,
    })
}

/// Whether a direct load sign-extends its narrow word.
fn load_signed(load: MemoryLoad) -> bool {
    matches!(
        load,
        MemoryLoad::I32Load8S
            | MemoryLoad::I32Load16S
            | MemoryLoad::I64Load8S
            | MemoryLoad::I64Load16S
            | MemoryLoad::I64Load32S
    )
}

/// The width in bytes a direct store writes.
fn store_width(store: MemoryStore) -> usize {
    match store {
        MemoryStore::I32Store8 | MemoryStore::I64Store8 => 1,
        MemoryStore::I32Store16 | MemoryStore::I64Store16 => 2,
        MemoryStore::I32Store | MemoryStore::F32Store | MemoryStore::I64Store32 => 4,
        _ => 8,
    }
}

/// Disjoint state compiled code accesses: an access to one never aliases an
/// access to another, so loads of one survive stores to the other.
#[derive(Clone, Copy, Debug)]
enum Region {
    /// The instance's linear memory.
    LinearMemory,
    /// Global values, which live in heap cells.
    Globals,
}

impl Region {
    const ALL: [Self; 2] = [Self::LinearMemory, Self::Globals];
    const COUNT: usize = Self::ALL.len();
}

/// Flags of a load of context state fixed for the whole call (the stack
/// limit, the callout, global addresses): it may be reused and hoisted.
fn invariant() -> MemFlagsData {
    MemFlagsData::trusted().with_readonly().with_can_move()
}

/// The memory facts a compiled call passes after the context: the base
/// unless the program fixes it as a constant, and the length unless the
/// guard bounds accesses. One program's functions share them.
#[derive(Clone, Copy)]
struct MemoryArguments {
    base: bool,
    length: bool,
}

impl MemoryArguments {
    fn of(fixed_base: Option<i32>, guarded: bool) -> Self {
        Self {
            base: fixed_base.is_none(),
            length: !guarded,
        }
    }

    /// The passed ones of `base` and `length`, in parameter order.
    fn select(self, base: ir::Value, length: ir::Value) -> impl Iterator<Item = ir::Value> {
        [self.base.then_some(base), self.length.then_some(length)]
            .into_iter()
            .flatten()
    }

    fn count(self) -> usize {
        usize::from(self.base) + usize::from(self.length)
    }
}

/// The internal signature of a compiled function: the context, the memory
/// arguments, then the Wasm parameters.
fn internal_signature(
    memory: MemoryArguments,
    plan_params: &[crate::WasmType],
    result: Option<crate::WasmType>,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    for _ in 0..1 + memory.count() {
        signature.params.push(AbiParam::new(types::I64));
    }
    for ty in plan_params {
        signature
            .params
            .push(AbiParam::new(Kind::of(*ty).expect("planned").ir()));
    }
    signature.returns.push(AbiParam::new(types::I64));
    if let Some(ty) = result {
        signature
            .returns
            .push(AbiParam::new(Kind::of(ty).expect("planned").ir()));
    }
    signature
}

/// Instructions a threaded copy may repeat between a constant's jump and the
/// branch table the constant decides; bounds the code each copy adds.
const THREAD_BUDGET: usize = 16;

/// Where a threaded copy continues once its path is done.
#[derive(Clone, Copy)]
enum ThreadEnd {
    /// The branch table's case for the constant.
    Case(usize),
    /// The successor the constant decided a branch to.
    Exit(usize),
    /// The table at the path's end, run for real: a jump that knows no
    /// selector gets its own copy of the dispatch (`dispatch_path`), so each
    /// case's back edge predicts its own successor.
    Dispatch,
}

/// A copy of the instructions from a jump target to a branch table, for a
/// jump that leaves the table's selector holding a known constant: along
/// the copy the selector cannot change, so branches on it are decided and
/// the table becomes a direct jump (jump threading; a state machine's
/// dispatch on the state it just set disappears). The copy repeats each
/// instruction at its own PC and exits only to block leaders.
struct Thread {
    selector: u16,
    value: i32,
    path: Vec<usize>,
    end: ThreadEnd,
}

/// Whether `instruction` may write `register`; windows and other register
/// groups count as writing every register.
fn writes_register(instruction: WideInstruction, register: u16) -> bool {
    use crate::bytecode::{FieldLayout, InstructionField};
    InstructionField::ALL.iter().any(|field| {
        let operand = match field {
            InstructionField::A => instruction.a(),
            InstructionField::B => instruction.b(),
            InstructionField::C => instruction.c(),
        };
        match instruction.op().field_layout(*field) {
            FieldLayout::ResultRegister
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => operand == register,
            FieldLayout::RegisterWindowBase
            | FieldLayout::RegisterCount
            | FieldLayout::ConstructArguments
            | FieldLayout::NumericLocalTarget => true,
            _ => false,
        }
    })
}

/// Whether conditional jump `instruction` is taken when `register` holds
/// `value`, if that alone decides it.
fn decided_branch(instruction: WideInstruction, register: u16, value: i32) -> Option<bool> {
    let op = instruction.op();
    if instruction.a() != register {
        return None;
    }
    if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
        let right = i32::from(instruction.b() as i16);
        return operator.evaluate(value, right).ok().map(|holds| holds != 0);
    }
    match op {
        Op::WasmJumpI32Zero => Some(value == 0),
        Op::WasmJumpI32NonZero => Some(value != 0),
        _ => None,
    }
}

/// The threaded copy from `start` for `selector` holding `value`, when one
/// reaches a branch table on the selector, or a branch the value decides,
/// within the budget.
fn thread_path(
    instructions: &[WideInstruction],
    plan: &Plan,
    start: usize,
    selector: u16,
    value: i32,
) -> Option<Thread> {
    fn walk(
        instructions: &[WideInstruction],
        plan: &Plan,
        pc: usize,
        selector: u16,
        value: i32,
        path: &mut Vec<usize>,
    ) -> Option<ThreadEnd> {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        if writes_register(instruction, selector) {
            return None;
        }
        path.push(pc);
        let end = match instruction.op().control_flow_layout() {
            ControlFlowLayout::BranchTable if instruction.a() == selector => {
                let entries: Vec<usize> = crate::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| instructions[entry].jump_target() as usize)
                    .collect();
                let (default, cases) = entries.split_last()?;
                let case = usize::try_from(value as u32).ok();
                Some(ThreadEnd::Case(
                    *case.and_then(|case| cases.get(case)).unwrap_or(default),
                ))
            }
            ControlFlowLayout::Fallthrough => {
                walk(instructions, plan, pc + 1, selector, value, path)
            }
            ControlFlowLayout::Jump => walk(
                instructions,
                plan,
                instruction.jump_target() as usize,
                selector,
                value,
                path,
            ),
            ControlFlowLayout::ConditionalJump => {
                let target = instruction.jump_target() as usize;
                match decided_branch(instruction, selector, value) {
                    Some(taken) => {
                        let next = if taken { target } else { pc + 1 };
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            Some(ThreadEnd::Exit(next))
                        })
                    }
                    None => [target, pc + 1].into_iter().find_map(|next| {
                        let length = path.len();
                        walk(instructions, plan, next, selector, value, path).or_else(|| {
                            path.truncate(length);
                            None
                        })
                    }),
                }
            }
            _ => None,
        };
        if end.is_none() {
            path.pop();
        }
        end
    }
    let mut path = Vec::new();
    let end = walk(instructions, plan, start, selector, value, &mut path)?;
    Some(Thread {
        selector,
        value,
        path,
        end,
    })
}

/// The straight-line copy from jump target `start` to the branch table it
/// reaches, within the threading budget; the table is the path's last
/// instruction.
fn dispatch_path(instructions: &[WideInstruction], plan: &Plan, start: usize) -> Option<Thread> {
    let mut path = Vec::new();
    let mut pc = start;
    loop {
        if path.len() == THREAD_BUDGET || !plan.reachable(pc) {
            return None;
        }
        let instruction = *instructions.get(pc)?;
        path.push(pc);
        pc = match instruction.op().control_flow_layout() {
            ControlFlowLayout::Fallthrough => pc + 1,
            ControlFlowLayout::Jump => instruction.jump_target() as usize,
            ControlFlowLayout::BranchTable => {
                return Some(Thread {
                    selector: instruction.a(),
                    value: 0,
                    path,
                    end: ThreadEnd::Dispatch,
                });
            }
            _ => return None,
        };
    }
}

struct Emitter<'a, 'b, H: Host> {
    guarded: bool,
    /// The guarded memory repeats above 2^32 (`CompiledProgram::wraps`).
    wraps: bool,
    /// The memory base addressed as a displacement (`displacement_base`).
    fixed_base: Option<i32>,
    /// Per `Region`, its alias region in this function.
    regions: [ir::AliasRegion; Region::COUNT],
    globals: &'a [u32],
    analysis: &'a Analysis<'a, H>,
    builder: FunctionBuilder<'b>,
    plan: &'a Plan,
    variables: Vec<[Variable; VIEW_TYPES.len()]>,
    blocks: Vec<Option<ir::Block>>,
    context: ir::Value,
    base: ir::Value,
    /// The memory's length, which only unguarded accesses compare.
    length: Option<ir::Value>,
    memory: MemoryArguments,
    trap_blocks: [Option<ir::Block>; TRAPS.len()],
    termination_block: Option<ir::Block>,
    /// Per callee, its imported name in this function.
    callees: Vec<Option<ir::FuncRef>>,
    callout_signature: Option<ir::SigRef>,
    plans: &'a [Option<Plan>],
    call_conv: cranelift_codegen::isa::CallConv,
    instructions: &'a [WideInstruction],
    /// Registers some branch table dispatches on.
    selectors: Vec<u16>,
    /// Selectors known to hold an i32 constant since the current block began.
    known: Vec<(u16, i32)>,
    /// Threaded copies by (target, selector, value); `None` where none fits.
    threads: Vec<((usize, u16, i32), Option<ir::Block>)>,
    /// Copies to emit once the function body is done.
    pending: Vec<(ir::Block, Thread)>,
    /// While a copy emits a branch, the block standing in for one target.
    redirect: Option<(usize, ir::Block)>,
    /// Whether jumps may enter threaded copies (not from within one).
    threading: bool,
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The flags of an access to `region`.
    fn flags(&self, region: Region) -> MemFlagsData {
        MemFlagsData::trusted()
            .with_endianness(ir::Endianness::Little)
            .with_alias_region(Some(self.regions[region as usize]))
    }

    fn read(&mut self, register: u16, kind: Kind) -> ir::Value {
        self.read_typed(register, kind.integer())
    }

    fn write(&mut self, register: u16, kind: Kind, value: ir::Value) {
        self.write_typed(register, kind.integer(), value);
    }

    /// The register's value viewed as `ty`.
    fn read_typed(&mut self, register: u16, ty: crate::WasmType) -> ir::Value {
        self.builder
            .use_var(self.variables[usize::from(register)][view(ty)])
    }

    /// Define the register as `value` of type `ty`, in both views; a view no
    /// instruction reads costs nothing, since its conversion is dead.
    fn write_typed(&mut self, register: u16, ty: crate::WasmType, value: ir::Value) {
        self.known.retain(|(known, _)| *known != register);
        let other = counterpart(ty);
        let converted = self
            .builder
            .ins()
            .bitcast(scalar_ir(other), MemFlagsData::new(), value);
        let variables = self.variables[usize::from(register)];
        self.builder.def_var(variables[view(ty)], value);
        self.builder.def_var(variables[view(other)], converted);
    }

    /// Define the register as the constant `bits` of width `kind`.
    fn write_constant(&mut self, register: u16, kind: Kind, bits: i64) {
        self.known.retain(|(known, _)| *known != register);
        if kind == Kind::I32 && self.selectors.contains(&register) {
            self.known.push((register, bits as i32));
        }
        let (integer, float) = match kind {
            Kind::I32 => (
                self.builder.ins().iconst(types::I32, bits),
                self.builder.ins().f32const(Ieee32::with_bits(bits as u32)),
            ),
            Kind::I64 => (
                self.builder.ins().iconst(types::I64, bits),
                self.builder.ins().f64const(Ieee64::with_bits(bits as u64)),
            ),
        };
        let float_type = counterpart(kind.integer());
        let variables = self.variables[usize::from(register)];
        self.builder
            .def_var(variables[view(kind.integer())], integer);
        self.builder.def_var(variables[view(float_type)], float);
    }

    /// The kind of the `b` register of instruction `pc`.
    fn operand_kind(&self, pc: usize) -> Kind {
        match self.plan.operand(pc) {
            Slot::Value(kind) => kind,
            _ => Kind::I32,
        }
    }

    fn block(&mut self, pc: usize) -> ir::Block {
        *self.blocks[pc].get_or_insert_with(|| self.builder.create_block())
    }

    fn trap_block(&mut self, trap: crate::WasmTrap) -> ir::Block {
        let index = (trap_status(trap) - 1) as usize;
        if let Some(block) = self.trap_blocks[index] {
            return block;
        }
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.trap_blocks[index] = Some(block);
        block
    }

    /// Branch to the trap when `condition` is nonzero; continue otherwise.
    fn trap_if(&mut self, condition: ir::Value, trap: crate::WasmTrap) {
        let trap = self.trap_block(trap);
        let next = self.builder.create_block();
        self.builder.ins().brif(condition, trap, &[], next, &[]);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }

    fn interrupt_safepoint(&mut self) {
        let pointer = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.context,
            CONTEXT_INTERRUPT,
        );
        let requested = self
            .builder
            .ins()
            .atomic_load(types::I8, MemFlagsData::trusted(), pointer);
        let stop = self.builder.ins().icmp_imm_u(IntCC::NotEqual, requested, 0);
        let termination = match self.termination_block {
            Some(block) => block,
            None => {
                let block = self.builder.create_block();
                self.termination_block = Some(block);
                block
            }
        };
        let next = self.builder.create_block();
        self.builder.ins().brif(stop, termination, &[], next, &[]);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }
    fn finish_traps(&mut self) {
        if let Some(block) = self.termination_block {
            self.builder.switch_to_block(block);
            let status = self.builder.ins().iconst(types::I64, TERMINATED_STATUS);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.builder.ins().iconst(kind.ir(), 0));
            }
            self.builder.ins().return_(&values);
        }
        for (index, block) in self.trap_blocks.into_iter().enumerate() {
            let Some(block) = block else { continue };
            self.builder.switch_to_block(block);
            let status = self.builder.ins().iconst(types::I64, index as i64 + 1);
            let mut values = vec![status];
            if let Some(ty) = self.plan.result {
                let kind = Kind::of(ty).expect("planned");
                values.push(self.builder.ins().iconst(kind.ir(), 0));
            }
            self.builder.ins().return_(&values);
        }
    }

    /// An integer operator at the operands' width.
    fn integer_binary(
        &mut self,
        operator: I64BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use I64BinaryOperator as B;
        let ty = self.builder.func.dfg.value_type(left);
        let compare = |this: &mut Self, cc: IntCC| {
            let flag = this.builder.ins().icmp(cc, left, right);
            this.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            B::Add => self.builder.ins().iadd(left, right),
            B::Subtract => self.builder.ins().isub(left, right),
            B::Multiply => self.builder.ins().imul(left, right),
            B::And => self.builder.ins().band(left, right),
            B::Or => self.builder.ins().bor(left, right),
            B::Xor => self.builder.ins().bxor(left, right),
            B::ShiftLeft => self.builder.ins().ishl(left, right),
            B::ShiftRightSigned => self.builder.ins().sshr(left, right),
            B::ShiftRightUnsigned => self.builder.ins().ushr(left, right),
            B::RotateLeft => self.builder.ins().rotl(left, right),
            B::RotateRight => self.builder.ins().rotr(left, right),
            B::MultiplyHighSigned => self.builder.ins().smulhi(left, right),
            B::MultiplyHighUnsigned => self.builder.ins().umulhi(left, right),
            B::Equal => compare(self, IntCC::Equal),
            B::NotEqual => compare(self, IntCC::NotEqual),
            B::LessSigned => compare(self, IntCC::SignedLessThan),
            B::LessUnsigned => compare(self, IntCC::UnsignedLessThan),
            B::GreaterSigned => compare(self, IntCC::SignedGreaterThan),
            B::GreaterUnsigned => compare(self, IntCC::UnsignedGreaterThan),
            B::LessEqualSigned => compare(self, IntCC::SignedLessThanOrEqual),
            B::LessEqualUnsigned => compare(self, IntCC::UnsignedLessThanOrEqual),
            B::GreaterEqualSigned => compare(self, IntCC::SignedGreaterThanOrEqual),
            B::GreaterEqualUnsigned => compare(self, IntCC::UnsignedGreaterThanOrEqual),
            B::DivideSigned | B::RemainderSigned | B::DivideUnsigned | B::RemainderUnsigned => {
                let zero = self.builder.ins().icmp_imm_s(IntCC::Equal, right, 0);
                self.trap_if(zero, crate::WasmTrap::IntegerDivideByZero);
                match operator {
                    B::DivideUnsigned => self.builder.ins().udiv(left, right),
                    B::RemainderUnsigned => self.builder.ins().urem(left, right),
                    B::DivideSigned => {
                        let minimum = if ty == types::I32 {
                            i64::from(i32::MIN)
                        } else {
                            i64::MIN
                        };
                        let minimum = self.builder.ins().icmp_imm_s(IntCC::Equal, left, minimum);
                        let negative_one = self.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let overflow = self.builder.ins().band(minimum, negative_one);
                        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
                        self.builder.ins().sdiv(left, right)
                    }
                    _ => {
                        // `MIN % -1` is 0; the machine division would fault.
                        let negative_one = self.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let one = self.builder.ins().iconst(ty, 1);
                        let divisor = self.builder.ins().select(negative_one, one, right);
                        self.builder.ins().srem(left, divisor)
                    }
                }
            }
        }
    }

    /// An integer unary operator at the operand's width.
    fn integer_unary(&mut self, operator: I64UnaryOperator, value: ir::Value) -> ir::Value {
        use I64UnaryOperator as U;
        let ty = self.builder.func.dfg.value_type(value);
        let mut extend = |narrow_type| {
            let narrow = self.builder.ins().ireduce(narrow_type, value);
            self.builder.ins().sextend(ty, narrow)
        };
        match operator {
            U::EqualZero => {
                let flag = self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
                self.builder.ins().uextend(types::I32, flag)
            }
            U::LeadingZeros => self.builder.ins().clz(value),
            U::TrailingZeros => self.builder.ins().ctz(value),
            U::PopulationCount => self.builder.ins().popcnt(value),
            U::ExtendSigned8 => extend(types::I8),
            U::ExtendSigned16 => extend(types::I16),
            U::ExtendSigned32 => extend(types::I32),
        }
    }

    /// The canonical NaN of float type `ty`, as `crate::wasm::float` produces.
    fn canonical_nan(&mut self, ty: ir::Type) -> ir::Value {
        use crate::wasm::numeric::canonical_nan_bits;
        if ty == types::F32 {
            let bits = canonical_nan_bits(f32::MANTISSA_DIGITS, u64::from(f32::INFINITY.to_bits()));
            self.builder.ins().f32const(Ieee32::with_bits(bits as u32))
        } else {
            let bits = canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits());
            self.builder.ins().f64const(Ieee64::with_bits(bits))
        }
    }

    /// A float operator at the operands' width.
    fn float_binary(
        &mut self,
        operator: F64BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use F64BinaryOperator as F;
        let compare = |this: &mut Self, cc: FloatCC| {
            let flag = this.builder.ins().fcmp(cc, left, right);
            this.builder.ins().uextend(types::I32, flag)
        };
        match operator {
            F::Add => self.builder.ins().fadd(left, right),
            F::Subtract => self.builder.ins().fsub(left, right),
            F::Multiply => self.builder.ins().fmul(left, right),
            F::Divide => self.builder.ins().fdiv(left, right),
            F::Minimum | F::Maximum => {
                // Signed zeros order as Wasm requires; a NaN operand gives
                // the canonical NaN.
                let extremum = if operator == F::Minimum {
                    self.builder.ins().fmin(left, right)
                } else {
                    self.builder.ins().fmax(left, right)
                };
                let unordered = self.builder.ins().fcmp(FloatCC::Unordered, left, right);
                let ty = self.builder.func.dfg.value_type(left);
                let nan = self.canonical_nan(ty);
                self.builder.ins().select(unordered, nan, extremum)
            }
            F::CopySign => self.builder.ins().fcopysign(left, right),
            F::Equal => compare(self, FloatCC::Equal),
            F::NotEqual => compare(self, FloatCC::NotEqual),
            F::Less => compare(self, FloatCC::LessThan),
            F::Greater => compare(self, FloatCC::GreaterThan),
            F::LessEqual => compare(self, FloatCC::LessThanOrEqual),
            F::GreaterEqual => compare(self, FloatCC::GreaterThanOrEqual),
        }
    }

    /// A float unary operator at the operand's width. The machine rounding
    /// and square root quiet a signaling NaN, as the operators do.
    fn float_unary(&mut self, operator: F64UnaryOperator, value: ir::Value) -> ir::Value {
        use F64UnaryOperator as U;
        match operator {
            U::Absolute => self.builder.ins().fabs(value),
            U::Negate => self.builder.ins().fneg(value),
            U::Ceiling => self.builder.ins().ceil(value),
            U::Floor => self.builder.ins().floor(value),
            U::Truncate => self.builder.ins().trunc(value),
            U::Nearest => self.builder.ins().nearest(value),
            U::SquareRoot => self.builder.ins().sqrt(value),
        }
    }

    /// A scalar conversion of `value`, already viewed as the source type.
    fn convert(&mut self, operator: ScalarConversionOperator, value: ir::Value) -> ir::Value {
        use ScalarConversionOperator as C;
        let result = scalar_ir(operator.result_type());
        let source = operator.source_type();
        match operator {
            C::WrapI64 => self.builder.ins().ireduce(result, value),
            C::ExtendI32Signed => self.builder.ins().sextend(result, value),
            C::ExtendI32Unsigned => self.builder.ins().uextend(result, value),
            C::TruncateF32SignedI32
            | C::TruncateF64SignedI32
            | C::TruncateF32SignedI64
            | C::TruncateF64SignedI64 => self.truncate(value, source, result, true),
            C::TruncateF32UnsignedI32
            | C::TruncateF64UnsignedI32
            | C::TruncateF32UnsignedI64
            | C::TruncateF64UnsignedI64 => self.truncate(value, source, result, false),
            C::SaturateF32SignedI32
            | C::SaturateF64SignedI32
            | C::SaturateF32SignedI64
            | C::SaturateF64SignedI64 => self.builder.ins().fcvt_to_sint_sat(result, value),
            C::SaturateF32UnsignedI32
            | C::SaturateF64UnsignedI32
            | C::SaturateF32UnsignedI64
            | C::SaturateF64UnsignedI64 => self.builder.ins().fcvt_to_uint_sat(result, value),
            C::ConvertI32SignedF32
            | C::ConvertI64SignedF32
            | C::ConvertI32SignedF64
            | C::ConvertI64SignedF64 => self.builder.ins().fcvt_from_sint(result, value),
            C::ConvertI32UnsignedF32
            | C::ConvertI64UnsignedF32
            | C::ConvertI32UnsignedF64
            | C::ConvertI64UnsignedF64 => self.builder.ins().fcvt_from_uint(result, value),
            C::PromoteF32 => self.builder.ins().fpromote(result, value),
            C::DemoteF64 => self.builder.ins().fdemote(result, value),
            C::ReinterpretF32I32
            | C::ReinterpretF64I64
            | C::ReinterpretI32F32
            | C::ReinterpretI64F64 => {
                self.builder
                    .ins()
                    .bitcast(result, MemFlagsData::new(), value)
            }
        }
    }

    /// A trapping truncation: NaN and out-of-range inputs trap, in the
    /// order and bounds of `crate::wasm::conversion`; the machine conversion
    /// then sees only representable values.
    fn truncate(
        &mut self,
        value: ir::Value,
        source: crate::WasmType,
        result: ir::Type,
        signed: bool,
    ) -> ir::Value {
        let wide = if source == crate::WasmType::F32 {
            self.builder.ins().fpromote(types::F64, value)
        } else {
            value
        };
        let nan = self.builder.ins().fcmp(FloatCC::Unordered, wide, wide);
        self.trap_if(nan, crate::WasmTrap::InvalidConversionToInteger);
        let truncated = self.builder.ins().trunc(wide);
        let (lower, limit) = crate::wasm::conversion::truncation_bounds(result.bits(), signed);
        let lower = self.builder.ins().f64const(lower);
        let limit = self.builder.ins().f64const(limit);
        let below = self.builder.ins().fcmp(FloatCC::LessThan, truncated, lower);
        let above = self
            .builder
            .ins()
            .fcmp(FloatCC::GreaterThanOrEqual, truncated, limit);
        let overflow = self.builder.ins().bor(below, above);
        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
        if signed {
            self.builder.ins().fcvt_to_sint_sat(result, truncated)
        } else {
            self.builder.ins().fcvt_to_uint_sat(result, truncated)
        }
    }

    /// The host address of a checked `width`-byte access at `address + offset`.
    fn address(&mut self, address: ir::Value, offset: u32, width: usize) -> ir::Value {
        // An i32 sum without an offset may be formed in 64 bits when the
        // memory repeats above 2^32: a sum that wrapped reads the bytes its
        // wrapped value names, and one past the accessible prefix faults.
        if self.wraps
            && offset == 0
            && let Some([left, right]) = self.i32_sum(address)
        {
            let left = self.builder.ins().uextend(types::I64, left);
            let right = self.builder.ins().uextend(types::I64, right);
            let index = self.builder.ins().iadd(left, right);
            return self.builder.ins().iadd(self.base, index);
        }
        if self.wraps && offset != 0 {
            // An index whose offset carries it past 2^32 would meet the
            // repeated memory instead of the guard: it traps here. The
            // memory ends below `RESERVED_LIMIT`, so for offsets up to a Wasm
            // page that limit bounds the index alone, and accesses through
            // one index share one check.
            use crate::wasm::memory::{RESERVED_LIMIT, WASM_PAGE, WRAP_ALIAS};
            let limit = if offset as usize <= WASM_PAGE {
                RESERVED_LIMIT
            } else {
                WRAP_ALIAS - offset as usize
            };
            let past = self.builder.ins().icmp_imm_u(
                IntCC::UnsignedGreaterThanOrEqual,
                address,
                limit as i64,
            );
            self.trap_if(past, crate::WasmTrap::OutOfBoundsMemory);
        }
        let address = self.builder.ins().uextend(types::I64, address);
        if self.guarded {
            // Index plus offset plus width stays inside the reservation; past
            // the accessible prefix the access faults, and the fault handler
            // raises the out-of-bounds trap (see `guard_fault`).
            let host = self.builder.ins().iadd(self.base, address);
            return self.builder.ins().iadd_imm_s(host, i64::from(offset));
        }
        let end = self
            .builder
            .ins()
            .iadd_imm_s(address, i64::from(offset) + width as i64);
        let out_of_bounds = self.builder.ins().icmp(
            IntCC::UnsignedGreaterThan,
            end,
            self.length.expect("unguarded accesses receive the length"),
        );
        self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
        let host = self.builder.ins().iadd(self.base, address);
        self.builder.ins().iadd_imm_s(host, i64::from(offset))
    }

    /// The operands of `value` when an i32 add defines it, worth forming
    /// in 64 bits: a constant operand joins the access's displacement, so
    /// one that is negative as i32 (its 64-bit form past 2^31) or carries
    /// the displacement past 32 bits keeps the add.
    fn i32_sum(&self, value: ir::Value) -> Option<[ir::Value; 2]> {
        let dfg = &self.builder.func.dfg;
        let ir::ValueDef::Result(inst, _) = dfg.value_def(value) else {
            return None;
        };
        let ir::InstructionData::Binary {
            opcode: ir::Opcode::Iadd,
            args,
        } = dfg.insts[inst]
        else {
            return None;
        };
        if dfg.value_type(value) != types::I32 {
            return None;
        }
        let displacement_fits = |operand: ir::Value| {
            let ir::ValueDef::Result(constant, _) = dfg.value_def(operand) else {
                return true;
            };
            let ir::InstructionData::UnaryImm { imm, .. } = dfg.insts[constant] else {
                return true;
            };
            let fixed = self.fixed_base.map_or(0, i64::from);
            u32::try_from(imm.bits())
                .ok()
                .and_then(|constant| i32::try_from(fixed + i64::from(constant)).ok())
                .is_some()
        };
        args.iter()
            .all(|&operand| displacement_fits(operand))
            .then_some(args)
    }

    /// The address of the value of the global bound at `slot`.
    fn global_address(&mut self, slot: u32) -> ir::Value {
        let position = self
            .globals
            .iter()
            .position(|candidate| *candidate == slot)
            .expect("planned globals are collected");
        let table = self
            .builder
            .ins()
            .load(types::I64, invariant(), self.context, CONTEXT_GLOBALS);
        self.builder
            .ins()
            .load(types::I64, invariant(), table, (position * 8) as i32)
    }

    /// The binding the `b` register of instruction `pc` holds.
    fn binding_slot(&self, pc: usize) -> u32 {
        match self.plan.operand(pc) {
            Slot::Binding(slot) => slot,
            _ => unreachable!("planned global accesses read a binding"),
        }
    }

    /// A nonzero status from a call (a trap or a raised callout) unwinds
    /// this activation too.
    fn propagate(&mut self, status: ir::Value) {
        let unwound = self.builder.create_block();
        self.builder.set_cold_block(unwound);
        let next = self.builder.create_block();
        self.builder.ins().brif(status, unwound, &[], next, &[]);
        self.builder.switch_to_block(unwound);
        self.builder.seal_block(unwound);
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(
                self.builder
                    .ins()
                    .iconst(Kind::of(ty).expect("planned").ir(), 0),
            );
        }
        self.builder.ins().return_(&values);
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
    }

    /// An imported call: arguments go to the VM in a stack array, through
    /// the context's callout.
    fn callout(&mut self, pc: usize, instruction: WideInstruction) {
        let Slot::Import(function) = self.plan.operand(pc) else {
            unreachable!("planned imported calls read an import reference");
        };
        let signature = self
            .analysis
            .import_signature(function)
            .expect("planned")
            .clone();
        let word = types::I64.bytes();
        let words = signature.params.len() + 1;
        let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            word * words as u32,
            word.trailing_zeros() as u8,
        ));
        let window = instruction.call_window();
        for (offset, ty) in signature.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            let value = self.read(window.base + offset as u16, kind);
            let value = match kind {
                Kind::I32 => self.builder.ins().uextend(types::I64, value),
                Kind::I64 => value,
            };
            let address =
                self.builder
                    .ins()
                    .stack_addr(types::I64, slot, (offset as u32 * word) as i32);
            self.builder
                .ins()
                .store(MemFlagsData::trusted(), value, address, 0);
        }
        let result_offset = (signature.params.len() as u32 * word) as i32;
        let args = self.builder.ins().stack_addr(types::I64, slot, 0);
        let out = self
            .builder
            .ins()
            .stack_addr(types::I64, slot, result_offset);
        let callout_signature = match self.callout_signature {
            Some(signature) => signature,
            None => {
                let mut callout = Signature::new(self.call_conv);
                for _ in 0..4 {
                    callout.params.push(AbiParam::new(types::I64));
                }
                callout.returns.push(AbiParam::new(types::I64));
                let callout = self.builder.import_signature(callout);
                self.callout_signature = Some(callout);
                callout
            }
        };
        let target =
            self.builder
                .ins()
                .load(types::I64, invariant(), self.context, CONTEXT_CALLOUT);
        let function = self.builder.ins().iconst(types::I64, i64::from(function));
        let call = self.builder.ins().call_indirect(
            callout_signature,
            target,
            &[self.context, function, args, out],
        );
        let status = self.builder.inst_results(call)[0];
        self.propagate(status);
        let destination = instruction.result_register();
        match signature.results.first() {
            Some(ty) => {
                let kind = Kind::of(*ty).expect("planned");
                let bits = self
                    .builder
                    .ins()
                    .load(types::I64, MemFlagsData::trusted(), out, 0);
                let value = match kind {
                    Kind::I32 => self.builder.ins().ireduce(types::I32, bits),
                    Kind::I64 => bits,
                };
                self.write(destination, kind, value);
            }
            None => self.write_constant(destination, Kind::I32, 0),
        }
    }

    fn jump(&mut self, target: usize) {
        self.interrupt_safepoint();
        let mut block = self.edge(target);
        if self.threading
            && Some(block) == self.blocks[target]
            && let Some(thread) = dispatch_path(self.instructions, self.plan, target)
        {
            block = self.builder.create_block();
            self.pending.push((block, thread));
        }
        self.builder.ins().jump(block, &[]);
    }

    /// A loop latch (a branch whose taken target lies behind its
    /// fallthrough) leaves the loop through a block of its own, so values
    /// only the exit passes (merge arguments) are computed after the loop,
    /// not on every iteration. The back edge stays direct.
    fn branch(&mut self, condition: ir::Value, target: usize, fallthrough: usize) {
        self.interrupt_safepoint();
        let taken = self.edge(target);
        let next = self.edge(fallthrough);
        if target >= fallthrough {
            self.builder.ins().brif(condition, taken, &[], next, &[]);
            return;
        }
        let exit = self.builder.create_block();
        self.builder.ins().brif(condition, taken, &[], exit, &[]);
        self.builder.switch_to_block(exit);
        self.builder.ins().jump(next, &[]);
    }

    /// The block a jump from here to `target` enters: a threaded copy when
    /// a known selector decides a branch table ahead, else the target's.
    fn edge(&mut self, target: usize) -> ir::Block {
        if let Some((redirected, block)) = self.redirect
            && redirected == target
        {
            return block;
        }
        if self.threading {
            for (selector, value) in self.known.clone() {
                if let Some(block) = self.thread(target, selector, value) {
                    return block;
                }
            }
        }
        self.block(target)
    }

    fn thread(&mut self, target: usize, selector: u16, value: i32) -> Option<ir::Block> {
        let key = (target, selector, value);
        if let Some((_, block)) = self.threads.iter().find(|(known, _)| *known == key) {
            return *block;
        }
        let thread = thread_path(self.instructions, self.plan, target, selector, value);
        let block = thread.map(|thread| {
            let block = self.builder.create_block();
            self.pending.push((block, thread));
            block
        });
        self.threads.push((key, block));
        block
    }

    /// Emit the threaded copy `thread` into `block`.
    fn emit_thread(&mut self, function: &Function, block: ir::Block, thread: &Thread) {
        self.builder.switch_to_block(block);
        self.interrupt_safepoint();
        for (index, &pc) in thread.path.iter().enumerate() {
            let instruction = self.instructions[pc];
            match instruction.op().control_flow_layout() {
                // A dispatch copy ends in its table, run for real.
                ControlFlowLayout::BranchTable if matches!(thread.end, ThreadEnd::Dispatch) => {
                    self.instruction(function, pc, instruction);
                }
                // The path continues at the jump's target, or the table's
                // case: nothing to emit.
                ControlFlowLayout::Jump | ControlFlowLayout::BranchTable => {}
                ControlFlowLayout::ConditionalJump => {
                    if decided_branch(instruction, thread.selector, thread.value).is_some() {
                        continue;
                    }
                    let next = thread.path[index + 1];
                    let continuation = self.builder.create_block();
                    self.redirect = Some((next, continuation));
                    self.instruction(function, pc, instruction);
                    self.redirect = None;
                    self.builder.switch_to_block(continuation);
                }
                _ => {
                    self.instruction(function, pc, instruction);
                }
            }
        }
        let (ThreadEnd::Case(target) | ThreadEnd::Exit(target)) = thread.end else {
            return;
        };
        let target = self.block(target);
        self.builder.ins().jump(target, &[]);
    }

    /// Emit the instruction at `pc`; returns whether control falls through.
    fn instruction(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
    ) -> bool {
        let op = instruction.op();
        if op == Op::WasmBranchTable {
            self.interrupt_safepoint();
        }
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if let Some(operator) = I32BinaryOperator::from_register_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self.read(c, Kind::I32);
            let value = self.integer_binary(widen_binary(operator), left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self
                .builder
                .ins()
                .iconst(types::I32, i64::from(instruction.imm() as i32));
            let value = self.integer_binary(widen_binary(operator), left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.read(b, Kind::I32);
            let holds = self.integer_binary(widen_binary(operator), left, right);
            self.branch(holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.builder.ins().iconst(types::I32, i64::from(b as i16));
            let holds = self.integer_binary(widen_binary(operator), left, right);
            self.branch(holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            let result = load_type(load).expect("planned");
            let kind = Kind::of(result).expect("planned");
            let address = self.read(c, Kind::I32);
            let host = self.address(address, instruction.imm(), load.width());
            let flags = self.flags(Region::LinearMemory);
            let ty = kind.ir();
            let value = match (load.width(), load_signed(load)) {
                // A float loads straight into its view.
                _ if result != kind.integer() => {
                    self.builder.ins().load(scalar_ir(result), flags, host, 0)
                }
                (1, false) => self.builder.ins().uload8(ty, flags, host, 0),
                (1, true) => self.builder.ins().sload8(ty, flags, host, 0),
                (2, false) => self.builder.ins().uload16(ty, flags, host, 0),
                (2, true) => self.builder.ins().sload16(ty, flags, host, 0),
                (4, false) if kind == Kind::I64 => self.builder.ins().uload32(flags, host, 0),
                (4, true) if kind == Kind::I64 => self.builder.ins().sload32(flags, host, 0),
                _ => self.builder.ins().load(ty, flags, host, 0),
            };
            self.write_typed(a, result, value);
            return true;
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            let kind = Kind::of(store.value_type()).expect("planned");
            let value = self.read_typed(a, store.value_type());
            let address = self.read(c, Kind::I32);
            let width = store_width(store);
            let host = self.address(address, instruction.imm(), width);
            let flags = self.flags(Region::LinearMemory);
            match width {
                1 => self.builder.ins().istore8(flags, value, host, 0),
                2 => self.builder.ins().istore16(flags, value, host, 0),
                4 if kind == Kind::I64 => self.builder.ins().istore32(flags, value, host, 0),
                _ => self.builder.ins().store(flags, value, host, 0),
            };
            return true;
        }
        if let Some((operator, ty)) = float_binary_operator(op, instruction.imm()) {
            let left = self.read_typed(b, ty);
            let right = self.read_typed(c, ty);
            let value = self.float_binary(operator, left, right);
            let result = if self.builder.func.dfg.value_type(value) == types::I32 {
                crate::WasmType::I32
            } else {
                ty
            };
            self.write_typed(a, result, value);
            return true;
        }
        if let Some((operator, ty)) = float_unary_operator(op, instruction.imm()) {
            let input = self.read_typed(b, ty);
            let value = self.float_unary(operator, input);
            self.write_typed(a, ty, value);
            return true;
        }
        match op {
            Op::WasmI64Binary => {
                let operator = I64BinaryOperator::from_tag(instruction.imm()).expect("planned");
                let left = self.read(b, Kind::I64);
                let right = self.read(c, Kind::I64);
                let value = self.integer_binary(operator, left, right);
                let kind = Kind::of_ir(self.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmI64Unary => {
                let operator = I64UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I64);
                let value = self.integer_unary(operator, input);
                let kind = Kind::of_ir(self.builder.func.dfg.value_type(value));
                self.write(a, kind, value);
            }
            Op::WasmScalarConvert => {
                let operator =
                    ScalarConversionOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read_typed(b, operator.source_type());
                let value = self.convert(operator, input);
                self.write_typed(a, operator.result_type(), value);
            }
            Op::Move => {
                let kind = self.operand_kind(pc);
                let value = self.read(b, kind);
                self.write(a, kind, value);
            }
            Op::LoadConst | Op::WasmFillRegisters => {
                let constant = self
                    .analysis
                    .constant(instruction.constant_index())
                    .expect("planned");
                let kind = self.analysis.constant_kind(constant);
                let bits = self.analysis.constant_bits(constant);
                if op == Op::LoadConst {
                    self.write_constant(a, kind, bits);
                } else {
                    let window = instruction.register_window();
                    for offset in 0..window.count {
                        self.write_constant(window.base + offset, kind, bits);
                    }
                }
            }
            Op::WasmI32Unary => {
                let operator = I32UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I32);
                let value = self.integer_unary(widen_unary(operator), input);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                let input = self.read(b, Kind::I32);
                let shift = self.builder.ins().iconst(types::I32, i64::from(c as i16));
                let shifted =
                    self.integer_binary(I64BinaryOperator::ShiftRightUnsigned, input, shift);
                let mask = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::from(instruction.imm() as i32));
                let value = self.integer_binary(I64BinaryOperator::And, shifted, mask);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32AddImmediateJumpNonZero => {
                let input = self.read(b, Kind::I32);
                let addend = self.builder.ins().iconst(types::I32, i64::from(c as i16));
                let sum = self.builder.ins().iadd(input, addend);
                self.write(a, Kind::I32, sum);
                self.branch(sum, instruction.jump_target() as usize, pc + 1);
                return false;
            }
            Op::WasmJumpI32Zero | Op::WasmJumpI32NonZero => {
                let value = self.read(a, Kind::I32);
                let (taken, other) = (instruction.jump_target() as usize, pc + 1);
                if op == Op::WasmJumpI32NonZero {
                    self.branch(value, taken, other);
                } else {
                    self.branch(value, other, taken);
                }
                return false;
            }
            Op::Jump => {
                self.jump(instruction.jump_target() as usize);
                return false;
            }
            Op::WasmBranchTable => {
                let selector = self.read(a, Kind::I32);
                let entries: Vec<usize> = crate::bytecode::branch_table_entries(pc, instruction)
                    .map(|entry| {
                        super::dispatch_fast::decoded_instruction(function, function.code[entry])
                            .jump_target() as usize
                    })
                    .collect();
                let (default, cases) = entries.split_last().expect("a default entry");
                let default = self.block(*default);
                let default = self.builder.func.dfg.block_call(default, &[]);
                let cases: Vec<_> = cases
                    .iter()
                    .map(|target| {
                        let block = self.block(*target);
                        self.builder.func.dfg.block_call(block, &[])
                    })
                    .collect();
                let table = ir::JumpTableData::new(default, &cases);
                let table = self.builder.create_jump_table(table);
                self.builder.ins().br_table(selector, table);
                return false;
            }
            Op::WasmSelect => {
                let (condition, _) = instruction.register_pair();
                let kind = self.operand_kind(pc);
                let condition = self.read(condition, Kind::I32);
                let taken = self.read(b, kind);
                let other = self.read(c, kind);
                let value = self.builder.ins().select(condition, taken, other);
                self.write(a, kind, value);
            }
            Op::CallKnown => {
                let callee = usize::from(instruction.known_function_index());
                let callee_plan = self.plans[callee].as_ref().expect("callable");
                let window = instruction.call_window();
                let mut args = vec![self.context];
                if self.memory.base {
                    args.push(self.base);
                }
                args.extend(self.length);
                for (offset, ty) in callee_plan.params.iter().enumerate() {
                    let kind = Kind::of(*ty).expect("planned");
                    args.push(self.read(window.base + offset as u16, kind));
                }
                // Compiled functions share one code buffer: a direct call,
                // linked at placement (`link`).
                let target = match self.callees[callee] {
                    Some(target) => target,
                    None => {
                        let signature = internal_signature(
                            self.memory,
                            &callee_plan.params,
                            callee_plan.result,
                            self.call_conv,
                        );
                        let signature = self.builder.import_signature(signature);
                        let name = self
                            .builder
                            .func
                            .declare_imported_user_function(function_name(callee));
                        let target = self.builder.import_function(ir::ExtFuncData {
                            name: ir::ExternalName::User(name),
                            signature,
                            colocated: true,
                            patchable: false,
                        });
                        self.callees[callee] = Some(target);
                        target
                    }
                };
                let call = self.builder.ins().call(target, &args);
                let results = self.builder.inst_results(call).to_vec();
                self.propagate(results[0]);
                let destination = instruction.result_register();
                match callee_plan.result {
                    Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
                    None => {
                        let zero = self.builder.ins().iconst(types::I32, 0);
                        self.write(destination, Kind::I32, zero);
                    }
                }
            }
            Op::WasmRefFunc => {}
            Op::Call => self.callout(pc, instruction),
            Op::Return => {
                let status = self.builder.ins().iconst(types::I64, 0);
                let mut values = vec![status];
                if let Some(ty) = self.plan.result {
                    values.push(self.read(a, Kind::of(ty).expect("planned")));
                }
                self.builder.ins().return_(&values);
                return false;
            }
            // A binding is known statically per instruction; the globals it
            // names are reached through the context.
            Op::WasmInstanceBinding => {}
            Op::WasmGlobalGet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let flags = self.flags(Region::Globals);
                let value = self.builder.ins().load(types::I32, flags, address, 0);
                self.write(a, Kind::I32, value);
            }
            Op::WasmGlobalSet => {
                let slot = self.binding_slot(pc);
                let address = self.global_address(slot);
                let value = self.read(a, Kind::I32);
                let wide = self.builder.ins().uextend(types::I64, value);
                let encoded = self.builder.ins().bor_imm_u(wide, INTEGER_TAG as i64);
                let flags = self.flags(Region::Globals);
                self.builder.ins().store(flags, encoded, address, 0);
            }
            _ => unreachable!("planned instructions have a denotation"),
        }
        true
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Compile `index` with `plan`; `None` when Cranelift refuses it or its
    /// code needs a relocation other than a call to a compiled function.
    fn compile_function(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        plans: &[Option<Plan>],
        guarded: bool,
        wraps: bool,
        fixed_base: Option<i32>,
        globals: &[u32],
    ) -> Option<(Vec<u8>, Vec<Call>)> {
        let function = &self.code.functions[index];
        let call_conv = isa.default_call_conv();
        let memory = MemoryArguments::of(fixed_base, guarded);
        let mut func = ir::Function::with_name_signature(
            UserFuncName::User(function_name(index)),
            internal_signature(memory, &plan.params, plan.result, call_conv),
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let variables: Vec<[Variable; VIEW_TYPES.len()]> = (0..function.registers)
            .map(|_| VIEW_TYPES.map(|ty| builder.declare_var(scalar_ir(ty))))
            .collect();
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let context = parameters[0];
        let mut passed = parameters[1..].iter().copied();
        // A constant base folds into each access's addressing mode and
        // frees the register a parameter would occupy.
        let base = match fixed_base {
            Some(fixed) => builder.ins().iconst(types::I64, i64::from(fixed)),
            None => passed.next().expect("a passed base"),
        };
        let length = memory
            .length
            .then(|| passed.next().expect("a passed length"));
        let wasm_parameters = &parameters[1 + memory.count()..];
        let regions = Region::ALL.map(|region| {
            builder.func.dfg.alias_regions.insert(ir::AliasRegionData {
                user_id: region as u32,
                description: format!("{region:?}").into(),
            })
        });
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        let mut selectors: Vec<u16> = instructions
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| {
                plan.reachable(*pc) && instruction.op() == Op::WasmBranchTable
            })
            .map(|(_, instruction)| instruction.a())
            .collect();
        selectors.sort_unstable();
        selectors.dedup();
        let mut emitter = Emitter {
            guarded,
            wraps,
            fixed_base,
            regions,
            globals,
            analysis: self,
            builder,
            plan,
            variables,
            blocks: vec![None; function.code.len()],
            context,
            base,
            length,
            memory,
            trap_blocks: [None; TRAPS.len()],
            termination_block: None,
            callees: vec![None; plans.len()],
            callout_signature: None,
            plans,
            call_conv,
            instructions: &instructions,
            selectors,
            known: Vec::new(),
            threads: Vec::new(),
            pending: Vec::new(),
            redirect: None,
            threading: true,
        };
        // Registers start at i32 zero, as an activation's register file does.
        for register in 0..function.registers {
            emitter.write_constant(register, Kind::I32, 0);
            emitter.write_constant(register, Kind::I64, 0);
        }
        let parameter_base = function.parameter_registers.expect("planned");
        for (offset, ty) in plan.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            emitter.write(
                parameter_base + offset as u16,
                kind,
                wasm_parameters[offset],
            );
        }
        // Each activation checks the stack it may still use.
        let stack_pointer = emitter.builder.ins().get_stack_pointer(types::I64);
        let limit =
            emitter
                .builder
                .ins()
                .load(types::I64, invariant(), context, CONTEXT_STACK_LIMIT);
        let exhausted = emitter
            .builder
            .ins()
            .icmp(IntCC::UnsignedLessThan, stack_pointer, limit);
        emitter.trap_if(exhausted, crate::WasmTrap::CallStackExhausted);
        emitter.known.clear();
        emitter.jump(0);
        // Every jump target starts a block, created before emission so a
        // backward jump finds its target's block already placed.
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                continue;
            }
            let targets = match instruction.op().control_flow_layout() {
                ControlFlowLayout::BranchTable => {
                    crate::bytecode::branch_table_entries(pc, instruction)
                        .map(|entry| instructions[entry].jump_target() as usize)
                        .collect()
                }
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    vec![instruction.jump_target() as usize, pc + 1]
                }
                _ => vec![],
            };
            for target in targets {
                if target < instructions.len() && plan.reachable(target) {
                    emitter.block(target);
                }
            }
        }
        let mut open = false;
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if !plan.reachable(pc) {
                open = false;
                continue;
            }
            let is_leader = emitter.blocks[pc].is_some() || !open;
            if is_leader {
                emitter.known.clear();
                let block = emitter.block(pc);
                if open {
                    emitter.builder.ins().jump(block, &[]);
                }
                emitter.builder.switch_to_block(block);
            }
            open = emitter.instruction(function, pc, instruction);
            // A later jump may target the next instruction; give it a block
            // now so falling into it stays explicit.
            if open && emitter.blocks.get(pc + 1).is_some_and(Option::is_some) {
                let next = emitter.block(pc + 1);
                emitter.builder.ins().jump(next, &[]);
                open = false;
            }
        }
        if open {
            return None;
        }
        emitter.threading = false;
        for (block, thread) in std::mem::take(&mut emitter.pending) {
            emitter.emit_thread(function, block, &thread);
        }
        emitter.finish_traps();
        emitter.builder.seal_all_blocks();
        emitter.builder.finalize(isa.frontend_config());
        let mut context = cranelift_codegen::Context::for_function(func);
        let compiled = context
            .compile(
                isa,
                &mut cranelift_codegen::control::ControlPlane::default(),
            )
            .ok()?;
        let code = compiled.code_buffer().to_vec();
        let relocations = compiled.buffer.relocs().to_vec();
        let names = context.func.params.user_named_funcs();
        let calls = relocations
            .iter()
            .map(|relocation| {
                let cranelift_codegen::FinalizedRelocTarget::ExternalName(ir::ExternalName::User(
                    name,
                )) = relocation.target
                else {
                    return None;
                };
                let name = names.get(name)?;
                (name.namespace == FUNCTION_NAMESPACE).then_some(Call {
                    offset: relocation.offset as usize,
                    kind: relocation.kind,
                    addend: relocation.addend,
                    callee: name.index as usize,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some((code, calls))
    }

    /// The C entry of `index`: unpack arguments, call through the table and
    /// store the result.
    fn compile_entry(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
        memory: MemoryArguments,
    ) -> Option<Vec<u8>> {
        let call_conv = isa.default_call_conv();
        let mut signature = Signature::new(call_conv);
        for _ in 0..5 {
            signature.params.push(AbiParam::new(types::I64));
        }
        signature.returns.push(AbiParam::new(types::I32));
        let mut func = ir::Function::with_name_signature(
            UserFuncName::user(ENTRY_NAMESPACE, index as u32),
            signature,
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        builder.seal_block(block);
        let parameters = builder.block_params(block).to_vec();
        let (context, base, length, args, out) = (
            parameters[0],
            parameters[1],
            parameters[2],
            parameters[3],
            parameters[4],
        );
        let mut call_args = vec![context];
        call_args.extend(memory.select(base, length));
        for (offset, ty) in plan.params.iter().enumerate() {
            let raw = builder.ins().load(
                types::I64,
                MemFlagsData::trusted(),
                args,
                (offset * 8) as i32,
            );
            call_args.push(match Kind::of(*ty).expect("planned") {
                Kind::I32 => builder.ins().ireduce(types::I32, raw),
                Kind::I64 => raw,
            });
        }
        let target_signature = builder.import_signature(internal_signature(
            memory,
            &plan.params,
            plan.result,
            call_conv,
        ));
        let table = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), context, CONTEXT_TABLE);
        let target = builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            table,
            (index * 8) as i32,
        );
        let call = builder
            .ins()
            .call_indirect(target_signature, target, &call_args);
        let results = builder.inst_results(call).to_vec();
        if let Some(ty) = plan.result {
            let value = match Kind::of(ty).expect("planned") {
                Kind::I32 => builder.ins().uextend(types::I64, results[1]),
                Kind::I64 => results[1],
            };
            builder.ins().store(MemFlagsData::trusted(), value, out, 0);
        }
        let status = builder.ins().ireduce(types::I32, results[0]);
        builder.ins().return_(&[status]);
        builder.finalize(isa.frontend_config());
        let mut context = cranelift_codegen::Context::for_function(func);
        let compiled = context
            .compile(
                isa,
                &mut cranelift_codegen::control::ControlPlane::default(),
            )
            .ok()?;
        if !compiled.buffer.relocs().is_empty() {
            return None;
        }
        Some(compiled.code_buffer().to_vec())
    }
}

/// The guard reservation of an unshared memory binding.
/// A memory base an x86-64 addressing mode can carry as its signed 32-bit
/// displacement.
fn displacement_base(base: *mut u8) -> Option<i32> {
    i32::try_from(base as usize).ok()
}

fn guarded_memory<H: Host>(vm: &Vm<H>, binding: Value) -> Option<(*mut u8, usize)> {
    memory_bytes(vm, binding, crate::wasm::memory::LinearBytes::guard)
}

/// Whether the guarded memory `binding` names repeats above 2^32.
fn wrapping_memory<H: Host>(vm: &Vm<H>, binding: Value) -> bool {
    memory_bytes(vm, binding, |bytes| Some(bytes.wraps())) == Some(true)
}

/// `query` of the bytes of the unshared memory `binding` names.
fn memory_bytes<H: Host, T>(
    vm: &Vm<H>,
    binding: Value,
    query: impl FnOnce(&crate::wasm::memory::LinearBytes) -> Option<T>,
) -> Option<T> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return None;
    };
    let crate::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
        return None;
    };
    query(&state.try_borrow().ok()?.bytes)
}

thread_local! {
    /// The context of the compiled call running on this thread, which a
    /// guard fault resumes from.
    static ACTIVE_CONTEXT: std::cell::Cell<*mut CallContext> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Enters compiled code: (context, base, length, arguments, result, entry)
/// -> status. It saves the caller's callee-saved registers, records its
/// stack pointer in the context, and calls `entry`; a guard fault resumes at
/// its landing, which returns the out-of-bounds status through the same
/// epilogue.
type Trampoline =
    unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64, Entry) -> u32;

/// The trampoline and its landing, once the fault handler is installed.
struct GuardSupport {
    _code: CodeMemory,
    trampoline: Trampoline,
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    landing: usize,
}

// SAFETY: immutable executable code, shared by every thread.
unsafe impl Send for GuardSupport {}
unsafe impl Sync for GuardSupport {}

/// The x86-64 System V trampoline, as machine code. Offsets of `RESUME` and
/// `LANDING` are into this array.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_CODE: [u8; 50] = [
    0x55, // push rbp
    0x48,
    0x89,
    0xe5, // mov rbp, rsp
    0x53, // push rbx
    0x41,
    0x54, // push r12
    0x41,
    0x55, // push r13
    0x41,
    0x56, // push r14
    0x41,
    0x57, // push r15
    0x48,
    0x83,
    0xec,
    0x08, // sub rsp, 8 (keeps the call aligned)
    0x48,
    0x89,
    0x67,
    CONTEXT_ENTRY_STACK, // mov [rdi + entry_stack], rsp
    0x41,
    0xff,
    0xd1, // call r9
    // RESUME (24):
    0x48,
    0x83,
    0xc4,
    0x08, // add rsp, 8
    0x41,
    0x5f, // pop r15
    0x41,
    0x5e, // pop r14
    0x41,
    0x5d, // pop r13
    0x41,
    0x5c, // pop r12
    0x5b, // pop rbx
    0x5d, // pop rbp
    0xc3, // ret
    // LANDING (39):
    0xb8,
    0,
    0,
    0,
    0, // mov eax, status (patched)
    0xe9,
    0,
    0,
    0,
    0,    // jmp RESUME (patched)
    0xcc, // int3
];

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_RESUME: usize = 24;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const TRAMPOLINE_LANDING: usize = 39;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
static PREVIOUS_SEGV: std::sync::OnceLock<libc::sigaction> = std::sync::OnceLock::new();

/// The guard support of this host, installing the fault handler on first
/// use; `None` where guard faults cannot be resumed.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn guard_support() -> Option<&'static GuardSupport> {
    static SUPPORT: std::sync::OnceLock<Option<GuardSupport>> = std::sync::OnceLock::new();
    SUPPORT
        .get_or_init(|| {
            let mut code = TRAMPOLINE_CODE;
            let status = trap_status(crate::WasmTrap::OutOfBoundsMemory) as u32;
            code[TRAMPOLINE_LANDING + 1..TRAMPOLINE_LANDING + 5]
                .copy_from_slice(&status.to_le_bytes());
            let after_jump = TRAMPOLINE_LANDING + 10;
            let displacement = TRAMPOLINE_RESUME as i32 - after_jump as i32;
            code[TRAMPOLINE_LANDING + 6..TRAMPOLINE_LANDING + 10]
                .copy_from_slice(&displacement.to_le_bytes());
            let memory = CodeMemory::new(&code)?;
            // SAFETY: the code implements `Trampoline` for this ABI.
            let trampoline = unsafe { std::mem::transmute::<*mut u8, Trampoline>(memory.pointer) };
            let landing = memory.pointer as usize + TRAMPOLINE_LANDING;
            // SAFETY: installs a handler that only redirects faults inside an
            // active compiled call's guard and forwards every other fault to
            // the previous handler.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = guard_fault as *const () as usize;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER;
                libc::sigemptyset(&mut action.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(libc::SIGSEGV, &action, &mut previous) != 0 {
                    return None;
                }
                let _ = PREVIOUS_SEGV.set(previous);
            }
            Some(GuardSupport {
                _code: memory,
                trampoline,
                landing,
            })
        })
        .as_ref()
}

#[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
fn guard_support() -> Option<&'static GuardSupport> {
    None
}

fn guard_trampoline() -> Option<Trampoline> {
    guard_support().map(|support| support.trampoline)
}

/// A fault inside the running compiled call's guard reservation resumes at
/// the trampoline's landing with the trampoline's saved stack; anything else
/// belongs to the previous handler.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
extern "C" fn guard_fault(signal: i32, info: *mut libc::siginfo_t, context: *mut libc::c_void) {
    let active = ACTIVE_CONTEXT.with(std::cell::Cell::get);
    // SAFETY: the kernel passes valid signal information and context.
    unsafe {
        let address = (*info).si_addr() as usize;
        if !active.is_null()
            && (*active).guard_start <= address
            && address < (*active).guard_end
            && let Some(support) = guard_support()
        {
            let context = &mut *context.cast::<libc::ucontext_t>();
            context.uc_mcontext.gregs[libc::REG_RSP as usize] = (*active).entry_stack as i64;
            context.uc_mcontext.gregs[libc::REG_RIP as usize] = support.landing as i64;
            return;
        }
        let Some(previous) = PREVIOUS_SEGV.get() else {
            return;
        };
        match previous.sa_sigaction {
            libc::SIG_DFL | libc::SIG_IGN => {
                // Restore the previous disposition; the fault repeats on return.
                libc::sigaction(libc::SIGSEGV, previous, std::ptr::null_mut());
            }
            handler if previous.sa_flags & libc::SA_SIGINFO != 0 => {
                let handler = std::mem::transmute::<
                    usize,
                    extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void),
                >(handler);
                handler(signal, info, context);
            }
            handler => {
                let handler = std::mem::transmute::<usize, extern "C" fn(i32)>(handler);
                handler(signal);
            }
        }
    }
}

/// The name namespace of compiled function bodies; their C entries use
/// `ENTRY_NAMESPACE`.
const FUNCTION_NAMESPACE: u32 = 0;
const ENTRY_NAMESPACE: u32 = 1;

fn function_name(index: usize) -> ir::UserExternalName {
    ir::UserExternalName::new(FUNCTION_NAMESPACE, index as u32)
}

/// A direct call in a function body, linked once bodies are placed.
struct Call {
    /// The relocated field, from the start of the body.
    offset: usize,
    kind: cranelift_codegen::binemit::Reloc,
    addend: i64,
    callee: usize,
}

/// Point `call` in the body at `body` to the callee placed at `callee`; all
/// are offsets in one buffer, so the relative displacement is final.
fn link(bytes: &mut [u8], body: usize, call: &Call, callee: usize) -> Option<()> {
    use cranelift_codegen::binemit::Reloc;
    let site = body + call.offset;
    let displacement = callee as i64 - site as i64 + call.addend;
    match call.kind {
        Reloc::X86CallPCRel4 => {
            let field = i32::try_from(displacement).ok()?;
            bytes
                .get_mut(site..site + size_of::<i32>())?
                .copy_from_slice(&field.to_le_bytes());
        }
        Reloc::Arm64Call => {
            /// A BL immediate: a signed word displacement in the low bits.
            const IMMEDIATE_BITS: u32 = 26;
            const WORD_SHIFT: u32 = 2;
            let words = displacement >> WORD_SHIFT;
            let range = 1i64 << (IMMEDIATE_BITS - 1);
            if displacement & ((1 << WORD_SHIFT) - 1) != 0 || !(-range..range).contains(&words) {
                return None;
            }
            let field = bytes.get_mut(site..site + size_of::<u32>())?;
            let mask = (1u32 << IMMEDIATE_BITS) - 1;
            let instruction = u32::from_le_bytes(field.try_into().ok()?);
            let instruction = (instruction & !mask) | (words as u32 & mask);
            field.copy_from_slice(&instruction.to_le_bytes());
        }
        _ => return None,
    }
    Some(())
}

/// Code alignment for each compiled function.
const FUNCTION_ALIGNMENT: usize = 64;

fn target_isa() -> Option<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    flags.set("enable_verifier", "false").ok()?;
    let builder = cranelift_native::builder().ok()?;
    builder.finish(settings::Flags::new(flags)).ok()
}

impl<H: Host> Vm<H> {
    pub(crate) fn set_execution_mode(&mut self, mode: ExecutionMode) {
        self.execution_mode = mode;
    }

    pub(crate) fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Compile every function of `program` that has a compiled denotation,
    /// with `env` as the instance whose bindings fix global types and memory.
    fn compile_program(&self, program: ProgramId, env: Value) -> Option<CompiledProgram> {
        let code = self.programs.get(program)?;
        if code.kind != crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        let mut analysis = Analysis {
            vm: self,
            program,
            code: &code,
            env,
            callouts: true,
        };
        let count = code.functions.len();
        // The callable set is the greatest fixpoint: assume every function
        // compiles, drop those that cannot, until nothing changes.
        let mut callable;
        let mut plans: Vec<Option<Plan>>;
        let mut memory_slot;
        let mut guarded;
        loop {
            callable = vec![true; count];
            loop {
                memory_slot = None;
                plans = (0..count)
                    .map(|index| {
                        if callable[index] {
                            analysis
                                .plan(index, &callable, &mut memory_slot)
                                .map(Plan::interface)
                        } else {
                            None
                        }
                    })
                    .collect();
                let next: Vec<bool> = plans.iter().map(Option::is_some).collect();
                if next == callable {
                    break;
                }
                callable = next;
            }
            // Accesses may rely on the guard when the memory has one and
            // this host resumes guard faults.
            guarded = guard_trampoline().is_some()
                && memory_slot.is_some_and(|slot| {
                    self.heap
                        .environment_slot(env, slot as usize)
                        .and_then(|binding| guarded_memory(self, binding))
                        .is_some()
                });
            // Callouts need a memory whose bytes stay put.
            if !analysis.callouts || guarded || memory_slot.is_none() {
                break;
            }
            analysis.callouts = false;
        }
        if plans.iter().all(Option::is_none) {
            return None;
        }
        let fixed_base = guarded
            .then(|| {
                let binding = self.heap.environment_slot(env, memory_slot? as usize)?;
                displacement_base(guarded_memory(self, binding)?.0)
            })
            .flatten();
        let wraps = guarded
            && memory_slot
                .and_then(|slot| self.heap.environment_slot(env, slot as usize))
                .is_some_and(|binding| wrapping_memory(self, binding));
        let isa = target_isa()?;
        // The full plan of a callable function, derived again where needed.
        let plan = |index: usize| analysis.plan(index, &callable, &mut memory_slot.clone());
        // The globals compiled code accesses, each once.
        let mut globals: Vec<u32> = Vec::new();
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let plan = plan(index)?;
            let function = &code.functions[index];
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                if !matches!(instruction.op(), Op::WasmGlobalGet | Op::WasmGlobalSet) {
                    continue;
                }
                if let Some(Slot::Binding(slot)) = plan.operands[pc]
                    && !globals.contains(&slot)
                {
                    globals.push(slot);
                }
            }
        }
        let mut bytes = Vec::new();
        let mut offsets = vec![None; count];
        let mut entries = vec![None; count];
        let mut calls = Vec::new();
        let place = |bytes: &mut Vec<u8>, code: Vec<u8>| {
            bytes.resize(bytes.len().next_multiple_of(FUNCTION_ALIGNMENT), 0);
            let offset = bytes.len();
            bytes.extend_from_slice(&code);
            offset
        };
        for index in 0..count {
            if plans[index].is_none() {
                continue;
            }
            let (body, function_calls) = analysis.compile_function(
                &*isa,
                index,
                &plan(index)?,
                &plans,
                guarded,
                wraps,
                fixed_base,
                &globals,
            )?;
            let offset = place(&mut bytes, body);
            offsets[index] = Some(offset);
            calls.extend(function_calls.into_iter().map(|call| (offset, call)));
            let memory = MemoryArguments::of(fixed_base, guarded);
            let entry = analysis.compile_entry(&*isa, index, plans[index].as_ref()?, memory)?;
            entries[index] = Some(place(&mut bytes, entry));
        }
        for (offset, call) in calls {
            link(&mut bytes, offset, &call, offsets[call.callee]?)?;
        }
        let memory = CodeMemory::new(&bytes)?;
        let base = memory.pointer as usize;
        let table: Box<[usize]> = offsets
            .iter()
            .map(|offset| offset.map_or(0, |offset| base + offset))
            .collect();
        let functions = plans
            .into_iter()
            .zip(entries)
            .map(|(plan, entry)| {
                let plan = plan?;
                let entry = entry?;
                // SAFETY: the entry was compiled with the `Entry` signature in
                // the host's default calling convention.
                let entry = unsafe { std::mem::transmute::<usize, Entry>(base + entry) };
                Some(CompiledFunction {
                    entry,
                    params: plan.params,
                    result: plan.result,
                })
            })
            .collect();
        Some(CompiledProgram {
            _code: memory,
            table,
            functions,
            memory_slot,
            guarded,
            fixed_base,
            wraps,
            globals,
        })
    }

    /// Run known function `id` of the caller's program as native code, with
    /// arguments from the caller's register `window`. `None` leaves the call
    /// to the interpreter.
    pub(super) fn call_compiled(
        &mut self,
        caller: usize,
        id: u32,
        window: crate::bytecode::RegisterWindow,
    ) -> Option<Result<Value, JsError>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let frame = &self.frames[caller];
        let (program, env) = (frame.program, frame.env);
        let base = usize::from(window.base);
        let args: smallvec::SmallVec<[Value; 8]> = frame
            .registers
            .get(base..base + usize::from(window.count))?
            .iter()
            .copied()
            .collect();
        self.run_compiled(program, env, id, &args)
    }

    /// Run function `id` of `program` in instance `env` as native code, with
    /// encoded Wasm `args`. `None` leaves the call to the interpreter, which
    /// is then exactly where the call would otherwise have gone.
    pub(super) fn run_compiled(
        &mut self,
        program: ProgramId,
        env: Value,
        id: u32,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        if self.execution_mode != ExecutionMode::Compiled {
            return None;
        }
        let compiled = match self.programs.compiled(program) {
            Some(compiled) => compiled?,
            None => {
                let compiled = self.compile_program(program, env).map(Rc::new);
                self.programs.set_compiled(program, compiled.clone());
                compiled?
            }
        };
        let function = compiled.functions.get(id as usize)?.as_ref()?;
        if args.len() != function.params.len() {
            return None;
        }
        let (base, length, guard) = match compiled.memory_slot {
            Some(slot) => {
                let binding = self.heap.environment_slot(env, slot as usize)?;
                let view = super::dispatch_fast::unshared_memory(self, binding)?;
                let guard = guarded_memory(self, binding);
                // Code compiled without bounds checks needs this memory's guard.
                if compiled.guarded && guard.is_none() {
                    return None;
                }
                if compiled.wraps && !wrapping_memory(self, binding) {
                    return None;
                }
                if compiled.fixed_base.is_some_and(|fixed| {
                    guard.map(|(base, _)| displacement_base(base)) != Some(Some(fixed))
                }) {
                    return None;
                }
                (view.0 as u64, view.1 as u64, guard)
            }
            None => (0, 0, None),
        };
        let mut raw: smallvec::SmallVec<[u64; 8]> = smallvec::SmallVec::new();
        for (value, ty) in args.iter().zip(&function.params) {
            raw.push(match Kind::of(*ty)? {
                Kind::I32 => u64::from(value.wasm_bits32()),
                Kind::I64 => self.decode_bits64(*value)?,
            });
        }
        let marker = 0u8;
        let stack_pointer = std::hint::black_box(&marker as *const u8 as usize);
        let mut globals: smallvec::SmallVec<[*mut Value; 8]> = smallvec::SmallVec::new();
        for slot in &compiled.globals {
            let binding = self.heap.environment_slot(env, *slot as usize)?;
            match self.heap.get_mut(binding)? {
                Cell::WasmGlobal { value, .. } => globals.push(value),
                _ => return None,
            }
        }
        let (guard_start, guard_end) = guard.map_or((0, 0), |(start, size)| {
            (start as usize, start as usize + size)
        });
        let mut context = CallContext {
            stack_limit: stack_pointer.saturating_sub(COMPILED_STACK_BUDGET),
            interrupt: self.execution_interrupt.atomic_flag(),
            table: compiled.table.as_ptr(),
            vm: (self as *mut Self).cast(),
            env,
            entry_stack: 0,
            guard_start,
            guard_end,
            globals: globals.as_ptr(),
            callout: import_callout::<H> as Callout as usize,
            program,
            raised: None,
        };
        let mut out = 0u64;
        let status = match guard_trampoline() {
            Some(trampoline) => {
                // Compiled calls nest only through the interpreter, so the
                // outer context returns when this call does.
                let outer = ACTIVE_CONTEXT.with(|active| active.replace(&mut context));
                // SAFETY: as below; the trampoline saves the caller's registers
                // and records where a guard fault resumes.
                let status = unsafe {
                    trampoline(
                        &mut context,
                        base,
                        length,
                        raw.as_ptr(),
                        &mut out,
                        function.entry,
                    )
                };
                ACTIVE_CONTEXT.with(|active| active.set(outer));
                status
            }
            // SAFETY: the entry is compiled code for this function's
            // signature; the context, arguments and result outlive the call,
            // and the memory view stays valid because compiled code never
            // grows memory.
            None => unsafe { (function.entry)(&mut context, base, length, raw.as_ptr(), &mut out) },
        };
        if i64::from(status) == TERMINATED_STATUS {
            return Some(Err(JsError::execution_terminated()));
        }
        if i64::from(status) == RAISED_STATUS {
            return match context.raised.take()? {
                Raised::Error(error) => Some(Err(error)),
                Raised::Panic(panic) => std::panic::resume_unwind(panic),
            };
        }
        if status != 0 {
            let trap = TRAPS[status as usize - 1];
            return Some(Err(JsError::wasm_trap_error(trap)));
        }
        Some(Ok(match function.result {
            Some(ty) => match Kind::of(ty)? {
                Kind::I32 => Value::integer(out as u32 as i32),
                Kind::I64 => self.encode_bits64(out),
            },
            None => self.programs.constant(program, VOID_RESULT_CONSTANT)?,
        }))
    }
}

impl<H: Host> Vm<H> {
    /// Call imported `function` of `program` in instance `env` with the Wasm
    /// bits at `args`, storing its result's bits at `out`.
    ///
    /// # Safety
    /// `args` holds a word per parameter of the import and `out` one word.
    unsafe fn call_import(
        &mut self,
        program: ProgramId,
        env: Value,
        function: u32,
        args: *const u64,
        out: *mut u64,
    ) -> Result<(), JsError> {
        let code = self
            .programs
            .get(program)
            .ok_or_else(|| JsError::validation("missing compiled Wasm program".into()))?;
        let signature = self
            .programs
            .wasm_signatures(program)
            .and_then(|signatures| signatures.get(function as usize))
            .cloned()
            .ok_or_else(|| JsError::validation("missing Wasm import signature".into()))?;
        let active_program = self.replace_active_program(program);
        let callee = self.wasm_function_reference(&code, function, env);
        self.set_active_program(active_program);
        let callee = callee?;
        // Allocation never collects, so the encoded arguments stay live until
        // the call roots them.
        let mut values: smallvec::SmallVec<[Value; 8]> = smallvec::SmallVec::new();
        for (offset, ty) in signature.params.iter().enumerate() {
            // SAFETY: the caller passes a word per parameter.
            let bits = unsafe { *args.add(offset) };
            values.push(self.encode_wasm_value(crate::WasmValue::from_bits(*ty, bits)));
        }
        let receiver = self
            .programs
            .constant(program, VOID_RESULT_CONSTANT)
            .ok_or_else(|| JsError::validation("missing Wasm void constant".into()))?;
        let result = self.call_value(&code, callee, receiver, &values)?;
        if let Some(ty) = signature.results.first() {
            let bits = match self.decode_wasm_value(result, *ty)?.bits() {
                Some(crate::wasm::ScalarBits::Bits32(bits)) => u64::from(bits),
                Some(crate::wasm::ScalarBits::Bits64(bits)) => bits,
                _ => return Err(JsError::validation("non-scalar Wasm import result".into())),
            };
            // SAFETY: the caller passes a result word.
            unsafe { *out = bits };
        }
        Ok(())
    }
}

/// The tag bits of an encoded i32, which a global's value carries.
const INTEGER_TAG: u64 = crate::value::INTEGER_TAG_BITS;

/// The constant a void Wasm function returns (see the Wasm lowering).
const VOID_RESULT_CONSTANT: usize = 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// Each i32 integer operator widens to its namesake, and each f32 float
    /// operator to its f64 namesake.
    #[test]
    fn narrow_selectors_name_their_wide_namesakes() {
        let mut tag = 0;
        while let Some(operator) = I32BinaryOperator::from_tag(tag) {
            assert_eq!(
                format!("{operator:?}"),
                format!("{:?}", widen_binary(operator))
            );
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = I32UnaryOperator::from_tag(tag) {
            assert_eq!(
                format!("{operator:?}"),
                format!("{:?}", widen_unary(operator))
            );
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = F32BinaryOperator::from_tag(tag) {
            let (wide, _) = float_binary_operator(Op::WasmF32Binary, tag).expect("f32 selector");
            assert_eq!(format!("{operator:?}"), format!("{wide:?}"));
            tag += 1;
        }
        let mut tag = 0;
        while let Some(operator) = F32UnaryOperator::from_tag(tag) {
            let (wide, _) = float_unary_operator(Op::WasmF32Unary, tag).expect("f32 selector");
            assert_eq!(format!("{operator:?}"), format!("{wide:?}"));
            tag += 1;
        }
    }
}
