//! A Number loop region built from FastJIT stencils (`quench-fastjit`) instead of Cranelift IR,
//! behind `QUENCH_DEV=fastjit`. The registers the region touches most live in pinned machine
//! registers through the whole chain of stencils and the rest stay tagged `Value` words in the
//! activation's register file; every exit writes the pinned registers back first, so it leaves
//! the lane's state canonical. A region with an instruction that is no row of the stencil
//! library is declined (`build` names the instruction) and keeps its Cranelift code.
//!
//! The result word is the exit PC: a leave out of the region resumes there, a guard inside it
//! exits at its instruction (nothing of that instruction has run), a backward jump exits at the
//! jump when an interrupt is requested so the interpreter's safepoint runs, and an entry guard
//! that finds a register that is no Number returns `LaneExit::Entry`.

use super::osr_loop::LoopRegion;
use crate::bytecode::{Class, OperandKind, Register, WideInstruction};
use crate::value::Value;
use oxc_ast::ast::BinaryOperator;
use super::fast_rows::{self, Heap, HeapRow};
use quench_fastjit::numeric::{self, Kind, Place};
use quench_fastjit::{PINNED, PINNED_F, Row as Placed, Stencil};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Bytes of a register slot in the activation's register file.
const SLOT: u64 = std::mem::size_of::<Value>() as u64;

/// `mov r11d, reg32` for the pinned registers rdx, rcx, r8 and r9: the raw int32 to tag.
const PIN_TO_R11: [[u8; 3]; PINNED] = [[0x41, 0x89, 0xd3], [0x41, 0x89, 0xcb], [0x45, 0x89, 0xc3], [0x45, 0x89, 0xcb]];
/// `movabs r10, imm64` (the int tag), `or r11, r10`, and `mov [rdi+disp32], r11`.
const MOVABS_R10: [u8; 2] = [0x49, 0xba];
const OR_R11_R10: [u8; 3] = [0x4d, 0x09, 0xd3];
const STORE_R11: [u8; 3] = [0x4c, 0x89, 0x9f];
/// The int tag a pinned int32 takes back into a register slot.
const INT_TAG: u64 = 0x7ffd_0000_0000_0000;
/// The callee-saved registers the thunk preserves (stencils of the tail convention save none):
/// push rbp, rbx, r12-r15 and r8 (the result pointer), in order.
const PROLOGUE: [u8; 12] = [0x55, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x41, 0x50];
/// `mov rsi, rdi; mov rdi, rcx`: the entry ABI's context and registers to the stencil convention.
const MOVE_ARGUMENTS: [u8; 6] = [0x48, 0x89, 0xfe, 0x48, 0x89, 0xcf];
const CALL_OPCODE: u8 = 0xe8;
/// `pop r8; mov [r8], rax; pop r15..r12, rbx, rbp; xor eax, eax; ret`.
const EPILOGUE: [u8; 18] =
    [0x41, 0x58, 0x49, 0x89, 0x00, 0x41, 0x5f, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5d, 0x31, 0xc0, 0xc3];
const MOV_EAX: u8 = 0xb8;
const MOVABS_RAX: [u8; 2] = [0x48, 0xb8];
const RET: u8 = 0xc3;

/// The entry thunk: saves the callee-saved registers, calls the first row at `rows_at`, stores
/// the word the chain returned through the result pointer and returns status 0.
fn thunk(rows_at: usize) -> Vec<u8> {
    let mut code = PROLOGUE.to_vec();
    code.extend(MOVE_ARGUMENTS);
    code.push(CALL_OPCODE);
    let call_end = code.len() + std::mem::size_of::<i32>();
    code.extend((rows_at as i64 - call_end as i64).to_le_bytes()[..4].iter());
    code.extend(EPILOGUE);
    code
}

/// A pinned register and the register of the activation it holds.
type Pin = (usize, Register);

/// The code that tags the pinned registers back into the register file.
fn tag_back(pins: &[Pin]) -> Vec<u8> {
    let mut code = Vec::new();
    if !pins.is_empty() {
        code.extend(MOVABS_R10);
        code.extend(INT_TAG.to_le_bytes());
    }
    for (pin, register) in pins {
        code.extend(PIN_TO_R11[*pin]);
        code.extend(OR_R11_R10);
        code.extend(STORE_R11);
        code.extend((u32::from(*register) * SLOT as u32).to_le_bytes());
    }
    code
}

