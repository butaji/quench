//! The loop tier: Cranelift code for a hot loop's supported body, entered at its backward
//! jump in compiled execution. An effectful call ends a native prefix; the lane resumes at
//! the call and returns to the loop tier at the next backward jump.
//!
//! A region is the loop's instructions, header to backward jump, or the supported prefix
//! before its first dynamic call. Its native instructions read and write Numbers in
//! registers, depth-0 captures and dense elements of record arrays: it reads present
//! elements, and stores below an array's length or appends at it within the capacity its
//! backing already has.
//! Native code keeps each Number register in an SSA value: an `i32` where every value it
//! takes is one (`representations`), else an `f64`. Registers are the
//! interpreter's state at every instruction boundary the code leaves at, so leaving is
//! writing them back:
//!
//! - entry guards every register the region uses: a Number, or a record array the region
//!   reads (and, for one it writes, a dense array whose backing it alone owns and no
//!   prototype of which holds an index, so a store defines or replaces an own data element);
//!   otherwise nothing runs and the interpreter takes the jump;
//! - a jump out of the region resumes the lane at its target;
//! - an instruction whose operands leave the subset (a hole, a non-Number element, an index
//!   past the length, an integer conversion out of range) exits before its effects, and the
//!   general path executes it;
//! - every backward jump polls host termination, exiting at the jump so the interpreter's
//!   safepoint runs.
//!
//! Nothing in a native region allocates, collects, or runs guest code, so the arrays, their
//! element storage and the captures the entry read stay as they are while native code runs.

use crate::bytecode::{
    FieldBase, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand,
    OperandKind, Register, WideInstruction,
};
use crate::compile::liveness::RegisterSet;
use crate::value::{Value, ValueLayout};
use super::super::operations::{NumberArithmetic, NumberBitwise, NumberUnary};
use cranelift_codegen::ir::{
    self, InstBuilder, UserFuncName,
    condcodes::{FloatCC, IntCC},
    types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use oxc_ast::ast::{BinaryOperator, UnaryOperator};

/// The most instructions a region spans. Native code polls termination only at backward
/// jumps, so the bound keeps compile time and code size proportional to small loop bodies.
const MAX_REGION_INSTRUCTIONS: usize = 512;
/// The most registers, captures and arrays a region names, bounding entry guards and the
/// entry record.
const MAX_REGION_REGISTERS: usize = 128;
const MAX_REGION_ARRAYS: usize = 8;
const MAX_REGION_CAPTURES: usize = 16;
const MAX_REGION_FIELDS: usize = 16;
const MAX_REGION_FIELD_STORES: usize = 16;
/// The largest native body a region may produce.
pub(super) const MAX_CODE_BYTES: usize = 64 * 1024;

/// What native code writes to its result word: the outcome in the high word, a bytecode PC
/// in the low one.
/// The outcome of one native run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopOutcome {
    /// An entry guard failed: registers are untouched, and the interpreter takes the jump.
    /// A representation guard names what the compilation assumed wrongly.
    NotEntered(Option<Widening>),
    /// Control left the region by a jump to this PC, which the lane runs next.
    Resume(u32),
    /// The instruction at this PC needs the general path, which executes it.
    Exit(u32),
    /// As `Exit`, because a value the code held as an `i32` is none; the next compilation
    /// widens what the code names for the PC (`CompiledLoop::widening`).
    Widen(u32),
    /// Termination was requested while the code ran: its poll faulted, so registers may
    /// hold values of an unfinished iteration, which nothing reads before the request ends
    /// the execution.
    Terminated,
}

/// A value a compilation held as an `i32` that turned out not to be one; the next
/// compilation holds it as an `f64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Widening {
    Register(Register),
    /// A capture, by its index in the region's capture order.
    Capture(u16),
    /// A Number field, by its index in the region's fields (`LoopRegion::fields`).
    Field(u16),
    /// The elements the region reads from the array in this register.
    Elements(Register),
    /// A register the region never writes, held as exactly 1 so that multiplying or
    /// dividing by it is the identity (`Lowering::units`), held something else at entry.
    Unit(Register),
}

/// A not-entered outcome's low word: zero, or a widening's kind bit over its payload.
const WIDEN_REGISTER: u64 = 1 << u16::BITS;
const WIDEN_CAPTURE: u64 = WIDEN_REGISTER << 1;
const WIDEN_UNIT: u64 = WIDEN_CAPTURE << 1;
const WIDEN_FIELD: u64 = WIDEN_UNIT << 1;
const _: () = assert!(WIDEN_FIELD <= u32::MAX as u64);
const WIDEN_PAYLOAD: u64 = WIDEN_REGISTER - 1;

impl Widening {
    /// The not-entered word of an entry guard's widening; element reads are guarded where
    /// they run, so an entry never names them.
    fn code(self) -> u32 {
        match self {
            Self::Register(register) => (WIDEN_REGISTER | u64::from(register)) as u32,
            Self::Capture(index) => (WIDEN_CAPTURE | u64::from(index)) as u32,
            Self::Unit(register) => (WIDEN_UNIT | u64::from(register)) as u32,
            Self::Field(index) => (WIDEN_FIELD | u64::from(index)) as u32,
            Self::Elements(_) => 0,
        }
    }

    fn decode(word: u64) -> Option<Self> {
        let payload = (word & WIDEN_PAYLOAD) as u16;
        if word & WIDEN_REGISTER != 0 {
            Some(Self::Register(payload))
        } else if word & WIDEN_CAPTURE != 0 {
            Some(Self::Capture(payload))
        } else if word & WIDEN_UNIT != 0 {
            Some(Self::Unit(payload))
        } else if word & WIDEN_FIELD != 0 {
            Some(Self::Field(payload))
        } else {
            None
        }
    }
}

impl LoopOutcome {
    pub(super) fn decode(word: u64) -> Self {
        match super::RegionExit::decode(word) {
            super::RegionExit::EntryMiss(payload) => Self::NotEntered(Widening::decode(u64::from(payload))),
            super::RegionExit::Resume(pc) => Self::Resume(pc),
            super::RegionExit::Fallback(pc) => Self::Exit(pc),
            super::RegionExit::Recompile(pc) => Self::Widen(pc),
        }
    }
}

/// An array a region indexes, by its register; `written` when the region stores into it.
/// The array is the register's value at entry, or, with `field`, the value of that cached
/// field (`LoopRegion::fields`), which the field's one `GetField` in the region writes to
/// the register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrayUse {
    pub(crate) register: Register,
    pub(crate) written: bool,
    pub(crate) field: Option<usize>,
}

/// A loop's admissible region and the facts its entry needs.
pub(crate) struct LoopRegion {
    pub(crate) header: u32,
    /// Last instruction lowered in this region. A dynamic call can end a guarded prefix
    /// before the loop's actual backedge; control resumes at that call in the interpreter.
    end: u32,
    pub(crate) backedge: u32,
    /// The Number registers; a register's index here names its entry definition (`Webs`).
    numbers: Vec<Register>,
    /// Registers held as tagged `Value` words in the shared register web.
    tagged: Vec<Register>,
    /// The arrays, in entry-record order. Entry admits one the region writes only when it is
    /// dense (`Vm::dense_array_length`), owns its backing alone, and no prototype of it holds
    /// an index (`Vm::prototypes_hold_no_index`).
    pub(crate) arrays: Vec<ArrayUse>,
    /// The depth-0 capture slots `LoadCapture` reads, in entry-record order.
    pub(crate) captures: Vec<u16>,
    /// The cached data fields the region reads, in entry-record order: a field read
    /// through another's value follows it (`intern_field`).
    pub(crate) fields: Vec<FieldUse>,
    /// The field each `GetField` of the region reads, by offset from the header.
    field_reads: std::collections::BTreeMap<u32, usize>,
    /// Existing own-data Number slots stored by the region, by offset from its header.
    field_stores: std::collections::BTreeMap<u32, usize>,
    /// Receivers and cache sites whose flat slots are guarded before native entry.
    pub(crate) field_store_sites: Vec<FieldStore>,
    /// The registers that hold a field's value at every read (`held_registers`), with the
    /// path they hold and that field's index.
    held: std::collections::BTreeMap<Register, (FieldPath, usize)>,
    /// Strict identity branches against `null` or `undefined` that read an unchanged
    /// tagged register directly from the activation's rooted register file.
    identity_branches: std::collections::BTreeMap<u32, IdentityBranch>,
    /// The region's instructions, from the header.
    instructions: Vec<WideInstruction>,
    /// The Number constants the region reads, by constant index.
    constants: std::collections::BTreeMap<u16, f64>,
    /// Tagged constants the region moves, by constant index.
    tagged_constants: std::collections::BTreeMap<u16, Value>,
    /// The function's live registers before each instruction (`compile::liveness`), when
    /// liveness covers it; for an inlining region, by its own PCs (`Inlining`).
    live: Option<crate::compile::liveness::RegisterMasks>,
    /// Where an inlining region's instructions come from, when the region inlines calls.
    pub(crate) inlining: Option<Box<Inlining>>,
}

/// Why a backward jump was excluded from OSR before native code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionRejection {
    RegionTooLarge,
    UnsupportedInstruction(Op),
    RegisterLimit,
    CaptureLimit,
    ArrayLimit,
    FieldLimit,
    FieldStoreLimit,
    FieldMutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldReceiver {
    This,
    /// A register the region never writes.
    Register(Register),
    /// The value of the region's field at this index (`LoopRegion::fields`), an earlier one.
    Field(usize),
}

/// What a `GetField` reads: named fields one after another from a root receiver (`this` or
/// a register), each step with the cache site that answers it.
#[derive(Clone, Debug)]
struct FieldPath {
    root: FieldReceiver,
    steps: Vec<(crate::bytecode::Atom, u16)>,
}

impl FieldPath {
    /// Whether the two paths read one value in a region, which writes no field: the same
    /// root and names, whichever caches answer them.
    fn reads_as(&self, other: &FieldPath) -> bool {
        self.root == other.root && self.steps.iter().map(|step| step.0).eq(other.steps.iter().map(|step| step.0))
    }
}

/// The base a `GetField` reads its first field from, its own or its nested site's.
fn field_base(instruction: WideInstruction, sites: &[FieldSite]) -> Option<FieldBase> {
    match instruction.field_lookup()? {
        FieldLookup::Atom { base, .. } => Some(base),
        FieldLookup::Site(index) => Some(sites.get(index)?.base),
    }
}

/// The path a `GetField` reads, a register base that `held` names (`held_registers`) read
/// through the path that gave it its value; a nested site (`FieldLookup::Site`) is its steps.
fn field_path(
    instruction: WideInstruction,
    sites: &[FieldSite],
    held: &std::collections::BTreeMap<Register, FieldPath>,
) -> Option<FieldPath> {
    (instruction.op() == Op::GetField && instruction.result_flags() == 0).then_some(())?;
    let from = |base: FieldBase| -> Option<FieldPath> {
        if base == FieldBase::THIS {
            return Some(FieldPath { root: FieldReceiver::This, steps: Vec::new() });
        }
        let register = base.register_index()?;
        Some(held.get(&register).cloned().unwrap_or(FieldPath { root: FieldReceiver::Register(register), steps: Vec::new() }))
    };
    match instruction.field_lookup()? {
        FieldLookup::Atom { atom, base, cache_site } => {
            let mut path = from(base)?;
            path.steps.push((atom, cache_site));
            Some(path)
        }
        FieldLookup::Site(index) => {
            let site = sites.get(index)?;
            site.sink.is_none().then_some(())?;
            let mut path = from(site.base)?;
            path.steps.push(site.first);
            path.steps.extend(site.second);
            Some(path)
        }
    }
}

/// The index in `fields` of the field whose value `path` reads, adding each step the
/// fields lack after the one it reads from: every step but the last is a receiver, so
/// tagged, and the last is `tagged` as its read's result.
fn intern_field(fields: &mut Vec<FieldUse>, path: &FieldPath, tagged: bool) -> usize {
    let mut receiver = path.root;
    let mut index = usize::MAX;
    for (step, (atom, cache_site)) in path.steps.iter().copied().enumerate() {
        let tagged = tagged || step + 1 < path.steps.len();
        index = match fields
            .iter()
            .position(|field| field.receiver == receiver && field.atom == atom && field.tagged == tagged)
        {
            Some(index) => index,
            None => {
                fields.push(FieldUse { receiver, atom, cache_site, tagged });
                fields.len() - 1
            }
        };
        receiver = FieldReceiver::Field(index);
    }
    index
}

/// The registers a region reads as objects (`object_reads`, and `objects`) that hold a
/// field's value at every read: each definition of one in the region is a `GetField` of
/// one path, and none is live at the header (`header_live`), so every read follows a
/// definition. As the region writes no field, the value is the one the path reads at
/// entry. With no liveness, none is.
fn held_registers(
    instructions: &[WideInstruction],
    sites: &[FieldSite],
    header_live: Option<RegisterSet>,
    objects: &std::collections::BTreeSet<Register>,
) -> std::collections::BTreeMap<Register, FieldPath> {
    let mut held = std::collections::BTreeMap::new();
    let Some(header_live) = header_live else {
        return held;
    };
    let definitions = |register: Register| {
        instructions.iter().copied().filter(move |instruction| number_definition(*instruction) == Some(register))
    };
    let mut pending: std::collections::BTreeSet<Register> = instructions
        .iter()
        .copied()
        .filter_map(object_reads)
        .chain(objects.iter().copied())
        .filter(|register| {
            !crate::compile::liveness::register_in(header_live, usize::from(*register))
                && definitions(*register).next().is_some()
                && definitions(*register).all(|instruction| instruction.op() == Op::GetField)
        })
        .collect();
    // A path through a register still pending waits for it; a register whose definitions
    // read different paths, or wait on one another, holds no field.
    loop {
        let mut decided = Vec::new();
        for register in pending.iter().copied() {
            let mut paths = Vec::new();
            for instruction in definitions(register) {
                let waits = field_base(instruction, sites).and_then(FieldBase::register_index);
                if waits.is_some_and(|base| pending.contains(&base)) {
                    paths.clear();
                    break;
                }
                paths.push(field_path(instruction, sites, &held));
            }
            if paths.is_empty() {
                continue;
            }
            let first = paths[0].clone();
            let one = first.as_ref().filter(|first| paths.iter().all(|path| path.as_ref().is_some_and(|path| path.reads_as(first))));
            decided.push((register, one.cloned()));
        }
        if decided.is_empty() {
            return held;
        }
        for (register, path) in decided {
            pending.remove(&register);
            if let Some(path) = path {
                held.insert(register, path);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldUse {
    pub(crate) receiver: FieldReceiver,
    pub(crate) atom: crate::bytecode::Atom,
    pub(crate) cache_site: u16,
    pub(crate) tagged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldStore {
    pub(crate) receiver: Register,
    pub(crate) atom: crate::bytecode::Atom,
    pub(crate) cache_site: u16,
}

#[derive(Clone, Copy)]
struct IdentityBranch {
    register: Register,
    constant: Value,
    negated: bool,
}

/// A branch whose exact identity comparison can be emitted from the tagged value word.
/// Equality to null/undefined has no coercion or heap effect, and comparison against these
/// constants cannot match a Number, string, BigInt, or object by contents.
fn identity_branch(instruction: WideInstruction, constants: &[Value]) -> Option<IdentityBranch> {
    if instruction.op() != Op::JumpBinaryFalse {
        return None;
    }
    let (strict, negated) =
        super::super::operations::equality_operator(instruction.binary_operator_field())?;
    strict.then_some(())?;
    let (register, constant) = match (instruction.operand_b().kind()?, instruction.operand_c().kind()?) {
        (OperandKind::Register, OperandKind::Constant) => (
            instruction.operand_b().payload(),
            *constants.get(usize::from(instruction.operand_c().payload()))?,
        ),
        (OperandKind::Constant, OperandKind::Register) => (
            instruction.operand_c().payload(),
            *constants.get(usize::from(instruction.operand_b().payload()))?,
        ),
        _ => return None,
    };
    (constant.is_null() || constant.is_undefined()).then_some(IdentityBranch {
        register,
        constant,
        negated,
    })
}

/// Registers whose bytecode definitions must stay tagged because a nullish identity
/// branch observes them. Propagate that fact backward only through moves and constants;
/// other definitions need a general value denotation before they can be admitted.
fn tagged_registers(
    instructions: &[WideInstruction],
    constants: &[Value],
    held: &std::collections::BTreeSet<Register>,
) -> Option<std::collections::BTreeSet<Register>> {
    let mut tagged: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| identity_branch(*instruction, constants).map(|branch| branch.register))
        .chain(held.iter().copied())
        .collect();
    loop {
        let before = tagged.len();
        for instruction in instructions.iter().rev().copied() {
            let Some(result) = number_definition(instruction) else {
                continue;
            };
            if !tagged.contains(&result) {
                continue;
            }
            match instruction.op() {
                Op::Move => {
                    tagged.insert(instruction.register_b());
                }
                Op::LoadConst => {
                    let index = u16::try_from(instruction.constant_index()).ok()?;
                    constants.get(usize::from(index))?;
                }
                // A cached field's value, which the entry record carries tagged.
                Op::GetField => {}
                _ => return None,
            }
        }
        if tagged.len() == before {
            return Some(tagged);
        }
    }
}

/// A call an inlining region runs in place (`inline_region`): the callee's instructions in
/// the caller's region, its registers above the caller's from `base`, its numeric
/// parameters the call's arguments and the others the caller registers they were moved
/// from, its receiver the caller's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InlineFrame {
    /// The caller's PC of the call.
    pub(crate) call: u32,
    /// The function inlined, which the cached field `field` (`LoopRegion::fields`) held when
    /// the region was made: the entry guards it still does.
    pub(crate) callee: Value,
    pub(crate) field: usize,
    /// The caller register holding the call's receiver, which the region never writes.
    pub(crate) receiver: Register,
    /// The callee's register 0 in the region.
    pub(crate) base: Register,
    pub(crate) registers: u16,
    /// The callee's parameters bound to caller registers, which hold its values for them.
    pub(crate) bound: [Option<Register>; MAX_INLINE_PARAMS],
}

/// The parameters an inlined callee may have, each bound or numeric (`InlineFrame::bound`).
pub(crate) const MAX_INLINE_PARAMS: usize = 8;

/// Where an inlining region's PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of `frame`'s callee.
    Inlined { frame: u16, pc: u32 },
    /// No instruction: a jump leaving the region, which the caller continues at this PC.
    Leave(u32),
}

/// An inlining region's PCs and frames. Its instructions take consecutive PCs from the
/// header, then each jump out of the region takes one past them (`Origin::Leave`).
pub(crate) struct Inlining {
    /// By offset from the header.
    origins: Vec<Origin>,
    pub(crate) frames: Vec<InlineFrame>,
    /// The caller's register count: every register from it on is an inlined callee's.
    pub(crate) caller_registers: u16,
}

/// The exit PC native code reports for an inlined callee's instruction: its region PC with
/// this bit, which no function PC has (`MAX_REGION_INSTRUCTIONS`).
pub(crate) const INLINED_EXIT: u32 = 1 << 31;

/// What `inline_region` needs of a call's callee.
pub(crate) struct InlineCallee<'a> {
    pub(crate) callee: Value,
    pub(crate) params: u16,
    pub(crate) registers: u16,
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) live: &'a crate::compile::liveness::RegisterMasks,
}

/// The ops an inlined callee's instructions may be: those `admit` lowers, its `Return`, and
/// none that names its function's own state (captures, frame, calls).
fn inlinable(op: Op) -> bool {
    matches!(
        op,
        Op::LoadConst
            | Op::Move
            | Op::Binary
            | Op::NumericAdd
            | Op::NumericMultiply
            | Op::IncDec
            | Op::ToNumeric
            | Op::Unary
            | Op::GetIndex
            | Op::SetIndex
            | Op::GetField
            | Op::RequireObjectCoercible
            | Op::Jump
            | Op::JumpBinaryFalse
            | Op::JumpFalse
            | Op::JumpTrue
            | Op::Return
    )
}

