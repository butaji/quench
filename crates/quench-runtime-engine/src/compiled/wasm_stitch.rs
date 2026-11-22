//! The b-jit's Wasm functions: each bytecode instruction becomes the stencil row of the staged
//! rule it runs (`quench-bjit/stencils/wasm_rows.rs`), copied and patched into one function
//! behind the entry routine (`wasm_entry.rs`), with the u-jit's `WasmEntry` ABI, so the driver
//! places a stitched function where it would place a compiled one. The instructions decode
//! through the tables every tier reads (`Step`, `WasmOperation`); nothing here names an opcode.
//!
//! A function's frame is the native stack words the entry reserves: the result pointer, then one
//! machine word per register (a 32-bit value zero-extended), so a register's hole is its byte
//! offset `8 * (1 + register)`. A row that traps leaves through the trap routine, which returns
//! the status the row left; a return copies its register through the result pointer.

use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_flow::Flow;
use quench_runtime::staged::wasm_instance::{Instance, Storage};
use quench_runtime::staged::wasm_memory::{Access, Memory};
use quench_runtime::staged::wasm_operation::{Operand, WasmOperation};
use quench_runtime::staged::wasm_step::Step;
use quench_runtime::wasm::integer::I64BinaryOperator as Binary;
use quench_bjit::wasm::{self as codes, WasmCode};
use quench_bjit::{HOLES, Row, Stencil, padded_holes};
use super::CONTEXT_MEMORY_LENGTH;
use quench_runtime::staged::frame::WASM_HEADER_WORDS;

/// The frame word a register is, past the header words.
const fn slot(register: u16) -> u64 {
    quench_bjit::call_record::REGISTER_BYTES as u64 * (WASM_HEADER_WORDS as u64 + register as u64)
}

/// The word an integer constant is at `width`: an i32 zero-extended.
const fn word(width: Width, value: i64) -> u64 {
    match width {
        Width::I32 => value as u32 as u64,
        Width::I64 => value as u64,
    }
}

const fn float_code(float: quench_runtime::staged::wasm_float::Float) -> u8 {
    match float {
        quench_runtime::staged::wasm_float::Float::F32 => codes::W32,
        quench_runtime::staged::wasm_float::Float::F64 => codes::W64,
    }
}

const fn width_code(width: Width) -> u8 {
    match width {
        Width::I32 => codes::W32,
        Width::I64 => codes::W64,
    }
}

/// What one instruction places: a row of the library or a routine, its holes and the instruction
/// its taken successor is.
struct Planned {
    stencil: &'static str,
    holes: [u64; HOLES],
    taken: Option<Target>,
}

/// Where a row's taken successor is: an instruction's first row, or a row of the same
/// instruction (a table's search).
#[derive(Clone, Copy)]
enum Target {
    Pc(usize),
    Row(usize),
}

fn row(code: WasmCode, holes: [u64; 3], taken: Option<usize>) -> Planned {
    Planned { stencil: leak(codes::wasm_name(code)), holes: padded_holes(holes), taken: taken.map(Target::Pc) }
}

/// A conditional row on the i32 register `selector` against `value`, taken to `target`.
fn compare(operator: Binary, selector: u16, value: u32, target: Target) -> Planned {
    Planned {
        stencil: leak(codes::wasm_name((codes::JUMP_IF, operator as u32, codes::W32, codes::IMMEDIATE))),
        holes: padded_holes([0, slot(selector), u64::from(value)]),
        taken: Some(target),
    }
}

/// The rows of a branch table on `selector`: past the cases, the default; inside, a binary
/// search down to runs of equality tests.
fn table(selector: u16, cases: &[usize], default: usize) -> Vec<Planned> {
    fn search(rows: &mut Vec<Planned>, selector: u16, cases: &[usize], low: usize, high: usize) {
        if high - low <= 4 {
            for case in low..high - 1 {
                rows.push(compare(Binary::Equal, selector, case as u32, Target::Pc(cases[case])));
            }
            rows.push(row((codes::GOTO, 0, codes::W64, codes::REGISTER), [0; 3], Some(cases[high - 1])));
            return;
        }
        let middle = (low + high) / 2;
        let split = rows.len();
        rows.push(compare(Binary::GreaterEqualUnsigned, selector, middle as u32, Target::Row(0)));
        search(rows, selector, cases, low, middle);
        rows[split].taken = Some(Target::Row(rows.len()));
        search(rows, selector, cases, middle, high);
    }
    let mut rows = Vec::new();
    if cases.is_empty() {
        rows.push(row((codes::GOTO, 0, codes::W64, codes::REGISTER), [0; 3], Some(default)));
        return rows;
    }
    rows.push(compare(Binary::GreaterEqualUnsigned, selector, cases.len() as u32, Target::Pc(default)));
    search(&mut rows, selector, cases, 0, cases.len());
    rows
}

