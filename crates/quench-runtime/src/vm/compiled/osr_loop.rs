//! The loop tier: Cranelift code for a hot loop whose body makes no call and allocates
//! nothing, entered at its backward jump in compiled execution.
//!
//! A region is a loop's instructions, header to backward jump, every one of which reads and
//! writes Numbers in registers, depth-0 captures and present dense elements of record arrays.
//! Native code keeps each Number register in an SSA value: an `i32` where every value it
//! takes is one (`representations`), else an `f64`. Registers are the
//! interpreter's state at every instruction boundary the code leaves at, so leaving is
//! writing them back:
//!
//! - entry guards every register the region uses: a Number, or a record array the region
//!   reads (and, for one it writes, one whose elements a write may replace in place);
//!   otherwise nothing runs and the interpreter takes the jump;
//! - a jump out of the region resumes the lane at its target;
//! - an instruction whose operands leave the subset (a hole, a non-Number element, an index
//!   past the length, an integer conversion out of range) exits before its effects, and the
//!   general path executes it;
//! - every backward jump polls host termination, exiting at the jump so the interpreter's
//!   safepoint runs.
//!
//! Nothing in a region allocates, collects, or runs guest code, so the arrays, their
//! element storage and the captures the entry read stay as they are while native code runs.

use crate::bytecode::{FieldBase, FieldLookup, Function, Op, Operand, OperandKind, Register, WideInstruction};
use crate::value::{Value, ValueLayout};
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
/// The largest native body a region may produce.
pub(super) const MAX_CODE_BYTES: usize = 64 * 1024;

/// What native code writes to its result word: the outcome in the high word, a bytecode PC
/// in the low one.
const OUTCOME_SHIFT: u32 = u32::BITS;
const PC_MASK: u64 = u32::MAX as u64;

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
    /// The elements the region reads from the array in this register.
    Elements(Register),
    /// A register the region never writes, held as exactly 1 so that multiplying or
    /// dividing by it is the identity (`Lowering::units`), held something else at entry.
    Unit(Register),
}

const OUTCOME_NOT_ENTERED: u64 = 0;
const OUTCOME_RESUME: u64 = 1;
const OUTCOME_EXIT: u64 = 2;
const OUTCOME_WIDEN: u64 = 3;
/// A not-entered outcome's low word: zero, or a widening's kind bit over its payload.
const WIDEN_REGISTER: u64 = 1 << u16::BITS;
const WIDEN_CAPTURE: u64 = WIDEN_REGISTER << 1;
const WIDEN_UNIT: u64 = WIDEN_CAPTURE << 1;
const _: () = assert!(WIDEN_UNIT <= PC_MASK);
const WIDEN_PAYLOAD: u64 = WIDEN_REGISTER - 1;

impl Widening {
    /// The not-entered word of an entry guard's widening; element reads are guarded where
    /// they run, so an entry never names them.
    fn code(self) -> u64 {
        match self {
            Self::Register(register) => WIDEN_REGISTER | u64::from(register),
            Self::Capture(index) => WIDEN_CAPTURE | u64::from(index),
            Self::Unit(register) => WIDEN_UNIT | u64::from(register),
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
        } else {
            None
        }
    }
}

impl LoopOutcome {
    pub(super) fn decode(word: u64) -> Self {
        let low = word & PC_MASK;
        match word >> OUTCOME_SHIFT {
            OUTCOME_RESUME => Self::Resume(low as u32),
            OUTCOME_EXIT => Self::Exit(low as u32),
            OUTCOME_WIDEN => Self::Widen(low as u32),
            _ => Self::NotEntered(Widening::decode(low)),
        }
    }
}

/// An array a region indexes, by its register; `written` when the region stores into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrayUse {
    pub(crate) register: Register,
    pub(crate) written: bool,
}