/// `instruction` with each register `register` renames, a `this` field base made the
/// `this` register, and a jump target `target` renames; `None` for a field it cannot
/// rename, or a register with flags.
fn renamed(
    instruction: WideInstruction,
    register: &dyn Fn(Register) -> Register,
    this: Register,
    target: &dyn Fn(u32) -> Option<u32>,
) -> Option<WideInstruction> {
    let op = instruction.op();
    let rename = |field: InstructionField, raw: u16| -> Option<u16> {
        Some(match op.field_layout(field) {
            FieldLayout::Unused
            | FieldLayout::BooleanFlag
            | FieldLayout::BinaryOperator
            | FieldLayout::CacheSiteIndex
            | FieldLayout::FieldLookupCacheSiteIndex => raw,
            FieldLayout::ResultRegister
            | FieldLayout::Register
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => {
                (raw <= Operand::PAYLOAD_MASK).then_some(())?;
                register(raw)
            }
            FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
                let operand = Operand(raw);
                match operand.kind()? {
                    OperandKind::Register => Operand::register(register(operand.payload())).0,
                    OperandKind::Constant => raw,
                    OperandKind::Field | OperandKind::Local => return None,
                }
            }
            FieldLayout::FieldBase => {
                let base = FieldBase(raw);
                if base == FieldBase::THIS {
                    FieldBase::register(this).0
                } else {
                    FieldBase::register(register(base.register_index()?)).0
                }
            }
            _ => return None,
        })
    };
    let a = rename(InstructionField::A, instruction.a())?;
    let b = rename(InstructionField::B, instruction.b())?;
    let c = rename(InstructionField::C, instruction.c())?;
    let imm = match op.immediate_role() {
        ImmediateRole::JumpTarget => target(instruction.imm())?,
        _ => instruction.imm(),
    };
    Some(WideInstruction::new(op, a, b, c, imm))
}

/// The registers an instruction reads as objects: a field read's receiver, an indexed
/// access's array.
fn object_reads(instruction: WideInstruction) -> Option<Register> {
    match instruction.op() {
        Op::GetField => match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => base.register_index(),
            _ => None,
        },
        Op::GetIndex => instruction.operand_b().register_index(),
        Op::SetIndex | Op::SetField | Op::SetFieldStrict | Op::RequireObjectCoercible => {
            Some(instruction.register_b())
        }
        _ => None,
    }
}

/// The PCs of `instructions` control reaches from the first.
fn reachable(instructions: &[WideInstruction]) -> Vec<bool> {
    let mut reached = vec![false; instructions.len()];
    let mut pending = vec![0usize];
    while let Some(pc) = pending.pop() {
        if pc >= instructions.len() || reached[pc] {
            continue;
        }
        reached[pc] = true;
        crate::bytecode::for_each_instruction_successor(pc, instructions[pc], |successor| {
            pending.push(successor);
        });
    }
    reached
}

/// Whether `instruction` names `register` (a read or a write); an instruction `renamed`
/// cannot rename names every register.
fn mentions(instruction: WideInstruction, register: Register, sites: &[FieldSite]) -> bool {
    // Only a GetField carries a field lookup immediate, so the op is checked first.
    if instruction.op() == Op::GetField && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) {
        return instruction.result_register() == register
            || field_base(instruction, sites).is_none_or(|base| base.register_index() == Some(register));
    }
    let named = std::cell::Cell::new(false);
    let rename = |name: Register| {
        named.set(named.get() || name == register);
        name
    };
    renamed(instruction, &rename, register, &|target| Some(target)).is_none() || named.get()
}

/// The ops a call's argument setup may run between reading its callee and the call: none
/// has an effect a second run could repeat, so leaving among them resumes at the read.
fn pure_setup(op: Op) -> bool {
    matches!(
        op,
        Op::LoadConst
            | Op::Move
            | Op::Binary
            | Op::NumericAdd
            | Op::NumericMultiply
            | Op::IncDec
            | Op::ToNumeric
            | Op::Unary
            | Op::GetIndex
            | Op::GetField
            | Op::RequireObjectCoercible
    )
}

/// The region of the loop from `header` to `backedge` with each `Call` in it inlined: the
/// call's callee (`resolve` of the cached field it is read from) runs in place, its registers
/// above the caller's, when it is a function of plain instructions whose every parameter is
/// a Number argument or an object a caller register holds unchanged, and the whole region
/// then is one the tier lowers.
///
/// The call's setup, from its callee's read to the call, is a straight run of `pure_setup`
/// instructions writing no register live at the read: leaving there resumes at the read,
/// which reruns them. The read itself and the moves of the object arguments drop out, so the
/// registers they write, which the setup's function may reuse otherwise, hold nothing for
/// the region. Leaving at a callee's instruction makes the callee's activation
/// (`INLINED_EXIT`).
#[allow(clippy::too_many_arguments)]
pub(super) fn inline_region<'a>(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: &crate::compile::liveness::RegisterMasks,
    header: u32,
    backedge: u32,
    resolve: &mut dyn FnMut(&[FieldUse]) -> Option<InlineCallee<'a>>,
) -> Option<LoopRegion> {
    let (start, end) = (header as usize, backedge as usize);
    (start <= end && end < decoded.len()).then_some(())?;
    let caller = &decoded[start..=end];
    let defines = |register: Register| caller.iter().any(|instruction| instruction.defined_register() == Some(register));
    let targets: std::collections::BTreeSet<u32> = caller
        .iter()
        .filter(|instruction| instruction.op().immediate_role() == ImmediateRole::JumpTarget)
        .map(|instruction| instruction.imm())
        .collect();
    let mask = |pc: u32| live.get(pc as usize);
    let caller_registers = function.registers;
    let mut total = u32::from(caller_registers);
    struct Site<'b> {
        frame: InlineFrame,
        callee: InlineCallee<'b>,
        /// The callee's read.
        read: u32,
        window: Register,
        result: Register,
        reached: Vec<bool>,
    }
    let mut calls: std::collections::BTreeMap<u32, Site<'a>> = std::collections::BTreeMap::new();
    // The caller PCs whose instructions drop out, the setup PCs by the PC they resume at, and
    // the caller instructions the region runs rewritten.
    let mut dropped = std::collections::BTreeSet::new();
    let mut setup: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut rewritten: std::collections::BTreeMap<u32, WideInstruction> = std::collections::BTreeMap::new();
    for (offset, instruction) in caller.iter().copied().enumerate() {
        if instruction.op() != Op::Call {
            continue;
        }
        let pc = header + offset as u32;
        (instruction.result_flags() == 0 && !instruction.direct_eval() && !instruction.parameter_eval()).then_some(())?;
        let argc = ImmediateLayout::argument_count(instruction.imm());
        let window = ImmediateLayout::call_window_base(instruction.imm());
        let (function_register, mut receiver) = (instruction.register_b(), instruction.register_c());
        // The callee's read: the last write of its register before the call, in a straight run.
        let read_offset = (0..offset).rev().find(|index| caller[*index].defined_register() == Some(function_register))?;
        let read = header + read_offset as u32;
        let mut callee_path = field_path(caller[read_offset], sites, &std::collections::BTreeMap::new())?;
        // A receiver the region writes is the call's own: a field read right before the
        // callee's, which reads the callee from it (`this.m.am(…)`), and nothing else
        // reads until the call, nor after it. The region gives that read a register of its
        // own, holding the field's value (`held_registers`), and the setup resumes at it.
        let mut resume_offset = read_offset;
        if defines(receiver) {
            let own = read_offset.checked_sub(1)?;
            let receiver_path = field_path(caller[own], sites, &std::collections::BTreeMap::new())?;
            (caller[own].defined_register() == Some(receiver)
                && receiver_path.root != FieldReceiver::Register(receiver)
                && callee_path.root == FieldReceiver::Register(receiver)
                && callee_path.steps.len() == 1
                && caller[read_offset + 1..offset].iter().all(|instruction| !mentions(*instruction, receiver, sites))
                && !crate::compile::liveness::register_in(mask(pc + 1)?, usize::from(receiver)))
            .then_some(())?;
            let fresh = Register::try_from(total).ok()?;
            total += 1;
            rewritten.insert(header + own as u32, WideInstruction::new(Op::GetField, fresh, caller[own].b(), caller[own].c(), caller[own].imm()));
            callee_path.root = receiver_path.root;
            callee_path.steps.splice(0..0, receiver_path.steps);
            receiver = fresh;
            resume_offset = own;
        }
        let resume = header + resume_offset as u32;
        let at_resume = mask(resume)?;
        let run = &caller[resume_offset + 1..offset];
        (run.iter().all(|instruction| pure_setup(instruction.op()))
            && (resume + 1..=pc).all(|pc| !targets.contains(&pc))
            && run.iter().all(|instruction| {
                instruction
                    .defined_register()
                    .is_none_or(|register| !crate::compile::liveness::register_in(at_resume, usize::from(register)))
            }))
        .then_some(())?;
        // The callee as the read finds it now.
        let mut chain = Vec::new();
        intern_field(&mut chain, &callee_path, true);
        let callee = resolve(&chain)?;
        (callee.params == argc && usize::from(argc) <= MAX_INLINE_PARAMS).then_some(())?;
        let reached = reachable(&callee.instructions);
        let body = || callee.instructions.iter().copied().zip(reached.iter()).filter(|(_, reached)| **reached).map(|(instruction, _)| instruction);
        body().all(|instruction| inlinable(instruction.op()) && instruction.result_flags() == 0).then_some(())?;
        // A parameter read as an object is bound to the caller register its argument was
        // moved from in the setup, which the region never writes; any other is a Number it
        // copies.
        let mut bound = [None; MAX_INLINE_PARAMS];
        for parameter in 0..argc {
            if !body().any(|instruction| object_reads(instruction) == Some(parameter)) {
                continue;
            }
            body().all(|instruction| instruction.defined_register() != Some(parameter)).then_some(())?;
            let argument = window + parameter;
            let moved = (read_offset + 1..offset).rev().find(|index| caller[*index].defined_register() == Some(argument))?;
            (caller[moved].op() == Op::Move && !defines(caller[moved].register_b())).then_some(())?;
            bound[usize::from(parameter)] = Some(caller[moved].register_b());
            dropped.insert(header + moved as u32);
        }
        dropped.insert(read);
        for setup_pc in resume..pc {
            setup.insert(setup_pc, resume);
        }
        let base = Register::try_from(total).ok()?;
        total += u32::from(callee.registers);
        (total <= RegisterSet::BITS).then_some(())?;
        calls.insert(
            pc,
            Site {
                frame: InlineFrame {
                    call: pc,
                    callee: callee.callee,
                    field: usize::MAX,
                    receiver,
                    base,
                    registers: callee.registers,
                    bound,
                },
                read,
                window,
                result: instruction.result_register(),
                reached,
                callee,
            },
        );
    }
    (!calls.is_empty()).then_some(())?;
    // Region PCs: each kept caller instruction takes one, a call its numeric parameters'
    // moves and its callee's reached instructions, each `Return` two (the result's move and
    // the jump past the call). A dropped instruction's PC is the next instruction's.
    let mut caller_flat = Vec::with_capacity(caller.len());
    let mut callee_flat: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    let mut length = 0u32;
    for (offset, _) in caller.iter().enumerate() {
        let pc = header + offset as u32;
        caller_flat.push(header + length);
        if dropped.contains(&pc) {
            continue;
        }
        let Some(site) = calls.get(&pc) else {
            length += 1;
            continue;
        };
        length += site.frame.bound[..usize::from(site.callee.params)].iter().filter(|bound| bound.is_none()).count() as u32;
        let mut flat = Vec::with_capacity(site.callee.instructions.len());
        for (index, instruction) in site.callee.instructions.iter().enumerate() {
            flat.push(header + length);
            if site.reached[index] {
                length += if instruction.op() == Op::Return { 2 } else { 1 };
            }
        }
        callee_flat.insert(pc, flat);
    }
    (length as usize <= MAX_REGION_INSTRUCTIONS).then_some(())?;
    let mut origins: Vec<Origin> = Vec::with_capacity(length as usize);
    let mut masks: Vec<RegisterSet> = Vec::with_capacity(length as usize);
    let mut leaves: Vec<u32> = Vec::new();
    let mut instructions = Vec::with_capacity(length as usize);
    let caller_target = |target: u32, leaves: &mut Vec<u32>| -> u32 {
        if (header..=backedge).contains(&target) {
            return caller_flat[(target - header) as usize];
        }
        let index = leaves.iter().position(|leave| *leave == target).unwrap_or_else(|| {
            leaves.push(target);
            leaves.len() - 1
        });
        header + length + index as u32
    };
    for (offset, instruction) in caller.iter().copied().enumerate() {
        let pc = header + offset as u32;
        if dropped.contains(&pc) {
            continue;
        }
        // A setup instruction leaves as the first of its run does.
        let resumes = setup.get(&pc).copied().unwrap_or(pc);
        let instruction = rewritten.get(&pc).copied().unwrap_or(instruction);
        let Some(site) = calls.get(&pc) else {
            // A caller instruction keeps its registers; a jump takes the region PC of its target.
            let renamed = if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
                let target = caller_target(instruction.imm(), &mut leaves);
                WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target)
            } else {
                instruction
            };
            instructions.push(renamed);
            origins.push(Origin::Caller(resumes));
            masks.push(mask(resumes)?);
            continue;
        };
        let frame = site.frame;
        let index = calls.keys().position(|key| *key == pc)? as u16;
        let flat = &callee_flat[&pc];
        let register = |register: Register| match frame.bound.get(usize::from(register)).copied().flatten() {
            Some(source) => source,
            None => frame.base + register,
        };
        // A callee's live registers there, in the region, with the caller's live past the call.
        let after = mask(pc + 1)? & !(1 << site.result);
        let frame_mask = |callee_pc: usize| -> Option<RegisterSet> {
            let own = site.callee.live.get(callee_pc)?;
            let mut set = after;
            for callee_register in 0..site.callee.registers {
                if crate::compile::liveness::register_in(own, usize::from(callee_register)) {
                    set |= 1 << register(callee_register);
                }
            }
            Some(set)
        };
        for parameter in 0..site.callee.params {
            if frame.bound[usize::from(parameter)].is_some() {
                continue;
            }
            instructions.push(WideInstruction::new(Op::Move, frame.base + parameter, site.window + parameter, 0, 0));
            origins.push(Origin::Caller(site.read));
            masks.push(mask(site.read)?);
        }
        let continuation = caller_flat.get(offset + 1).copied().unwrap_or_else(|| caller_target(pc + 1, &mut leaves));
        for (callee_pc, instruction) in site.callee.instructions.iter().copied().enumerate() {
            if !site.reached[callee_pc] {
                continue;
            }
            let origin = Origin::Inlined { frame: index, pc: callee_pc as u32 };
            let set = frame_mask(callee_pc)?;
            if instruction.op() == Op::Return {
                instructions.push(WideInstruction::new(Op::Move, site.result, register(instruction.register_a()), 0, 0));
                instructions.push(WideInstruction::new(Op::Jump, 0, 0, 0, continuation));
                origins.extend([origin, origin]);
                masks.extend([set, set]);
                continue;
            }
            let target = |target: u32| flat.get(target as usize).copied().filter(|_| site.reached[target as usize]);
            instructions.push(renamed(instruction, &register, frame.receiver, &target)?);
            origins.push(origin);
            masks.push(set);
        }
    }
    debug_assert_eq!(instructions.len(), length as usize);
    for target in &leaves {
        origins.push(Origin::Leave(*target));
        masks.push(mask(*target)?);
    }
    let receivers = calls.values().map(|site| site.frame.receiver).collect();
    let header_live = masks.first().copied();
    let backedge = header + instructions.len() as u32 - 1;
    let mut region = admit_region(
        instructions,
        total as usize,
        constants,
        sites,
        Some(crate::compile::liveness::RegisterMasks::new(masks)),
        header_live,
        header,
        &receivers,
        backedge,
    )
    .ok()?;
    // Each callee's field, which the entry reads and guards still holds it, read through
    // the region's held registers.
    let held: std::collections::BTreeMap<Register, FieldPath> =
        region.held.iter().map(|(register, (path, _))| (*register, path.clone())).collect();
    let mut frames = Vec::with_capacity(calls.len());
    for site in calls.into_values() {
        let mut frame = site.frame;
        let written = frame.receiver >= caller_registers || defines(frame.receiver);
        (!written || held.contains_key(&frame.receiver)).then_some(())?;
        let path = field_path(caller[(site.read - header) as usize], sites, &held)?;
        frame.field = intern_field(&mut region.fields, &path, true);
        frames.push(frame);
    }
    (region.fields.len() <= MAX_REGION_FIELDS).then_some(())?;
    region.inlining = Some(Box::new(Inlining { origins, frames, caller_registers }));
    Some(region)
}

impl LoopRegion {
    fn entry_count(&self) -> usize {
        self.numbers.len() + self.tagged.len()
    }

    fn entry_register(&self, index: usize) -> Option<Register> {
        self.numbers
            .get(index)
            .or_else(|| self.tagged.get(index.saturating_sub(self.numbers.len())))
            .copied()
    }

    fn entry_registers(&self) -> impl Iterator<Item = Register> + '_ {
        self.numbers.iter().chain(&self.tagged).copied()
    }

    /// Whether the interpreter may read `register` resuming at `pc` before writing it; every
    /// register is, where liveness does not cover the function.
    fn is_live(&self, pc: u32, register: Register) -> bool {
        let Some(live) = &self.live else {
            return true;
        };
        // An inlining region's liveness is by its own PCs, from the header.
        let index = if self.inlining.is_some() { (pc - self.header) as usize } else { pc as usize };
        live.get(index)
            .is_none_or(|mask| !crate::compile::liveness::register_dead(mask, usize::from(register)))
    }

    /// The PC native code reports for leaving at region PC `pc`: the caller's, or an inlined
    /// callee instruction's region PC marked `INLINED_EXIT`.
    fn exit_pc(&self, pc: u32) -> u32 {
        let Some(inlining) = &self.inlining else {
            return pc;
        };
        match inlining.origins[(pc - self.header) as usize] {
            Origin::Caller(caller) | Origin::Leave(caller) => caller,
            Origin::Inlined { .. } => pc | INLINED_EXIT,
        }
    }

    /// The field whose value `register` holds at every read, when it is one the region
    /// writes only with that field's reads (`held_registers`).
    pub(crate) fn held_field(&self, register: Register) -> Option<usize> {
        self.held.get(&register).map(|(_, index)| *index)
    }

    /// Whether native code leaving at the `INLINED_EXIT` PC `pc` wrote back `register`.
    pub(crate) fn wrote_back(&self, pc: u32, register: Register) -> bool {
        self.is_live(pc & !INLINED_EXIT, register)
    }

    /// The frame and callee PC of an `INLINED_EXIT` PC native code reported.
    pub(crate) fn inlined_exit(&self, pc: u32) -> Option<(&InlineFrame, u32)> {
        let inlining = self.inlining.as_ref()?;
        (pc & INLINED_EXIT != 0).then_some(())?;
        match inlining.origins.get(((pc & !INLINED_EXIT) - self.header) as usize)? {
            Origin::Inlined { frame, pc } => Some((&inlining.frames[usize::from(*frame)], *pc)),
            Origin::Caller(_) | Origin::Leave(_) => None,
        }
    }
}

/// The entry record native code reads: each capture's value, then each cached field's,
/// then each array's element pointer, length and capacity, then each guarded field-store
/// pointer. Only the words pushed are ever read, by native code and by `captures` and
/// `field_value`, so the rest stay uninitialized.
pub(crate) struct LoopEntry {
    words: [std::mem::MaybeUninit<u64>; ENTRY_WORDS],
    length: usize,
    /// An inlining region's callee registers, which leaving at an inlined instruction
    /// writes back here (`Lowering::store_registers`) for its activation.
    scratch: [std::mem::MaybeUninit<u64>; MAX_REGION_REGISTERS],
}