/// A row name, interned for the life of the process: the names are a fixed, small set.
fn leak(name: String) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().expect("the name table");
    if let Some(name) = names.get(&name) {
        return name;
    }
    let interned: &'static str = Box::leak(name.clone().into_boxed_str());
    names.insert(name, interned);
    interned
}

/// The rows of `operation`, or `None` for one without rows yet.
fn numeric(instruction: WideInstruction, operation: WasmOperation) -> Option<Vec<Planned>> {
    let target = || Some(instruction.jump_target() as usize);
    let (a, b) = (instruction.a(), instruction.b());
    let binary = |operator: Binary, width, right: Operand, destination: u16, left: u16| {
        let (shape, right) = match right {
            Operand::Register(register) => (codes::REGISTER, slot(register)),
            Operand::Constant(value) => (codes::IMMEDIATE, word(width, value)),
        };
        row((codes::BINARY, operator as u32, width_code(width), shape), [slot(destination), slot(left), right], None)
    };
    let branch = |operator: Binary, width, left: u16, right: Operand| {
        let (shape, right) = match right {
            Operand::Register(register) => (codes::REGISTER, slot(register)),
            Operand::Constant(value) => (codes::IMMEDIATE, word(width, value)),
        };
        row((codes::JUMP_IF, operator as u32, width_code(width), shape), [0, slot(left), right], target())
    };
    Some(match operation {
        WasmOperation::Binary { operator, width, form } if form.branches() => {
            vec![branch(operator, width, form.left(instruction), form.right(instruction))]
        }
        WasmOperation::Binary { operator, width, form } => {
            vec![binary(operator, width, form.right(instruction), a, form.left(instruction))]
        }
        WasmOperation::Unary { operator, width } => {
            vec![row((codes::UNARY, operator as u32, width_code(width), codes::REGISTER), [slot(a), slot(b), 0], None)]
        }
        WasmOperation::Test { nonzero } => {
            let operator = if nonzero { Binary::NotEqual } else { Binary::Equal };
            vec![branch(operator, Width::I32, a, Operand::Constant(0))]
        }
        WasmOperation::AddBranch => vec![
            binary(Binary::Add, Width::I32, Operand::Constant(i64::from(instruction.c() as i16)), a, b),
            branch(Binary::NotEqual, Width::I32, a, Operand::Constant(0)),
        ],
        WasmOperation::BitField => vec![
            binary(Binary::ShiftRightUnsigned, Width::I32, Operand::Constant(i64::from(instruction.c() as i16)), a, b),
            binary(Binary::And, Width::I32, Operand::Constant(i64::from(instruction.imm() as i32)), a, a),
        ],
        WasmOperation::FloatBinary { operator, float } => {
            vec![row((codes::FLOAT_BINARY, operator as u32, float_code(float), codes::REGISTER), [slot(a), slot(b), slot(instruction.c())], None)]
        }
        WasmOperation::FloatUnary { operator, float } => {
            vec![row((codes::FLOAT_UNARY, operator as u32, float_code(float), codes::REGISTER), [slot(a), slot(b), 0], None)]
        }
        WasmOperation::Convert { operator } => {
            vec![row((codes::CONVERT, operator as u32, codes::W64, codes::REGISTER), [slot(a), slot(b), 0], None)]
        }
    })
}

/// The code of a scalar access in the row library (`ACCESSES`).
fn access_code(access: Access) -> Option<u32> {
    let ty = match access.ty {
        quench_runtime::WasmType::I32 => codes::T_I32,
        quench_runtime::WasmType::I64 => codes::T_I64,
        quench_runtime::WasmType::F32 => codes::T_F32,
        quench_runtime::WasmType::F64 => codes::T_F64,
        _ => return None,
    };
    codes::ACCESSES.iter().position(|&code| code == (ty, access.bytes, access.signed)).map(|index| index as u32)
}