/// A stub that tags the pinned registers back into the register file and returns `word`.
fn stub(pins: &[Pin], word: u64) -> Vec<u8> {
    let mut code = tag_back(pins);
    match u32::try_from(word) {
        Ok(small) => {
            code.push(MOV_EAX);
            code.extend(small.to_le_bytes());
        }
        Err(_) => {
            code.extend(MOVABS_RAX);
            code.extend(word.to_le_bytes());
        }
    }
    code.push(RET);
    code
}

/// `pop rax; ret`: the shared exit routine returns the PC its stub pushed.
const POP_RET: [u8; 2] = [0x58, RET];
/// `push imm32` and `jmp rel32`: a row's exit stub, which names its PC and the routine that
/// writes the live registers back.
const PUSH_IMM32: u8 = 0x68;
const JMP_REL32: u8 = 0xe9;
const EXIT_STUB_LEN: usize = 1 + 4 + 1 + 4;

#[derive(Default)]
struct Totals {
    regions: u64,
    ops: u64,
    bytes: u64,
    nanos: u64,
    rejected: u64,
}

static TOTALS: Mutex<Totals> = Mutex::new(Totals { regions: 0, ops: 0, bytes: 0, nanos: 0, rejected: 0 });
static REJECTS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
static REPORT: std::sync::Once = std::sync::Once::new();

extern "C" fn report() {
    let (Ok(totals), Ok(rejects)) = (TOTALS.lock(), REJECTS.lock()) else { return };
    let ops = totals.ops.max(1);
    eprintln!(
        "QUENCH_FASTJIT regions={} rejected={} ops={} bytes={} bytes_per_op={:.1} build_ns={} build_ns_per_op={:.1} stencil_gen_ns={}",
        totals.regions, totals.rejected, totals.ops, totals.bytes, totals.bytes as f64 / ops as f64, totals.nanos,
        totals.nanos as f64 / ops as f64, numeric::generation_ns()
    );
    let mut ranked: Vec<_> = rejects.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, count) in ranked {
        eprintln!("QUENCH_FASTJIT_REJECT {name} {count}");
    }
}

fn note(update: impl FnOnce(&mut Totals, &mut BTreeMap<String, u64>)) {
    if !crate::trace::Channel::JsJit.on() {
        return;
    }
    REPORT.call_once(|| {
        // SAFETY: `report` is an `extern "C" fn()` that touches only statics.
        unsafe { libc::atexit(report) };
    });
    if let (Ok(mut totals), Ok(mut rejects)) = (TOTALS.lock(), REJECTS.lock()) {
        update(&mut totals, &mut rejects);
    }
}

/// The row kind of a binary operator, from one table for arithmetic, bitwise and relational
/// operators.
const OPERATORS: [(BinaryOperator, Kind); 18] = [
    (BinaryOperator::Addition, Kind::Add),
    (BinaryOperator::Subtraction, Kind::Sub),
    (BinaryOperator::Multiplication, Kind::Mul),
    (BinaryOperator::Division, Kind::Div),
    (BinaryOperator::BitwiseAnd, Kind::And),
    (BinaryOperator::BitwiseOR, Kind::Or),
    (BinaryOperator::BitwiseXOR, Kind::Xor),
    (BinaryOperator::ShiftLeft, Kind::Shl),
    (BinaryOperator::ShiftRight, Kind::Shr),
    (BinaryOperator::ShiftRightZeroFill, Kind::UShr),
    (BinaryOperator::LessThan, Kind::Lt),
    (BinaryOperator::LessEqualThan, Kind::Le),
    (BinaryOperator::GreaterThan, Kind::Gt),
    (BinaryOperator::GreaterEqualThan, Kind::Ge),
    (BinaryOperator::Equality, Kind::Eq),
    (BinaryOperator::StrictEquality, Kind::Eq),
    (BinaryOperator::Inequality, Kind::Ne),
    (BinaryOperator::StrictInequality, Kind::Ne),
];

fn kind_of(operator: u32, relational: bool) -> Option<Kind> {
    let found = OPERATORS.iter().find(|(candidate, _)| *candidate as u32 == operator)?.1;
    (relational == (found >= Kind::Lt)).then_some(found)
}

/// An operand: a register of the activation or a constant `Value` word.
#[derive(Clone, Copy)]
enum Source {
    Register(Register),
    Constant(u64),
}