/// The longest array a region indexes: a negative `i32` index zero-extends past it, so one
/// unsigned comparison bounds every `i32` index.
const MAX_INDEXED_LENGTH: usize = i32::MAX as usize;
/// The first record word counts executions of the region's own backward edge when shared
/// GeneralDispatch pricing is enabled.
const ENTRY_BACKEDGE_COUNT: usize = 0;
const ENTRY_CAPTURES: usize = 1;
const WORD_BYTES: i32 = 8;
/// An array's entry words: its element pointer, its length, then its capacity (which only
/// a written array's appends read).
const ARRAY_WORDS: usize = 3;
const FIELD_STORE_WORDS: usize = 1;
/// The scratch registers' address an inlining region's record ends with.
const SCRATCH_WORDS: usize = 1;
const ENTRY_WORDS: usize =
    ENTRY_CAPTURES
        + MAX_REGION_CAPTURES
        + ARRAY_WORDS * MAX_REGION_ARRAYS
        + MAX_REGION_FIELDS
        + FIELD_STORE_WORDS * MAX_REGION_FIELD_STORES
        + SCRATCH_WORDS;

impl LoopEntry {
    pub(crate) fn new() -> Self {
        let mut words = [std::mem::MaybeUninit::uninit(); ENTRY_WORDS];
        // The backward-edge counter is the one leading word native code reads before any push.
        words[ENTRY_BACKEDGE_COUNT].write(0);
        Self { words, length: ENTRY_CAPTURES, scratch: [std::mem::MaybeUninit::uninit(); MAX_REGION_REGISTERS] }
    }

    /// Number of taken executions of this region's backward edge in the most recent run.
    pub(crate) fn backedge_count(&self) -> u64 {
        // SAFETY: `new` initializes the counter word.
        unsafe { self.words[ENTRY_BACKEDGE_COUNT].assume_init() }
    }

    /// The record's words. Native OSR may also update the private first counter word.
    pub(super) fn words_mut(&mut self) -> *mut u64 {
        debug_assert!(self.length <= ENTRY_WORDS);
        self.words.as_mut_ptr().cast()
    }

    fn push(&mut self, word: u64) {
        self.words[self.length].write(word);
        self.length += 1;
    }

    /// Captures come first, in the region's order.
    pub(crate) fn push_capture(&mut self, value: Value) {
        self.push(value.to_bits());
    }

    /// The first `count` capture values, as pushed.
    pub(crate) fn captures(&self, count: usize) -> Vec<Value> {
        debug_assert!(ENTRY_CAPTURES + count <= self.length);
        self.words[ENTRY_CAPTURES..ENTRY_CAPTURES + count]
            .iter()
            // SAFETY: the first `count` words after the leading ones are the pushed captures.
            .map(|word| Value::from_bits(unsafe { word.assume_init() }))
            .collect()
    }

    /// Arrays follow the fields, in the region's order. An array longer than
    /// `MAX_INDEXED_LENGTH` declines the entry.
    pub(crate) fn push_array(&mut self, elements: *mut Value, length: usize, capacity: usize) -> Option<()> {
        (length <= MAX_INDEXED_LENGTH).then_some(())?;
        self.push(elements.addr() as u64);
        self.push(length as u64);
        self.push(capacity.min(MAX_INDEXED_LENGTH) as u64);
        Some(())
    }

    /// A warmed own-data field slot guarded by the VM during entry setup.
    pub(crate) fn push_field_store(&mut self, slot: *mut Value) {
        self.push(slot.addr() as u64);
    }

    /// The value of the region's `index`th field as pushed: its word follows the captures
    /// and the fields before it.
    pub(crate) fn field_value(&self, region: &LoopRegion, index: usize) -> Value {
        let word = ENTRY_CAPTURES + region.captures.len() + index;
        debug_assert!(word < self.length);
        // SAFETY: the caller pushed the field, after the captures and the fields before it.
        Value::from_bits(unsafe { self.words[word].assume_init() })
    }

    /// An inlining region's last word: its scratch registers' address.
    pub(crate) fn push_scratch(&mut self) {
        let address = self.scratch.as_mut_ptr().addr() as u64;
        self.push(address);
    }

    /// The value an inlined callee's register `register` of an inlining region took when
    /// native code left at an inlined instruction where it is live.
    ///
    /// # Safety
    /// The exit wrote the register back: it is live at the exit's PC (`LoopRegion::is_live`).
    pub(crate) unsafe fn scratch_value(&self, inlining: &Inlining, register: Register) -> Value {
        // SAFETY: as the caller promises.
        Value::from_bits(unsafe { self.scratch[usize::from(register - inlining.caller_registers)].assume_init() })
    }

    /// A field's value; a Number field's must be a Number.
    pub(crate) fn push_field(&mut self, value: Value, tagged: bool) -> Option<()> {
        (tagged || value.as_number().is_some()).then_some(())?;
        self.push(value.to_bits());
        Some(())
    }
}

/// How an instruction of the region names a value it reads.
#[derive(Clone, Copy)]
enum Source {
    Register(Register),
    Number(f64),
    Tagged(Value),
}

/// The arithmetic and comparison a region lowers, by the operator's tag.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arithmetic {
    Float(NumberArithmetic),
    Int32(super::super::operations::NumberBitwise),
}

fn arithmetic(instruction: Op, operator: u32) -> Option<Arithmetic> {
    if let Some(operation) = NumberArithmetic::from_js_instruction(instruction, operator) {
        return Some(Arithmetic::Float(operation));
    }
    super::super::operations::NumberBitwise::from_js_instruction(instruction, operator)
        .map(Arithmetic::Int32)
}

/// A unary operator a region lowers on a Number.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unary {
    Number(NumberUnary),
    BitwiseNot,
}

fn unary(instruction: Op, operator: u32) -> Option<Unary> {
    if let Some(operation) = NumberUnary::from_js_instruction(instruction, operator) {
        return Some(Unary::Number(operation));
    }
    (instruction == Op::Unary && operator == UnaryOperator::BitwiseNot as u32)
        .then_some(Unary::BitwiseNot)
}

/// What each register of a region holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Number,
    Tagged,
    Array { written: bool },
}

struct Admission<'a> {
    constants: &'a [Value],
    roles: std::collections::BTreeMap<Register, Role>,
    captures: Vec<u16>,
    pub(crate) fields: Vec<FieldUse>,
    numbers: std::collections::BTreeMap<u16, f64>,
    values: std::collections::BTreeMap<u16, Value>,
    /// The arrays registers hold that hold a field's value (`held_registers`), each with
    /// whether the region stores into it.
    field_arrays: std::collections::BTreeMap<Register, bool>,
}

impl Admission<'_> {
    /// Notes `register` as an indexed array: one a field read defines, else an entry array.
    fn array(&mut self, register: Register, written: bool) -> Option<()> {
        match self.field_arrays.get_mut(&register) {
            Some(stored) => {
                *stored |= written;
                Some(())
            }
            None => self.note(register, Role::Array { written }),
        }
    }

    fn note(&mut self, register: Register, role: Role) -> Option<()> {
        match (self.roles.get(&register).copied(), role) {
            (None, role) => {
                self.roles.insert(register, role);
            }
            (Some(Role::Number), Role::Number) => {}
            (Some(Role::Tagged), Role::Tagged) => {}
            (Some(Role::Array { written }), Role::Array { written: more }) => {
                self.roles.insert(register, Role::Array { written: written || more });
            }
            _ => return None,
        }
        Some(())
    }

    fn number(&mut self, register: Register) -> Option<()> {
        self.note(register, Role::Number)
    }

    fn tagged(&mut self, register: Register) -> Option<()> {
        self.note(register, Role::Tagged)
    }

    fn any_constant(&mut self, index: u16) -> Option<Value> {
        let value = *self.constants.get(usize::from(index))?;
        self.values.insert(index, value);
        Some(value)
    }

    fn constant(&mut self, index: u16) -> Option<f64> {
        let number = self.any_constant(index)?.as_number()?;
        self.numbers.insert(index, number);
        Some(number)
    }

    /// A Number operand: a register, noted as one, or a Number constant.
    fn source(&mut self, operand: Operand) -> Option<Source> {
        match operand.kind()? {
            OperandKind::Register => {
                self.number(operand.payload())?;
                Some(Source::Register(operand.payload()))
            }
            OperandKind::Constant => Some(Source::Number(self.constant(operand.payload())?)),
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// The result register of an instruction whose result is only a register write.
    fn result(&mut self, instruction: WideInstruction) -> Option<Register> {
        (instruction.result_flags() == 0).then_some(())?;
        let register = instruction.result_register();
        self.number(register)?;
        Some(register)
    }
}

/// The region of the loop from `header` to its backward jump at `backedge`, or the supported
/// prefix before its first call, when every included instruction is one the tier lowers.
pub(super) fn region(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<&crate::compile::liveness::RegisterMasks>,
    header: u32,
    backedge: u32,
) -> Result<LoopRegion, RegionRejection> {
    let (start, end) = (header as usize, backedge as usize);
    if start > end || end >= decoded.len() || end - start >= MAX_REGION_INSTRUCTIONS {
        return Err(RegionRejection::RegionTooLarge);
    }
    let header_live = live.and_then(|live| live.get(start));
    // Dynamic calls are effect boundaries. Compile the supported prefix before the first
    // call and resume the interpreter at that exact PC. The interpreter reaches this
    // region again at the loop backedge, so the prefix remains useful across iterations.
    // Branches that skip the call naturally resume at their exact target.
    let mut prefix_end = end;
    if let Some(call) = decoded[start..=end].iter().position(|instruction| instruction.op() == Op::Call) {
        if call == 0 {
            return Err(RegionRejection::UnsupportedInstruction(Op::Call));
        }
        prefix_end = start + call - 1;
    }
    admit_region(
        decoded[start..=prefix_end].to_vec(),
        usize::from(function.registers),
        constants,
        sites,
        live.cloned(),
        header_live,
        header,
        &std::collections::BTreeSet::new(),
        backedge,
    )
}

/// The region of `instructions`, at PCs from `header`, of a function with `registers`
/// registers whose liveness by PC is `live` (`header_live` at the header); `objects` are
/// registers read as objects beyond those the instructions show, so tagged
/// (`tagged_registers`) and holding a field's value where they can (`held_registers`).
#[allow(clippy::too_many_arguments)]
fn admit_region(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<crate::compile::liveness::RegisterMasks>,
    header_live: Option<RegisterSet>,
    header: u32,
    objects: &std::collections::BTreeSet<Register>,
    backedge: u32,
) -> Result<LoopRegion, RegionRejection> {
    let end = header + instructions.len() as u32 - 1;
    let paths = held_registers(&instructions, sites, header_live, objects);
    let mut tagged = tagged_registers(&instructions, constants, &objects.iter().chain(paths.keys()).copied().collect())
        .ok_or(RegionRejection::UnsupportedInstruction(Op::JumpBinaryFalse))?;
    tagged.extend(paths.keys().copied());
    let mut admission = Admission {
        constants,
        roles: std::collections::BTreeMap::new(),
        captures: Vec::new(),
        fields: Vec::new(),
        numbers: std::collections::BTreeMap::new(),
        values: std::collections::BTreeMap::new(),
        field_arrays: std::collections::BTreeMap::new(),
    };
    let mut field_reads = std::collections::BTreeMap::new();
    let mut field_stores = std::collections::BTreeMap::new();
    let mut field_store_sites = Vec::new();
    let defined_registers: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| instruction.defined_register())
        .collect();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        if instruction.op() == Op::GetField {
            let path = field_path(instruction, sites, &paths).ok_or(RegionRejection::UnsupportedInstruction(Op::GetField))?;
            let index = intern_field(&mut admission.fields, &path, tagged.contains(&instruction.result_register()));
            field_reads.insert(offset as u32, index);
        }
        if matches!(instruction.op(), Op::SetField | Op::SetFieldStrict) {
            if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                return Err(RegionRejection::UnsupportedInstruction(instruction.op()));
            }
            let receiver = instruction.register_b();
            let atom = instruction.atom_index();
            let cache_site = instruction.cache_site_index();
            // A cached read of this atom could otherwise keep using its entry value after a
            // native store. Be conservative about aliases: reject every same-name read in
            // this small region.
            if instructions.iter().any(|candidate| {
                candidate.op() == Op::GetField && candidate.atom_index() == atom
            }) {
                return Err(RegionRejection::FieldMutation);
            }
            // The slot pointer is captured at entry. The receiver must therefore stay in its
            // tagged register for the entire region.
            if instructions.iter().any(|candidate| candidate.defined_register() == Some(receiver)) {
                return Err(RegionRejection::FieldMutation);
            }
            let site = FieldStore { receiver, atom, cache_site };
            let index = field_store_sites.iter().position(|existing| *existing == site).unwrap_or_else(|| {
                field_store_sites.push(site);
                field_store_sites.len() - 1
            });
            field_stores.insert(offset as u32, index);
        }
        if let Some(register) = object_reads(instruction).filter(|register| paths.contains_key(register)) {
            admission.field_arrays.entry(register).or_insert(false);
        }
        admit(&mut admission, instruction, constants, &tagged, &defined_registers)
            .ok_or(RegionRejection::UnsupportedInstruction(instruction.op()))?;
    }
    let held: std::collections::BTreeMap<Register, (FieldPath, usize)> = paths
        .into_iter()
        .map(|(register, path)| {
            let index = intern_field(&mut admission.fields, &path, true);
            (register, (path, index))
        })
        .collect();
    let mut identity_branches = std::collections::BTreeMap::new();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        let Some(branch) = identity_branch(instruction, constants) else {
            continue;
        };
        identity_branches.insert(header + offset as u32, branch);
    }
    if admission.fields.len() > MAX_REGION_FIELDS {
        return Err(RegionRejection::FieldLimit);
    }
    if field_store_sites.len() > MAX_REGION_FIELD_STORES {
        return Err(RegionRejection::FieldStoreLimit);
    }
    // A field read holds the value its receiver had at entry, so no instruction may give a
    // receiver register another value. Element stores and admitted field stores preserve
    // receiver registers and object shapes. A field store to any name read in the region was
    // rejected above, so it cannot stale a cached field value.
    if admission.fields.iter().any(|field| match field.receiver {
        FieldReceiver::This | FieldReceiver::Field(_) => false,
        FieldReceiver::Register(register) => instructions
            .iter()
            .any(|instruction| number_definition(*instruction) == Some(register)),
    }) {
        return Err(RegionRejection::FieldMutation);
    }
    if admission.roles.keys().any(|register| usize::from(*register) >= registers)
        || admission.roles.len() > MAX_REGION_REGISTERS
    {
        return Err(RegionRejection::RegisterLimit);
    }
    if admission.captures.len() > MAX_REGION_CAPTURES {
        return Err(RegionRejection::CaptureLimit);
    }
    let numbers = admission
        .roles
        .iter()
        .filter(|(_, role)| **role == Role::Number)
        .map(|(register, _)| *register)
        .collect();
    let tagged: Vec<Register> = admission
        .roles
        .iter()
        .filter(|(_, role)| **role == Role::Tagged)
        .map(|(register, _)| *register)
        .collect();
    let mut arrays: Vec<ArrayUse> = admission
        .roles
        .iter()
        .filter_map(|(register, role)| match role {
            Role::Array { written } => Some(ArrayUse { register: *register, written: *written, field: None }),
            Role::Number | Role::Tagged => None,
        })
        .collect();
    for (register, written) in &admission.field_arrays {
        // A held register read as an object only by field reads indexes nothing.
        if !instructions.iter().any(|instruction| {
            matches!(instruction.op(), Op::GetIndex | Op::SetIndex | Op::RequireObjectCoercible)
                && object_reads(*instruction) == Some(*register)
        }) {
            continue;
        }
        arrays.push(ArrayUse { register: *register, written: *written, field: Some(held[register].1) });
    }
    if arrays.len() > MAX_REGION_ARRAYS {
        return Err(RegionRejection::ArrayLimit);
    }
    Ok(LoopRegion {
        header,
        end,
        backedge,
        numbers,
        tagged,
        arrays,
        captures: admission.captures,
        fields: admission.fields,
        field_reads,
        field_stores,
        field_store_sites,
        held,
        identity_branches,
        instructions,
        constants: admission.numbers,
        tagged_constants: admission.values,
        live,
        inlining: None,
    })
}

fn admit(
    admission: &mut Admission<'_>,
    instruction: WideInstruction,
    constants: &[Value],
    tagged: &std::collections::BTreeSet<Register>,
    defined_registers: &std::collections::BTreeSet<Register>,
) -> Option<()> {
    match instruction.op() {
        Op::LoadConst => {
            let index = u16::try_from(instruction.constant_index()).ok()?;
            if tagged.contains(&instruction.result_register()) {
                admission.any_constant(index)?;
                admission.tagged(instruction.result_register())?;
            } else {
                admission.constant(index)?;
                admission.result(instruction)?;
            }
        }
        Op::Move => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        Op::LoadCapture => {
            (instruction.capture_depth() == 0).then_some(())?;
            admission.result(instruction)?;
            let slot = instruction.capture_slot();
            if !admission.captures.contains(&slot) {
                admission.captures.push(slot);
            }
        }
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
            arithmetic(instruction.op(), instruction.binary_operator())?;
            admission.result(instruction)?;
            admission.source(instruction.operand_b())?;
            admission.source(instruction.operand_c())?;
        }
        Op::ToNumeric => {
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        Op::IncDec => {
            instruction.boolean_flag()?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        Op::Unary => {
            unary(instruction.op(), instruction.unary_operator())?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        // An array the entry guard proved is an object passes.
        Op::RequireObjectCoercible => {
            admission.array(instruction.register_b(), false)?;
        }
        Op::GetIndex => {
            admission.result(instruction)?;
            let base = instruction.operand_b().register_index()?;
            admission.array(base, false)?;
            admission.source(instruction.operand_c())?;
        }
        // Its field is the region's (`LoopRegion::field_reads`).
        Op::GetField => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
            } else {
                admission.result(instruction)?;
            }
        }
        Op::SetIndex => {
            admission.number(instruction.register_a())?;
            admission.array(instruction.register_b(), true)?;
            admission.number(instruction.register_c())?;
        }
        Op::SetField | Op::SetFieldStrict => {
            let value_register = instruction.register_a();
            if defined_registers.contains(&value_register) {
                admission.number(value_register)?;
            } else {
                // An unchanged entry value can be any tagged value. The VM logs the
                // receiver in the generational barrier before native writes begin.
                admission.tagged(value_register)?;
            }
            admission.tagged(instruction.register_b())?;
        }
        Op::Jump => {}
        Op::JumpBinaryFalse => {
            if let Some(branch) = identity_branch(instruction, constants) {
                admission.tagged(branch.register)?;
            } else if super::super::operations::NumberRelation::from_operator(
                instruction.binary_operator_field(),
            )
            .is_some()
            {
                admission.source(instruction.operand_b())?;
                admission.source(instruction.operand_c())?;
            } else {
                return None;
            }
        }
        Op::JumpFalse | Op::JumpTrue => {
            admission.number(instruction.register_a())?;
        }
        _ => return None,
    }
    Some(())
}

/// How native code holds a Number register or capture: as an `i32` while every value it
/// takes is one, else as an `f64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Representation {
    Int32,
    Float64,
    Tagged,
}

/// The values a compilation starts from: what the region's registers and captures held when
/// it compiled, and what earlier compilations found was no `i32` (`Widening`).
pub(crate) struct Observation<'a> {
    pub(crate) register: &'a dyn Fn(Register) -> Value,
    /// Whether the array in a register looks to hold `i32` elements.
    pub(crate) int32_elements: &'a dyn Fn(Register) -> bool,
    pub(crate) captures: &'a [Value],
    /// The fields' values at the entry the compilation follows, in the region's order.
    pub(crate) fields: &'a [Value],
    pub(crate) widened: &'a std::collections::BTreeSet<Widening>,
}

/// How native code holds the values the entry record carries, by their record order.
struct EntryRepresentations {
    captures: Vec<Representation>,
    fields: Vec<Representation>,
}

/// A definition of a Number register in a region: the value it holds at entry, by its
/// index in `LoopRegion::numbers`, or the instruction at a PC offset from the header.
type Definition = usize;