/// The rows of a load or store of the memory the frame runs over: `a = load[b][c + imm]`,
/// `store[b][c + imm] = a`, `b` naming that memory (`plan` admits no other binding). An offset
/// past 31 bits, which the row's 32-bit hole cannot hold, has none.
fn memory(instruction: WideInstruction, memory: Memory) -> Option<Vec<Planned>> {
    let (kind, access) = match memory {
        Memory::Load(access) => (codes::LOAD, access),
        Memory::Store(access) => (codes::STORE, access),
        _ => return None,
    };
    let offset = instruction.imm();
    (offset <= i32::MAX as u32).then_some(())?;
    let holes = [slot(instruction.a()), slot(instruction.c()), u64::from(offset)];
    Some(vec![row((kind, access_code(access)?, codes::W64, codes::REGISTER), holes, None)])
}

/// The row of a direct call (`Call::Known`) of the defined function `callee`: the result to
/// `destination`, the arguments from `window`. A stitched callee is called through its entry cell
/// (`CALL`). An unstitched one runs in the VM through the callout (`CALL_VM`: its index in hole
/// FOURTH, the callout in hole B, the length's offset in hole A's high word). The VM may grow the
/// memory, so that row is planned only where the base is stable.
fn known_call(destination: u16, window: u16, callee: usize, world: &World<'_>) -> Option<Planned> {
    let length = u64::try_from(CONTEXT_MEMORY_LENGTH).ok()?;
    if let Some(cell) = (world.callee)(callee) {
        let holes = [slot(destination), slot(window), cell];
        // A callee that may grow the memory makes the row reload the base and length: the length
        // comes from the context, at hole `HOLE_FOURTH` (`reload_memory`, `wasm_rows.rs`). One that
        // cannot keeps them (`CALL_KEEP`), and its row reads no length.
        if world.grows_memory.len() > callee && !world.grows_memory.contains(callee) {
            return Some(row((codes::CALL_KEEP, 0, codes::W64, codes::REGISTER), holes, None));
        }
        let mut call = row((codes::CALL, 0, codes::W64, codes::REGISTER), holes, None);
        call.holes[3] = length;
        return Some(call);
    }
    if !world.memory_stable {
        return None;
    }
    let holes = [slot(destination), slot(window) | length << 32, world.vm_call, u64::try_from(callee).ok()?];
    let name = leak(codes::wasm_name((codes::CALL_VM, 0, codes::W64, codes::REGISTER)));
    Some(Planned { stencil: name, holes: padded_holes(holes), taken: None })
}

/// The rows of the instruction at `pc`, or `None` where one has no rows yet. `constant` gives a
/// constant's frame word, `None` for one a row cannot hold (a vector); `memory_slot` is the
/// instance binding of the memory the entry passes (`WasmEntry`'s base and length), whose
/// binding instruction then places nothing.
fn plan(
    pc: usize,
    instructions: &[WideInstruction],
    returns: bool,
    world: &World<'_>,
) -> Option<Vec<Planned>> {
    let instruction = instructions[pc];
    let Some(step) = Step::of(instruction.op(), instruction.imm()) else {
        return wasm_stitch_simd_more::simd_more(pc, instruction, world);
    };
    quench_runtime::staged::wasm_step::dispatch(&mut Rows { instructions, returns, world }, pc, instruction, step)
}

/// The b-jit as a Wasm tier (`staged::wasm_step::WasmTier`): each family plans its stencil
/// rows over frame slots, `None` where it has none yet (the function stays interpreted).
struct Rows<'r, 'w> {
    instructions: &'r [WideInstruction],
    returns: bool,
    world: &'r World<'w>,
}

impl quench_runtime::staged::wasm_step::WasmTier for Rows<'_, '_> {
    type Out = Option<Vec<Planned>>;

    fn numeric(&mut self, _pc: usize, instruction: WideInstruction, operation: quench_runtime::staged::wasm_operation::WasmOperation) -> Self::Out {
        numeric(instruction, operation)
    }