/// An instruction as a row: its kind, destination, operands and branch target.
struct Decoded {
    kind: Kind,
    dst: Option<Register>,
    a: Option<Source>,
    b: Option<Source>,
    target: Option<u32>,
    /// A heap row (`fast_rows`) in place of the numeric `kind`, with the array register a store
    /// writes through and the entry-record byte offset a field read loads.
    heap: Option<Heap>,
    base: Option<Register>,
    hole: u64,
}

impl Decoded {
    fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, a, b, target, heap: None, base: None, hole: 0 }
    }

    fn heap(heap: Heap, dst: Option<Register>, a: Option<Source>, b: Option<Source>) -> Self {
        Self { heap: Some(heap), ..Self::number(Kind::Move, dst, a, b, None) }
    }

    /// The registers the row names.
    fn registers(&self) -> impl Iterator<Item = Register> {
        [self.dst, self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
    }
}

fn source(region: &LoopRegion, operand: crate::bytecode::Operand) -> Result<Source, String> {
    match operand.kind() {
        Some(OperandKind::Register) => Ok(Source::Register(operand.payload())),
        Some(OperandKind::Constant) => region
            .number_constant(usize::from(operand.payload()))
            .map(|number| Source::Constant(Value::number(number).to_bits()))
            .ok_or_else(|| "operand: constant that is no Number".to_string()),
        other => Err(format!("operand: {other:?}")),
    }
}

fn binary(region: &LoopRegion, instruction: WideInstruction) -> Result<Decoded, String> {
    let operator = instruction.binary_operator();
    let kind = kind_of(operator, false).ok_or_else(|| format!("{:?}:{operator}", instruction.op()))?;
    Ok(Decoded::number(
        kind,
        Some(instruction.result_register()),
        Some(source(region, instruction.operand_b())?),
        Some(source(region, instruction.operand_c())?),
        None,
    ))
}

fn jump_binary(region: &LoopRegion, instruction: WideInstruction) -> Result<Decoded, String> {
    let operator = instruction.binary_operator_field();
    let kind = kind_of(operator, true).ok_or_else(|| format!("{:?}:{operator}", instruction.op()))?;
    Ok(Decoded::number(
        kind,
        None,
        Some(source(region, instruction.operand_b())?),
        Some(source(region, instruction.operand_c())?),
        Some(instruction.jump_target()),
    ))
}

fn load_const(region: &LoopRegion, instruction: WideInstruction) -> Result<Decoded, String> {
    let number = region.number_constant(instruction.constant_index()).ok_or_else(|| "LoadConst: not a Number".to_string())?;
    let word = Source::Constant(Value::number(number).to_bits());
    Ok(Decoded::number(Kind::Move, Some(instruction.result_register()), Some(word), None, None))
}

fn inc_dec(instruction: WideInstruction) -> Result<Decoded, String> {
    let decrement = instruction.boolean_flag().ok_or("IncDec")?;
    Ok(Decoded::number(
        if decrement { Kind::Sub } else { Kind::Add },
        Some(instruction.result_register()),
        Some(Source::Register(instruction.register_b())),
        Some(Source::Constant(Value::integer(1).to_bits())),
        None,
    ))
}

/// The heap instructions: a capture, a dense element read or store, a recorded field, the proof
/// that a base is an array.
fn heap_row(region: &LoopRegion, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    let register = |register| Some(Source::Register(register));
    match instruction.op().class() {
        Class::LoadCapture if instruction.capture_depth() == 0 => {
            Some(Decoded::heap(Heap::Capture(instruction.capture_slot()), Some(instruction.result_register()), None, None))
        }
        Class::GetIndex if !instruction.returns_from_frame() => {
            let base = instruction.operand_b().register_index()?;
            let index = source(region, instruction.operand_c()).ok()?;
            Some(Decoded::heap(Heap::Index, Some(instruction.result_register()), register(base), Some(index)))
        }
        Class::SetIndex => {
            let value = register(instruction.register_a());
            let row = Decoded::heap(Heap::Store, None, value, register(instruction.register_c()));
            Some(Decoded { base: Some(instruction.register_b()), ..row })
        }
        Class::RequireObjectCoercible => Some(Decoded::heap(Heap::Array, None, register(instruction.register_b()), None)),
        Class::GetField if !instruction.returns_from_frame() && !instruction.writes_current_this() => {
            let index = *region.field_reads().get(&(pc - region.header))?;
            let word = super::osr_loop::field_word(region, index);
            let row = Decoded::heap(Heap::Field, Some(instruction.result_register()), None, None);
            (!region.is_dynamic_field(index)).then_some(Decoded { hole: word as u64 * SLOT, ..row })
        }
        _ => None,
    }
}