/// A loop's admissible region and the facts its entry needs.
pub(crate) struct LoopRegion {
    pub(crate) header: u32,
    pub(crate) backedge: u32,
    /// The Number registers; a register's index here names its entry definition (`Webs`).
    numbers: Vec<Register>,
    /// The arrays, in entry-record order.
    pub(crate) arrays: Vec<ArrayUse>,
    /// The depth-0 capture slots `LoadCapture` reads, in entry-record order.
    pub(crate) captures: Vec<u16>,
    /// Numeric cached data fields, in entry-record order.
    pub(crate) fields: Vec<FieldUse>,
    /// The region's instructions, from the header.
    instructions: Vec<WideInstruction>,
    /// The Number constants the region reads, by constant index.
    constants: std::collections::BTreeMap<u16, f64>,
    /// The function's live registers before each instruction (`compile::liveness`), when
    /// liveness covers it.
    live: Option<Box<[u64]>>,
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
    FieldMutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldReceiver {
    This,
    Register(Register),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldUse {
    pub(crate) receiver: FieldReceiver,
    pub(crate) atom: crate::bytecode::Atom,
    pub(crate) cache_site: u16,
}

impl LoopRegion {
    /// Whether the interpreter may read `register` resuming at `pc` before writing it; every
    /// register is, where liveness does not cover the function.
    fn is_live(&self, pc: u32, register: Register) -> bool {
        let Some(live) = &self.live else {
            return true;
        };
        let bit = u32::from(register);
        bit >= u64::BITS || live.get(pc as usize).is_none_or(|mask| mask & (1 << bit) != 0)
    }
}

/// The entry record native code reads: each capture's value, then each array's element
/// pointer and length.
pub(crate) struct LoopEntry {
    words: [u64; ENTRY_WORDS],
    length: usize,
}

/// The longest array a region indexes: a negative `i32` index zero-extends past it, so one
/// unsigned comparison bounds every `i32` index.
const MAX_INDEXED_LENGTH: usize = i32::MAX as usize;
const ENTRY_CAPTURES: usize = 0;
const WORD_BYTES: i32 = 8;
/// An array's two entry words: its element pointer, then its length.
const ARRAY_WORDS: usize = 2;
const ENTRY_WORDS: usize =
    ENTRY_CAPTURES + MAX_REGION_CAPTURES + ARRAY_WORDS * MAX_REGION_ARRAYS + MAX_REGION_FIELDS;

impl LoopEntry {
    pub(crate) fn new() -> Self {
        Self { words: [0; ENTRY_WORDS], length: ENTRY_CAPTURES }
    }

    /// The record's words, as native code reads them.
    pub(super) fn words(&self) -> *const u64 {
        debug_assert!(self.length <= ENTRY_WORDS);
        self.words.as_ptr()
    }

    fn push(&mut self, word: u64) {
        self.words[self.length] = word;
        self.length += 1;
    }

    /// Captures come first, in the region's order.
    pub(crate) fn push_capture(&mut self, value: Value) {
        self.push(value.to_bits());
    }

    /// The first `count` capture values, as pushed.
    pub(crate) fn captures(&self, count: usize) -> Vec<Value> {
        self.words[ENTRY_CAPTURES..ENTRY_CAPTURES + count].iter().map(|bits| Value::from_bits(*bits)).collect()
    }

    /// Arrays follow the captures, in the region's order. An array longer than
    /// `MAX_INDEXED_LENGTH` declines the entry.
    pub(crate) fn push_array(&mut self, elements: *mut Value, length: usize) -> Option<()> {
        (length <= MAX_INDEXED_LENGTH).then_some(())?;
        self.push(elements.addr() as u64);
        self.push(length as u64);
        Some(())
    }

    pub(crate) fn push_field(&mut self, value: f64) {
        self.push(value.to_bits());
    }
}

/// How an instruction of the region names a value it reads.
#[derive(Clone, Copy)]
enum Source {
    Register(Register),
    Number(f64),
}

/// The arithmetic and comparison a region lowers, by the operator's tag.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arithmetic {
    Float(FloatOperator),
    Int32(Int32Operator),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FloatOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Int32Operator {
    Or,
    And,
    Xor,
    ShiftLeft,
    ShiftRight,
    ShiftRightUnsigned,
}

fn arithmetic(instruction: Op, operator: u32) -> Option<Arithmetic> {
    if let Some(operation) = super::super::operations::NumberArithmetic::from_js_instruction(
        instruction,
        operator,
    ) {
        return Some(Arithmetic::Float(match operation {
            super::super::operations::NumberArithmetic::Add => FloatOperator::Add,
            super::super::operations::NumberArithmetic::Subtract => FloatOperator::Subtract,
            super::super::operations::NumberArithmetic::Multiply => FloatOperator::Multiply,
            super::super::operations::NumberArithmetic::Divide => FloatOperator::Divide,
        }));
    }
    let operation = super::super::operations::NumberBitwise::from_js_instruction(
        instruction,
        operator,
    )?;
    Some(Arithmetic::Int32(match operation {
        super::super::operations::NumberBitwise::Or => Int32Operator::Or,
        super::super::operations::NumberBitwise::And => Int32Operator::And,
        super::super::operations::NumberBitwise::Xor => Int32Operator::Xor,
        super::super::operations::NumberBitwise::ShiftLeft => Int32Operator::ShiftLeft,
        super::super::operations::NumberBitwise::ShiftRight => Int32Operator::ShiftRight,
        super::super::operations::NumberBitwise::ShiftRightUnsigned => {
            Int32Operator::ShiftRightUnsigned
        }
    }))
}

/// A unary operator a region lowers on a Number.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unary {
    Plus,
    Negate,
    BitwiseNot,
}

fn unary(operator: u32) -> Option<Unary> {
    use UnaryOperator as U;
    Some(match operator {
        op if op == U::UnaryPlus as u32 => Unary::Plus,
        op if op == U::UnaryNegation as u32 => Unary::Negate,
        op if op == U::BitwiseNot as u32 => Unary::BitwiseNot,
        _ => return None,
    })
}

/// What each register of a region holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Number,
    Array { written: bool },
}

struct Admission<'a> {
    constants: &'a [Value],
    roles: std::collections::BTreeMap<Register, Role>,
    captures: Vec<u16>,
    pub(crate) fields: Vec<FieldUse>,
    numbers: std::collections::BTreeMap<u16, f64>,
    writes_indexed: bool,
}

