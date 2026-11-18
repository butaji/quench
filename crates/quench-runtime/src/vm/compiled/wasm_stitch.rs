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

use crate::bytecode::{Function, WideInstruction};
use crate::staged::wasm::Width;
use crate::staged::wasm_flow::Flow;
use crate::staged::wasm_instance::Instance;
use crate::staged::wasm_memory::{Access, Memory};
use crate::staged::wasm_operation::{Operand, WasmOperation};
use crate::staged::wasm_step::Step;
use crate::wasm::integer::I64BinaryOperator as Binary;
use quench_bjit::wasm::{self as codes, WasmCode};
use quench_bjit::{HOLES, Row, Stencil, padded_holes};

/// The frame word a register is, past the header words.
const fn slot(register: u16) -> u64 {
    8 * (codes::FRAME_HEADER_WORDS + register as u64)
}

/// The word an integer constant is at `width`: an i32 zero-extended.
const fn word(width: Width, value: i64) -> u64 {
    match width {
        Width::I32 => value as u32 as u64,
        Width::I64 => value as u64,
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
        WasmOperation::FloatBinary { .. } | WasmOperation::FloatUnary { .. } | WasmOperation::Convert { .. } => return None,
    })
}

/// The code of a scalar access in the row library (`ACCESSES`).
fn access_code(access: Access) -> Option<u32> {
    let ty = match access.ty {
        crate::WasmType::I32 => codes::T_I32,
        crate::WasmType::I64 => codes::T_I64,
        crate::WasmType::F32 => codes::T_F32,
        crate::WasmType::F64 => codes::T_F64,
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
    let (constant, memory_slot) = (world.constant, world.memory_slot);
    let (a, b) = (instruction.a(), instruction.b());
    match Step::of(instruction.op(), instruction.imm())? {
        Step::Numeric(operation) => numeric(instruction, operation),
        Step::Instance(step) => {
            if let Some(registers) = step.constant_registers(instruction) {
                let value = constant(instruction.constant_index())?;
                return Some(
                    registers
                        .map(|register| row((codes::CONSTANT, 0, codes::W64, codes::IMMEDIATE), [slot(register), 0, value], None))
                        .collect(),
                );
            }
            match step {
                Instance::Move => Some(vec![row((codes::MOVE, 0, codes::W64, codes::REGISTER), [slot(a), slot(b), 0], None)]),
                Instance::Binding if memory_slot == Some(instruction.imm()) => Some(vec![]),
                _ => None,
            }
        }
        Step::Memory(step) => memory(instruction, step),
        Step::Flow(Flow::Jump) => Some(vec![row((codes::GOTO, 0, codes::W64, codes::REGISTER), [0; 3], Some(instruction.jump_target() as usize))]),
        Step::Flow(Flow::Table) => {
            let entries = crate::bytecode::branch_table_entries(pc, instruction);
            let targets: Vec<usize> = entries.map(|entry| instructions.get(entry).map(|jump| jump.jump_target() as usize)).collect::<Option<_>>()?;
            let (default, cases) = targets.split_last()?;
            Some(table(a, cases, *default))
        }
        Step::Call(crate::staged::wasm_call::Call::Known) => {
            // A tail call (its destination flagged) has no row yet.
            if a & crate::bytecode::RETURN_REGISTER != 0 {
                return None;
            }
            let cell = (world.callee)(usize::from(instruction.known_function_index()))?;
            let window = crate::bytecode::ImmediateLayout::call_window_base(instruction.imm());
            Some(vec![row((codes::CALL, 0, codes::W64, codes::REGISTER), [slot(a), slot(window), cell], None)])
        }
        Step::Flow(Flow::Return) => Some(vec![if returns {
            Planned { stencil: "leave_wasm_word", holes: padded_holes([slot(a)]), taken: None }
        } else {
            Planned { stencil: "leave_wasm_void", holes: padded_holes([]), taken: None }
        }]),
        _ => None,
    }
}

/// The frame's bytes: the result pointer and the registers, 8 more than a multiple of 16 so the
/// entry's call of the first row is aligned (`wasm_entry.rs`).
const fn frame_bytes(registers: u16) -> u64 {
    let bytes = slot(registers);
    if bytes % 16 == 8 { bytes } else { bytes + 8 }
}

fn stencil(name: &str) -> Option<Stencil> {
    quench_bjit::rustc::stencil(name)
}

/// The code of `function`: the entry, the trap routine, then the rows, every successor patched;
/// `None` where an instruction has no rows yet. The code is position independent: it calls and
/// jumps only within itself.
/// What the rows of a module's functions name outside the function: its constants' frame words
/// (`None` for one a row cannot hold, a vector), the instance binding of the memory the entry
/// passes (`WasmEntry`'s base and length), whose binding instruction then places nothing, and
/// the address of the cell holding a function's entry, which its direct calls call through
/// (`None` for a function without an entry).
pub(super) struct World<'a> {
    pub(super) constant: &'a dyn Fn(usize) -> Option<u64>,
    pub(super) memory_slot: Option<u32>,
    pub(super) callee: &'a dyn Fn(usize) -> Option<u64>,
}

pub(super) fn stitch(function: &Function, params: usize, returns: bool, world: &World<'_>) -> Option<Vec<u8>> {
    let instructions: Vec<WideInstruction> =
        function.code.iter().map(|packed| super::super::dispatch_fast::decoded_instruction(function, *packed)).collect();
    let mut planned = Vec::with_capacity(instructions.len());
    for pc in 0..instructions.len() {
        planned.push(plan(pc, &instructions, returns, world)?);
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
    let holes: Vec<Box<dyn Fn(u32) -> u64>> = flat
        .iter()
        .map(|planned| {
            let holes = planned.holes;
            Box::new(move |index: u32| holes[index as usize]) as Box<dyn Fn(u32) -> u64>
        })
        .collect();
    let placed: Vec<Row> = flat
        .iter()
        .zip(&stencils)
        .zip(&holes)
        .map(|((planned, stencil), hole)| Row { stencil, hole: &**hole, taken: planned.taken.map(|target| row_index(target, &first)) })
        .collect();

    let entry = stencil("entry_wasm")?;
    let trap = stencil("leave_wasm_trap")?;
    let rows_at = entry.code.len() + trap.code.len();
    let mut code = vec![0u8; rows_at + stencils.iter().map(|stencil| stencil.code.len()).sum::<usize>()];
    // The parameters follow the instance bindings (`Function::parameter_registers`).
    let first_parameter = function.parameter_registers?;
    let rest = u64::from(function.registers).checked_sub(u64::from(first_parameter) + params as u64)?;
    let entry_holes = padded_holes([frame_bytes(function.registers), params as u64, rest, slot(first_parameter)]);
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

#[cfg(test)]
#[path = "wasm_stitch_tests.rs"]
mod tests;
