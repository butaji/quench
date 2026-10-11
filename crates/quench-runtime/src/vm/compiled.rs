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
//! `I32BinaryOperator`/`I32UnaryOperator`, memory accesses from
//! `MemoryLoad`/`MemoryStore`, traps from `WasmTrap`. A function compiles only
//! when every instruction it can reach has a compiled denotation and every
//! function it calls compiles; anything else stays interpreted, so the mode
//! never changes behavior, only speed. Compiled code never allocates, calls
//! the host or collects, so it holds no heap roots; values cross its boundary
//! in their Wasm encoding, and a trap leaves as a status that becomes the
//! interpreter's `WasmTrap`.

use super::*;
use crate::bytecode::{ControlFlowLayout, Function, WideInstruction};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, MemFlagsData, Signature, UserFuncName, condcodes::IntCC, types,
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

/// A value's machine representation inside compiled code: f32 bits travel
/// as i32 and f64 bits as i64, since compiled code only moves them.
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

    fn index(self) -> usize {
        self as usize
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
const TRAPS: [crate::WasmTrap; 4] = [
    crate::WasmTrap::OutOfBoundsMemory,
    crate::WasmTrap::IntegerDivideByZero,
    crate::WasmTrap::IntegerOverflow,
    crate::WasmTrap::CallStackExhausted,
];

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
    table: *const usize,
    vm: *mut (),
    env: u64,
    /// The entry trampoline's stack pointer after it saved the caller's
    /// registers; a guard fault resumes there.
    entry_stack: usize,
    /// The guarded memory reservation compiled code may fault in.
    guard_start: usize,
    guard_end: usize,
    /// Per global the program accesses (`CompiledProgram::globals`), the
    /// address of the global's value in this instance.
    globals: *const *mut Value,
}

const CONTEXT_STACK_LIMIT: i32 = 0;
const CONTEXT_TABLE: i32 = 8;
const CONTEXT_ENTRY_STACK: u8 = 32;
const CONTEXT_GLOBALS: i32 = 56;

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
    /// The register state on entry to each reachable instruction.
    states: Vec<Option<Vec<Slot>>>,
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
}