    fn instance(&mut self, pc: usize, instruction: WideInstruction, step: Instance) -> Self::Out {
        let world = self.world;
        let (a, b) = (instruction.a(), instruction.b());
        if let Some(rows) = wasm_stitch_simd::instance(pc, instruction, step, world) {
            return Some(rows);
        }
        if let Some(registers) = step.constant_registers(instruction) {
            let value = (world.constant)(instruction.constant_index())?;
            return Some(
                registers
                    .map(|register| row((codes::CONSTANT, 0, codes::W64, codes::IMMEDIATE), [slot(register), 0, value], None))
                    .collect(),
            );
        }
        match step {
            Instance::Move => Some(vec![row((codes::MOVE, 0, codes::W64, codes::REGISTER), [slot(a), slot(b), 0], None)]),
            Instance::Binding if world.memory_slot == Some(instruction.imm()) => Some(vec![]),
            // Any other binding register is read only by global and table rows (the u-jit's
            // plan admits no other reader of a binding), which read the context; it places nothing.
            Instance::Binding => Some(vec![]),
            Instance::GlobalGet | Instance::GlobalSet => wasm_stitch_global::global(pc, instruction, step, world),
            _ => None,
        }
    }

    fn memory(&mut self, pc: usize, instruction: WideInstruction, step: quench_runtime::staged::wasm_memory::Memory) -> Self::Out {
        memory(instruction, step)
            .or_else(|| bulk(instruction, step))
            .or_else(|| instance_op(pc, instruction, step, self.world))
    }