fn decode(region: &LoopRegion, pc: u32, instruction: WideInstruction) -> Result<Decoded, String> {
    let plain = |decoded: Decoded| (instruction.result_flags() == 0).then_some(decoded);
    let op = instruction.op();
    let decoded = match op.class() {
        Class::Move => plain(Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None)),
        // A Number operand (the entry guards and the region's own rows prove it) is its own numeric value.
        Class::ToNumeric => plain(Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None)),
        Class::LoadConst => load_const(region, instruction).ok().and_then(plain),
        Class::Binary | Class::NumericAdd | Class::NumericMultiply => binary(region, instruction).ok().and_then(plain),
        Class::IncDec => inc_dec(instruction).ok().and_then(plain),
        Class::JumpBinaryFalse => jump_binary(region, instruction).ok(),
        Class::Jump => Some(Decoded::number(Kind::Jump, None, None, None, Some(instruction.jump_target()))),
        _ => heap_row(region, pc, instruction).and_then(plain),
    };
    decoded.ok_or_else(|| match op.class() {
        Class::GetField => "GetField: dynamic or unrecorded".to_string(),
        Class::Binary | Class::NumericAdd | Class::NumericMultiply => binary(region, instruction).err().unwrap_or_else(|| format!("{op:?}")),
        Class::JumpBinaryFalse => jump_binary(region, instruction).err().unwrap_or_else(|| format!("{op:?}")),
        Class::LoadConst => load_const(region, instruction).err().unwrap_or_else(|| format!("{op:?}")),
        _ => format!("{op:?}"),
    })
}

/// The high 32 bits of an int word.
const INT_HIGH: u64 = INT_TAG >> 32;

/// How the values of a register are held, from the narrowest to the widest: a raw int32, a
/// Number as `f64`, a tagged word. A register takes the widest of what its writers produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Repr {
    Unseen,
    Int,
    Double,
    Tagged,
}

type Reprs = BTreeMap<Register, Repr>;

fn operand_repr(reprs: &Reprs, source: Option<Source>) -> Repr {
    match source {
        Some(Source::Register(register)) => reprs.get(&register).copied().unwrap_or(Repr::Tagged),
        Some(Source::Constant(word)) if word >> 32 == INT_HIGH => Repr::Int,
        Some(Source::Constant(_)) => Repr::Double,
        None => Repr::Unseen,
    }
}

/// The representation a row's result needs: a division, a double operand or a `widened` row
/// (its int32 speculation failed) gives a double; a bitwise row an int32 unless widened.
fn result_repr(row: &Decoded, reprs: &Reprs, widened: bool, numbers: bool) -> Repr {
    if row.heap.is_some() {
        return match (numbers, widened) {
            (false, _) => Repr::Tagged,
            (true, false) => Repr::Int,
            (true, true) => Repr::Double,
        };
    }
    let operands = operand_repr(reprs, row.a).max(operand_repr(reprs, row.b));
    match row.kind {
        _ if operands == Repr::Tagged => Repr::Tagged,
        Kind::Move | Kind::Const => operands,
        Kind::Div => Repr::Double,
        Kind::Add | Kind::Sub | Kind::Mul => if widened || operands == Repr::Double { Repr::Double } else { Repr::Int },
        _ if widened => Repr::Tagged,
        _ => Repr::Int,
    }
}