/// The region's values as webs: the definitions that reach a common read (an operand, or a
/// register written back where native code leaves) form one web, which native code holds in
/// one SSA variable with one representation. A register reused for unrelated values (an
/// index, then an element) gets a web for each, so one does not widen the other.
struct Webs {
    /// Union-find parents over definitions.
    parent: Vec<Definition>,
    /// The definitions reaching each instruction, by PC offset and register.
    reaching: Vec<std::collections::BTreeMap<Register, std::collections::BTreeSet<Definition>>>,
    /// Per leave PC and live register, a definition of the web written back there.
    leaves: std::collections::BTreeMap<(u32, Register), Definition>,
    /// Whether the value a register holds at entry is read (its web reaches a read).
    entry_read: Vec<bool>,
}

impl Webs {
    fn find(&self, mut definition: Definition) -> Definition {
        while self.parent[definition] != definition {
            definition = self.parent[definition];
        }
        definition
    }

    fn union(&mut self, left: Definition, right: Definition) {
        let (left, right) = (self.find(left), self.find(right));
        if left != right {
            self.parent[right] = left;
        }
    }

    /// The web `register` holds before the instruction at `pc`; a PC no path from the header
    /// reaches reads the entry value.
    fn read(&self, region: &LoopRegion, pc: u32, register: Register) -> Definition {
        let entry = || region.entry_registers().position(|number| number == register).unwrap_or(0);
        let definition = self.reaching[(pc - region.header) as usize]
            .get(&register)
            .and_then(|definitions| definitions.first().copied())
            .unwrap_or_else(entry);
        self.find(definition)
    }

    /// The register a web's definitions write: every definition reaching a common read
    /// writes the register read.
    fn register(&self, region: &LoopRegion, web: Definition) -> Register {
        match region.entry_register(web) {
            Some(register) => register,
            None => number_definition(region.instructions[web - region.entry_count()])
                .expect("a web's definitions write Number registers"),
        }
    }

    /// The web of the instruction at `pc`'s result.
    fn defined(&self, region: &LoopRegion, pc: u32) -> Definition {
        self.find(region.entry_count() + (pc - region.header) as usize)
    }
}

/// The Number registers an instruction reads.
fn number_reads(instruction: WideInstruction) -> Vec<Register> {
    let register = |operand: Operand| operand.register_index();
    match instruction.op() {
        Op::Move | Op::IncDec | Op::Unary | Op::ToNumeric => vec![instruction.register_b()],
        Op::Binary | Op::NumericAdd | Op::NumericMultiply | Op::JumpBinaryFalse => [
            register(instruction.operand_b()),
            register(instruction.operand_c()),
        ]
        .into_iter()
        .flatten()
        .collect(),
        Op::JumpFalse | Op::JumpTrue => vec![instruction.register_a()],
        Op::GetIndex => register(instruction.operand_c()).into_iter().collect(),
        Op::SetIndex => vec![instruction.register_a(), instruction.register_c()],
        _ => Vec::new(),
    }
}

/// The Number register an instruction defines.
fn number_definition(instruction: WideInstruction) -> Option<Register> {
    matches!(
        instruction.op(),
        Op::LoadConst
            | Op::Move
            | Op::LoadCapture
            | Op::Binary
            | Op::NumericAdd
            | Op::NumericMultiply
            | Op::IncDec
            | Op::Unary
            | Op::ToNumeric
            | Op::GetIndex
            | Op::GetField
    )
    .then(|| instruction.result_register())
}

/// The region PCs control continues at from the instruction at `pc`, and the PCs outside it
/// where the lane resumes; as lowered (`Lowering::jump`, `Lowering::fall_through`).
fn successors(region: &LoopRegion, pc: u32) -> Vec<u32> {
    let instruction = region.instructions[(pc - region.header) as usize];
    let mut successors = Vec::with_capacity(2);
    crate::bytecode::for_each_instruction_successor(pc as usize, instruction, |successor| {
        successors.push(successor as u32);
    });
    successors
}

fn webs(region: &LoopRegion) -> Webs {
    use std::collections::{BTreeMap, BTreeSet};
    let count = region.instructions.len();
    let entries = region.entry_count();
    let inside = |pc: u32| (region.header..=region.end).contains(&pc);
    let mut reaching: Vec<BTreeMap<Register, BTreeSet<Definition>>> = vec![BTreeMap::new(); count];
    for (index, register) in region.entry_registers().enumerate() {
        reaching[0].entry(register).or_default().insert(index);
    }
    // Forward reaching definitions to a fixpoint; regions are small.
    loop {
        let mut changed = false;
        for offset in 0..count {
            let pc = region.header + offset as u32;
            let mut out = reaching[offset].clone();
            if let Some(register) = number_definition(region.instructions[offset]) {
                out.insert(register, BTreeSet::from([entries + offset]));
            }
            for successor in successors(region, pc).into_iter().filter(|pc| inside(*pc)) {
                let target = &mut reaching[(successor - region.header) as usize];
                for (register, definitions) in &out {
                    let set = target.entry(*register).or_default();
                    let before = set.len();
                    set.extend(definitions.iter().copied());
                    changed |= set.len() != before;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut webs = Webs {
        parent: (0..entries + count).collect(),
        reaching,
        leaves: BTreeMap::new(),
        entry_read: vec![false; entries],
    };
    let mut reads: Vec<Definition> = Vec::new();
    fn unify(
        webs: &mut Webs,
        definitions: &std::collections::BTreeSet<Definition>,
        reads: &mut Vec<Definition>,
    ) -> Option<Definition> {
        let mut definitions = definitions.iter().copied();
        let first = definitions.next()?;
        for other in definitions {
            webs.union(first, other);
        }
        reads.push(first);
        Some(first)
    }
    for offset in 0..count {
        let pc = region.header + offset as u32;
        let instruction = region.instructions[offset];
        for register in number_reads(instruction) {
            let definitions = webs.reaching[offset].get(&register).cloned().unwrap_or_default();
            unify(&mut webs, &definitions, &mut reads);
        }
        // Native code may leave at any instruction of the region, before its effects, and
        // resumes the lane where a jump leaves the region; each writes back what is live.
        let leave = |webs: &mut Webs, at: u32, reads: &mut Vec<Definition>| {
            for register in region.entry_registers().filter(|register| region.is_live(at, *register)) {
                let mut definitions = webs.reaching[offset].get(&register).cloned().unwrap_or_default();
                if let Some(previous) = webs.leaves.get(&(at, register)) {
                    definitions.insert(*previous);
                }
                if let Some(first) = unify(webs, &definitions, reads) {
                    webs.leaves.insert((at, register), first);
                }
            }
        };
        leave(&mut webs, pc, &mut reads);
        for successor in successors(region, pc).into_iter().filter(|pc| !inside(*pc)) {
            leave(&mut webs, successor, &mut reads);
        }
    }
    let read_webs: BTreeSet<Definition> = reads.iter().map(|definition| webs.find(*definition)).collect();
    webs.entry_read = (0..entries).map(|entry| read_webs.contains(&webs.find(entry))).collect();
    webs
}

fn int32_number(number: f64) -> bool {
    Value::number(number).as_int().is_some()
}

/// Each web's, capture's and field's representation: an `i32` where every definition in
/// the web yields one (an entry value observed as one, an instruction's result from `i32`
/// operands) and no widening names it; the least fixpoint from that start, widening a web
/// any definition gives an `f64`. Webs are indexed by definition, meaningful at each web's
/// root.
fn representations(
    region: &LoopRegion,
    webs: &Webs,
    observation: &Observation<'_>,
) -> (Vec<Representation>, EntryRepresentations) {
    let of = |value: Value, widening: Widening| {
        if value.as_int().is_some() && !observation.widened.contains(&widening) {
            Representation::Int32
        } else {
            Representation::Float64
        }
    };
    let captures: Vec<Representation> = (0..region.captures.len())
        .map(|index| {
            let value = observation.captures.get(index).copied().unwrap_or(Value::UNDEFINED);
            of(value, Widening::Capture(index as u16))
        })
        .collect();
    let fields: Vec<Representation> = region
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            if field.tagged {
                return Representation::Tagged;
            }
            let value = observation.fields.get(index).copied().unwrap_or(Value::UNDEFINED);
            of(value, Widening::Field(index as u16))
        })
        .collect();
    let entry = EntryRepresentations { captures, fields };
    let mut webs_representation = vec![Representation::Int32; webs.parent.len()];
    for (entry, register) in region.entry_registers().enumerate() {
        let representation = if region.tagged.contains(&register) {
            Representation::Tagged
        } else {
            of((observation.register)(register), Widening::Register(register))
        };
        if webs.entry_read[entry] {
            webs_representation[webs.find(entry)] = representation;
        }
    }
    loop {
        let mut changed = false;
        for (offset, instruction) in region.instructions.iter().enumerate() {
            let pc = region.header + offset as u32;
            let read = |register: Register| webs_representation[webs.read(region, pc, register)];
            let Some((register, representation)) =
                definition(region, offset as u32, instruction, &read, &entry, observation)
            else {
                continue;
            };
            let representation = if observation.widened.contains(&Widening::Register(register)) {
                Representation::Float64
            } else {
                representation
            };
            let web = webs.defined(region, pc);
            if representation == Representation::Tagged
                && webs_representation[web] != Representation::Tagged
            {
                webs_representation[web] = Representation::Tagged;
                changed = true;
            } else if representation == Representation::Float64
                && webs_representation[web] == Representation::Int32
            {
                webs_representation[web] = Representation::Float64;
                changed = true;
            }
        }
        if !changed {
            return (webs_representation, entry);
        }
    }
}

/// The register an instruction defines and the representation its result has.
fn definition(
    region: &LoopRegion,
    offset: u32,
    instruction: &WideInstruction,
    registers: &dyn Fn(Register) -> Representation,
    entry: &EntryRepresentations,
    observation: &Observation<'_>,
) -> Option<(Register, Representation)> {
    if let Some(result) = instruction.defined_register()
        && region.tagged.contains(&result)
    {
        return Some((result, Representation::Tagged));
    }
    let source = |operand: Operand| match operand.kind() {
        Some(OperandKind::Register) => registers(operand.payload()),
        _ => match region.constants.get(&operand.payload()) {
            Some(number) if int32_number(*number) => Representation::Int32,
            _ => Representation::Float64,
        },
    };
    let both = |left: Representation, right: Representation| {
        if left == Representation::Int32 && right == Representation::Int32 {
            Representation::Int32
        } else {
            Representation::Float64
        }
    };
    let representation = match instruction.op() {
        Op::LoadConst => {
            let index = u16::try_from(instruction.constant_index()).ok()?;
            match region.constants.get(&index) {
                Some(number) if int32_number(*number) => Representation::Int32,
                _ => Representation::Float64,
            }
        }
        Op::Move | Op::IncDec | Op::ToNumeric => registers(instruction.register_b()),
        Op::LoadCapture => {
            let slot = instruction.capture_slot();
            let index = region.captures.iter().position(|capture| *capture == slot)?;
            entry.captures[index]
        }
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
            match arithmetic(instruction.op(), instruction.binary_operator())? {
                Arithmetic::Float(NumberArithmetic::Divide) => Representation::Float64,
                Arithmetic::Float(_) => {
                    both(source(instruction.operand_b()), source(instruction.operand_c()))
                }
                Arithmetic::Int32(NumberBitwise::ShiftRightUnsigned) => Representation::Float64,
                Arithmetic::Int32(_) => Representation::Int32,
            }
        }
        Op::Unary => match unary(instruction.op(), instruction.unary_operator())? {
            Unary::Number(_) => registers(instruction.register_b()),
            Unary::BitwiseNot => Representation::Int32,
        },
        Op::GetIndex => element_representation(instruction, observation),
        Op::GetField => entry.fields[*region.field_reads.get(&offset)?],
        _ => return None,
    };
    Some((instruction.result_register(), representation))
}

/// The representation of what `GetIndex` reads: an `i32` from an array that looks to hold
/// them and no earlier compilation widened.
fn element_representation(instruction: &WideInstruction, observation: &Observation<'_>) -> Representation {
    match instruction.operand_b().register_index() {
        Some(base)
            if (observation.int32_elements)(base)
                && !observation.widened.contains(&Widening::Elements(base)) =>
        {
            Representation::Int32
        }
        _ => Representation::Float64,
    }
}

/// The largest step and offset a counted loop's proof takes (`Affine`). Entry values are
/// `i32`s and trip counts stay below 2^34, so with terms this small a value's own part
/// stays below 2^60; an invariant addend (`Term`) is an `i32` sum or a product of two such
/// values, below 2^62.1, so every `i64` the proof computes (`Lowering::proof`) stays below
/// 2^63.
const MAX_AFFINE_TERM: i64 = 1 << 24;

/// What an affine value of a counted loop's body counts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    /// Zero: the value is its offset.
    Constant,
    /// A capture, by its index in the region's capture order; no region instruction writes
    /// one.
    Capture(usize),
    /// The register's value where the iteration starts, at the loop's header.
    Start(Register),
    /// A loop-invariant sum or product (`CountedLoop::terms`), by its index.
    Term(usize),
}

/// A value every iteration of a counted loop computes alike from values the body never
/// writes: the sum or the product of two invariant values, none of them a product, so the
/// proof evaluates it once, exactly, from the values arriving at the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Term {
    multiply: bool,
    left: Affine,
    right: Affine,
}

/// A value of a counted loop's body: its base's value in the iteration plus `offset`,
/// plus the value of an invariant `addend` (a capture, a register the body never writes,
/// or a `Term`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Affine {
    base: Base,
    offset: i64,
    addend: Option<Base>,
}

impl Affine {
    fn of(base: Base) -> Self {
        Self { base, offset: 0, addend: None }
    }

    fn plus(self, amount: i64) -> Option<Self> {
        let offset = self.offset.checked_add(amount)?;
        (offset.abs() <= MAX_AFFINE_TERM).then_some(Self { offset, ..self })
    }
}

/// A counted loop continues while its tested value is at most, or at least, its bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Continue {
    AtMost,
    AtLeast,
}

/// What a fact proves of the value an instruction of a counted loop computes, over every
/// iteration that executes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    /// The key of an access lies within the length of the array in this register, so the
    /// access needs no bounds guard.
    Index(Register),
    /// The `i32` sum is exact, so the arithmetic needs no overflow guard.
    Int32,
    /// The invariant product (`Term`) is an `i32` and no -0, so the multiplication needs no
    /// guard.
    Product,
}

#[derive(Clone, Copy, Debug)]
struct Fact {
    pc: u32,
    value: Affine,
    bound: Bound,
}

/// An array as native code indexes it: its element pointer and how its length is known.
#[derive(Clone, Copy)]
struct ArrayView {
    elements: ir::Value,
    access: ArrayAccess,
}

#[derive(Clone, Copy)]
enum ArrayAccess {
    /// A read array keeps the length the entry read: only a written array's appends extend
    /// a length, so one aliasing a written array reads the elements it read at entry or
    /// exits for the general path.
    Read { length: ir::Value },
    /// A written array's length is its backing's, which an append within `capacity`
    /// advances (`Lowering::store_index`): every array of one backing reads it alike.
    Written { capacity: ir::Value },
}

/// An element index as an access uses it: a 64-bit index scaled by the element size, plus a
/// constant byte displacement.
#[derive(Clone, Copy)]
struct ElementIndex {
    wide: ir::Value,
    displacement: i32,
}

/// A counted loop of a region: a loop whose body, header to backward jump, is one path but
/// for the test that leaves it, and whose test compares an induction variable with a bound
/// the body leaves unchanged. Every arrival at its header from outside it runs the proof of
/// its facts (`Lowering::proof`), which selects the copy of the body without their guards
/// or the copy with them.
#[derive(Clone, Debug)]
struct CountedLoop {
    header: u32,
    backedge: u32,
    test: u32,
    /// The test continues while `induction` is `relation` to `bound`.
    induction: Affine,
    relation: Continue,
    bound: Affine,
    /// Each register's change per iteration, for each register the body changes by a
    /// constant; a register the body never writes changes by zero.
    steps: std::collections::BTreeMap<Register, i64>,
    /// The invariant sums and products `Base::Term` names.
    terms: Vec<Term>,
    facts: Vec<Fact>,
}

impl CountedLoop {
    fn contains(&self, pc: u32) -> bool {
        (self.header..=self.backedge).contains(&pc)
    }

    /// How much a fact's base changes per iteration (`counted_loop` keeps only facts whose
    /// bases change by a constant).
    fn base_step(&self, base: Base) -> i64 {
        match base {
            Base::Start(register) => self.steps.get(&register).copied().unwrap_or(0),
            Base::Constant | Base::Capture(_) | Base::Term(_) => 0,
        }
    }

    /// Whether the proved copy drops the guard of the instruction at `pc`.
    fn proves(&self, pc: u32) -> bool {
        self.facts.iter().any(|fact| fact.pc == pc)
    }
}

/// The counted loops of `region`, each with the facts its proof can establish under the
/// compilation's representations: a fact needs every value it names held as an `i32`.
/// The values an `i32` may take, both bounds included.
type Range = (i64, i64);
/// Every `i32`.
const INT32_RANGE: Range = (i32::MIN as i64, i32::MAX as i64);
/// How often a web's range may grow before it is taken to be every `i32`: a loop-carried
/// value that keeps growing (a sum of itself) would otherwise grow by one step per pass.
const MAX_RANGE_GROWTH: u8 = 4;

/// What the region knows of its `i32` arithmetic before lowering it (`int32_facts`).
#[derive(Default)]
struct Int32Facts {
    /// The PCs whose sum, difference or product of `i32`s is an `i32` for every value its
    /// operands can hold, so it needs no overflow guard.
    exact: std::collections::BTreeSet<u32>,
    /// The PCs of products whose zero needs no sign: no factor is ever negative, or every
    /// read of the product treats -0 as +0 and no leave writes it back.
    signless_zero: std::collections::BTreeSet<u32>,
    /// The PCs of sums and differences every read of which takes ToInt32 of them (a
    /// bitwise operand, or an operand of another such sum) and no leave writes back: the
    /// wrapped `i32` is the exact result modulo 2^32, which is all any read observes, so it
    /// needs no overflow guard. An exit's write-back gives the interpreter that `i32`, which
    /// its same reads observe alike.
    truncated: std::collections::BTreeSet<u32>,
}

/// The range of an operation's exact result from its operands' ranges; `None` while an
/// operand's range is not known yet. An operation it does not bound may give any `i32`.
fn exact_range(instruction: WideInstruction, operand: &dyn Fn(Operand) -> Option<Range>, register: &dyn Fn(Register) -> Option<Range>, constants: &std::collections::BTreeMap<u16, f64>) -> Option<Range> {
    use super::super::operations::NumberBitwise as Bitwise;
    let constant = |operand: Operand| match operand.kind() {
        Some(OperandKind::Constant) => constants.get(&operand.payload()).copied().filter(|number| int32_number(*number)).map(|number| number as i64),
        _ => None,
    };
    let corners = |(a, b): Range, (c, d): Range, f: fn(i64, i64) -> i64| {
        let values = [f(a, c), f(a, d), f(b, c), f(b, d)];
        (*values.iter().min().expect("four corners"), *values.iter().max().expect("four corners"))
    };
    Some(match instruction.op() {
        Op::LoadConst => {
            let index = u16::try_from(instruction.constant_index()).ok();
            match index.and_then(|index| constants.get(&index)) {
                Some(number) if int32_number(*number) => (*number as i64, *number as i64),
                _ => INT32_RANGE,
            }
        }
        Op::Move | Op::ToNumeric => register(instruction.register_b())?,
        Op::IncDec => {
            let (low, high) = register(instruction.register_b())?;
            let step = if instruction.boolean_flag() == Some(true) { -1 } else { 1 };
            (low + step, high + step)
        }
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
            let (left, right) = (operand(instruction.operand_b())?, operand(instruction.operand_c())?);
            match arithmetic(instruction.op(), instruction.binary_operator()) {
                Some(Arithmetic::Float(NumberArithmetic::Add)) => (left.0 + right.0, left.1 + right.1),
                Some(Arithmetic::Float(NumberArithmetic::Subtract)) => (left.0 - right.1, left.1 - right.0),
                Some(Arithmetic::Float(NumberArithmetic::Multiply)) => corners(left, right, |a, b| a * b),
                Some(Arithmetic::Int32(Bitwise::And)) if left.0 >= 0 || right.0 >= 0 => {
                    let high = [left, right].iter().filter(|range| range.0 >= 0).map(|range| range.1).min().expect("a nonnegative side");
                    (0, high)
                }
                Some(Arithmetic::Int32(Bitwise::Or | Bitwise::Xor)) if left.0 >= 0 && right.0 >= 0 => {
                    let high = left.1.max(right.1);
                    (0, ((high + 1) as u64).next_power_of_two() as i64 - 1)
                }
                Some(Arithmetic::Int32(shift @ (Bitwise::ShiftLeft | Bitwise::ShiftRight))) => match constant(instruction.operand_c()) {
                    // ToInt32 of a value in `left`, an `i32` range, is itself.
                    Some(count) if left.0 >= INT32_RANGE.0 && left.1 <= INT32_RANGE.1 => {
                        let count = (count & 31) as u32;
                        match shift {
                            Bitwise::ShiftRight => (left.0 >> count, left.1 >> count),
                            _ => (left.0 << count, left.1 << count),
                        }
                    }
                    _ => INT32_RANGE,
                },
                _ => INT32_RANGE,
            }
        }
        _ => INT32_RANGE,
    })
}