    fn flow(&mut self, pc: usize, instruction: WideInstruction, flow: Flow) -> Self::Out {
        let (a, b) = (instruction.a(), instruction.b());
        match flow {
            Flow::Jump => Some(vec![row((codes::GOTO, 0, codes::W64, codes::REGISTER), [0; 3], Some(instruction.jump_target() as usize))]),
            Flow::Select => {
                let (condition, _) = instruction.register_pair();
                let mut select = row((codes::SELECT, 0, codes::W64, codes::REGISTER), [slot(a), slot(b), slot(instruction.c())], None);
                select.holes[3] = slot(condition);
                Some(vec![select])
            }
            Flow::Unreachable => Some(vec![row((codes::TRAP, 0, codes::W64, codes::REGISTER), [0; 3], None)]),
            Flow::Table => {
                let entries = quench_runtime::bytecode::branch_table_entries(pc, instruction);
                let targets: Vec<usize> = entries.map(|entry| self.instructions.get(entry).map(|jump| jump.jump_target() as usize)).collect::<Option<_>>()?;
                let (default, cases) = targets.split_last()?;
                Some(table(a, cases, *default))
            }
            Flow::Return => Some(vec![if self.returns {
                Planned { stencil: "leave_wasm_word", holes: padded_holes([slot(a)]), taken: None }
            } else {
                Planned { stencil: "leave_wasm_void", holes: padded_holes([]), taken: None }
            }]),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    fn call(&mut self, pc: usize, instruction: WideInstruction, call: quench_runtime::staged::wasm_call::Call) -> Self::Out {
        let (world, a) = (self.world, instruction.a());
        match call {
            quench_runtime::staged::wasm_call::Call::Known => {
                // A tail call (its destination flagged) calls, then leaves, as the u-jit's does.
                if a & quench_runtime::bytecode::RETURN_REGISTER != 0 {
                    return wasm_stitch_tail::tail(pc, instruction, self.returns, world);
                }
                let window = quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm());
                Some(vec![known_call(a, window, usize::from(instruction.known_function_index()), world)?])
            }
            quench_runtime::staged::wasm_call::Call::IndirectTarget => indirect::target(pc, self.instructions),
            quench_runtime::staged::wasm_call::Call::Reference => {
                if !world.memory_stable {
                    return None;
                }
                let mut rows = indirect::reference(pc, self.instructions, world)?;
                // A flagged call returns its result: nothing follows it on this path.
                if instruction.result_flags() & quench_runtime::bytecode::RETURN_REGISTER != 0 {
                    rows.push(wasm_stitch_tail::leave(instruction, self.returns));
                }
                Some(rows)
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    fn table(&mut self, pc: usize, instruction: WideInstruction, op: quench_runtime::staged::wasm_table_ops::TableOp) -> Self::Out {
        wasm_stitch_table::table(pc, instruction, op, self.world)
    }

    fn simd(&mut self, pc: usize, instruction: WideInstruction, step: quench_runtime::staged::wasm_simd::Simd) -> Self::Out {
        wasm_stitch_simd::simd(instruction, step, self.world).or_else(|| wasm_stitch_simd_more::simd_more(pc, instruction, self.world))
    }

    fn gc(&mut self, pc: usize, instruction: WideInstruction, op: quench_runtime::staged::wasm_gc::GcOp) -> Self::Out {
        wasm_stitch_gc::gc(pc, instruction, op, self.world)
    }

    /// The b-jit has no rows for exception operations yet: such a function stays interpreted.
    fn exception(&mut self, _pc: usize, _instruction: WideInstruction, _op: quench_runtime::staged::wasm_exception::Exception) -> Self::Out {
        None
    }

    /// The b-jit has no rows for atomic operations: such a function stays interpreted.
    fn atomic(&mut self, _pc: usize, _instruction: WideInstruction, _op: quench_runtime::staged::wasm_atomic::Atomic) -> Self::Out {
        None
    }
}

/// The frame's bytes: the result pointer and the registers, 8 more than a multiple of 16 so the
/// entry's call of the first row is aligned (`wasm_entry.rs`).
const fn frame_bytes(registers: u16) -> u64 {
    padded_frame(slot(registers))
}

/// The frame's bytes for a frame of `bytes` bytes before padding: the padding of `frame_bytes`,
/// shared with the vector area's frame (`wasm_stitch_simd::frame_bytes`).
#[cfg(target_arch = "x86_64")]
const fn padded_frame(bytes: u64) -> u64 {
    if bytes % 16 == 8 { bytes } else { bytes + 8 }
}

/// The frame's bytes on AArch64 (`wasm_entry_a64.rs`): `sp` must stay 16-byte aligned, and there is no
/// pushed return address, so the frame is a multiple of 16.
#[cfg(not(target_arch = "x86_64"))]
const fn padded_frame(bytes: u64) -> u64 {
    (bytes + 15) & !15
}

fn stencil(name: &str) -> Option<Stencil> {
    quench_bjit::rustc::stencil(name)
}

/// The entry for `params` arguments and `rest` zeroed registers: the variant of its parameter
/// count (`wasm_entry.rs`, unzeroed when `rest` is 0), else the generic entry. A target without
/// the variants (AArch64) has the generic entry alone.
fn entry_stencil(params: usize, rest: u64) -> Option<Stencil> {
    let variants = if rest == 0 { &ENTRY_UNZEROED } else { &ENTRY_ZEROED };
    variants.get(params).and_then(|name| stencil(name)).or_else(|| stencil("entry_wasm"))
}

/// The x86-64 entry variants by parameter count, zeroing the registers after the arguments.
const ENTRY_ZEROED: [&str; 5] = ["entry_wasm_p0", "entry_wasm_p1", "entry_wasm_p2", "entry_wasm_p3", "entry_wasm_p4"];
/// The same, for an entry with no registers after its arguments.
const ENTRY_UNZEROED: [&str; 5] = ["entry_wasm_p0_nz", "entry_wasm_p1_nz", "entry_wasm_p2_nz", "entry_wasm_p3_nz", "entry_wasm_p4_nz"];

pub(super) use quench_runtime::vm::engine_types::World;

pub(super) use quench_runtime::vm::engine_types::IndirectSite;

pub(super) fn stitch(function: &Function, params: usize, returns: bool, world: &World<'_>) -> Option<Vec<u8>> {
    let instructions: Vec<WideInstruction> =
        function.code.iter().map(|packed| function.widened(*packed)).collect();
    let mut planned = Vec::with_capacity(instructions.len());
    for pc in 0..instructions.len() {
        let Some(rows) = plan(pc, &instructions, returns, world) else {
            // `QUENCH_TRACE=wasm-jit` names the first instruction a function has no rows for.
            if quench_runtime::trace::Channel::WasmJit.on() {
                eprintln!("QUENCH_WASM_BJIT_REJECT pc={pc} op={:?}", instructions[pc].op());
            }
            return None;
        };
        planned.push(wasm_stitch_roots::rooted(pc, instructions[pc], rows, world)?);
    }
    // The first row of each instruction, and past the last.
    let mut first = Vec::with_capacity(planned.len() + 1);
    let mut rows = 0;
    for rows_of in &planned {
        first.push(rows);
        rows += rows_of.len();
    }
    first.push(rows);
    let flat: Vec<Planned> = planned
        .into_iter()
        .enumerate()
        .flat_map(|(pc, rows)| {
            let first = first[pc];
            rows.into_iter().map(move |mut planned| {
                // A row target becomes the row's index in the function.
                if let Some(Target::Row(row)) = planned.taken {
                    planned.taken = Some(Target::Row(first + row));
                }
                planned
            })
        })
        .collect();
    // Control never runs past the last row: it leaves.
    if !flat.last().is_some_and(|last| last.stencil.starts_with("leave_")) {
        return None;
    }
    let stencils: Vec<Stencil> = flat.iter().map(|planned| stencil(planned.stencil)).collect::<Option<_>>()?;
    let placed: Vec<Row> = flat
        .iter()
        .zip(&stencils)
        .map(|(planned, stencil)| Row { stencil, hole: &planned.holes, taken: planned.taken.map(|target| row_index(target, &first)), cold: false })
        .collect();

    let trap = stencil("leave_wasm_trap")?;
    // The parameters follow the instance bindings (`Function::parameter_registers`).
    let first_parameter = function.parameter_registers?;
    let past_parameters = u64::from(first_parameter) + params as u64;
    let rest = u64::from(function.registers).checked_sub(past_parameters)?;
    let vectors = wasm_stitch_simd::uses_vectors(&instructions, world);
    // A v128 local starts as zero like any other: the vector area follows the registers, so the
    // entry's zeroing runs on through its two words per register.
    let rest = if vectors { rest + 2 * u64::from(function.registers) } else { rest };
    let entry = entry_stencil(params, rest)?;
    let rows_at = entry.code.len() + trap.code.len();
    let mut code = vec![0u8; rows_at + stencils.iter().map(|stencil| stencil.code.len()).sum::<usize>()];
    let entry_holes = padded_holes([wasm_stitch_simd::frame_bytes(function.registers, vectors), params as u64, rest, slot(first_parameter)]);
    patch_routine(&entry, &mut code[..entry.code.len()], &entry_holes, rows_at)?;
    code[entry.code.len()..rows_at].copy_from_slice(&trap.code);
    let trap_at = code.as_ptr() as usize + entry.code.len();
    // SAFETY: `code` holds every row's full length past `rows_at`, which bounds the copy.
    let (length, _) = unsafe { quench_bjit::copy_patch(&placed, code.as_mut_ptr().add(rows_at), |_, _| trap_at) };
    code.truncate(rows_at + length);
    Some(code)
}

/// The index of the row `target` is, a row target already being one (`stitch`).
fn row_index(target: Target, first: &[usize]) -> usize {
    match target {
        Target::Pc(pc) => first[pc],
        Target::Row(row) => row,
    }
}

/// Copies `routine` into `code` (its own length), its value holes `holes` and its one successor
/// (the call of the first row) the rows at offset `rows_at`.
fn patch_routine(routine: &Stencil, code: &mut [u8], holes: &[u64; HOLES], rows_at: usize) -> Option<()> {
    code.copy_from_slice(&routine.code);
    for reloc in &routine.relocs {
        let at = reloc.offset as usize;
        if quench_bjit::class::is_successor(reloc.class) {
            quench_bjit::patch_successor(code, at, reloc.class, rows_at as i64 + reloc.addend - at as i64).ok()?;
        } else {
            quench_bjit::patch_value(code, at, reloc.class, holes[reloc.index as usize]);
        }
    }
    Some(())
}

#[path = "wasm_stitch_memory.rs"]
mod wasm_stitch_memory;
use self::wasm_stitch_memory::bulk;
#[path = "wasm_stitch_global.rs"]
mod wasm_stitch_global;
#[path = "wasm_stitch_table.rs"]
mod wasm_stitch_table;
#[path = "wasm_stitch_gc.rs"]
mod wasm_stitch_gc;
#[path = "wasm_stitch_roots.rs"]
mod wasm_stitch_roots;
#[path = "wasm_stitch_simd.rs"]
mod wasm_stitch_simd;
#[path = "wasm_stitch_simd_more.rs"]
mod wasm_stitch_simd_more;
#[path = "wasm_stitch_tail.rs"]
mod wasm_stitch_tail;
#[path = "wasm_stitch_instance.rs"]
mod wasm_stitch_instance;
use self::wasm_stitch_instance::instance_op;
#[path = "wasm_stitch_indirect.rs"]
pub(super) mod indirect;

#[cfg(test)]
#[path = "wasm_stitch_tests.rs"]
mod tests;