/// The representation of every register the region names, to a fixed point: where the
/// compile began (`ints`, `doubles`; dead at the header: none yet), then each writer's result.
fn representations(region: &LoopRegion, decoded: &[Decoded], widened: &[usize], seen: [&[Register]; 2]) -> Reprs {
    let header = region.header;
    let mut reprs = Reprs::new();
    for row in decoded {
        for register in row.registers() {
            let start = match (region.is_live(header, register), seen[0].contains(&register), seen[1].contains(&register)) {
                (false, ..) => Repr::Unseen,
                (_, true, _) => Repr::Int,
                (_, _, true) => Repr::Double,
                _ => Repr::Tagged,
            };
            reprs.insert(register, start);
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for (k, row) in decoded.iter().enumerate() {
            let Some(dst) = row.dst else { continue };
            let result = result_repr(row, &reprs, widened.contains(&(header as usize + k)), region.number_registers().contains(&dst));
            if reprs[&dst] < result {
                reprs.insert(dst, result);
                changed = true;
            }
        }
    }
    reprs
}

/// Whether the row takes general (tagged word) semantics: its result or an operand of a
/// relation is no int32.
fn is_general(row: &Decoded, reprs: &Reprs) -> bool {
    match row.dst {
        Some(dst) => reprs[&dst] != Repr::Int,
        None => operand_repr(reprs, row.a).max(operand_repr(reprs, row.b)) >= Repr::Double,
    }
}

/// The registers of `repr`, the most named first.
fn ranked(decoded: &[Decoded], reprs: &Reprs, repr: Repr) -> Vec<Register> {
    let mut uses: BTreeMap<Register, usize> = BTreeMap::new();
    for row in decoded {
        for register in row.registers() {
            *uses.entry(register).or_default() += 1;
        }
    }
    let mut ranked: Vec<_> = uses.into_iter().filter(|(register, _)| reprs[register] == repr).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked.into_iter().map(|(register, _)| register).collect()
}

fn register_of(source: Source) -> Option<Register> {
    match source {
        Source::Register(register) => Some(register),
        Source::Constant(_) => None,
    }
}

/// Where the registers of the region live: pinned (int32 or double), int slots, or tagged
/// frame slots.
struct Homes {
    pins: Vec<Pin>,
    floats: Vec<Pin>,
    slots: Vec<Register>,
}

impl Homes {
    fn of(ints: Vec<Register>, doubles: Vec<Register>) -> Self {
        let pinned = |registers: &[Register], count| registers.iter().take(count).copied().enumerate().collect();
        Self { pins: pinned(&ints, PINNED), floats: pinned(&doubles, PINNED_F), slots: ints.into_iter().skip(PINNED).collect() }
    }

    fn place(&self, register: Register) -> Place {
        let find = |pins: &[Pin]| pins.iter().find(|(_, pinned)| *pinned == register).map(|(pin, _)| *pin as u8);
        if let Some(pin) = find(&self.pins) {
            return Place::Pin(pin);
        }
        if let Some(pin) = find(&self.floats) {
            return Place::PinF(pin);
        }
        if self.slots.contains(&register) { Place::MemInt } else { Place::Mem }
    }
}

/// The place and hole of an operand: an int32 constant is an immediate.
fn operand(homes: &Homes, source: Option<Source>) -> (Place, u64) {
    match source {
        Some(Source::Register(register)) => (homes.place(register), u64::from(register) * SLOT),
        Some(Source::Constant(word)) if word >> 32 == INT_HIGH => (Place::Imm, word & u64::from(u32::MAX)),
        Some(Source::Constant(word)) => (Place::Const, word),
        None => (Place::Mem, 0),
    }
}

/// A row placed in the program: its stencil, hole values and branch target.
struct Planned {
    stencil: &'static Stencil,
    holes: [u64; 3],
    target: Option<u32>,
    /// The PC a guard of the row exits at.
    exit_pc: Option<u32>,
}

fn plan(decoded: &Decoded, homes: &Homes, pc: u32, general: bool) -> Result<Planned, String> {
    let (a, a_hole) = operand(homes, decoded.a);
    let (b, b_hole) = operand(homes, decoded.b);
    let (dst, dst_hole) = operand(homes, decoded.dst.map(Source::Register));
    // A double row reads an int32 constant as the double it converts to: a constant word, which no code folds.
    let double = |place, hole: u64| if place == Place::Imm { (Place::Const, f64::from(hole as u32 as i32).to_bits()) } else { (place, hole) };
    let floats = [dst, a, b].iter().any(|place| matches!(place, Place::PinF(_))) && decoded.heap.is_none();
    let ((a, a_hole), (b, b_hole)) = if floats { (double(a, a_hole), double(b, b_hole)) } else { ((a, a_hole), (b, b_hole)) };
    if let Some(heap) = decoded.heap {
        let base = decoded.base.map_or(0, |register| u64::from(register) * SLOT);
        let holes = match heap {
            Heap::Store => [base, a_hole, b_hole],
            Heap::Field => [dst_hole, decoded.hole, 0],
            _ => [dst_hole, a_hole, b_hole],
        };
        return Ok(Planned { stencil: fast_rows::stencil(HeapRow { heap, dst, a, b })?, holes, target: None, exit_pc: Some(pc) });
    }
    let polls = decoded.target.is_some_and(|target| target <= pc) && decoded.kind == Kind::Jump;
    let kind = if polls { Kind::JumpPoll } else { decoded.kind };
    let stencil = numeric::stencil(numeric::Row { kind, dst, a, b, speculate: !general })?;
    let interrupt = std::mem::offset_of!(super::CallContext, interrupt) as u64;
    let exits = polls || !matches!(kind, Kind::Jump | Kind::Move) || matches!(dst, Place::Pin(_) | Place::PinF(_) | Place::MemInt);
    Ok(Planned {
        stencil,
        holes: [if polls { interrupt } else { dst_hole }, a_hole, b_hole],
        target: decoded.target,
        exit_pc: exits.then_some(pc),
    })
}

/// A leave row: the stub that tags the pinned registers back and returns `word`.
fn leave_row(code: Vec<u8>) -> Stencil {
    Stencil { raw_len: code.len(), code, relocs: Vec::new(), imm32: Vec::new(), next_jump: None, tail_len: 0 }
}

/// The machine code of `region`, entered at the shared native entry ABI, or the instruction that
/// has no row. `ints` are the registers that held an int32 as the compile began; `widened` lists the PCs whose int32 speculation failed: they take general rows.
pub(super) fn build(region: &LoopRegion, widened: &[usize], seen: [&[Register]; 2]) -> Result<Vec<u8>, String> {
    let started = std::time::Instant::now();
    let built = assemble(region, widened, seen);
    let nanos = started.elapsed().as_nanos() as u64;
    note(|totals, rejects| match &built {
        Ok(code) => {
            totals.regions += 1;
            totals.ops += u64::from(region.backedge - region.header) + 1;
            totals.bytes += code.len() as u64;
            totals.nanos += nanos;
        }
        Err(name) => {
            totals.rejected += 1;
            *rejects.entry(name.clone()).or_default() += 1;
        }
    });
    built
}

/// What the linker needs of the region: its span, the registers live at a PC, and the entry
/// loads.
struct Layout<'a> {
    header: u32,
    backedge: u32,
    homes: Homes,
    region: &'a LoopRegion,
}