impl Admission<'_> {
    fn note(&mut self, register: Register, role: Role) -> Option<()> {
        match (self.roles.get(&register).copied(), role) {
            (None, role) => {
                self.roles.insert(register, role);
            }
            (Some(Role::Number), Role::Number) => {}
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

    fn constant(&mut self, index: u16) -> Option<f64> {
        let number = self.constants.get(usize::from(index))?.as_number()?;
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

/// The region of the loop from `header` to its backward jump at `backedge`, when every
/// instruction in it is one the tier lowers.
pub(super) fn region(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    live: Option<&[u64]>,
    header: u32,
    backedge: u32,
) -> Result<LoopRegion, RegionRejection> {
    let (start, end) = (header as usize, backedge as usize);
    if start > end || end >= decoded.len() || end - start >= MAX_REGION_INSTRUCTIONS {
        return Err(RegionRejection::RegionTooLarge);
    }
    let mut admission = Admission {
        constants,
        roles: std::collections::BTreeMap::new(),
        captures: Vec::new(),
        fields: Vec::new(),
        numbers: std::collections::BTreeMap::new(),
        writes_indexed: false,
    };
    for instruction in &decoded[start..=end] {
        admit(&mut admission, *instruction)
            .ok_or(RegionRejection::UnsupportedInstruction(instruction.op()))?;
    }
    if admission.fields.len() > MAX_REGION_FIELDS {
        return Err(RegionRejection::FieldLimit);
    }
    if !admission.fields.is_empty()
        && (admission.writes_indexed
            || admission.fields.iter().any(|field| match field.receiver {
                FieldReceiver::This => false,
                FieldReceiver::Register(register) => decoded[start..=end]
                    .iter()
                    .any(|instruction| number_definition(*instruction) == Some(register)),
            }))
    {
        return Err(RegionRejection::FieldMutation);
    }
    let registers = usize::from(function.registers);
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
    let arrays: Vec<ArrayUse> = admission
        .roles
        .iter()
        .filter_map(|(register, role)| match role {
            Role::Array { written } => Some(ArrayUse { register: *register, written: *written }),
            Role::Number => None,
        })
        .collect();
    if arrays.len() > MAX_REGION_ARRAYS {
        return Err(RegionRejection::ArrayLimit);
    }
    Ok(LoopRegion {
        header,
        backedge,
        numbers,
        arrays,
        captures: admission.captures,
        fields: admission.fields,
        instructions: decoded[start..=end].to_vec(),
        constants: admission.numbers,
        live: live.map(Box::from),
    })
}

fn admit(admission: &mut Admission<'_>, instruction: WideInstruction) -> Option<()> {
    match instruction.op() {
        Op::LoadConst => {
            admission.constant(u16::try_from(instruction.constant_index()).ok()?)?;
            admission.result(instruction)?;
        }
        Op::Move => {
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
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
            unary(instruction.unary_operator())?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        // An array the entry guard proved is an object passes.
        Op::RequireObjectCoercible => {
            admission.note(instruction.register_b(), Role::Array { written: false })?;
        }
        Op::GetIndex => {
            admission.result(instruction)?;
            let base = instruction.operand_b().register_index()?;
            admission.note(base, Role::Array { written: false })?;
            admission.source(instruction.operand_c())?;
        }
        Op::GetField => {
            let FieldLookup::Atom { atom, base, cache_site } = instruction.field_lookup()? else {
                return None;
            };
            let receiver = if base == FieldBase::THIS {
                FieldReceiver::This
            } else {
                FieldReceiver::Register(base.register_index()?)
            };
            let field = FieldUse { receiver, atom, cache_site };
            if !admission.fields.contains(&field) {
                admission.fields.push(field);
            }
            admission.result(instruction)?;
        }
        Op::SetIndex => {
            admission.writes_indexed = true;
            admission.number(instruction.register_a())?;
            admission.note(instruction.register_b(), Role::Array { written: true })?;
            admission.number(instruction.register_c())?;
        }
        Op::Jump => {}
        Op::JumpBinaryFalse => {
            super::super::operations::NumberRelation::from_operator(
                instruction.binary_operator_field(),
            )?;
            admission.source(instruction.operand_b())?;
            admission.source(instruction.operand_c())?;
        }
        _ => return None,
    }
    Some(())
}

/// How native code holds a Number register or capture: as an `i32` while every value it
/// takes is one, else as an `f64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Representation {
    Int32,
    Float64,
}

/// The values a compilation starts from: what the region's registers and captures held when
/// it compiled, and what earlier compilations found was no `i32` (`Widening`).
pub(crate) struct Observation<'a> {
    pub(crate) register: &'a dyn Fn(Register) -> Value,
    /// Whether the array in a register looks to hold `i32` elements.
    pub(crate) int32_elements: &'a dyn Fn(Register) -> bool,
    pub(crate) captures: &'a [Value],
    pub(crate) widened: &'a std::collections::BTreeSet<Widening>,
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
        let entry = || region.numbers.iter().position(|number| *number == register).unwrap_or(0);
        let definition = self.reaching[(pc - region.header) as usize]
            .get(&register)
            .and_then(|definitions| definitions.first().copied())
            .unwrap_or_else(entry);
        self.find(definition)
    }

    /// The register a web's definitions write: every definition reaching a common read
    /// writes the register read.
    fn register(&self, region: &LoopRegion, web: Definition) -> Register {
        match region.numbers.get(web) {
            Some(register) => *register,
            None => number_definition(region.instructions[web - region.numbers.len()])
                .expect("a web's definitions write Number registers"),
        }
    }

    /// The web of the instruction at `pc`'s result.
    fn defined(&self, region: &LoopRegion, pc: u32) -> Definition {
        self.find(region.numbers.len() + (pc - region.header) as usize)
    }
}

/// The Number registers an instruction reads.
fn number_reads(instruction: WideInstruction) -> Vec<Register> {
    let register = |operand: Operand| operand.register_index();
    match instruction.op() {
        Op::Move | Op::IncDec | Op::Unary | Op::ToNumeric => vec![instruction.register_b()],
        Op::Binary | Op::NumericAdd | Op::NumericMultiply | Op::JumpBinaryFalse => {
            [register(instruction.operand_b()), register(instruction.operand_c())].into_iter().flatten().collect()
        }
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
    match instruction.op() {
        Op::Jump => vec![instruction.jump_target()],
        Op::JumpBinaryFalse => vec![pc + 1, instruction.jump_target()],
        _ => vec![pc + 1],
    }
}

fn webs(region: &LoopRegion) -> Webs {
    use std::collections::{BTreeMap, BTreeSet};
    let count = region.instructions.len();
    let entries = region.numbers.len();
    let inside = |pc: u32| (region.header..=region.backedge).contains(&pc);
    let mut reaching: Vec<BTreeMap<Register, BTreeSet<Definition>>> = vec![BTreeMap::new(); count];
    for (index, register) in region.numbers.iter().enumerate() {
        reaching[0].entry(*register).or_default().insert(index);
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
            for register in region.numbers.iter().copied().filter(|register| region.is_live(at, *register)) {
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

/// Each web's and capture's representation: an `i32` where every definition in the web
/// yields one (an entry value observed as one, an instruction's result from `i32` operands)
/// and no widening names its register; the least fixpoint from that start, widening a web
/// any definition gives an `f64`. Indexed by definition, meaningful at each web's root.
fn representations(
    region: &LoopRegion,
    webs: &Webs,
    observation: &Observation<'_>,
) -> (Vec<Representation>, Vec<Representation>) {
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
    let mut webs_representation = vec![Representation::Int32; webs.parent.len()];
    for (entry, register) in region.numbers.iter().enumerate() {
        if webs.entry_read[entry]
            && of((observation.register)(*register), Widening::Register(*register)) == Representation::Float64
        {
            webs_representation[webs.find(entry)] = Representation::Float64;
        }
    }
    loop {
        let mut changed = false;
        for (offset, instruction) in region.instructions.iter().enumerate() {
            let pc = region.header + offset as u32;
            let read = |register: Register| webs_representation[webs.read(region, pc, register)];
            let Some((register, representation)) =
                definition(region, instruction, &read, &captures, observation)
            else {
                continue;
            };
            let representation = if observation.widened.contains(&Widening::Register(register)) {
                Representation::Float64
            } else {
                representation
            };
            let web = webs.defined(region, pc);
            if representation == Representation::Float64 && webs_representation[web] == Representation::Int32 {
                webs_representation[web] = Representation::Float64;
                changed = true;
            }
        }
        if !changed {
            return (webs_representation, captures);
        }
    }
}

/// The register an instruction defines and the representation its result has.
fn definition(
    region: &LoopRegion,
    instruction: &WideInstruction,
    registers: &dyn Fn(Register) -> Representation,
    captures: &[Representation],
    observation: &Observation<'_>,
) -> Option<(Register, Representation)> {
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
            captures[index]
        }
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
            match arithmetic(instruction.op(), instruction.binary_operator())? {
                Arithmetic::Float(FloatOperator::Divide) => Representation::Float64,
                Arithmetic::Float(_) => {
                    both(source(instruction.operand_b()), source(instruction.operand_c()))
                }
                Arithmetic::Int32(Int32Operator::ShiftRightUnsigned) => Representation::Float64,
                Arithmetic::Int32(_) => Representation::Int32,
            }
        }
        Op::Unary => match unary(instruction.unary_operator())? {
            Unary::Plus | Unary::Negate => registers(instruction.register_b()),
            Unary::BitwiseNot => Representation::Int32,
        },
        Op::GetIndex => element_representation(instruction, observation),
        Op::GetField => Representation::Float64,
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
fn counted_loops(
    region: &LoopRegion,
    webs: &Webs,
    representations: &[Representation],
    captures: &[Representation],
) -> Vec<CountedLoop> {
    (region.header..=region.backedge)
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
            _ => {}
        }
    }
    let test = test?;
    // Control enters only at the header.
    let enters_inside = (region.header..=region.backedge).any(|pc| {
        let instruction = at(pc);
        matches!(instruction.op(), Op::Jump | Op::JumpBinaryFalse)
            && (header + 1..=backedge).contains(&instruction.jump_target())
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
                    (Arithmetic::Float(FloatOperator::Add), Some(left), Some(right)) => match (left, right) {
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
                    (Arithmetic::Float(FloatOperator::Subtract), Some(left), Some(right))
                        if right.base == Base::Constant && right.addend.is_none() =>
                    {
                        left.plus(-right.offset)
                    }
                    (Arithmetic::Float(FloatOperator::Multiply), Some(left), Some(right))
                        if held && factor(&left) && factor(&right) =>
                    {
                        terms.push(Term { multiply: true, left, right });
                        let value = Affine::of(Base::Term(terms.len() - 1));
                        candidates.push(Fact { pc, value, bound: Bound::Product });
                        Some(value)
                    }
                    _ => None,
                };
                if operator != Arithmetic::Float(FloatOperator::Multiply)
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
) -> Option<(ir::Function, std::collections::BTreeMap<u32, Widening>)> {
    let pointer = isa.pointer_type();
    let signature = super::native_entry_signature(pointer, isa.default_call_conv());
    let mut native = ir::Function::with_name_signature(
        UserFuncName::user(LOOP_NAMESPACE, region.header),
        signature,
    );
    let webs = webs(region);
    let (representations, captures) = representations(region, &webs, observation);
    let counted = counted_loops(region, &webs, &representations, &captures);
    let mut context = FunctionBuilderContext::new();
    let builder = FunctionBuilder::new(&mut native, &mut context);
    let widenings = Lowering::new(builder, region, observation, pointer, webs, representations, captures, counted, poll)
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
    capture_representations: Vec<Representation>,
    blocks: Vec<ir::Block>,
    /// The region's counted loops; each has a preheader block, which every arrival at its
    /// header from outside it takes, and a proved copy of its body's blocks.
    counted: Vec<CountedLoop>,
    preheaders: Vec<ir::Block>,
    proved_blocks: Vec<Vec<ir::Block>>,
    /// The counted loop whose proved copy is being lowered.
    copy: Option<usize>,
    /// Per PC the code leaves at, the block that writes back the Number registers live
    /// there and returns its parameter, the outcome word.
    leaves: std::collections::BTreeMap<u32, ir::Block>,
    /// The cold blocks that end the run with a fixed outcome (`outcome_block`).
    outcomes: Vec<(ir::Block, u64, u64)>,
    registers: ir::Value,
    entry: ir::Value,
    /// The result word the outcome is written to.
    result: ir::Value,
    /// Each array's element pointer and length, by register.
    arrays: std::collections::BTreeMap<Register, (ir::Value, ir::Value)>,
    widenings: std::collections::BTreeMap<u32, Widening>,
    /// Per block, the first block of the chain it continues: each block after the first
    /// has its predecessors inside the chain (a guard's passing branch, the join of a test
    /// whose other arm returns into the chain), so every chain block dominates the blocks
    /// lowered after it in the chain.
    chains: std::collections::HashMap<ir::Block, ir::Block>,
    /// The exact `i32` results computed so far, by operator and operands, with the chain
    /// computing them: the same arithmetic later in that chain is that result, its guards
    /// having passed.
    exact: std::collections::BTreeMap<(FloatOperator, ir::Value, ir::Value), (ir::Value, ir::Block)>,
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
    for pc in region.header..=region.backedge {
        for successor in successors(region, pc) {
            if (region.header..=region.backedge).contains(&successor) {
                predecessors[(successor - region.header) as usize].push(pc);
            }
        }
    }
    (region.header..=region.backedge)
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
        capture_representations: Vec<Representation>,
        counted: Vec<CountedLoop>,
        poll: Option<i64>,
    ) -> Self {
        let start = builder.create_block();
        builder.append_block_params_for_function_params(start);
        builder.switch_to_block(start);
        let &[context, registers, _, entry, result] = builder.block_params(start) else {
            unreachable!("the entry signature has five parameters")
        };
        let blocks = (region.header..=region.backedge).map(|_| builder.create_block()).collect();
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
            capture_representations,
            blocks,
            counted,
            preheaders,
            proved_blocks,
            copy: None,
            leaves: std::collections::BTreeMap::new(),
            outcomes: Vec::new(),
            chains: std::collections::HashMap::new(),
            exact: std::collections::BTreeMap::new(),
            encodings: std::collections::HashMap::new(),
            straight,
            registers,
            entry,
            result,
            arrays: std::collections::BTreeMap::new(),
            widenings: std::collections::BTreeMap::new(),
        }
    }

    fn lower(
        mut self,
        frontend: cranelift_codegen::isa::TargetFrontendConfig,
    ) -> Option<std::collections::BTreeMap<u32, Widening>> {
        self.enter()?;
        for pc in self.region.header..=self.region.backedge {
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
        self.body.builder.finalize(frontend);
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
        (self.region.header..=self.region.backedge)
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
                    let length = self.arrays[&array].1;
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

    fn outcome(&mut self, outcome: u64, low: u64) -> ir::Value {
        self.body.builder
            .ins()
            .iconst(types::I64, ((outcome << OUTCOME_SHIFT) | low) as i64)
    }

    fn word(&mut self, index: usize) -> ir::Value {
        self.body.builder
            .ins()
            .load(types::I64, flags(), self.entry, index as i32 * WORD_BYTES)
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
        self.outcome_block(OUTCOME_NOT_ENTERED, widening.map_or(0, Widening::code))
    }

    /// A cold block that ends the run with `outcome` over `low`, filled once the body is
    /// lowered (`fill_outcomes`): the builder leaves a block only once it is filled.
    fn outcome_block(&mut self, outcome: u64, low: u64) -> ir::Block {
        let block = self.cold_block();
        self.outcomes.push((block, outcome, low));
        block
    }

    /// Fills each `outcome_block`: not entered returns at once, and every other outcome
    /// leaves at its PC through the write-back.
    fn fill_outcomes(&mut self) {
        for (block, outcome, low) in std::mem::take(&mut self.outcomes) {
            self.body.builder.switch_to_block(block);
            if outcome == OUTCOME_NOT_ENTERED {
                let word = self.outcome(outcome, low);
                self.finish(word);
            } else {
                self.leave(outcome, low as u32);
            }
        }
    }

    /// Unboxes every Number register and capture in its representation, entering only when
    /// all hold such values.
    fn enter(&mut self) -> Option<()> {
        for (index, array) in self.region.arrays.iter().enumerate() {
            let base = ENTRY_CAPTURES + self.region.captures.len() + ARRAY_WORDS * index;
            let elements = self.word(base);
            let length = self.word(base + 1);
            self.arrays.insert(array.register, (elements, length));
        }
        for index in 0..self.region.captures.len() {
            let bits = self.word(ENTRY_CAPTURES + index);
            let representation = self.capture_representations[index];
            let otherwise = self.not_entered(Some(Widening::Capture(index as u16)));
            let value = self.unbox(bits, representation, otherwise);
            self.captures.push(Typed { value, representation });
        }
        let fields_start = ENTRY_CAPTURES
            + self.region.captures.len()
            + ARRAY_WORDS * self.region.arrays.len();
        for index in 0..self.region.fields.len() {
            let bits = self.word(fields_start + index);
            let value = self.body.builder.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
            self.fields.push(Typed { value, representation: Representation::Float64 });
        }
        // Only an entry value some read reaches is loaded and guarded.
        let registers: Vec<(Register, Definition)> = (0..self.region.numbers.len())
            .filter(|entry| self.webs.entry_read[*entry])
            .map(|entry| (self.region.numbers[entry], self.webs.find(entry)))
            .collect();
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
        }
    }

    /// Fills each leave block: the interpreter resumes at its PC, so only the registers live
    /// there (`LoopRegion::live`) take their native values; any other keeps the value it
    /// held, which nothing reads before writing.
    fn write_back(&mut self) {
        let leaves: Vec<(u32, ir::Block)> = self.leaves.iter().map(|(pc, block)| (*pc, *block)).collect();
        for (pc, block) in leaves {
            self.body.builder.switch_to_block(block);
            let word = self.body.builder.block_params(block)[0];
            let registers: Vec<(Register, Definition)> = self
                .region
                .numbers
                .iter()
                .filter_map(|register| {
                    let web = self.webs.leaves.get(&(pc, *register))?;
                    Some((*register, self.webs.find(*web)))
                })
                .collect();
            self.store_registers(&registers);
            self.finish(word);
        }
    }

    fn store_registers(&mut self, registers: &[(Register, Definition)]) {
        for &(register, web) in registers {
            let value = self.web(web);
            let bits = self.boxed(value);
            self.body.builder.ins().store(
                flags(),
                bits,
                self.registers,
                i32::from(register) * WORD_BYTES,
            );
        }
    }

    /// Leaves with `outcome` at `pc`.
    fn leave(&mut self, outcome: u64, pc: u32) {
        let word = self.outcome(outcome, u64::from(pc));
        let block = match self.leaves.get(&pc) {
            Some(block) => *block,
            None => {
                let block = self.cold_block();
                self.body.builder.append_block_param(block, types::I64);
                self.leaves.insert(pc, block);
                block
            }
        };
        self.body.builder.ins().jump(block, &[ir::BlockArg::Value(word)]);
    }

    /// A block that exits at `pc` for the general path.
    fn exit_block(&mut self, pc: u32) -> ir::Block {
        self.outcome_block(OUTCOME_EXIT, u64::from(pc))
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
        self.widenings.insert(pc, widening);
        self.outcome_block(OUTCOME_WIDEN, u64::from(pc))
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
        }
        match self.target_block(Some(pc), target) {
            Some(block) => {
                self.body.builder.ins().jump(block, &[]);
            }
            None => self.leave(OUTCOME_RESUME, target),
        }
        Some(())
    }

    fn fall_through(&mut self, pc: u32) -> Option<()> {
        match self.target_block(Some(pc), pc + 1) {
            Some(block) => {
                self.body.builder.ins().jump(block, &[]);
            }
            None => self.leave(OUTCOME_RESUME, pc + 1),
        }
        Some(())
    }

    /// The element index `key` names in an array of `length` elements, exiting at `pc`
    /// unless it is an integral Number below the length; an `i32` key a proof covers is.
    fn index(&mut self, pc: u32, key: Typed, length: ir::Value) -> ElementIndex {
        let (index, element) = match key.representation {
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
        };
        if !self.proved(pc) {
            let beyond = self.body.builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, index, length);
            self.guard(pc, beyond);
        }
        element
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

    /// ToInt32 of `value`. A Float converts saturating, which is ToInt32 for NaN (zero) and
    /// for every truncation strictly inside `i32`; a result at either bound may have
    /// saturated, so it exits at `pc` for the general path.
    fn int32(&mut self, pc: u32, value: Typed) -> ir::Value {
        if value.representation == Representation::Int32 {
            return value.value;
        }
        let integer = self.body.builder.ins().fcvt_to_sint_sat(types::I32, value.value);
        // `integer - (MIN + 1)` wraps both bounds, and only them, to the top two values.
        let shifted = self.body.builder.ins().iadd_imm_s(integer, -(i64::from(i32::MIN) + 1));
        let bound = self
            .body.builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, shifted, i64::from(u32::MAX - 1));
        self.guard(pc, bound);
        integer
    }

    /// `left OP right` as `i32` arithmetic into `register`, exiting at `pc` when the exact
    /// result is no `i32` (an overflow, or the -0 of a zero product with a negative factor);
    /// a proof covers a sum's.
    fn int32_arithmetic(
        &mut self,
        pc: u32,
        operator: FloatOperator,
        register: Register,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        let current = self.body.builder.current_block().expect("lowering inside a block");
        let chain = self.chain(current);
        // A variable read where a join's parameter proved trivial is an alias of its value.
        let (left, right) = (self.body.builder.func.dfg.resolve_aliases(left), self.body.builder.func.dfg.resolve_aliases(right));
        if let Some((result, computed)) = self.exact.get(&(operator, left, right)) {
            if *computed == chain {
                return *result;
            }
        }
        let result = self.int32_arithmetic_guarded(pc, operator, register, left, right);
        self.exact.insert((operator, left, right), (result, chain));
        result
    }

    /// `left ± c` for a constant `c`, whose `i32` result overflows exactly when `left` lies
    /// beyond the bound `c` leaves: the guard compares `left` with that bound before the
    /// sum, which needs no overflow flag.
    fn int32_offset(
        &mut self,
        pc: u32,
        operator: FloatOperator,
        register: Register,
        left: ir::Value,
        right: ir::Value,
    ) -> Option<ir::Value> {
        let (value, offset) = match operator {
            FloatOperator::Add => match (self.int32_constant(left), self.int32_constant(right)) {
                (_, Some(offset)) => (left, offset),
                (Some(offset), None) => (right, offset),
                (None, None) => return None,
            },
            FloatOperator::Subtract => (left, -self.int32_constant(right)?),
            FloatOperator::Multiply | FloatOperator::Divide => return None,
        };
        if !self.proved(pc) && offset != 0 {
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
        operator: FloatOperator,
        register: Register,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        if let Some(result) = self.int32_offset(pc, operator, register, left, right) {
            return result;
        }
        // A proof covers a sum's exactness, so it needs no overflow flag.
        match operator {
            FloatOperator::Add if self.proved(pc) => return self.body.builder.ins().iadd(left, right),
            FloatOperator::Subtract if self.proved(pc) => return self.body.builder.ins().isub(left, right),
            _ => {}
        }
        let (result, overflow) = match operator {
            FloatOperator::Add => self.body.builder.ins().sadd_overflow(left, right),
            FloatOperator::Subtract => self.body.builder.ins().ssub_overflow(left, right),
            FloatOperator::Multiply if self.proved(pc) => return self.body.builder.ins().imul(left, right),
            FloatOperator::Multiply => {
                let (product, overflow) = self.body.builder.ins().smul_overflow(left, right);
                self.guard_int32(pc, overflow, Widening::Register(register));
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
            FloatOperator::Divide => unreachable!("division has a Float64 result"),
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
                let ins = self.body.builder.ins();
                let value = match operator {
                    FloatOperator::Add => ins.fadd(left, right),
                    FloatOperator::Subtract => ins.fsub(left, right),
                    FloatOperator::Multiply => ins.fmul(left, right),
                    FloatOperator::Divide => ins.fdiv(left, right),
                };
                Typed { value, representation: Representation::Float64 }
            }
            Arithmetic::Int32(operator) => {
                let left = self.int32(pc, left);
                let right = self.int32(pc, right);
                let ins = self.body.builder.ins();
                // Shift counts are taken modulo 32, as Cranelift's shifts take them.
                let value = match operator {
                    Int32Operator::Or => ins.bor(left, right),
                    Int32Operator::And => ins.band(left, right),
                    Int32Operator::Xor => ins.bxor(left, right),
                    Int32Operator::ShiftLeft => ins.ishl(left, right),
                    Int32Operator::ShiftRight => ins.sshr(left, right),
                    Int32Operator::ShiftRightUnsigned => {
                        let shifted = ins.ushr(left, right);
                        let value = self.body.builder.ins().fcvt_from_uint(types::F64, shifted);
                        return Typed { value, representation: Representation::Float64 };
                    }
                };
                Typed { value, representation: Representation::Int32 }
            }
        }
    }

    fn instruction(&mut self, pc: u32) -> Option<()> {
        let instruction = self.region.instructions[(pc - self.region.header) as usize];
        match instruction.op() {
            Op::LoadConst => {
                let index = u16::try_from(instruction.constant_index()).ok()?;
                let number = *self.region.constants.get(&index)?;
                let value = self.read(Source::Number(number));
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
                    Arithmetic::Float(FloatOperator::Multiply) if unit(instruction.operand_c()) => left,
                    Arithmetic::Float(FloatOperator::Multiply) if unit(instruction.operand_b()) => right,
                    Arithmetic::Float(FloatOperator::Divide) if unit(instruction.operand_c()) => left,
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
                    let operator = if decrement { FloatOperator::Subtract } else { FloatOperator::Add };
                    let one = self.read(Source::Number(1.0));
                    self.arithmetic(pc, Arithmetic::Float(operator), result, operand, one)
                };
                self.define(result, value);
                self.fall_through(pc)
            }
            Op::Unary => {
                let operand = self.register(instruction.register_b());
                let result = instruction.result_register();
                let value = match unary(instruction.unary_operator())? {
                    Unary::Plus => operand,
                    Unary::Negate
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
                    Unary::Negate => {
                        let number = self.float(operand);
                        let value = self.body.builder.ins().fneg(number);
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
                let FieldLookup::Atom { atom, base, cache_site } = instruction.field_lookup()? else {
                    return None;
                };
                let receiver = if base == FieldBase::THIS {
                    FieldReceiver::This
                } else {
                    FieldReceiver::Register(base.register_index()?)
                };
                let field = self
                    .region
                    .fields
                    .iter()
                    .position(|field| field.receiver == receiver && field.atom == atom && field.cache_site == cache_site)?;
                let value = self.fields[field];
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::GetIndex => {
                let base = instruction.operand_b().register_index()?;
                let (elements, length) = self.arrays[&base];
                let key = self.operand(instruction.operand_c())?;
                let index = self.index(pc, key, length);
                let (address, displacement) = self.element_address(elements, index);
                // A hole is tagged, so unboxing sends it to the general path too. A Float64
                // element loads straight into a float register.
                let representation = element_representation(&instruction, self.observation);
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
                };
                self.define(instruction.result_register(), Typed { value, representation });
                self.fall_through(pc)
            }
            Op::SetIndex => {
                let (elements, length) = self.arrays[&instruction.register_b()];
                let key = self.register(instruction.register_c());
                let index = self.index(pc, key, length);
                let (address, displacement) = self.element_address(elements, index);
                let old = self.body.builder.ins().load(types::I64, flags(), address, displacement);
                let tag = self.body.builder.ins().band_imm_u(old, ValueLayout::TAG_MASK as i64);
                let hole = self.body.builder.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::DELETED_TAG as i64);
                self.guard(pc, hole);
                let value = self.register(instruction.register_a());
                let bits = self.encoding(value);
                self.body.builder.ins().store(flags(), bits, address, displacement);
                self.fall_through(pc)
            }
            Op::Jump => self.jump(pc, instruction.jump_target()),
            Op::JumpBinaryFalse => {
                let operator = instruction.binary_operator_field();
                let left = self.operand(instruction.operand_b())?;
                let right = self.operand(instruction.operand_c())?;
                let holds = if left.representation == Representation::Int32
                    && right.representation == Representation::Int32
                {
                    let condition = integer_comparison(operator)?;
                    self.body.builder.ins().icmp(condition, left.value, right.value)
                } else {
                    let (left, right) = (self.float(left), self.float(right));
                    super::emit_number_relation(&mut self.body.builder, operator, left, right)?
                };
                let taken = self.body.builder.create_block();
                let not_taken = self.body.builder.create_block();
                let tested = self.body.builder.current_block().expect("lowering inside a block");
                self.body.builder.ins().brif(holds, not_taken, &[], taken, &[]);
                // Each arm's only predecessor is the test's block.
                self.body.builder.switch_to_block(taken);
                self.body.builder.seal_block(taken);
                self.jump(pc, instruction.jump_target())?;
                let chain = self.chain(tested);
                self.chains.insert(not_taken, chain);
                self.body.builder.switch_to_block(not_taken);
                self.body.builder.seal_block(not_taken);
                self.fall_through(pc)
            }
            _ => None,
        }
    }
}

/// `comparison` of two `i32` Numbers.
fn integer_comparison(operator: u32) -> Option<IntCC> {
    struct IntegerCondition;

    impl super::super::operations::NumberRelationStage<()> for IntegerCondition {
        type Output = IntCC;

        fn equal(&mut self, (): (), (): ()) -> Self::Output {
            IntCC::Equal
        }
        fn not_equal(&mut self, (): (), (): ()) -> Self::Output {
            IntCC::NotEqual
        }
        fn less_than(&mut self, (): (), (): ()) -> Self::Output {
            IntCC::SignedLessThan
        }
        fn less_equal(&mut self, (): (), (): ()) -> Self::Output {
            IntCC::SignedLessThanOrEqual
        }
        fn greater_than(&mut self, (): (), (): ()) -> Self::Output {
            IntCC::SignedGreaterThan
        }
        fn greater_equal(&mut self, (): (), (): ()) -> Self::Output {
            IntCC::SignedGreaterThanOrEqual
        }
    }

    Some(
        super::super::operations::NumberRelation::from_operator(operator)?.apply(
            &mut IntegerCondition,
            (),
            (),
        ),
    )
}