/// Each `i32` web's range and the arithmetic that needs no guard (`Int32Facts`): the least
/// fixpoint of each definition's `exact_range`, clamped to the `i32`s its web holds, from the
/// entry values, which may be any. A web whose range keeps growing takes every `i32`.
fn int32_facts(region: &LoopRegion, webs: &Webs, representations: &[Representation]) -> Int32Facts {
    let int32 = |web: Definition| representations[web] == Representation::Int32;
    let mut ranges: Vec<Option<Range>> = vec![None; webs.parent.len()];
    let mut growth = vec![0u8; webs.parent.len()];
    for entry in 0..region.entry_count() {
        if webs.entry_read[entry] {
            ranges[webs.find(entry)] = Some(INT32_RANGE);
        }
    }
    // The range a read at `pc` sees: a non-`i32` web's ToInt32 may be any `i32`.
    let register_range = |ranges: &[Option<Range>], pc: u32, register: Register| {
        let web = webs.read(region, pc, register);
        if int32(web) { ranges[web] } else { Some(INT32_RANGE) }
    };
    let operand_range = |ranges: &[Option<Range>], pc: u32, operand: Operand| match operand.kind()? {
        OperandKind::Register => register_range(ranges, pc, operand.payload()),
        OperandKind::Constant => match region.constants.get(&operand.payload()) {
            Some(number) if int32_number(*number) => Some((*number as i64, *number as i64)),
            _ => Some(INT32_RANGE),
        },
        OperandKind::Field | OperandKind::Local => Some(INT32_RANGE),
    };
    loop {
        let mut changed = false;
        for (offset, instruction) in region.instructions.iter().copied().enumerate() {
            let pc = region.header + offset as u32;
            if number_definition(instruction).is_none() || !int32(webs.defined(region, pc)) {
                continue;
            }
            let Some((low, high)) = exact_range(
                instruction,
                &|operand| operand_range(&ranges, pc, operand),
                &|register| register_range(&ranges, pc, register),
                &region.constants,
            ) else {
                continue;
            };
            // The web holds the result only where it is an `i32`; any other leaves.
            let held = (low.max(INT32_RANGE.0), high.min(INT32_RANGE.1));
            let held = if held.0 <= held.1 { held } else { INT32_RANGE };
            let web = webs.defined(region, pc);
            let joined = match ranges[web] {
                Some((low, high)) => (low.min(held.0), high.max(held.1)),
                None => held,
            };
            if ranges[web] != Some(joined) {
                growth[web] += 1;
                ranges[web] = Some(if growth[web] > MAX_RANGE_GROWTH { INT32_RANGE } else { joined });
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // A web no definition reached holds nothing; it is taken to hold any `i32`.
    let ranges: Vec<Option<Range>> = ranges.into_iter().map(|range| Some(range.unwrap_or(INT32_RANGE))).collect();
    let inside = |pc: u32| (region.header..=region.end).contains(&pc);
    let written_back: std::collections::BTreeSet<Definition> = webs
        .leaves
        .iter()
        .filter(|((pc, _), _)| !inside(*pc))
        .map(|(_, definition)| webs.find(*definition))
        .collect();
    // Whether every read of `web` treats its -0 as +0: a bitwise operand (ToInt32), a
    // comparison, or a sum with an `i32`, which is never -0 itself.
    let signless_reads = |web: Definition| {
        region.instructions.iter().copied().enumerate().all(|(offset, instruction)| {
            let pc = region.header + offset as u32;
            let reads = |register: Register| webs.read(region, pc, register) == web;
            if !number_reads(instruction).into_iter().any(reads) {
                return true;
            }
            match instruction.op() {
                Op::JumpBinaryFalse => true,
                Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                    let reads_operand = |operand: Operand| operand.register_index().is_some_and(reads);
                    match arithmetic(instruction.op(), instruction.binary_operator()) {
                        Some(Arithmetic::Int32(_)) => true,
                        Some(Arithmetic::Float(NumberArithmetic::Add)) => {
                            !(reads_operand(instruction.operand_b()) && reads_operand(instruction.operand_c()))
                                && int32(webs.defined(region, pc))
                        }
                        Some(Arithmetic::Float(NumberArithmetic::Subtract)) => {
                            !reads_operand(instruction.operand_b()) && int32(webs.defined(region, pc))
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        })
    };
    // The webs only ToInt32 reads, as the greatest fixpoint: a sum's operand counts while
    // the sum is one.
    let sums = |instruction: WideInstruction| {
        matches!(instruction.op(), Op::Binary | Op::NumericAdd | Op::NumericMultiply).then(|| arithmetic(instruction.op(), instruction.binary_operator())).flatten()
    };
    let mut truncated: std::collections::BTreeSet<Definition> = (0..webs.parent.len())
        .filter(|definition| webs.find(*definition) == *definition && int32(*definition) && !written_back.contains(definition))
        .collect();
    loop {
        let before = truncated.len();
        let kept: std::collections::BTreeSet<Definition> = truncated
            .iter()
            .copied()
            .filter(|web| {
                region.instructions.iter().copied().enumerate().all(|(offset, instruction)| {
                    let pc = region.header + offset as u32;
                    if !number_reads(instruction).into_iter().any(|register| webs.read(region, pc, register) == *web) {
                        return true;
                    }
                    match sums(instruction) {
                        Some(Arithmetic::Int32(_)) => true,
                        Some(Arithmetic::Float(NumberArithmetic::Add | NumberArithmetic::Subtract)) => {
                            truncated.contains(&webs.defined(region, pc))
                        }
                        _ => false,
                    }
                })
            })
            .collect();
        truncated = kept;
        if truncated.len() == before {
            break;
        }
    }
    let mut facts = Int32Facts::default();
    for (offset, instruction) in region.instructions.iter().copied().enumerate() {
        let pc = region.header + offset as u32;
        let Some(Arithmetic::Float(operator @ (NumberArithmetic::Add | NumberArithmetic::Subtract | NumberArithmetic::Multiply))) =
            matches!(instruction.op(), Op::Binary | Op::NumericAdd | Op::NumericMultiply)
                .then(|| arithmetic(instruction.op(), instruction.binary_operator()))
                .flatten()
        else {
            continue;
        };
        let web = webs.defined(region, pc);
        if !int32(web) {
            continue;
        }
        if operator != NumberArithmetic::Multiply && truncated.contains(&web) {
            facts.truncated.insert(pc);
        }
        let operand = |operand: Operand| operand_range(&ranges, pc, operand);
        if let Some((low, high)) =
            exact_range(instruction, &operand, &|register| register_range(&ranges, pc, register), &region.constants)
            && low >= INT32_RANGE.0
            && high <= INT32_RANGE.1
        {
            facts.exact.insert(pc);
        }
        if operator == NumberArithmetic::Multiply {
            let nonnegative = [instruction.operand_b(), instruction.operand_c()]
                .into_iter()
                .all(|side| operand(side).is_some_and(|range| range.0 >= 0));
            if nonnegative || (signless_reads(web) && !written_back.contains(&web)) {
                facts.signless_zero.insert(pc);
            }
        }
    }
    facts
}

fn counted_loops(
    region: &LoopRegion,
    webs: &Webs,
    representations: &[Representation],
    captures: &[Representation],
) -> Vec<CountedLoop> {
    (region.header..=region.end)
        .filter_map(|backedge| {
            let instruction = region.instructions[(backedge - region.header) as usize];
            (instruction.op() == Op::Jump).then_some(())?;
            let header = instruction.jump_target();
            (region.header..backedge).contains(&header).then_some(())?;
            counted_loop(region, webs, representations, captures, header, backedge)
        })
        .collect()
}

fn counted_loop(
    region: &LoopRegion,
    webs: &Webs,
    representations: &[Representation],
    captures: &[Representation],
    header: u32,
    backedge: u32,
) -> Option<CountedLoop> {
    use std::collections::{BTreeMap, BTreeSet};
    let at = |pc: u32| region.instructions[(pc - region.header) as usize];
    let body = header..backedge;
    // One path: the test is the body's only branch, and it leaves the loop.
    let mut test = None;
    for pc in body.clone() {
        let instruction = at(pc);
        match instruction.op() {
            Op::Jump => return None,
            Op::JumpBinaryFalse if test.is_none() && !(header..=backedge).contains(&instruction.jump_target()) => {
                test = Some(pc);
            }
            Op::JumpBinaryFalse => return None,
            Op::JumpFalse | Op::JumpTrue => return None,
            _ => {}
        }
    }
    let test = test?;
    // Control enters only at the header.
    let enters_inside = (region.header..=region.end).any(|pc| {
        let instruction = at(pc);
        matches!(
            instruction.op(),
            Op::Jump | Op::JumpFalse | Op::JumpTrue | Op::JumpBinaryFalse
        ) && (header + 1..=backedge).contains(&instruction.jump_target())
    });
    if enters_inside {
        return None;
    }
    let int32 = |web: Definition| representations[web] == Representation::Int32;
    // The registers the body writes; any other register is invariant in it.
    let written: BTreeSet<Register> = body.clone().filter_map(|pc| number_definition(at(pc))).collect();
    let invariant = |value: &Affine| {
        let base = |base: Base| match base {
            Base::Start(register) => !written.contains(&register),
            Base::Constant | Base::Capture(_) | Base::Term(_) => true,
        };
        base(value.base) && value.addend.is_none_or(base)
    };
    let mut terms: Vec<Term> = Vec::new();
    let constant = |number: f64| int32_number(number).then(|| Affine { offset: number as i64, ..Affine::of(Base::Constant) });
    let mut state: BTreeMap<Register, Option<Affine>> = BTreeMap::new();
    let value = |state: &BTreeMap<Register, Option<Affine>>, register: Register| {
        state
            .get(&register)
            .copied()
            .unwrap_or(Some(Affine::of(Base::Start(register))))
    };
    // Whether a source the instruction at `pc` reads is held as an `i32`, and its value.
    let source = |state: &BTreeMap<Register, Option<Affine>>, pc: u32, operand: Operand| match operand.kind()? {
        OperandKind::Register => {
            let register = operand.payload();
            Some((int32(webs.read(region, pc, register)), value(state, register)))
        }
        OperandKind::Constant => {
            let number = *region.constants.get(&operand.payload())?;
            Some((int32_number(number), constant(number)))
        }
        OperandKind::Field | OperandKind::Local => None,
    };
    let mut candidates: Vec<Fact> = Vec::new();
    let mut tested = None;
    for pc in body.clone() {
        let instruction = at(pc);
        let register = |register: Register| (int32(webs.read(region, pc, register)), value(&state, register));
        let defined = || int32(webs.defined(region, pc));
        let result = match instruction.op() {
            Op::LoadConst => {
                let index = u16::try_from(instruction.constant_index()).ok()?;
                constant(*region.constants.get(&index)?)
            }
            Op::LoadCapture => {
                let slot = instruction.capture_slot();
                let index = region.captures.iter().position(|capture| *capture == slot)?;
                (captures[index] == Representation::Int32).then_some(Affine::of(Base::Capture(index)))
            }
            Op::Move | Op::ToNumeric => register(instruction.register_b()).1,
            Op::IncDec => {
                let (held, operand) = register(instruction.register_b());
                let step = if instruction.boolean_flag()? { -1 } else { 1 };
                let sum = operand.and_then(|operand| operand.plus(step));
                if let Some(value) = sum.filter(|_| held && defined()) {
                    candidates.push(Fact { pc, value, bound: Bound::Int32 });
                }
                sum
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let left = source(&state, pc, instruction.operand_b())?;
                let right = source(&state, pc, instruction.operand_c())?;
                let held = left.0 && right.0 && defined();
                // A product of invariant values that are no products is a term.
                let factor = |value: &Affine| {
                    invariant(value)
                        && ![Some(value.base), value.addend].into_iter().flatten().any(|base| {
                            matches!(base, Base::Term(term) if terms[term].multiply)
                        })
                };
                let operator = arithmetic(instruction.op(), instruction.binary_operator())?;
                let sum = match (operator, left.1, right.1) {
                    (Arithmetic::Float(NumberArithmetic::Add), Some(left), Some(right)) => match (left, right) {
                        (_, Affine { base: Base::Constant, addend: None, offset }) => left.plus(offset),
                        (Affine { base: Base::Constant, addend: None, offset }, _) => right.plus(offset),
                        // An invariant value folds into the other's addend.
                        (_, Affine { base, addend: None, offset }) if left.addend.is_none() && invariant(&right) => {
                            Affine { addend: Some(base), ..left }.plus(offset)
                        }
                        (Affine { base, addend: None, offset }, _) if right.addend.is_none() && invariant(&left) => {
                            Affine { addend: Some(base), ..right }.plus(offset)
                        }
                        _ => None,
                    },
                    (Arithmetic::Float(NumberArithmetic::Subtract), Some(left), Some(right))
                        if right.base == Base::Constant && right.addend.is_none() =>
                    {
                        left.plus(-right.offset)
                    }
                    (Arithmetic::Float(NumberArithmetic::Multiply), Some(left), Some(right))
                        if held && factor(&left) && factor(&right) =>
                    {
                        terms.push(Term { multiply: true, left, right });
                        let value = Affine::of(Base::Term(terms.len() - 1));
                        candidates.push(Fact { pc, value, bound: Bound::Product });
                        Some(value)
                    }
                    _ => None,
                };
                if operator != Arithmetic::Float(NumberArithmetic::Multiply)
                    && let Some(value) = sum.filter(|_| held)
                {
                    candidates.push(Fact { pc, value, bound: Bound::Int32 });
                }
                sum
            }
            Op::GetIndex => {
                let array = instruction.operand_b().register_index()?;
                if let (true, Some(value)) = source(&state, pc, instruction.operand_c())? {
                    candidates.push(Fact { pc, value, bound: Bound::Index(array) });
                }
                None
            }
            Op::SetIndex => {
                if let (true, Some(value)) = register(instruction.register_c()) {
                    candidates.push(Fact { pc, value, bound: Bound::Index(instruction.register_b()) });
                }
                None
            }
            Op::JumpBinaryFalse => {
                let left = source(&state, pc, instruction.operand_b())?;
                let right = source(&state, pc, instruction.operand_c())?;
                tested = Some((instruction.binary_operator_field(), left, right));
                None
            }
            _ => None,
        };
        if let Some(register) = number_definition(instruction) {
            state.insert(register, result);
        }
    }
    let steps: BTreeMap<Register, i64> = state
        .iter()
        .filter_map(|(register, value)| match value {
            Some(Affine { base: Base::Start(base), offset, addend: None }) if base == register => {
                Some((*register, *offset))
            }
            _ => None,
        })
        .collect();
    // How much a base changes per iteration, when it changes by a constant: a register the
    // body never writes does not change.
    let step = |base: Base| match base {
        Base::Constant | Base::Capture(_) | Base::Term(_) => Some(0),
        Base::Start(register) if !written.contains(&register) => Some(0),
        Base::Start(register) => steps.get(&register).copied().filter(|step| step.abs() <= MAX_AFFINE_TERM),
    };
    // A base the proof reads at the header is an `i32` there; a term's values are, and its
    // own values were found invariant when it was made.
    fn held_base(base: Base, terms: &[Term], read: &dyn Fn(Register) -> bool) -> bool {
        match base {
            Base::Start(register) => read(register),
            Base::Constant | Base::Capture(_) => true,
            Base::Term(term) => [terms[term].left, terms[term].right].iter().all(|value| {
                held_base(value.base, terms, read) && value.addend.is_none_or(|addend| held_base(addend, terms, read))
            }),
        }
    }
    let read = |register: Register| int32(webs.read(region, header, register));
    let held = |base: Base| held_base(base, &terms, &read);
    // An addend must not change across iterations.
    let fixed = |value: &Affine| value.addend.is_none_or(|addend| step(addend) == Some(0) && held(addend));
    let (operator, (left_held, left), (right_held, right)) = tested?;
    let (left, right) = (left?, right?);
    (left_held && right_held && held(left.base) && held(right.base) && fixed(&left) && fixed(&right)).then_some(())?;
    // The induction variable is the side that moves; a relation with it on the right
    // reads mirrored.
    let (induction, bound, mirrored) = match (step(left.base)?, step(right.base)?) {
        (moving, 0) if moving != 0 => (left, right, false),
        (0, moving) if moving != 0 => (right, left, true),
        _ => return None,
    };
    use BinaryOperator as B;
    let operator = match operator {
        op if op == B::LessThan as u32 => (Continue::AtMost, -1),
        op if op == B::LessEqualThan as u32 => (Continue::AtMost, 0),
        op if op == B::GreaterThan as u32 => (Continue::AtLeast, 1),
        op if op == B::GreaterEqualThan as u32 => (Continue::AtLeast, 0),
        _ => return None,
    };
    // `b < x` is `x > b`: the relation flips, and so does the strict bound's adjustment.
    let (relation, adjustment) = match (operator, mirrored) {
        (relation, false) => relation,
        ((Continue::AtMost, adjustment), true) => (Continue::AtLeast, -adjustment),
        ((Continue::AtLeast, adjustment), true) => (Continue::AtMost, -adjustment),
    };
    let facts: Vec<Fact> = candidates
        .into_iter()
        .filter(|fact| held(fact.value.base) && step(fact.value.base).is_some() && fixed(&fact.value))
        .collect();
    (!facts.is_empty()).then_some(())?;
    Some(CountedLoop {
        header,
        backedge,
        test,
        induction,
        relation,
        bound: bound.plus(adjustment)?,
        steps,
        terms,
        facts,
    })
}

/// Lower `region` to a shared-entry Cranelift function and return its exact-PC widening map.
pub(super) fn lower(
    region: &LoopRegion,
    observation: &Observation<'_>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    poll: Option<i64>,
    track_backedges: bool,
) -> Option<(ir::Function, std::collections::BTreeMap<u32, Widening>)> {
    let pointer = isa.pointer_type();
    let signature = super::native_entry_signature(pointer, isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(
        UserFuncName::user(LOOP_NAMESPACE, region.header),
        signature,
    );
    let webs = webs(region);
    let (representations, entry) = representations(region, &webs, observation);
    let counted = counted_loops(region, &webs, &representations, &entry.captures);
    let facts = int32_facts(region, &webs, &representations);
    let mut context = FunctionBuilderContext::new();
    let builder = FunctionBuilder::new(&mut native, &mut context);
    let widenings = Lowering::new(
        builder,
        region,
        observation,
        pointer,
        webs,
        representations,
        entry,
        counted,
        facts,
        poll,
        track_backedges,
    )
        .lower(isa.frontend_config())?;
    Some((native, widenings))
}

/// The Cranelift user-function namespace of loop regions ("QLOP").
const LOOP_NAMESPACE: u32 = 0x514c_4f50;

/// A value in native code with its representation.
#[derive(Clone, Copy)]
struct Typed {
    value: ir::Value,
    representation: Representation,
}

struct Lowering<'a, 'b> {
    /// The shared native body: the builder, each register's views, which hold each web's value
    /// in its representation (`view_type`), and the interrupt check.
    body: super::Body<'b>,
    region: &'a LoopRegion,
    observation: &'a Observation<'a>,
    pointer: ir::Type,
    webs: Webs,
    /// Each web's representation, at its root definition.
    representations: Vec<Representation>,
    /// The instruction being lowered, whose reads and result name webs.
    pc: u32,
    /// The entry registers the region never writes that held exactly 1 when it compiled:
    /// each is guarded at entry (`Widening::Unit`), and a product with or a quotient by one
    /// is its other operand, exactly, for every double (signed zeros, infinities and NaN
    /// alike).
    units: std::collections::BTreeSet<Register>,
    /// Each capture's value, unboxed at entry, in region order.
    captures: Vec<Typed>,
    /// Numeric cached field values, loaded from the guarded entry record.
    fields: Vec<Typed>,
    entry_representations: EntryRepresentations,
    /// The `i32` arithmetic that needs no guard (`int32_facts`).
    facts: Int32Facts,
    blocks: Vec<ir::Block>,
    /// The region's counted loops; each has a preheader block, which every arrival at its
    /// header from outside it takes, and a proved copy of its body's blocks.
    counted: Vec<CountedLoop>,
    preheaders: Vec<ir::Block>,
    proved_blocks: Vec<Vec<ir::Block>>,
    /// The counted loop whose proved copy is being lowered.
    copy: Option<usize>,
    /// Shared handoff blocks; this tier fills per-PC leaves with its live-state writeback.
    exits: super::RegionExitBlocks,
    registers: ir::Value,
    /// An inlining region's scratch registers (`LoopEntry::scratch`).
    scratch: Option<ir::Value>,
    entry: ir::Value,
    /// The result word the outcome is written to.
    result: ir::Value,
    /// Count the region's taken backedges only for the opt-in shared-tier admission path.
    track_backedges: bool,
    /// Each array as native code indexes it, by register.
    arrays: std::collections::BTreeMap<Register, ArrayView>,
    widenings: std::collections::BTreeMap<u32, Widening>,
    /// Per block, the first block of the chain it continues: each block after the first
    /// has its predecessors inside the chain (a guard's passing branch, the join of a test
    /// whose other arm returns into the chain), so every chain block dominates the blocks
    /// lowered after it in the chain.
    chains: std::collections::HashMap<ir::Block, ir::Block>,
    /// The exact `i32` results computed so far, by operator and operands (`int32_key`), with the chain
    /// computing them: the same arithmetic later in that chain is that result, its guards
    /// having passed.
    exact: std::collections::BTreeMap<
        (NumberArithmetic, ir::Value, ir::Value),
        (ir::Value, ir::Block),
    >,
    /// The elements read so far, by array register, key and representation, with the chain
    /// reading them and the element stores lowered before: the same read later in that
    /// chain, with no store between, is that value, its guards having passed.
    element_reads: std::collections::HashMap<(Register, ir::Value, Representation), (Typed, ir::Block, usize)>,
    /// The element stores lowered so far; any may write any array's elements.
    element_stores: usize,
    /// The Number each canonical encoding read from memory unboxes to, mapped to the bits as
    /// read: boxing the Number gives those bits back, so a store of it stores them.
    encodings: std::collections::HashMap<ir::Value, ir::Value>,
    /// Per region PC, whether control reaches its instruction only by falling through from
    /// the one before (`straight_pcs`), so its block continues that one's chain.
    straight: Vec<bool>,
}

/// Per region PC, whether the instruction before it is its only predecessor: no jump
/// targets it, and it is neither the header nor a counted loop's header, which the entry
/// and preheaders reach.
fn straight_pcs(region: &LoopRegion, counted: &[CountedLoop]) -> Vec<bool> {
    let count = region.instructions.len();
    let mut predecessors = vec![Vec::new(); count];
    for pc in region.header..=region.end {
        for successor in successors(region, pc) {
            if (region.header..=region.end).contains(&successor) {
                predecessors[(successor - region.header) as usize].push(pc);
            }
        }
    }
    (region.header..=region.end)
        .map(|pc| {
            pc != region.header
                && !counted.iter().any(|counted| counted.header == pc)
                && predecessors[(pc - region.header) as usize] == [pc - 1]
        })
        .collect()
}

fn flags() -> ir::MemFlagsData {
    ir::MemFlagsData::trusted()
}

/// The register view that holds a value in `representation` (`super::Body`).
fn view_type(representation: Representation) -> crate::WasmType {
    match representation {
        Representation::Int32 => crate::WasmType::I32,
        Representation::Float64 => crate::WasmType::F64,
        Representation::Tagged => crate::WasmType::I64,
    }
}

impl<'a, 'b> Lowering<'a, 'b> {
    fn new(
        mut builder: FunctionBuilder<'b>,
        region: &'a LoopRegion,
        observation: &'a Observation<'a>,
        pointer: ir::Type,
        webs: Webs,
        representations: Vec<Representation>,
        entry_representations: EntryRepresentations,
        counted: Vec<CountedLoop>,
        facts: Int32Facts,
        poll: Option<i64>,
        track_backedges: bool,
    ) -> Self {
        let start = builder.create_block();
        builder.append_block_params_for_function_params(start);
        builder.switch_to_block(start);
        let &[context, registers, _, entry, result] = builder.block_params(start) else {
            unreachable!("the entry signature has five parameters")
        };
        let blocks = (region.header..=region.end).map(|_| builder.create_block()).collect();
        let straight = straight_pcs(region, &counted);
        let preheaders = counted.iter().map(|_| builder.create_block()).collect();
        let proved_blocks = counted
            .iter()
            .map(|counted| (counted.header..=counted.backedge).map(|_| builder.create_block()).collect())
            .collect();
        let written: std::collections::BTreeSet<Register> =
            region.instructions.iter().filter_map(|instruction| number_definition(*instruction)).collect();
        let one = Value::number(1.0);
        let units = (0..region.numbers.len())
            .filter(|entry| webs.entry_read[*entry])
            .map(|entry| region.numbers[entry])
            .filter(|register| {
                !written.contains(register)
                    && !observation.widened.contains(&Widening::Unit(*register))
                    && (observation.register)(*register) == one
            })
            .collect();
        Self {
            body: super::Body::new(builder, context, poll),
            region,
            observation,
            pointer,
            webs,
            representations,
            pc: region.header,
            units,
            captures: Vec::new(),
            fields: Vec::new(),
            entry_representations,
            facts,
            blocks,
            counted,
            preheaders,
            proved_blocks,
            copy: None,
            exits: super::RegionExitBlocks::default(),
            chains: std::collections::HashMap::new(),
            exact: std::collections::BTreeMap::new(),
            element_reads: std::collections::HashMap::new(),
            element_stores: 0,
            encodings: std::collections::HashMap::new(),
            straight,
            registers,
            scratch: None,
            entry,
            result,
            track_backedges,
            arrays: std::collections::BTreeMap::new(),
            widenings: std::collections::BTreeMap::new(),
        }
    }

    fn lower(
        mut self,
        frontend: cranelift_codegen::isa::TargetFrontendConfig,
    ) -> Option<std::collections::BTreeMap<u32, Widening>> {
        self.enter()?;
        for pc in self.region.header..=self.region.end {
            let block = self.block(pc)?;
            self.enter_instruction(pc, block);
            self.pc = pc;
            self.instruction(pc)?;
        }
        for index in 0..self.counted.len() {
            self.copy = Some(index);
            let (header, backedge) = (self.counted[index].header, self.counted[index].backedge);
            for pc in header..=backedge {
                self.enter_instruction(pc, self.proved_blocks[index][(pc - header) as usize]);
                self.pc = pc;
                self.instruction(pc)?;
            }
            self.copy = None;
            self.preheader(index);
        }
        self.fill_outcomes();
        self.write_back();
        if let Some(block) = self.body.termination_block {
            self.body.builder.switch_to_block(block);
            let status = self.body.builder.ins().iconst(types::I32, super::TERMINATED_STATUS);
            self.body.builder.ins().return_(&[status]);
        }
        self.body.builder.seal_all_blocks();
        self.body.into_builder().finalize(frontend);
        Some(self.widenings)
    }

    /// Switches to the block of the instruction at `pc`: one only the instruction before it
    /// reaches has all its predecessors, so it is sealed and continues that one's chain.
    fn enter_instruction(&mut self, pc: u32, block: ir::Block) {
        if self.straight[(pc - self.region.header) as usize] {
            self.continue_chain(block);
            self.body.builder.seal_block(block);
        } else {
            self.body.builder.switch_to_block(block);
        }
    }

    fn block(&self, pc: u32) -> Option<ir::Block> {
        (self.region.header..=self.region.end)
            .contains(&pc)
            .then(|| self.blocks[(pc - self.region.header) as usize])
    }

    /// The block control continues at for `target`, from the instruction at `from` or from
    /// the entry: an arrival at a counted loop's header from outside it runs the loop's
    /// preheader, and within a proved copy control stays in the copy.
    fn target_block(&self, from: Option<u32>, target: u32) -> Option<ir::Block> {
        let arrival = self.counted.iter().position(|counted| {
            counted.header == target && !from.is_some_and(|pc| counted.contains(pc))
        });
        if let Some(index) = arrival {
            return Some(self.preheaders[index]);
        }
        if let Some(index) = self.copy
            && self.counted[index].contains(target)
        {
            return Some(self.proved_blocks[index][(target - self.counted[index].header) as usize]);
        }
        self.block(target)
    }

    /// Whether the code being lowered is a proved copy whose proof covers the instruction at
    /// `pc`, so its guard is dropped.
    fn proved(&self, pc: u32) -> bool {
        self.copy.is_some_and(|index| self.counted[index].proves(pc))
    }

    /// The `i64` value at a counted loop's header of a base the proof reads, which
    /// `counted_loop` found is held as an `i32` there.
    fn start(&mut self, counted: &CountedLoop, base: Base) -> ir::Value {
        let value = match base {
            Base::Constant => return self.body.builder.ins().iconst(types::I64, 0),
            Base::Capture(index) => self.captures[index].value,
            Base::Start(register) => {
                let web = self.webs.read(self.region, counted.header, register);
                self.web(web).value
            }
            Base::Term(term) => {
                let Term { multiply, left, right } = counted.terms[term];
                let (left, right) = (self.affine_start(counted, left), self.affine_start(counted, right));
                return if multiply { self.body.builder.ins().imul(left, right) } else { self.body.builder.ins().iadd(left, right) };
            }
        };
        self.body.builder.ins().sextend(types::I64, value)
    }

    fn affine_start(&mut self, counted: &CountedLoop, value: Affine) -> ir::Value {
        let start = self.start(counted, value.base);
        let start = self.body.builder.ins().iadd_imm_s(start, value.offset);
        match value.addend {
            Some(addend) => {
                let addend = self.start(counted, addend);
                self.body.builder.ins().iadd(start, addend)
            }
            None => start,
        }
    }

    /// The counted loop's preheader: runs its proof from the values arriving at the header
    /// and continues in the proved copy when every fact holds, else in the guarded body.
    fn preheader(&mut self, index: usize) {
        let counted = self.counted[index].clone();
        self.body.builder.switch_to_block(self.preheaders[index]);
        let holds = self.proof(&counted);
        let proved = self.proved_blocks[index][0];
        let guarded = self.blocks[(counted.header - self.region.header) as usize];
        self.body.builder.ins().brif(holds, proved, &[], guarded, &[]);
    }

    /// Whether every fact of `counted` holds for the iterations its arrival runs. The test
    /// passes `trips` times and fails on the next arrival, so an instruction before the test
    /// runs for iterations `0..=trips` and one after it for `0..trips`; an affine value is
    /// monotonic in the iteration, so its extremes are at the first and last of them.
    fn proof(&mut self, counted: &CountedLoop) -> ir::Value {
        let tested = self.affine_start(counted, counted.induction);
        let bound = self.affine_start(counted, counted.bound);
        let step = counted.base_step(counted.induction.base);
        let (trips, mut holds) = match (counted.relation, step > 0) {
            // Moving toward the bound, the test passes until the value crosses it.
            (Continue::AtMost, true) | (Continue::AtLeast, false) => {
                let distance = match counted.relation {
                    Continue::AtMost => self.body.builder.ins().isub(bound, tested),
                    Continue::AtLeast => self.body.builder.ins().isub(tested, bound),
                };
                let span = if step.abs() == 1 {
                    distance
                } else {
                    let magnitude = self.body.builder.ins().iconst(types::I64, step.abs());
                    self.body.builder.ins().udiv(distance, magnitude)
                };
                let passing = self.body.builder.ins().iadd_imm_s(span, 1);
                let zero = self.body.builder.ins().iconst(types::I64, 0);
                let crossed = self.body.builder.ins().icmp_imm_s(IntCC::SignedLessThan, distance, 0);
                let trips = self.body.builder.ins().select(crossed, zero, passing);
                (trips, self.body.builder.ins().iconst(types::I8, 1))
            }
            // Moving away from it, the test passes until the value leaves `i32`, which the
            // guarded body meets; the proof holds only when the first test fails.
            (Continue::AtMost, false) | (Continue::AtLeast, true) => {
                let condition = match counted.relation {
                    Continue::AtMost => IntCC::SignedGreaterThan,
                    Continue::AtLeast => IntCC::SignedLessThan,
                };
                let fails = self.body.builder.ins().icmp(condition, tested, bound);
                (self.body.builder.ins().iconst(types::I64, 0), fails)
            }
        };
        let none = self.body.builder.ins().icmp_imm_s(IntCC::Equal, trips, 0);
        let trips_after = self.body.builder.ins().iadd_imm_s(trips, -1);
        for fact in &counted.facts {
            let first = self.affine_start(counted, fact.value);
            let step = counted.base_step(fact.value.base);
            let last_iteration = if fact.pc < counted.test { trips } else { trips_after };
            let travel = self.body.builder.ins().imul_imm_s(last_iteration, step);
            let last = self.body.builder.ins().iadd(first, travel);
            let (low, high) = if step >= 0 { (first, last) } else { (last, first) };
            let (above, below) = match fact.bound {
                Bound::Index(array) => {
                    let length = self.length(self.arrays[&array]);
                    (
                        self.body.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, low, 0),
                        self.body.builder.ins().icmp(IntCC::SignedLessThan, high, length),
                    )
                }
                Bound::Int32 => (
                    self.body.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, low, i64::from(i32::MIN)),
                    self.body.builder.ins().icmp_imm_s(IntCC::SignedLessThanOrEqual, high, i64::from(i32::MAX)),
                ),
                Bound::Product => {
                    // The product is invariant (`low` is `high`); a zero one is -0 when a
                    // factor is negative.
                    let Base::Term(term) = fact.value.base else { unreachable!("a product fact names its term") };
                    let Term { left, right, .. } = counted.terms[term];
                    let (left, right) = (self.affine_start(counted, left), self.affine_start(counted, right));
                    let signs = self.body.builder.ins().bor(left, right);
                    let nonnegative = self.body.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, signs, 0);
                    let nonzero = self.body.builder.ins().icmp_imm_s(IntCC::NotEqual, low, 0);
                    let signed_zero_free = self.body.builder.ins().bor(nonnegative, nonzero);
                    let fits = self.body.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, low, i64::from(i32::MIN));
                    let fits_high = self.body.builder.ins().icmp_imm_s(IntCC::SignedLessThanOrEqual, high, i64::from(i32::MAX));
                    let fits = self.body.builder.ins().band(fits, fits_high);
                    (fits, signed_zero_free)
                }
            };
            let within = self.body.builder.ins().band(above, below);
            let within = if fact.pc < counted.test { within } else { self.body.builder.ins().bor(within, none) };
            holds = self.body.builder.ins().band(holds, within);
        }
        holds
    }

    /// Writes the outcome `word` to the result word and returns the status of a run that
    /// finished.
    fn finish(&mut self, word: ir::Value) {
        self.body.builder.ins().store(flags(), word, self.result, 0);
        let status = self.body.builder.ins().iconst(types::I32, 0);
        self.body.builder.ins().return_(&[status]);
    }

    fn word(&mut self, index: usize) -> ir::Value {
        self.body.builder
            .ins()
            .load(types::I64, flags(), self.entry, index as i32 * WORD_BYTES)
    }

    fn field_store_pointer(&mut self, index: usize) -> ir::Value {
        let start = ENTRY_CAPTURES
            + self.region.captures.len()
            + self.region.fields.len()
            + ARRAY_WORDS * self.region.arrays.len();
        self.word(start + index)
    }

    /// A block laid out after the hot code: an exit, or a representation a region rarely
    /// meets.
    fn cold_block(&mut self) -> ir::Block {
        let block = self.body.builder.create_block();
        self.body.builder.set_cold_block(block);
        block
    }

    /// A block that returns not entered, naming `widening`.
    fn not_entered(&mut self, widening: Option<Widening>) -> ir::Block {
        self.exits.outcome_block(
            &mut self.body.builder,
            super::RegionExit::EntryMiss(widening.map_or(0, Widening::code)),
        )
    }

    /// Fills each `outcome_block`: not entered returns at once, and every other outcome
    /// leaves at its PC through the write-back.
    fn fill_outcomes(&mut self) {
        for (block, outcome) in self.exits.take_outcomes() {
            self.body.builder.switch_to_block(block);
            if matches!(outcome, super::RegionExit::EntryMiss(_)) {
                let word = self.body.builder.ins().iconst(types::I64, outcome.encode() as i64);
                self.finish(word);
            } else {
                self.leave(outcome);
            }
        }
    }

    /// Unboxes every Number register and capture in its representation, entering only when
    /// all hold such values.
    fn enter(&mut self) -> Option<()> {
        let fields_start = ENTRY_CAPTURES + self.region.captures.len();
        let arrays_start = fields_start + self.region.fields.len();
        for (index, array) in self.region.arrays.iter().enumerate() {
            let base = arrays_start + ARRAY_WORDS * index;
            let elements = self.word(base);
            let access = if array.written {
                ArrayAccess::Written { capacity: self.word(base + 2) }
            } else {
                ArrayAccess::Read { length: self.word(base + 1) }
            };
            self.arrays.insert(array.register, ArrayView { elements, access });
        }
        for index in 0..self.region.captures.len() {
            let bits = self.word(ENTRY_CAPTURES + index);
            let representation = self.entry_representations.captures[index];
            let otherwise = self.not_entered(Some(Widening::Capture(index as u16)));
            let value = self.unbox(bits, representation, otherwise);
            self.captures.push(Typed { value, representation });
        }
        for index in 0..self.region.fields.len() {
            let bits = self.word(fields_start + index);
            let representation = self.entry_representations.fields[index];
            let value = if representation == Representation::Tagged {
                bits
            } else {
                let otherwise = self.not_entered(Some(Widening::Field(index as u16)));
                self.unbox(bits, representation, otherwise)
            };
            self.fields.push(Typed { value, representation });
        }
        if let Some(inlining) = &self.region.inlining {
            // The scratch registers' address follows arrays and field-store slots; each
            // inlined callee is still the function its field held when the region was made.
            self.scratch = Some(self.word(
                arrays_start
                    + ARRAY_WORDS * self.region.arrays.len()
                    + self.region.field_store_sites.len(),
            ));
            for frame in inlining.frames.clone() {
                let held = self.fields[frame.field].value;
                let other = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, held, frame.callee.to_bits() as i64);
                let otherwise = self.not_entered(None);
                self.pass_unless(other, otherwise);
            }
        }
        // Only an entry value some read reaches is loaded and guarded.
        let registers: Vec<(Register, Definition)> = (0..self.region.entry_count())
            .filter(|entry| self.webs.entry_read[*entry])
            .map(|entry| (self.region.entry_register(entry).expect("entry register"), self.webs.find(entry)))
            .collect();
        // An inlined callee's register holds nothing at entry: one read before written there
        // has no value to read.
        if let Some(inlining) = &self.region.inlining
            && registers.iter().any(|(register, _)| *register >= inlining.caller_registers)
        {
            return None;
        }
        for (register, web) in registers {
            let representation = self.representations[web];
            let bits = self.body.builder.ins().load(
                types::I64,
                flags(),
                self.registers,
                i32::from(register) * WORD_BYTES,
            );
            if self.units.contains(&register) {
                let one = self.body.builder.ins().iconst(types::I64, Value::number(1.0).to_bits() as i64);
                let other = self.body.builder.ins().icmp(IntCC::NotEqual, bits, one);
                let otherwise = self.not_entered(Some(Widening::Unit(register)));
                let pass = self.body.builder.create_block();
                self.body.builder.ins().brif(other, otherwise, &[], pass, &[]);
                self.body.builder.switch_to_block(pass);
            }
            if representation == Representation::Tagged {
                self.body.write(register, view_type(representation), bits);
                continue;
            }
            let otherwise = self.not_entered(Some(Widening::Register(register)));
            let value = self.unbox(bits, representation, otherwise);
            self.body.write(register, view_type(representation), value);
        }
        let header = self.target_block(None, self.region.header)?;
        self.body.builder.ins().jump(header, &[]);
        Some(())
    }

    /// The value `bits` encode in `representation`: an encoded `i32` for `Int32`, any Number
    /// for `Float64`, testing for a double first; anything else branches to `otherwise`.
    fn unbox(&mut self, bits: ir::Value, representation: Representation, otherwise: ir::Block) -> ir::Value {
        if representation == Representation::Tagged {
            return bits;
        }
        if representation == Representation::Int32 {
            let tag = self.body.builder.ins().band_imm_u(bits, ValueLayout::TAG_MASK as i64);
            let other = self.body.builder.ins().icmp_imm_u(IntCC::NotEqual, tag, ValueLayout::INT_TAG as i64);
            self.pass_unless(other, otherwise);
            return self.body.builder.ins().ireduce(types::I32, bits);
        }
        let double = self.body.builder.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
        self.unbox_float64(double, otherwise)
    }

    /// The Number a value's bits encode, given as the double they read as: every tag is a
    /// NaN pattern, so a value that is no NaN as a double is one, which one comparison
    /// finds; only a NaN pattern, a tagged value or a NaN double, takes the cold test of the
    /// tag field. Anything but a Number branches to `otherwise`.
    fn unbox_float64(&mut self, double: ir::Value, otherwise: ir::Block) -> ir::Value {
        let entered = self.body.builder.current_block().expect("lowering inside a block");
        let unordered = self.body.builder.ins().fcmp(FloatCC::Unordered, double, double);
        let nan_block = self.cold_block();
        let tagged_block = self.cold_block();
        let integer_block = self.cold_block();
        let join = self.body.builder.create_block();
        self.body.builder.append_block_param(join, types::F64);
        self.body.builder.ins().brif(unordered, nan_block, &[], join, &[ir::BlockArg::Value(double)]);
        self.body.builder.switch_to_block(nan_block);
        let bits = self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), double);
        // A tag field within the tag range marks a non-double.
        let field = self.body.builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
        let offset = self.body.builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
        let tagged = self
            .body.builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedLessThanOrEqual, offset, ValueLayout::TAG_FIELD_SPAN as i64);
        self.body.builder.ins().brif(tagged, tagged_block, &[], join, &[ir::BlockArg::Value(double)]);
        self.body.builder.switch_to_block(tagged_block);
        let int_field = (ValueLayout::INT_TAG >> ValueLayout::TAG_SHIFT) as i64;
        let is_int = self.body.builder.ins().icmp_imm_u(IntCC::Equal, field, int_field);
        self.body.builder.ins().brif(is_int, integer_block, &[], otherwise, &[]);
        self.body.builder.switch_to_block(integer_block);
        let integer = self.body.builder.ins().ireduce(types::I32, bits);
        let number = self.body.builder.ins().fcvt_from_sint(types::F64, integer);
        self.body.builder.ins().jump(join, &[ir::BlockArg::Value(number)]);
        // The join's other predecessors are the cold tests, which `entered` dominates.
        let chain = self.chain(entered);
        self.chains.insert(join, chain);
        self.body.builder.switch_to_block(join);
        self.body.builder.seal_block(join);
        let number = self.body.builder.block_params(join)[0];
        self.encodings.insert(number, double);
        number
    }

    /// The encoded bits of an `i32` Number.
    fn boxed_int32(&mut self, integer: ir::Value) -> ir::Value {
        let payload = self.body.builder.ins().uextend(types::I64, integer);
        self.body.builder.ins().bor_imm_u(payload, ValueLayout::INT_TAG as i64)
    }

    /// `Value::number(number)`'s bits: an integer when the truncation converts back to the
    /// same bits, the canonical NaN for any NaN, else the double's bits. A double its
    /// truncation differs from, ordered (any non-integral value but NaN), takes the short
    /// path, which is the double's bits; integral values, infinities and NaN take the
    /// conversion.
    fn boxed_float64(&mut self, number: ir::Value) -> ir::Value {
        let truncated = self.body.builder.ins().trunc(number);
        let integral = self.body.builder.ins().fcmp(FloatCC::UnorderedOrEqual, truncated, number);
        // Boxing on an exit's path (`write_back`) is as cold as the exit, so its blocks are
        // laid out after the hot code too, not between a loop body's blocks.
        let cold = self
            .body.builder
            .current_block()
            .is_some_and(|block| self.body.builder.func.layout.is_cold(block));
        let fractional_block = if cold { self.cold_block() } else { self.body.builder.create_block() };
        let integral_block = self.cold_block();
        let join = if cold { self.cold_block() } else { self.body.builder.create_block() };
        self.body.builder.append_block_param(join, types::I64);
        let tested = self.body.builder.current_block().expect("lowering inside a block");
        self.body.builder.ins().brif(integral, integral_block, &[], fractional_block, &[]);
        self.body.builder.switch_to_block(fractional_block);
        self.body.builder.seal_block(fractional_block);
        let fractional = self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
        self.body.builder.ins().jump(join, &[ir::BlockArg::Value(fractional)]);
        self.body.builder.switch_to_block(integral_block);
        self.body.builder.seal_block(integral_block);
        let integer = self.body.builder.ins().fcvt_to_sint_sat(types::I32, number);
        let back = self.body.builder.ins().fcvt_from_sint(types::F64, integer);
        let back_bits = self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), back);
        let bits = self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
        let exact = self.body.builder.ins().icmp(IntCC::Equal, back_bits, bits);
        let encoded = self.boxed_int32(integer);
        let integral = self.body.builder.ins().select(exact, encoded, bits);
        // A NaN converts to zero, which is not exact, so only the NaN test remains.
        let nan = self.body.builder.ins().fcmp(FloatCC::Unordered, number, number);
        let canonical = self.body.builder.ins().iconst(types::I64, ValueLayout::CANONICAL_NAN as i64);
        let integral = self.body.builder.ins().select(nan, canonical, integral);
        self.body.builder.ins().jump(join, &[ir::BlockArg::Value(integral)]);
        // Both arms return from the test's block, so the join continues its chain.
        let chain = self.chain(tested);
        self.chains.insert(join, chain);
        self.body.builder.switch_to_block(join);
        self.body.builder.seal_block(join);
        self.body.builder.block_params(join)[0]
    }

    fn boxed(&mut self, value: Typed) -> ir::Value {
        let encoding = self.encoding(value);
        if self.body.builder.func.dfg.value_type(encoding) == types::I64 {
            return encoding;
        }
        self.body.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), encoding)
    }

    /// `value`'s encoding as an `i64`, or as the `f64` its bits read as when it was read
    /// from memory (`encodings`), which a store writes unchanged.
    fn encoding(&mut self, value: Typed) -> ir::Value {
        match value.representation {
            Representation::Int32 => self.boxed_int32(value.value),
            Representation::Float64 => match self.encodings.get(&value.value) {
                Some(read) => *read,
                None => self.boxed_float64(value.value),
            },
            Representation::Tagged => value.value,
        }
    }

    /// Fills each leave block: the interpreter resumes at its PC, so only the registers live
    /// there (`LoopRegion::live`) take their native values; any other keeps the value it
    /// held, which nothing reads before writing.
    fn write_back(&mut self) {
        let leaves = self.exits.leave_blocks();
        for (pc, block) in leaves {
            self.body.builder.switch_to_block(block);
            let word = self.body.builder.block_params(block)[0];
            let registers: Vec<(Register, Definition)> = self
                .region
                .entry_registers()
                .filter_map(|register| {
                    let web = self.webs.leaves.get(&(pc, register))?;
                    Some((register, self.webs.find(*web)))
                })
                .collect();
            self.store_registers(&registers);
            self.finish(word);
        }
    }

    /// Writes back each register's web: a caller register to the activation's register
    /// file, an inlined callee's to the entry's scratch registers (`LoopEntry::scratch`),
    /// from which leaving at its instruction makes its activation.
    fn store_registers(&mut self, registers: &[(Register, Definition)]) {
        for &(register, web) in registers {
            let value = self.web(web);
            let bits = self.boxed(value);
            let (file, slot) = match (&self.region.inlining, self.scratch) {
                (Some(inlining), Some(scratch)) if register >= inlining.caller_registers => {
                    (scratch, register - inlining.caller_registers)
                }
                _ => (self.registers, register),
            };
            self.body.builder.ins().store(flags(), bits, file, i32::from(slot) * WORD_BYTES);
        }
    }

    /// Leaves with `outcome` at its exact continuation PC, reported as the region's
    /// `exit_pc` of it.
    fn leave(&mut self, outcome: super::RegionExit) {
        let reported = match outcome {
            super::RegionExit::Resume(pc) => super::RegionExit::Resume(self.region.exit_pc(pc)),
            super::RegionExit::Fallback(pc) => super::RegionExit::Fallback(self.region.exit_pc(pc)),
            super::RegionExit::Recompile(pc) => super::RegionExit::Recompile(self.region.exit_pc(pc)),
            super::RegionExit::EntryMiss(_) => outcome,
        };
        self.exits.leave(&mut self.body.builder, outcome, reported);
    }

    /// A block that exits at `pc` for the general path.
    fn exit_block(&mut self, pc: u32) -> ir::Block {
        self.exits
            .outcome_block(&mut self.body.builder, super::RegionExit::Fallback(pc))
    }

    /// Continues past a guard that `failed` decides, exiting at `pc` when it holds.
    fn guard(&mut self, pc: u32, failed: ir::Value) {
        let exit = self.exit_block(pc);
        self.pass_unless(failed, exit);
    }

    /// Branches to `failed_block` when `failed` holds, and continues the current chain in
    /// a new block otherwise, sealed at once since the branch is its only predecessor.
    fn pass_unless(&mut self, failed: ir::Value, failed_block: ir::Block) {
        let pass = self.body.builder.create_block();
        self.body.builder.ins().brif(failed, failed_block, &[], pass, &[]);
        self.continue_chain(pass);
        self.body.builder.seal_block(pass);
    }

    /// Switches to `block`, which continues the current block's chain (`chains`).
    fn continue_chain(&mut self, block: ir::Block) {
        let current = self.body.builder.current_block().expect("lowering inside a block");
        let head = self.chain(current);
        self.chains.insert(block, head);
        self.body.builder.switch_to_block(block);
    }

    fn chain(&self, block: ir::Block) -> ir::Block {
        self.chains.get(&block).copied().unwrap_or(block)
    }

    /// A block that exits at `pc` for the general path, naming `widening` for the next
    /// compilation.
    fn widen_block(&mut self, pc: u32, widening: Widening) -> ir::Block {
        self.widenings.insert(self.region.exit_pc(pc), widening);
        self.exits
            .outcome_block(&mut self.body.builder, super::RegionExit::Recompile(pc))
    }

    /// `guard` of an `i32` assumption: `widening` was no `i32` when it fails.
    fn guard_int32(&mut self, pc: u32, failed: ir::Value, widening: Widening) {
        let exit = self.widen_block(pc, widening);
        self.pass_unless(failed, exit);
    }

    /// The value of the web rooted at `web`, which its register's view holds where the web
    /// is live.
    fn web(&mut self, web: Definition) -> Typed {
        let representation = self.representations[web];
        let register = self.webs.register(self.region, web);
        Typed { value: self.body.read(register, view_type(representation)), representation }
    }

    /// `register`'s value before the instruction being lowered.
    fn register(&mut self, register: Register) -> Typed {
        let web = self.webs.read(self.region, self.pc, register);
        self.web(web)
    }

    /// The representation of the instruction being lowered's result.
    fn result_representation(&self) -> Representation {
        self.representations[self.webs.defined(self.region, self.pc)]
    }

    fn read(&mut self, source: Source) -> Typed {
        match source {
            Source::Register(register) => self.register(register),
            Source::Tagged(value) => Typed {
                value: self.body.builder.ins().iconst(types::I64, value.to_bits() as i64),
                representation: Representation::Tagged,
            },
            Source::Number(number) if int32_number(number) => Typed {
                value: self.body.builder.ins().iconst(types::I32, i64::from(number as i32)),
                representation: Representation::Int32,
            },
            Source::Number(number) => Typed {
                value: self.body.builder.ins().f64const(number),
                representation: Representation::Float64,
            },
        }
    }

    fn float(&mut self, value: Typed) -> ir::Value {
        match value.representation {
            Representation::Int32 => self.body.builder.ins().fcvt_from_sint(types::F64, value.value),
            Representation::Float64 => value.value,
            Representation::Tagged => unreachable!("tagged values are not Number operands"),
        }
    }

    fn source(&self, operand: Operand) -> Option<Source> {
        match operand.kind()? {
            OperandKind::Register => Some(Source::Register(operand.payload())),
            OperandKind::Constant => Some(Source::Number(*self.region.constants.get(&operand.payload())?)),
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    fn operand(&mut self, operand: Operand) -> Option<Typed> {
        let source = self.source(operand)?;
        Some(self.read(source))
    }

    /// Writes the instruction being lowered's result, `value`, in its web's representation;
    /// an `i32` result only reaches an `i32` web (`representations`). `register` is the
    /// result register, which names the web through the PC.
    fn define(&mut self, register: Register, value: Typed) {
        debug_assert_eq!(number_definition(self.region.instructions[(self.pc - self.region.header) as usize]), Some(register));
        let web = self.webs.defined(self.region, self.pc);
        let representation = self.representations[web];
        let value = match (representation, value.representation) {
            (Representation::Float64, Representation::Int32) => self.float(value),
            (Representation::Tagged, Representation::Int32 | Representation::Float64) => self.boxed(value),
            _ => value.value,
        };
        self.body.write(register, view_type(representation), value);
    }

    /// Continues at `target`: its block within the region, else the lane resumes there. A
    /// backward jump checks for termination first (`Body::interrupt_safepoint`), which ends
    /// the run as `LoopOutcome::Terminated`.
    fn jump(&mut self, pc: u32, target: u32) -> Option<()> {
        if target <= pc {
            self.body.interrupt_safepoint();
            if self.track_backedges && pc == self.region.backedge && target == self.region.header {
                let offset = i32::try_from(ENTRY_BACKEDGE_COUNT * std::mem::size_of::<u64>())
                    .expect("OSR entry counter offset fits i32");
                let count = self
                    .body
                    .builder
                    .ins()
                    .load(types::I64, flags(), self.entry, offset);
                let count = self.body.builder.ins().iadd_imm_s(count, 1);
                self.body
                    .builder
                    .ins()
                    .store(flags(), count, self.entry, offset);
            }
        }
        match self.target_block(Some(pc), target) {
            Some(block) => {
                self.body.builder.ins().jump(block, &[]);
            }
            None => self.leave(super::RegionExit::Resume(target)),
        }
        Some(())
    }

    /// Branches on the bytecode condition, preserving whether its true or false arm takes
    /// `target`; a branch arm that leaves the region resumes at that exact PC.
    fn branch(
        &mut self,
        pc: u32,
        target: u32,
        condition: ir::Value,
        jump_when: bool,
    ) -> Option<()> {
        let taken = self.body.builder.create_block();
        let not_taken = self.body.builder.create_block();
        let tested = self
            .body
            .builder
            .current_block()
            .expect("lowering inside a block");
        let (on_true, on_false) = if jump_when {
            (taken, not_taken)
        } else {
            (not_taken, taken)
        };
        self.body
            .builder
            .ins()
            .brif(condition, on_true, &[], on_false, &[]);
        self.body.builder.switch_to_block(taken);
        self.body.builder.seal_block(taken);
        self.jump(pc, target)?;
        let chain = self.chain(tested);
        self.chains.insert(not_taken, chain);
        self.body.builder.switch_to_block(not_taken);
        self.body.builder.seal_block(not_taken);
        self.fall_through(pc)
    }

    fn fall_through(&mut self, pc: u32) -> Option<()> {
        match self.target_block(Some(pc), pc + 1) {
            Some(block) => {
                self.body.builder.ins().jump(block, &[]);
            }
            None => self.leave(super::RegionExit::Resume(pc + 1)),
        }
        Some(())
    }

    /// The element index `key` names and the access it makes, exiting at `pc` unless `key`
    /// is an integral Number.
    fn element_index(&mut self, pc: u32, key: Typed) -> (ir::Value, ElementIndex) {
        match key.representation {
            // Every `i32` sum the region computes is exact (guarded or proved), so a key that
            // is a root plus constants is the root's sign extension plus their total, which
            // an access folds into its displacement. A negative key extends below zero,
            // which compares unsigned past every length (`MAX_INDEXED_LENGTH`).
            Representation::Int32 => {
                let (root, offset) = self.int32_root(key.value);
                let wide = self.body.builder.ins().sextend(types::I64, root);
                let index = self.body.builder.ins().iadd_imm_s(wide, offset);
                match i32::try_from(offset * i64::from(WORD_BYTES)) {
                    Ok(displacement) => (index, ElementIndex { wide, displacement }),
                    Err(_) => (index, ElementIndex { wide: index, displacement: 0 }),
                }
            }
            Representation::Float64 => {
                let index = self.body.builder.ins().fcvt_to_sint_sat(types::I64, key.value);
                let back = self.body.builder.ins().fcvt_from_sint(types::F64, index);
                let fractional = self.body.builder.ins().fcmp(FloatCC::NotEqual, back, key.value);
                self.guard(pc, fractional);
                (index, ElementIndex { wide: index, displacement: 0 })
            }
            Representation::Tagged => unreachable!("indexed operands are Number values"),
        }
    }

    /// Exits at `pc` unless `index` is below `array`'s length; an `i32` key a proof covers is.
    fn read_index(&mut self, pc: u32, array: ArrayView, index: ir::Value) {
        if self.proved(pc) {
            return;
        }
        let length = self.length(array);
        let beyond = self.body.builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, index, length);
        self.guard(pc, beyond);
    }

    /// `array`'s length as an `i64`.
    fn length(&mut self, array: ArrayView) -> ir::Value {
        match array.access {
            ArrayAccess::Read { length } => length,
            ArrayAccess::Written { .. } => {
                let length = self.body.builder.ins().load(types::I32, flags(), array.elements, crate::heap::LENGTH_FROM_VALUES);
                self.body.builder.ins().uextend(types::I64, length)
            }
        }
    }

    /// Makes `index` a slot `array`'s store at `pc` may write: one below its length, or its
    /// length itself while the capacity has room, which the store appends, advancing the
    /// backing's length. Any other index exits for the general path; an `i32` key a proof
    /// covers is below the length.
    fn store_index(&mut self, pc: u32, array: ArrayView, index: ir::Value) {
        if self.proved(pc) {
            return;
        }
        let ArrayAccess::Written { capacity } = array.access else {
            unreachable!("a region stores only into arrays it writes")
        };
        let length = self.length(array);
        let within = self.body.builder.ins().icmp(IntCC::UnsignedLessThan, index, length);
        let current = self.body.builder.current_block().expect("lowering inside a block");
        let chain = self.chain(current);
        let append = self.cold_block();
        let join = self.body.builder.create_block();
        self.body.builder.ins().brif(within, join, &[], append, &[]);
        self.body.builder.switch_to_block(append);
        self.body.builder.seal_block(append);
        let past = self.body.builder.ins().icmp(IntCC::NotEqual, index, length);
        self.guard(pc, past);
        let full = self.body.builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, length, capacity);
        self.guard(pc, full);
        let grown = self.body.builder.ins().iadd_imm_s(length, 1);
        let word = self.body.builder.ins().ireduce(types::I32, grown);
        self.body.builder.ins().store(flags(), word, array.elements, crate::heap::LENGTH_FROM_VALUES);
        self.body.builder.ins().jump(join, &[]);
        // The join's other predecessor is the append, which `current` dominates.
        self.chains.insert(join, chain);
        self.body.builder.switch_to_block(join);
        self.body.builder.seal_block(join);
    }

    /// The value an `i32` is a constant sum from, and that sum: the chain of constant
    /// additions ending at `value`.
    fn int32_root(&self, value: ir::Value) -> (ir::Value, i64) {
        let dfg = &self.body.builder.func.dfg;
        let (mut value, mut offset) = (dfg.resolve_aliases(value), 0i64);
        loop {
            let ir::ValueDef::Result(instruction, 0) = dfg.value_def(value) else { break };
            let ir::InstructionData::Binary { opcode: ir::Opcode::Iadd, args } = dfg.insts[instruction] else { break };
            let (left, right) = (dfg.resolve_aliases(args[0]), dfg.resolve_aliases(args[1]));
            match (self.int32_constant(left), self.int32_constant(right)) {
                (_, Some(constant)) => (value, offset) = (left, offset + constant),
                (Some(constant), None) => (value, offset) = (right, offset + constant),
                (None, None) => break,
            }
        }
        (value, offset)
    }

    /// The address of an element with the byte displacement its access carries.
    fn element_address(&mut self, elements: ir::Value, index: ElementIndex) -> (ir::Value, i32) {
        let offset = self.body.builder.ins().imul_imm_u(index.wide, i64::from(WORD_BYTES));
        let offset = if self.pointer == types::I64 {
            offset
        } else {
            self.body.builder.ins().ireduce(self.pointer, offset)
        };
        (self.body.builder.ins().iadd(elements, offset), index.displacement)
    }

    /// ToInt32 of `value`: its truncation modulo 2^32. A Float converts to `i64` saturating,
    /// whose low word is ToInt32 for NaN (zero) and for every truncation strictly inside
    /// `i64`; a result at either bound may have saturated (an infinity, or a magnitude from
    /// 2^63), so it exits at `pc` for the general path.
    fn int32(&mut self, pc: u32, value: Typed) -> ir::Value {
        if value.representation == Representation::Int32 {
            return value.value;
        }
        let integer = self.body.builder.ins().fcvt_to_sint_sat(types::I64, value.value);
        // `integer - (MIN + 1)` wraps both bounds, and only them, to the top two values.
        let shifted = self.body.builder.ins().iadd_imm_s(integer, -(i64::MIN + 1));
        let bound = self
            .body.builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, shifted, (u64::MAX - 1) as i64);
        self.guard(pc, bound);
        self.body.builder.ins().ireduce(types::I32, integer)
    }

    /// `left OP right` as `i32` arithmetic into `register`, exiting at `pc` when the exact
    /// result is no `i32` (an overflow, or the -0 of a zero product with a negative factor);
    /// a proof covers a sum's.
    fn int32_arithmetic(
        &mut self,
        pc: u32,
        operator: NumberArithmetic,
        register: Register,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        let current = self.body.builder.current_block().expect("lowering inside a block");
        let chain = self.chain(current);
        // A variable read where a join's parameter proved trivial is an alias of its value.
        let (left, right) = (self.body.builder.func.dfg.resolve_aliases(left), self.body.builder.func.dfg.resolve_aliases(right));
        let key = {
            let dfg = &self.body.builder.func.dfg;
            (operator, int32_key(dfg, left), int32_key(dfg, right))
        };
        if let Some((result, computed)) = self.exact.get(&key) {
            if *computed == chain {
                return *result;
            }
        }
        let result = self.int32_arithmetic_guarded(pc, operator, register, left, right);
        self.exact.insert(key, (result, chain));
        result
    }

    /// `left ± c` for a constant `c`, whose `i32` result overflows exactly when `left` lies
    /// beyond the bound `c` leaves: the guard compares `left` with that bound before the
    /// sum, which needs no overflow flag.
    fn int32_offset(
        &mut self,
        pc: u32,
        operator: NumberArithmetic,
        register: Register,
        left: ir::Value,
        right: ir::Value,
    ) -> Option<ir::Value> {
        let (value, offset) = match operator {
            NumberArithmetic::Add => match (self.int32_constant(left), self.int32_constant(right)) {
                (_, Some(offset)) => (left, offset),
                (Some(offset), None) => (right, offset),
                (None, None) => return None,
            },
            NumberArithmetic::Subtract => (left, -self.int32_constant(right)?),
            NumberArithmetic::Multiply | NumberArithmetic::Divide => return None,
        };
        let unguarded = self.proved(pc) || self.facts.exact.contains(&pc) || self.facts.truncated.contains(&pc);
        if !unguarded && offset != 0 {
            let (condition, bound) = if offset > 0 {
                (IntCC::SignedGreaterThan, i64::from(i32::MAX) - offset)
            } else {
                (IntCC::SignedLessThan, i64::from(i32::MIN) - offset)
            };
            let overflow = self.body.builder.ins().icmp_imm_s(condition, value, bound);
            self.guard_int32(pc, overflow, Widening::Register(register));
        }
        Some(self.body.builder.ins().iadd_imm_s(value, offset))
    }

    /// The value of an `i32` constant operand.
    fn int32_constant(&self, value: ir::Value) -> Option<i64> {
        let dfg = &self.body.builder.func.dfg;
        let ir::ValueDef::Result(instruction, _) = dfg.value_def(dfg.resolve_aliases(value)) else {
            return None;
        };
        match dfg.insts[instruction] {
            ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } => Some(i64::from(imm.bits() as i32)),
            _ => None,
        }
    }

    fn int32_arithmetic_guarded(
        &mut self,
        pc: u32,
        operator: NumberArithmetic,
        register: Register,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        if let Some(result) = self.int32_offset(pc, operator, register, left, right) {
            return result;
        }
        // A proof or the operands' ranges cover a result's exactness, so it needs no
        // overflow flag.
        let exact = self.proved(pc) || self.facts.exact.contains(&pc);
        let unguarded = exact || self.facts.truncated.contains(&pc);
        // `int32_offset` leaves the same sums unguarded.
        match operator {
            NumberArithmetic::Add if unguarded => {
                return self.body.builder.ins().iadd(left, right);
            }
            NumberArithmetic::Subtract if unguarded => {
                return self.body.builder.ins().isub(left, right);
            }
            _ => {}
        }
        let (result, overflow) = match operator {
            NumberArithmetic::Add => self.body.builder.ins().sadd_overflow(left, right),
            NumberArithmetic::Subtract => self.body.builder.ins().ssub_overflow(left, right),
            NumberArithmetic::Multiply if self.proved(pc) => {
                return self.body.builder.ins().imul(left, right);
            }
            NumberArithmetic::Multiply if exact && self.facts.signless_zero.contains(&pc) => {
                return self.body.builder.ins().imul(left, right);
            }
            NumberArithmetic::Multiply => {
                let product = if exact {
                    self.body.builder.ins().imul(left, right)
                } else {
                    let (product, overflow) = self.body.builder.ins().smul_overflow(left, right);
                    self.guard_int32(pc, overflow, Widening::Register(register));
                    product
                };
                if self.facts.signless_zero.contains(&pc) {
                    return product;
                }
                // A zero product is -0 when a factor is negative; the signs are tested only
                // for a zero product, off the common path.
                let zero = self.body.builder.ins().icmp_imm_u(IntCC::Equal, product, 0);
                let signs_block = self.cold_block();
                let done = self.body.builder.create_block();
                let tested = self.body.builder.current_block().expect("lowering inside a block");
                self.body.builder.ins().brif(zero, signs_block, &[], done, &[]);
                self.body.builder.switch_to_block(signs_block);
                self.body.builder.seal_block(signs_block);
                let signs = self.body.builder.ins().bor(left, right);
                let negative = self.body.builder.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                self.guard_int32(pc, negative, Widening::Register(register));
                self.body.builder.ins().jump(done, &[]);
                // `done`'s predecessors are the test's block and the signs' passing branch,
                // which that block dominates, so it continues the test's chain.
                let chain = self.chain(tested);
                self.chains.insert(done, chain);
                self.body.builder.switch_to_block(done);
                self.body.builder.seal_block(done);
                return product;
            }
            NumberArithmetic::Divide => unreachable!("division has a Float64 result"),
        };
        if !self.proved(pc) {
            self.guard_int32(pc, overflow, Widening::Register(register));
        }
        result
    }

    fn arithmetic(
        &mut self,
        pc: u32,
        operator: Arithmetic,
        result: Register,
        left: Typed,
        right: Typed,
    ) -> Typed {
        match operator {
            Arithmetic::Float(operator)
                if self.result_representation() == Representation::Int32
                    && left.representation == Representation::Int32
                    && right.representation == Representation::Int32 =>
            {
                let value = self.int32_arithmetic(pc, operator, result, left.value, right.value);
                Typed { value, representation: Representation::Int32 }
            }
            Arithmetic::Float(operator) => {
                let (left, right) = (self.float(left), self.float(right));
                let value = super::emit_number_arithmetic_operation(
                    &mut self.body.builder,
                    operator,
                    left,
                    right,
                );
                Typed { value, representation: Representation::Float64 }
            }
            Arithmetic::Int32(operator) => {
                let left = self.int32(pc, left);
                let right = self.int32(pc, right);
                let bits = super::emit_number_bitwise_bits(
                    &mut self.body.builder,
                    operator,
                    left,
                    right,
                );
                if operator == NumberBitwise::ShiftRightUnsigned {
                    let value = self.body.builder.ins().fcvt_from_uint(types::F64, bits);
                    return Typed { value, representation: Representation::Float64 };
                }
                let value = bits;
                Typed { value, representation: Representation::Int32 }
            }
        }
    }

    fn instruction(&mut self, pc: u32) -> Option<()> {
        let instruction = self.region.instructions[(pc - self.region.header) as usize];
        match instruction.op() {
            Op::LoadConst => {
                let index = u16::try_from(instruction.constant_index()).ok()?;
                let source = if self.region.tagged.contains(&instruction.result_register()) {
                    Source::Tagged(*self.region.tagged_constants.get(&index)?)
                } else {
                    Source::Number(*self.region.constants.get(&index)?)
                };
                let value = self.read(source);
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::Move | Op::ToNumeric => {
                let value = self.register(instruction.register_b());
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::LoadCapture => {
                let slot = instruction.capture_slot();
                let index = self.region.captures.iter().position(|capture| *capture == slot)?;
                let value = self.captures[index];
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let operator = arithmetic(instruction.op(), instruction.binary_operator())?;
                let left = self.operand(instruction.operand_b())?;
                let right = self.operand(instruction.operand_c())?;
                let result = instruction.result_register();
                let unit = |operand: Operand| operand.register_index().is_some_and(|register| self.units.contains(&register));
                let value = match operator {
                    // `x * 1`, `1 * x` and `x / 1` are `x` (`Lowering::units`).
                    Arithmetic::Float(NumberArithmetic::Multiply)
                        if unit(instruction.operand_c()) =>
                    {
                        left
                    }
                    Arithmetic::Float(NumberArithmetic::Multiply)
                        if unit(instruction.operand_b()) =>
                    {
                        right
                    }
                    Arithmetic::Float(NumberArithmetic::Divide)
                        if unit(instruction.operand_c()) =>
                    {
                        left
                    }
                    _ => self.arithmetic(pc, operator, result, left, right),
                };
                self.define(result, value);
                self.fall_through(pc)
            }
            Op::IncDec => {
                let operand = self.register(instruction.register_b());
                let decrement = instruction.boolean_flag()?;
                let result = instruction.result_register();
                let value = if operand.representation == Representation::Int32
                    && self.result_representation() == Representation::Int32
                {
                    // Only the extreme the step leaves `i32` from overflows.
                    let (limit, step) = if decrement { (i32::MIN, -1) } else { (i32::MAX, 1) };
                    if !self.proved(pc) {
                        let overflow =
                            self.body.builder.ins().icmp_imm_s(IntCC::Equal, operand.value, i64::from(limit));
                        self.guard_int32(pc, overflow, Widening::Register(result));
                    }
                    let value = self.body.builder.ins().iadd_imm_s(operand.value, step);
                    Typed { value, representation: Representation::Int32 }
                } else {
                    let operator = if decrement {
                        NumberArithmetic::Subtract
                    } else {
                        NumberArithmetic::Add
                    };
                    let one = self.read(Source::Number(1.0));
                    self.arithmetic(pc, Arithmetic::Float(operator), result, operand, one)
                };
                self.define(result, value);
                self.fall_through(pc)
            }
            Op::Unary => {
                let operand = self.register(instruction.register_b());
                let result = instruction.result_register();
                let value = match unary(instruction.op(), instruction.unary_operator())? {
                    Unary::Number(NumberUnary::Plus) => operand,
                    Unary::Number(NumberUnary::Negate)
                        if operand.representation == Representation::Int32
                            && self.result_representation() == Representation::Int32 =>
                    {
                        // -0 and -(2^31) are no `i32`.
                        let zero = self.body.builder.ins().icmp_imm_u(IntCC::Equal, operand.value, 0);
                        let minimum =
                            self.body.builder.ins().icmp_imm_s(IntCC::Equal, operand.value, i64::from(i32::MIN));
                        let failed = self.body.builder.ins().bor(zero, minimum);
                        self.guard_int32(pc, failed, Widening::Register(result));
                        let value = self.body.builder.ins().ineg(operand.value);
                        Typed { value, representation: Representation::Int32 }
                    }
                    Unary::Number(NumberUnary::Negate) => {
                        let number = self.float(operand);
                        let value = super::emit_number_unary_operation(
                            &mut self.body.builder,
                            NumberUnary::Negate,
                            number,
                        );
                        Typed { value, representation: Representation::Float64 }
                    }
                    Unary::BitwiseNot => {
                        let integer = self.int32(pc, operand);
                        let value = self.body.builder.ins().bnot(integer);
                        Typed { value, representation: Representation::Int32 }
                    }
                };
                self.define(result, value);
                self.fall_through(pc)
            }
            Op::RequireObjectCoercible => self.fall_through(pc),
            Op::GetField => {
                let field = *self.region.field_reads.get(&(pc - self.region.header))?;
                let value = self.fields[field];
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::GetIndex => {
                let base = instruction.operand_b().register_index()?;
                let array = self.arrays[&base];
                let key = self.operand(instruction.operand_c())?;
                let representation = element_representation(&instruction, self.observation);
                let current = self.body.builder.current_block().expect("lowering inside a block");
                let chain = self.chain(current);
                let read = (base, self.body.builder.func.dfg.resolve_aliases(key.value), representation);
                if let Some((value, computed, stores)) = self.element_reads.get(&read).copied()
                    && computed == chain
                    && stores == self.element_stores
                {
                    self.define(instruction.result_register(), value);
                    return self.fall_through(pc);
                }
                let (index, element) = self.element_index(pc, key);
                self.read_index(pc, array, index);
                let (address, displacement) = self.element_address(array.elements, element);
                // A hole is tagged, so unboxing sends it to the general path too. A Float64
                // element loads straight into a float register.
                let value = match representation {
                    Representation::Int32 => {
                        let bits = self.body.builder.ins().load(types::I64, flags(), address, displacement);
                        let widen = self.widen_block(pc, Widening::Elements(base));
                        self.unbox(bits, representation, widen)
                    }
                    Representation::Float64 => {
                        let double = self.body.builder.ins().load(types::F64, flags(), address, displacement);
                        let exit = self.exit_block(pc);
                        self.unbox_float64(double, exit)
                    }
                    Representation::Tagged => return None,
                };
                self.element_reads.insert(read, (Typed { value, representation }, chain, self.element_stores));
                self.define(instruction.result_register(), Typed { value, representation });
                self.fall_through(pc)
            }
            Op::SetIndex => {
                self.element_stores += 1;
                let array = self.arrays[&instruction.register_b()];
                let key = self.register(instruction.register_c());
                let (index, element) = self.element_index(pc, key);
                self.store_index(pc, array, index);
                let (address, displacement) = self.element_address(array.elements, element);
                // A hole takes the value as an own data element: the entry found no prototype
                // that holds an index (`LoopRegion::arrays`).
                let value = self.register(instruction.register_a());
                let bits = self.encoding(value);
                self.body.builder.ins().store(flags(), bits, address, displacement);
                self.fall_through(pc)
            }
            Op::SetField | Op::SetFieldStrict => {
                let index = *self.region.field_stores.get(&(pc - self.region.header))?;
                let pointer = self.field_store_pointer(index);
                let value = self.register(instruction.register_a());
                let bits = self.boxed(value);
                self.body.builder.ins().store(flags(), bits, pointer, 0);
                self.fall_through(pc)
            }
            Op::Jump => self.jump(pc, instruction.jump_target()),
            Op::JumpBinaryFalse => {
                if let Some(branch) = self.region.identity_branches.get(&pc).copied() {
                    let value = self.register(branch.register);
                    if value.representation != Representation::Tagged {
                        return None;
                    }
                    let identity = self.body.builder.ins().icmp_imm_u(
                        if branch.negated { IntCC::NotEqual } else { IntCC::Equal },
                        value.value,
                        branch.constant.to_bits() as i64,
                    );
                    return self.branch(pc, instruction.jump_target(), identity, false);
                }
                let operator = instruction.binary_operator_field();
                let left = self.operand(instruction.operand_b())?;
                let right = self.operand(instruction.operand_c())?;
                let (left, right, domain) = if left.representation == Representation::Int32
                    && right.representation == Representation::Int32
                {
                    (
                        left.value,
                        right.value,
                        super::NumberRelationDomain::Int32,
                    )
                } else {
                    let (left, right) = (self.float(left), self.float(right));
                    (
                        left,
                        right,
                        super::NumberRelationDomain::Float64,
                    )
                };
                let holds = super::emit_number_relation(
                    &mut self.body.builder,
                    operator,
                    left,
                    right,
                    domain,
                )?;
                self.branch(pc, instruction.jump_target(), holds, false)
            }
            Op::JumpFalse | Op::JumpTrue => {
                let value = self.read(Source::Register(instruction.register_a()));
                let truthy = match value.representation {
                    Representation::Int32 => {
                        self.body
                            .builder
                            .ins()
                            .icmp_imm_s(IntCC::NotEqual, value.value, 0)
                    }
                    Representation::Float64 => {
                        super::emit_number_truthiness(&mut self.body.builder, value.value)
                    }
                    Representation::Tagged => return None,
                };
                let jump_when = instruction.op() == Op::JumpTrue;
                self.branch(pc, instruction.jump_target(), truthy, jump_when)
            }
            _ => None,
        }
    }
}

/// The SSA value an `i32` operand is a pure function of, so that two reads of one register
/// definition key the same arithmetic: each read of an `i32` view narrows its stored
/// zero-extended value anew (`Body::read`), giving distinct `ireduce`s of one value, and a
/// view written in the same block chain reads back as `ireduce(uextend(x))`, which is `x`.
fn int32_key(dfg: &ir::DataFlowGraph, value: ir::Value) -> ir::Value {
    let operand = |value: ir::Value, opcode: ir::Opcode| match dfg.value_def(value) {
        ir::ValueDef::Result(instruction, 0) => match dfg.insts[instruction] {
            ir::InstructionData::Unary { opcode: found, arg } if found == opcode => Some(dfg.resolve_aliases(arg)),
            _ => None,
        },
        _ => None,
    };
    let value = dfg.resolve_aliases(value);
    let Some(wide) = operand(value, ir::Opcode::Ireduce) else { return value };
    operand(wide, ir::Opcode::Uextend)
        .filter(|narrow| dfg.value_type(*narrow) == types::I32)
        .unwrap_or(wide)
}