impl Layout<'_> {
    /// The pinned registers live at `pc`: the ones a stub there writes back.
    fn live_pins(&self, pc: u32) -> Vec<Pin> {
        self.homes.pins.iter().copied().filter(|(_, register)| self.region.is_live(pc, *register)).collect()
    }

    fn live_floats(&self, pc: u32) -> Vec<Pin> {
        self.homes.floats.iter().copied().filter(|(_, register)| self.region.is_live(pc, *register)).collect()
    }

    /// The code that leaves with `word`: spills the live pinned doubles (canonical words), tags
    /// the live pinned int32s back, and returns.
    fn exit_code(&self, pc: u32, word: u64) -> Result<Vec<u8>, String> {
        self.chain(pc, stub(&self.live_pins(pc), word))
    }

    /// The routine a row's exit stub jumps to: as `exit_code`, returning the PC on the stack.
    fn routine(&self, pc: u32) -> Result<Vec<u8>, String> {
        let mut tail = tag_back(&self.live_pins(pc));
        tail.extend(POP_RET);
        self.chain(pc, tail)
    }

    fn chain(&self, pc: u32, ending: Vec<u8>) -> Result<Vec<u8>, String> {
        let tail = leave_row(ending);
        let spills = self.homes.floats.iter().filter(|(_, register)| self.region.is_live(pc, *register));
        let mut chain = Vec::new();
        for (pin, register) in spills {
            let row = numeric::Row { kind: Kind::Spill, dst: Place::Mem, a: Place::PinF(*pin as u8), b: Place::Mem, speculate: false };
            chain.push((numeric::stencil(row)?, [u64::from(*register) * SLOT, 0, 0]));
        }
        let holes: Vec<_> = chain.iter().map(|(_, holes)| *holes).chain([[0; 3]]).collect();
        let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> = holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
        let rows: Vec<Placed> = chain
            .iter()
            .map(|(stencil, _)| *stencil)
            .chain([&tail])
            .enumerate()
            .map(|(k, stencil)| Placed { stencil, hole: &*hole_fns[k], taken: None })
            .collect();
        let mut code = vec![0u8; rows.iter().map(|row| row.stencil.code.len()).sum()];
        // SAFETY: `code` has room for every stencil's full length, which bounds the chain.
        let (len, _) = unsafe { quench_fastjit::copy_patch(&rows, code.as_mut_ptr(), |_, _| 0) };
        code.truncate(len);
        Ok(code)
    }
}