impl<H: Host> Analysis<'_, H> {
    fn constant(&self, index: usize) -> Option<Value> {
        self.vm.programs.constant(self.program, index)
    }

    /// The machine kind of a constant: i32-encoded scalars, boxed 64-bit
    /// scalars, and any other constant (the void result) as an inert i32.
    fn constant_kind(&self, value: Value) -> Kind {
        match self.vm.heap.get(value) {
            Some(Cell::WasmBits64(_)) => Kind::I64,
            _ => Kind::I32,
        }
    }

    fn constant_bits(&self, value: Value) -> i64 {
        match self.vm.heap.get(value) {
            Some(Cell::WasmBits64(bits)) => *bits as i64,
            _ => i64::from(value.wasm_bits32() as i32),
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
        Some(Plan {
            params,
            result,
            states,
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
            return set(state, instruction.a(), Slot::Value(load_kind(load)?));
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
        match op {
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

/// The machine kind a direct load produces.
fn load_kind(load: MemoryLoad) -> Option<Kind> {
    Some(match load {
        MemoryLoad::I32Load
        | MemoryLoad::F32Load
        | MemoryLoad::I32Load8S
        | MemoryLoad::I32Load8U
        | MemoryLoad::I32Load16S
        | MemoryLoad::I32Load16U => Kind::I32,
        MemoryLoad::I64Load
        | MemoryLoad::F64Load
        | MemoryLoad::I64Load8S
        | MemoryLoad::I64Load8U
        | MemoryLoad::I64Load16S
        | MemoryLoad::I64Load16U
        | MemoryLoad::I64Load32S
        | MemoryLoad::I64Load32U => Kind::I64,
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

/// The internal signature of a compiled function.
fn internal_signature(
    plan_params: &[crate::WasmType],
    result: Option<crate::WasmType>,
    call_conv: cranelift_codegen::isa::CallConv,
) -> Signature {
    let mut signature = Signature::new(call_conv);
    for _ in 0..3 {
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


struct Emitter<'a, 'b, H: Host> {
    guarded: bool,
    globals: &'a [u32],
    analysis: &'a Analysis<'a, H>,
    builder: FunctionBuilder<'b>,
    plan: &'a Plan,
    variables: Vec<[Variable; 2]>,
    blocks: Vec<Option<ir::Block>>,
    context: ir::Value,
    base: ir::Value,
    length: ir::Value,
    trap_blocks: [Option<ir::Block>; TRAPS.len()],
    signatures: Vec<Option<ir::SigRef>>,
    plans: &'a [Option<Plan>],
    call_conv: cranelift_codegen::isa::CallConv,
}

impl<H: Host> Emitter<'_, '_, H> {
    fn read(&mut self, register: u16, kind: Kind) -> ir::Value {
        self.builder
            .use_var(self.variables[usize::from(register)][kind.index()])
    }

    fn write(&mut self, register: u16, kind: Kind, value: ir::Value) {
        self.builder
            .def_var(self.variables[usize::from(register)][kind.index()], value);
    }

    fn kind_at(&self, pc: usize, register: u16) -> Kind {
        match self.plan.states[pc].as_ref().expect("reachable")[usize::from(register)] {
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

    fn finish_traps(&mut self) {
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

    fn i32_binary(
        &mut self,
        operator: I32BinaryOperator,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        use I32BinaryOperator as B;
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
                        let minimum =
                            self.builder
                                .ins()
                                .icmp_imm_s(IntCC::Equal, left, i64::from(i32::MIN));
                        let negative_one = self.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let overflow = self.builder.ins().band(minimum, negative_one);
                        self.trap_if(overflow, crate::WasmTrap::IntegerOverflow);
                        self.builder.ins().sdiv(left, right)
                    }
                    _ => {
                        // `i32::MIN % -1` is 0; the machine division would fault.
                        let negative_one = self.builder.ins().icmp_imm_s(IntCC::Equal, right, -1);
                        let one = self.builder.ins().iconst(types::I32, 1);
                        let divisor = self.builder.ins().select(negative_one, one, right);
                        self.builder.ins().srem(left, divisor)
                    }
                }
            }
        }
    }

    fn i32_unary(&mut self, operator: I32UnaryOperator, value: ir::Value) -> ir::Value {
        use I32UnaryOperator as U;
        match operator {
            U::EqualZero => {
                let flag = self.builder.ins().icmp_imm_s(IntCC::Equal, value, 0);
                self.builder.ins().uextend(types::I32, flag)
            }
            U::LeadingZeros => self.builder.ins().clz(value),
            U::TrailingZeros => self.builder.ins().ctz(value),
            U::PopulationCount => self.builder.ins().popcnt(value),
            U::ExtendSigned8 => {
                let narrow = self.builder.ins().ireduce(types::I8, value);
                self.builder.ins().sextend(types::I32, narrow)
            }
            U::ExtendSigned16 => {
                let narrow = self.builder.ins().ireduce(types::I16, value);
                self.builder.ins().sextend(types::I32, narrow)
            }
        }
    }

    /// The host address of a checked `width`-byte access at `address + offset`.
    fn address(&mut self, address: ir::Value, offset: u32, width: usize) -> ir::Value {
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
        let out_of_bounds = self
            .builder
            .ins()
            .icmp(IntCC::UnsignedGreaterThan, end, self.length);
        self.trap_if(out_of_bounds, crate::WasmTrap::OutOfBoundsMemory);
        let host = self.builder.ins().iadd(self.base, address);
        self.builder.ins().iadd_imm_s(host, i64::from(offset))
    }

    /// The address of the value of the global bound at `slot`.
    fn global_address(&mut self, slot: u32) -> ir::Value {
        let position = self
            .globals
            .iter()
            .position(|candidate| *candidate == slot)
            .expect("planned globals are collected");
        let table = self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            self.context,
            CONTEXT_GLOBALS,
        );
        self.builder.ins().load(
            types::I64,
            MemFlagsData::trusted().with_readonly(),
            table,
            (position * 8) as i32,
        )
    }

    fn binding_slot(&self, pc: usize, register: u16) -> u32 {
        match self.plan.states[pc].as_ref().expect("reachable")[usize::from(register)] {
            Slot::Binding(slot) => slot,
            _ => unreachable!("planned global accesses read a binding"),
        }
    }

    fn jump(&mut self, target: usize) {
        let block = self.block(target);
        self.builder.ins().jump(block, &[]);
    }

    fn branch(&mut self, condition: ir::Value, target: usize, fallthrough: usize) {
        let taken = self.block(target);
        let next = self.block(fallthrough);
        self.builder.ins().brif(condition, taken, &[], next, &[]);
    }

    /// Emit the instruction at `pc`; returns whether control falls through.
    fn instruction(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
    ) -> bool {
        let op = instruction.op();
        let (a, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if let Some(operator) = I32BinaryOperator::from_register_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self.read(c, Kind::I32);
            let value = self.i32_binary(operator, left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_op(op) {
            let left = self.read(b, Kind::I32);
            let right = self
                .builder
                .ins()
                .iconst(types::I32, i64::from(instruction.imm() as i32));
            let value = self.i32_binary(operator, left, right);
            self.write(a, Kind::I32, value);
            return true;
        }
        if let Some(operator) = I32BinaryOperator::from_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.read(b, Kind::I32);
            let holds = self.i32_binary(operator, left, right);
            self.branch(holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(operator) = I32BinaryOperator::from_immediate_jump_op(op) {
            let left = self.read(a, Kind::I32);
            let right = self.builder.ins().iconst(types::I32, i64::from(b as i16));
            let holds = self.i32_binary(operator, left, right);
            self.branch(holds, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            let kind = load_kind(load).expect("planned");
            let address = self.read(c, Kind::I32);
            let host = self.address(address, instruction.imm(), load.width());
            let flags = MemFlagsData::trusted().with_endianness(ir::Endianness::Little);
            let ty = kind.ir();
            let value = match (load.width(), load_signed(load)) {
                (1, false) => self.builder.ins().uload8(ty, flags, host, 0),
                (1, true) => self.builder.ins().sload8(ty, flags, host, 0),
                (2, false) => self.builder.ins().uload16(ty, flags, host, 0),
                (2, true) => self.builder.ins().sload16(ty, flags, host, 0),
                (4, false) if kind == Kind::I64 => self.builder.ins().uload32(flags, host, 0),
                (4, true) if kind == Kind::I64 => self.builder.ins().sload32(flags, host, 0),
                _ => self.builder.ins().load(ty, flags, host, 0),
            };
            self.write(a, kind, value);
            return true;
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            let kind = Kind::of(store.value_type()).expect("planned");
            let value = self.read(a, kind);
            let address = self.read(c, Kind::I32);
            let width = store_width(store);
            let host = self.address(address, instruction.imm(), width);
            let flags = MemFlagsData::trusted().with_endianness(ir::Endianness::Little);
            match width {
                1 => self.builder.ins().istore8(flags, value, host, 0),
                2 => self.builder.ins().istore16(flags, value, host, 0),
                4 if kind == Kind::I64 => self.builder.ins().istore32(flags, value, host, 0),
                _ => self.builder.ins().store(flags, value, host, 0),
            };
            return true;
        }
        match op {
            Op::Move => {
                let kind = self.kind_at(pc, b);
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
                let value = self.builder.ins().iconst(kind.ir(), bits);
                if op == Op::LoadConst {
                    self.write(a, kind, value);
                } else {
                    let window = instruction.register_window();
                    for offset in 0..window.count {
                        self.write(window.base + offset, kind, value);
                    }
                }
            }
            Op::WasmI32Unary => {
                let operator = I32UnaryOperator::from_tag(instruction.imm()).expect("planned");
                let input = self.read(b, Kind::I32);
                let value = self.i32_unary(operator, input);
                self.write(a, Kind::I32, value);
            }
            Op::WasmI32ShiftRightUnsignedAndImmediate => {
                let input = self.read(b, Kind::I32);
                let shift = self.builder.ins().iconst(types::I32, i64::from(c as i16));
                let shifted = self.i32_binary(I32BinaryOperator::ShiftRightUnsigned, input, shift);
                let mask = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::from(instruction.imm() as i32));
                let value = self.i32_binary(I32BinaryOperator::And, shifted, mask);
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
                let kind = self.kind_at(pc, b);
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
                let mut args = vec![self.context, self.base, self.length];
                for (offset, ty) in callee_plan.params.iter().enumerate() {
                    let kind = Kind::of(*ty).expect("planned");
                    args.push(self.read(window.base + offset as u16, kind));
                }
                let signature = match self.signatures[callee] {
                    Some(signature) => signature,
                    None => {
                        let signature = internal_signature(
                            &callee_plan.params,
                            callee_plan.result,
                            self.call_conv,
                        );
                        let signature = self.builder.import_signature(signature);
                        self.signatures[callee] = Some(signature);
                        signature
                    }
                };
                let table = self.builder.ins().load(
                    types::I64,
                    MemFlagsData::trusted(),
                    self.context,
                    CONTEXT_TABLE,
                );
                let target = self.builder.ins().load(
                    types::I64,
                    MemFlagsData::trusted().with_readonly(),
                    table,
                    (callee * 8) as i32,
                );
                let call = self.builder.ins().call_indirect(signature, target, &args);
                let results = self.builder.inst_results(call).to_vec();
                // A trap in the callee unwinds this activation too.
                let trapped = self.builder.create_block();
                self.builder.set_cold_block(trapped);
                let next = self.builder.create_block();
                self.builder.ins().brif(results[0], trapped, &[], next, &[]);
                self.builder.switch_to_block(trapped);
                self.builder.seal_block(trapped);
                let mut values = vec![results[0]];
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
                let destination = instruction.result_register();
                match callee_plan.result {
                    Some(ty) => self.write(destination, Kind::of(ty).expect("planned"), results[1]),
                    None => {
                        let zero = self.builder.ins().iconst(types::I32, 0);
                        self.write(destination, Kind::I32, zero);
                    }
                }
            }
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
                let slot = self.binding_slot(pc, b);
                let address = self.global_address(slot);
                let value = self
                    .builder
                    .ins()
                    .load(types::I32, MemFlagsData::trusted().with_endianness(ir::Endianness::Little), address, 0);
                self.write(a, Kind::I32, value);
            }
            Op::WasmGlobalSet => {
                let slot = self.binding_slot(pc, b);
                let address = self.global_address(slot);
                let value = self.read(a, Kind::I32);
                let wide = self.builder.ins().uextend(types::I64, value);
                let encoded = self.builder.ins().bor_imm_u(wide, INTEGER_TAG as i64);
                self.builder
                    .ins()
                    .store(MemFlagsData::trusted(), encoded, address, 0);
            }
            _ => unreachable!("planned instructions have a denotation"),
        }
        true
    }
}

impl<H: Host> Analysis<'_, H> {
    /// Compile `index` with `plan`; `None` when Cranelift refuses it or its
    /// code needs relocation.
    fn compile_function(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plans: &[Option<Plan>],
        guarded: bool,
        globals: &[u32],
    ) -> Option<Vec<u8>> {
        let plan = plans[index].as_ref()?;
        let function = &self.code.functions[index];
        let call_conv = isa.default_call_conv();
        let mut func = ir::Function::with_name_signature(
            UserFuncName::user(0, index as u32),
            internal_signature(&plan.params, plan.result, call_conv),
        );
        let mut builder_context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut builder_context);
        let variables: Vec<[Variable; 2]> = (0..function.registers)
            .map(|_| {
                [
                    builder.declare_var(types::I32),
                    builder.declare_var(types::I64),
                ]
            })
            .collect();
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let (context, base, length) = (parameters[0], parameters[1], parameters[2]);
        let mut emitter = Emitter {
            guarded,
            globals,
            analysis: self,
            builder,
            plan,
            variables,
            blocks: vec![None; function.code.len()],
            context,
            base,
            length,
            trap_blocks: [None; TRAPS.len()],
            signatures: vec![None; plans.len()],
            plans,
            call_conv,
        };
        // Registers start at i32 zero, as an activation's register file does.
        let zero32 = emitter.builder.ins().iconst(types::I32, 0);
        let zero64 = emitter.builder.ins().iconst(types::I64, 0);
        for register in 0..function.registers {
            emitter.write(register, Kind::I32, zero32);
            emitter.write(register, Kind::I64, zero64);
        }
        let parameter_base = function.parameter_registers.expect("planned");
        for (offset, ty) in plan.params.iter().enumerate() {
            let kind = Kind::of(*ty).expect("planned");
            emitter.write(parameter_base + offset as u16, kind, parameters[3 + offset]);
        }
        // Each activation checks the stack it may still use.
        let stack_pointer = emitter.builder.ins().get_stack_pointer(types::I64);
        let limit = emitter.builder.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            context,
            CONTEXT_STACK_LIMIT,
        );
        let exhausted = emitter
            .builder
            .ins()
            .icmp(IntCC::UnsignedLessThan, stack_pointer, limit);
        emitter.trap_if(exhausted, crate::WasmTrap::CallStackExhausted);
        emitter.jump(0);
        let instructions: Vec<WideInstruction> = function
            .code
            .iter()
            .map(|packed| super::dispatch_fast::decoded_instruction(function, *packed))
            .collect();
        // Every jump target starts a block, created before emission so a
        // backward jump finds its target's block already placed.
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if plan.states[pc].is_none() {
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
                if target < instructions.len() && plan.states[target].is_some() {
                    emitter.block(target);
                }
            }
        }
        let mut open = false;
        for (pc, instruction) in instructions.iter().copied().enumerate() {
            if plan.states[pc].is_none() {
                open = false;
                continue;
            }
            let is_leader = emitter.blocks[pc].is_some() || !open;
            if is_leader {
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
        if !compiled.buffer.relocs().is_empty() {
            return None;
        }
        Some(compiled.code_buffer().to_vec())
    }

    /// The C entry of `index`: unpack arguments, call through the table and
    /// store the result.
    fn compile_entry(
        &self,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
        index: usize,
        plan: &Plan,
    ) -> Option<Vec<u8>> {
        let call_conv = isa.default_call_conv();
        let mut signature = Signature::new(call_conv);
        for _ in 0..5 {
            signature.params.push(AbiParam::new(types::I64));
        }
        signature.returns.push(AbiParam::new(types::I32));
        let mut func =
            ir::Function::with_name_signature(UserFuncName::user(1, index as u32), signature);
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
        let mut call_args = vec![context, base, length];
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
        let target_signature =
            builder.import_signature(internal_signature(&plan.params, plan.result, call_conv));
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
fn guarded_memory<H: Host>(vm: &Vm<H>, binding: Value) -> Option<(*mut u8, usize)> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(binding) else {
        return None;
    };
    let crate::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
        return None;
    };
    state.try_borrow().ok()?.bytes.guard()
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
    0x48, 0x89, 0xe5, // mov rbp, rsp
    0x53, // push rbx
    0x41, 0x54, // push r12
    0x41, 0x55, // push r13
    0x41, 0x56, // push r14
    0x41, 0x57, // push r15
    0x48, 0x83, 0xec, 0x08, // sub rsp, 8 (keeps the call aligned)
    0x48, 0x89, 0x67, CONTEXT_ENTRY_STACK, // mov [rdi + entry_stack], rsp
    0x41, 0xff, 0xd1, // call r9
    // RESUME (24):
    0x48, 0x83, 0xc4, 0x08, // add rsp, 8
    0x41, 0x5f, // pop r15
    0x41, 0x5e, // pop r14
    0x41, 0x5d, // pop r13
    0x41, 0x5c, // pop r12
    0x5b, // pop rbx
    0x5d, // pop rbp
    0xc3, // ret
    // LANDING (39):
    0xb8, 0, 0, 0, 0, // mov eax, status (patched)
    0xe9, 0, 0, 0, 0, // jmp RESUME (patched)
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

    /// Compile every function of `program` that has a compiled denotation,
    /// with `env` as the instance whose bindings fix global types and memory.
    fn compile_program(&self, program: ProgramId, env: Value) -> Option<CompiledProgram> {
        let code = self.programs.get(program)?;
        if code.kind != crate::bytecode::ProgramKind::Wasm {
            return None;
        }
        let analysis = Analysis {
            vm: self,
            program,
            code: &code,
            env,
        };
        let count = code.functions.len();
        // The callable set is the greatest fixpoint: assume every function
        // compiles, drop those that cannot, until nothing changes.
        let mut callable = vec![true; count];
        let mut plans: Vec<Option<Plan>>;
        let mut memory_slot;
        loop {
            memory_slot = None;
            plans = (0..count)
                .map(|index| {
                    if callable[index] {
                        analysis.plan(index, &callable, &mut memory_slot)
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
        if plans.iter().all(Option::is_none) {
            return None;
        }
        let isa = target_isa()?;
        // The globals compiled code accesses, each once.
        let mut globals: Vec<u32> = Vec::new();
        for (index, plan) in plans.iter().enumerate() {
            let Some(plan) = plan else { continue };
            let function = &code.functions[index];
            for (pc, packed) in function.code.iter().enumerate() {
                let instruction = super::dispatch_fast::decoded_instruction(function, *packed);
                if !matches!(instruction.op(), Op::WasmGlobalGet | Op::WasmGlobalSet) {
                    continue;
                }
                if let Some(Slot::Binding(slot)) = plan.states[pc]
                    .as_ref()
                    .map(|state| state[usize::from(instruction.b())])
                    && !globals.contains(&slot)
                {
                    globals.push(slot);
                }
            }
        }
        // Accesses may rely on the guard when the memory has one and this
        // host resumes guard faults.
        let guarded = guard_trampoline().is_some()
            && memory_slot.is_some_and(|slot| {
                self.heap
                    .environment_slot(env, slot as usize)
                    .and_then(|binding| guarded_memory(self, binding))
                    .is_some()
            });
        let mut bytes = Vec::new();
        let mut offsets = vec![None; count];
        let mut entries = vec![None; count];
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
            let body = analysis.compile_function(&*isa, index, &plans, guarded, &globals)?;
            offsets[index] = Some(place(&mut bytes, body));
            let entry = analysis.compile_entry(&*isa, index, plans[index].as_ref()?)?;
            entries[index] = Some(place(&mut bytes, entry));
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
                (view.0 as u64, view.1 as u64, guard)
            }
            None => (0, 0, None),
        };
        let mut raw: smallvec::SmallVec<[u64; 8]> = smallvec::SmallVec::new();
        for (value, ty) in args.iter().zip(&function.params) {
            raw.push(match Kind::of(*ty)? {
                Kind::I32 => u64::from(value.wasm_bits32()),
                Kind::I64 => match self.heap.get(*value)? {
                    Cell::WasmBits64(bits) => *bits,
                    _ => return None,
                },
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
            table: compiled.table.as_ptr(),
            vm: (self as *mut Self).cast(),
            env: env.to_bits(),
            entry_stack: 0,
            guard_start,
            guard_end,
            globals: globals.as_ptr(),
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
        if status != 0 {
            let trap = TRAPS[status as usize - 1];
            return Some(Err(JsError::wasm_trap_error(trap)));
        }
        Some(Ok(match function.result {
            Some(ty) => match Kind::of(ty)? {
                Kind::I32 => Value::integer(out as u32 as i32),
                Kind::I64 => self.heap.alloc(Cell::WasmBits64(out)),
            },
            None => self.programs.constant(program, VOID_RESULT_CONSTANT)?,
        }))
    }
}

/// The tag bits of an encoded i32, which a global's value carries.
const INTEGER_TAG: u64 = crate::value::INTEGER_TAG_BITS;

/// The constant a void Wasm function returns (see the Wasm lowering).
const VOID_RESULT_CONSTANT: usize = 1;