fn assemble(region: &LoopRegion, widened: &[usize], seen: [&[Register]; 2]) -> Result<Vec<u8>, String> {
    let (header, backedge) = (region.header, region.backedge);
    let count = (backedge - header) as usize + 1;
    let instructions = region.instructions().get(..count).ok_or("region instructions")?;
    let decoded = instructions.iter().enumerate().map(|(k, instruction)| decode(region, header + k as u32, *instruction)).collect::<Result<Vec<_>, _>>()?;
    let reprs = representations(region, &decoded, widened, seen);
    let homes = Homes::of(ranked(&decoded, &reprs, Repr::Int), ranked(&decoded, &reprs, Repr::Double));
    let layout = Layout { header, backedge, homes, region };
    let mut body = Vec::with_capacity(count);
    for (k, row) in decoded.iter().enumerate() {
        body.push(plan(row, &layout.homes, header + k as u32, is_general(row, &reprs))?);
    }
    link(&layout, &body)
}

/// The entry rows: each pinned register live at the header loads its int32, each int slot is
/// checked (or, dead there, tagged), and each other Number register live there is checked, so a
/// loop whose values are no Numbers is never entered.
fn entry_rows(layout: &Layout) -> Result<Vec<(&'static Stencil, [u64; 3])>, String> {
    let (region, header) = (layout.region, layout.header);
    let guard = |kind, dst, offset: u64| {
        numeric::stencil(numeric::Row { kind, dst, a: Place::Mem, b: Place::Mem, speculate: true }).map(|stencil| {
            (stencil, if kind == Kind::TagInit { [offset, 0, 0] } else { [0, offset, 0] })
        })
    };
    let mut rows = Vec::new();
    for (pin, register) in layout.homes.pins.iter().filter(|(_, register)| region.is_live(header, *register)) {
        rows.push(guard(Kind::Guard, Place::Pin(*pin as u8), u64::from(*register) * SLOT)?);
    }
    for (pin, register) in layout.homes.floats.iter().filter(|(_, register)| region.is_live(header, *register)) {
        rows.push(guard(Kind::Guard, Place::PinF(*pin as u8), u64::from(*register) * SLOT)?);
    }
    for register in &layout.homes.slots {
        let offset = u64::from(*register) * SLOT;
        rows.push(if region.is_live(header, *register) { guard(Kind::Guard, Place::MemInt, offset)? } else { guard(Kind::TagInit, Place::MemInt, offset)? });
    }
    for register in region.number_registers() {
        if layout.homes.place(*register) == Place::Mem && region.is_live(header, *register) {
            rows.push(guard(Kind::Guard, Place::Mem, u64::from(*register) * SLOT)?);
        }
    }
    Ok(rows)
}

/// Lays the entry rows, rows and leave stubs out as one code buffer behind the entry thunk.
fn link(layout: &Layout, body: &[Planned]) -> Result<Vec<u8>, String> {
    let (header, backedge) = (layout.header, layout.backedge);
    let entry = entry_rows(layout)?;
    // Leave stubs: the fall-through past the backedge, the failed entry (nothing to write
    // back), then each branch target outside the region.
    let mut leaves: Vec<(u64, Vec<u8>)> = vec![
        (u64::from(backedge) + 1, layout.exit_code(backedge + 1, u64::from(backedge) + 1)?),
        (super::LaneExit::Entry.word(), stub(&[], super::LaneExit::Entry.word())),
    ];
    let (g, n) = (entry.len(), body.len());
    let mut targets: Vec<Option<usize>> = Vec::with_capacity(n);
    for row in body {
        targets.push(match row.target {
            Some(pc) if (header..=backedge).contains(&pc) => Some(g + (pc - header) as usize),
            Some(pc) => Some(g + n + leaf_index(layout, &mut leaves, pc)?),
            None => None,
        });
    }
    let leave_stencils: Vec<Stencil> = leaves.into_iter().map(|(_, code)| leave_row(code)).collect();
    let mut holes: Vec<[u64; 3]> = entry.iter().map(|(_, holes)| *holes).collect();
    holes.extend(body.iter().map(|row| row.holes));
    holes.extend(std::iter::repeat_n([0; 3], leave_stencils.len()));
    let hole_fns: Vec<Box<dyn Fn(u32) -> u64 + '_>> =
        holes.iter().map(|h| Box::new(move |i: u32| h[i as usize]) as Box<dyn Fn(u32) -> u64>).collect();
    let mut rows: Vec<Placed> = Vec::with_capacity(g + n + leave_stencils.len());
    rows.extend(entry.iter().enumerate().map(|(k, (stencil, _))| Placed { stencil, hole: &*hole_fns[k], taken: Some(g + n + 1) }));
    rows.extend(body.iter().enumerate().map(|(k, row)| Placed { stencil: row.stencil, hole: &*hole_fns[g + k], taken: targets[k] }));
    rows.extend(leave_stencils.iter().enumerate().map(|(k, stencil)| Placed { stencil, hole: &*hole_fns[g + n + k], taken: None }));
    // One routine per set of live pinned registers; each exiting row's stub names its PC.
    let mut routines: Vec<(Vec<Pin>, Vec<Pin>, Vec<u8>)> = Vec::new();
    let mut exits: Vec<(usize, u32, usize)> = Vec::new();
    for (k, row) in body.iter().enumerate() {
        let Some(pc) = row.exit_pc else { continue };
        let (ints, floats) = (layout.live_pins(pc), layout.live_floats(pc));
        let found = routines.iter().position(|(i, f, _)| *i == ints && *f == floats);
        let at = match found {
            Some(at) => at,
            None => {
                routines.push((ints, floats, layout.routine(pc)?));
                routines.len() - 1
            }
        };
        exits.push((g + k, pc, at));
    }
    let routines: Vec<Vec<u8>> = routines.into_iter().map(|(_, _, code)| code).collect();
    place_rows(&rows, &exits, &routines)
}

fn leaf_index(layout: &Layout, leaves: &mut Vec<(u64, Vec<u8>)>, pc: u32) -> Result<usize, String> {
    let word = u64::from(pc);
    if let Some(found) = leaves.iter().position(|(candidate, _)| *candidate == word) {
        return Ok(found);
    }
    leaves.push((word, layout.exit_code(pc, word)?));
    Ok(leaves.len() - 1)
}

/// Copies the rows behind the thunk, then the exit routines and each row's stub, patching each
/// row's exit to its stub; the code is position independent, so the buffer can move to
/// executable memory.
fn place_rows(rows: &[Placed], exits: &[(usize, u32, usize)], routines: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let head = thunk(0).len();
    let bound: usize = rows.iter().map(|row| row.stencil.code.len()).sum();
    let tail: usize = routines.iter().map(Vec::len).sum::<usize>() + exits.len() * EXIT_STUB_LEN;
    let mut code = vec![0u8; head + bound + tail];
    let base = code.as_mut_ptr();
    // SAFETY: `base + head` has room for every stencil's full length, which bounds the rows.
    let (len, _) = unsafe { quench_fastjit::copy_patch(rows, base.add(head), |_, _| base as usize) };
    let mut at = head + len;
    let mut routine_at = Vec::with_capacity(routines.len());
    for routine in routines {
        code[at..at + routine.len()].copy_from_slice(routine);
        routine_at.push(at);
        at += routine.len();
    }
    let mut addresses = vec![base as usize; rows.len()];
    for (row, pc, routine) in exits {
        addresses[*row] = base as usize + at;
        let displacement = routine_at[*routine] as i64 - (at + EXIT_STUB_LEN) as i64;
        code[at] = PUSH_IMM32;
        code[at + 1..at + 5].copy_from_slice(&pc.to_le_bytes());
        code[at + 5] = JMP_REL32;
        code[at + 6..at + 10].copy_from_slice(&(displacement as i32).to_le_bytes());
        at += EXIT_STUB_LEN;
    }
    // SAFETY: as above; the stubs lie after the copied code inside `code`.
    unsafe { quench_fastjit::copy_patch(rows, base.add(head), |row, _| addresses[row]) };
    let thunk_bytes = thunk(head);
    code[..head].copy_from_slice(&thunk_bytes);
    code.truncate(at);
    Ok(code)
}
