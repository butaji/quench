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

use crate::bytecode::{Function, Op, Operand, OperandKind, Register, WideInstruction};
use crate::value::{Value, ValueLayout};
use cranelift_codegen::ir::{
    self, AbiParam, InstBuilder, UserFuncName,
    condcodes::{FloatCC, IntCC},
    types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use oxc_ast::ast::{BinaryOperator, UnaryOperator};

/// The most instructions a region spans. Native code polls termination only at backward
/// jumps, so the bound keeps compile time and code size proportional to small loop bodies.
const MAX_REGION_INSTRUCTIONS: usize = 512;
/// The most registers, captures and arrays a region names, bounding entry guards and the
/// entry record.
const MAX_REGION_REGISTERS: usize = 128;
const MAX_REGION_ARRAYS: usize = 8;
const MAX_REGION_CAPTURES: usize = 16;
/// The largest native body a region may produce.
const MAX_CODE_BYTES: usize = 64 * 1024;

/// What native code returns: the outcome in the high word, a bytecode PC in the low one.
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
}

const OUTCOME_NOT_ENTERED: u64 = 0;
const OUTCOME_RESUME: u64 = 1;
const OUTCOME_EXIT: u64 = 2;
const OUTCOME_WIDEN: u64 = 3;
/// A not-entered outcome's low word: zero, or a widening's kind bit over its payload.
const WIDEN_REGISTER: u64 = 1 << u16::BITS;
const WIDEN_CAPTURE: u64 = WIDEN_REGISTER << 1;
const WIDEN_PAYLOAD: u64 = WIDEN_REGISTER - 1;

impl Widening {
    /// The not-entered word of an entry guard's widening; element reads are guarded where
    /// they run, so an entry never names them.
    fn code(self) -> u64 {
        match self {
            Self::Register(register) => WIDEN_REGISTER | u64::from(register),
            Self::Capture(index) => WIDEN_CAPTURE | u64::from(index),
            Self::Elements(_) => 0,
        }
    }

    fn decode(word: u64) -> Option<Self> {
        let payload = (word & WIDEN_PAYLOAD) as u16;
        if word & WIDEN_REGISTER != 0 {
            Some(Self::Register(payload))
        } else if word & WIDEN_CAPTURE != 0 {
            Some(Self::Capture(payload))
        } else {
            None
        }
    }
}

impl LoopOutcome {
    fn decode(word: u64) -> Self {
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
    /// The region's instructions, from the header.
    instructions: Vec<WideInstruction>,
    /// The Number constants the region reads, by constant index.
    constants: std::collections::BTreeMap<u16, f64>,
    /// The function's live registers before each instruction (`compile::liveness`), when
    /// liveness covers it.
    live: Option<Box<[u64]>>,
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

/// The entry record native code reads: the termination flag, then each capture's value,
/// then each array's element pointer and length.
pub(crate) struct LoopEntry {
    words: [u64; ENTRY_WORDS],
    length: usize,
}

/// The longest array a region indexes: a negative `i32` index zero-extends past it, so one
/// unsigned comparison bounds every `i32` index.
const MAX_INDEXED_LENGTH: usize = i32::MAX as usize;
const ENTRY_INTERRUPT: usize = 0;
const ENTRY_CAPTURES: usize = 1;
const WORD_BYTES: i32 = 8;
/// An array's two entry words: its element pointer, then its length.
const ARRAY_WORDS: usize = 2;
const ENTRY_WORDS: usize = ENTRY_CAPTURES + MAX_REGION_CAPTURES + ARRAY_WORDS * MAX_REGION_ARRAYS;

impl LoopEntry {
    pub(crate) fn new(interrupt: &std::sync::atomic::AtomicBool) -> Self {
        let mut words = [0; ENTRY_WORDS];
        words[ENTRY_INTERRUPT] = std::ptr::from_ref(interrupt).addr() as u64;
        Self { words, length: ENTRY_CAPTURES }
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

#[derive(Clone, Copy, PartialEq, Eq)]
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

fn arithmetic(operator: u32) -> Option<Arithmetic> {
    use BinaryOperator as B;
    Some(match operator {
        op if op == B::Addition as u32 => Arithmetic::Float(FloatOperator::Add),
        op if op == B::Subtraction as u32 => Arithmetic::Float(FloatOperator::Subtract),
        op if op == B::Multiplication as u32 => Arithmetic::Float(FloatOperator::Multiply),
        op if op == B::Division as u32 => Arithmetic::Float(FloatOperator::Divide),
        op if op == B::BitwiseOR as u32 => Arithmetic::Int32(Int32Operator::Or),
        op if op == B::BitwiseAnd as u32 => Arithmetic::Int32(Int32Operator::And),
        op if op == B::BitwiseXOR as u32 => Arithmetic::Int32(Int32Operator::Xor),
        op if op == B::ShiftLeft as u32 => Arithmetic::Int32(Int32Operator::ShiftLeft),
        op if op == B::ShiftRight as u32 => Arithmetic::Int32(Int32Operator::ShiftRight),
        op if op == B::ShiftRightZeroFill as u32 => Arithmetic::Int32(Int32Operator::ShiftRightUnsigned),
        _ => return None,
    })
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

/// The float condition under which a Number comparison holds; NaN operands make every
/// relation and `===` false and `!==` true, as these conditions do.
fn comparison(operator: u32) -> Option<FloatCC> {
    use BinaryOperator as B;
    Some(match operator {
        op if op == B::LessThan as u32 => FloatCC::LessThan,
        op if op == B::LessEqualThan as u32 => FloatCC::LessThanOrEqual,
        op if op == B::GreaterThan as u32 => FloatCC::GreaterThan,
        op if op == B::GreaterEqualThan as u32 => FloatCC::GreaterThanOrEqual,
        op if op == B::StrictEquality as u32 || op == B::Equality as u32 => FloatCC::Equal,
        op if op == B::StrictInequality as u32 || op == B::Inequality as u32 => FloatCC::NotEqual,
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
    numbers: std::collections::BTreeMap<u16, f64>,
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
) -> Option<LoopRegion> {
    let (start, end) = (header as usize, backedge as usize);
    if start > end || end >= decoded.len() || end - start >= MAX_REGION_INSTRUCTIONS {
        return None;
    }
    let mut admission = Admission {
        constants,
        roles: std::collections::BTreeMap::new(),
        captures: Vec::new(),
        numbers: std::collections::BTreeMap::new(),
    };
    for instruction in &decoded[start..=end] {
        admit(&mut admission, *instruction)?;
    }
    let registers = usize::from(function.registers);
    if admission.roles.keys().any(|register| usize::from(*register) >= registers)
        || admission.roles.len() > MAX_REGION_REGISTERS
        || admission.captures.len() > MAX_REGION_CAPTURES
    {
        return None;
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
    (arrays.len() <= MAX_REGION_ARRAYS).then_some(())?;
    Some(LoopRegion {
        header,
        backedge,
        numbers,
        arrays,
        captures: admission.captures,
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
            arithmetic(instruction.binary_operator())?;
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
        Op::SetIndex => {
            admission.number(instruction.register_a())?;
            admission.note(instruction.register_b(), Role::Array { written: true })?;
            admission.number(instruction.register_c())?;
        }
        Op::Jump => {}
        Op::JumpBinaryFalse => {
            comparison(instruction.binary_operator_field())?;
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
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => match arithmetic(instruction.binary_operator())? {
            Arithmetic::Float(FloatOperator::Divide) => Representation::Float64,
            Arithmetic::Float(_) => both(source(instruction.operand_b()), source(instruction.operand_c())),
            Arithmetic::Int32(Int32Operator::ShiftRightUnsigned) => Representation::Float64,
            Arithmetic::Int32(_) => Representation::Int32,
        },
        Op::Unary => match unary(instruction.unary_operator())? {
            Unary::Plus | Unary::Negate => registers(instruction.register_b()),
            Unary::BitwiseNot => Representation::Int32,
        },
        Op::GetIndex => element_representation(instruction, observation),
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
/// `i32`s and trip counts stay below 2^34, so with terms this small every `i64` the proof
/// computes (`Lowering::proof`) stays below 2^60.
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
}

/// A value of a counted loop's body: its base's value in the iteration plus `offset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Affine {
    base: Base,
    offset: i64,
}

impl Affine {
    fn plus(self, amount: i64) -> Option<Self> {
        let offset = self.offset.checked_add(amount)?;
        (offset.abs() <= MAX_AFFINE_TERM).then_some(Self { base: self.base, offset })
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
}

#[derive(Clone, Copy, Debug)]
struct Fact {
    pc: u32,
    value: Affine,
    bound: Bound,
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
            Base::Constant | Base::Capture(_) => 0,
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
            let header = instruction.jump_target();
            (instruction.op() == Op::Jump && (region.header..backedge).contains(&header)).then_some(())?;
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
    let constant = |number: f64| int32_number(number).then(|| Affine { base: Base::Constant, offset: number as i64 });
    let mut state: BTreeMap<Register, Option<Affine>> = BTreeMap::new();
    let value = |state: &BTreeMap<Register, Option<Affine>>, register: Register| {
        state
            .get(&register)
            .copied()
            .unwrap_or(Some(Affine { base: Base::Start(register), offset: 0 }))
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
                (captures[index] == Representation::Int32).then_some(Affine { base: Base::Capture(index), offset: 0 })
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
                let sum = match (arithmetic(instruction.binary_operator())?, left.1, right.1) {
                    (Arithmetic::Float(FloatOperator::Add), Some(left), Some(right)) => match (left.base, right.base) {
                        (_, Base::Constant) => left.plus(right.offset),
                        (Base::Constant, _) => right.plus(left.offset),
                        _ => None,
                    },
                    (Arithmetic::Float(FloatOperator::Subtract), Some(left), Some(right))
                        if right.base == Base::Constant =>
                    {
                        left.plus(-right.offset)
                    }
                    _ => None,
                };
                if let Some(value) = sum.filter(|_| left.0 && right.0 && defined()) {
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
    let written: BTreeSet<Register> = state.keys().copied().collect();
    let steps: BTreeMap<Register, i64> = state
        .iter()
        .filter_map(|(register, value)| match value {
            Some(Affine { base: Base::Start(base), offset }) if base == register => Some((*register, *offset)),
            _ => None,
        })
        .collect();
    // How much a base changes per iteration, when it changes by a constant: a register the
    // body never writes does not change.
    let step = |base: Base| match base {
        Base::Constant | Base::Capture(_) => Some(0),
        Base::Start(register) if !written.contains(&register) => Some(0),
        Base::Start(register) => steps.get(&register).copied().filter(|step| step.abs() <= MAX_AFFINE_TERM),
    };
    // A base the proof reads at the header is an `i32` there.
    let held = |base: Base| match base {
        Base::Start(register) => int32(webs.read(region, header, register)),
        Base::Constant | Base::Capture(_) => true,
    };
    let (operator, (left_held, left), (right_held, right)) = tested?;
    let (left, right) = (left?, right?);
    (left_held && right_held && held(left.base) && held(right.base)).then_some(())?;
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
        .filter(|fact| held(fact.value.base) && step(fact.value.base).is_some())
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
        facts,
    })
}

/// A compiled region: one W^X mapping, its entry, and what each `LoopOutcome::Widen` PC
/// found was no `i32`.
pub(super) struct CompiledLoop {
    _code: super::compiled::CodeMemory,
    entry: unsafe extern "C" fn(*mut Value, *const u64) -> u64,
    widenings: std::collections::BTreeMap<u32, Widening>,
}

impl CompiledLoop {
    /// Runs the region on `registers` with `entry`'s record.
    ///
    /// # Safety
    /// `registers` is the running activation's register file, at least `Function::registers`
    /// long; `entry` was built for this region from the current values of its arrays and
    /// captures, which nothing changes while the call runs.
    pub(super) unsafe fn run(&self, registers: *mut Value, entry: &LoopEntry) -> LoopOutcome {
        debug_assert!(entry.length <= ENTRY_WORDS);
        // SAFETY: as the caller promises; the code reads and writes only those.
        LoopOutcome::decode(unsafe { (self.entry)(registers, entry.words.as_ptr()) })
    }

    /// What a `LoopOutcome::Widen` at `pc` found was no `i32`.
    pub(super) fn widening(&self, pc: u32) -> Option<Widening> {
        self.widenings.get(&pc).copied()
    }
}

/// Native code for `region` under `observation`, or `None` when Cranelift declines it.
pub(super) fn compile(region: &LoopRegion, observation: &Observation<'_>) -> Option<CompiledLoop> {
    let isa = super::jit_backend::target_isa("speed", false)?;
    let pointer = isa.pointer_type();
    let mut signature = ir::Signature::new(isa.default_call_conv());
    signature.params.push(AbiParam::new(pointer));
    signature.params.push(AbiParam::new(pointer));
    signature.returns.push(AbiParam::new(types::I64));
    let mut native = ir::Function::with_name_signature(
        UserFuncName::user(LOOP_NAMESPACE, region.header),
        signature,
    );
    let webs = webs(region);
    let (representations, captures) = representations(region, &webs, observation);
    let counted = counted_loops(region, &webs, &representations, &captures);
    let mut context = FunctionBuilderContext::new();
    let builder = FunctionBuilder::new(&mut native, &mut context);
    let widenings = Lowering::new(builder, region, observation, pointer, webs, representations, captures, counted)
        .lower(isa.frontend_config())?;
    let mut context = cranelift_codegen::Context::for_function(native);
    let compiled = context
        .compile(&*isa, &mut cranelift_codegen::control::ControlPlane::default())
        .ok()?;
    if !compiled.buffer.relocs().is_empty() {
        return None;
    }
    let code = compiled.code_buffer();
    if code.is_empty() || code.len() > MAX_CODE_BYTES {
        return None;
    }
    let memory = super::compiled::CodeMemory::new(code)?;
    // SAFETY: the lowering emits `extern "C" fn(*mut Value, *const u64) -> u64`.
    let entry = unsafe {
        std::mem::transmute::<*mut u8, unsafe extern "C" fn(*mut Value, *const u64) -> u64>(
            memory.pointer(),
        )
    };
    Some(CompiledLoop { _code: memory, entry, widenings })
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
    builder: FunctionBuilder<'b>,
    region: &'a LoopRegion,
    observation: &'a Observation<'a>,
    pointer: ir::Type,
    webs: Webs,
    /// Each web's representation, at its root definition.
    representations: Vec<Representation>,
    /// Each web's SSA variable, by its root definition, declared on first use.
    variables: std::collections::BTreeMap<Definition, Variable>,
    /// The instruction being lowered, whose reads and result name webs.
    pc: u32,
    /// Each capture's value, unboxed at entry, in region order.
    captures: Vec<Typed>,
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
    registers: ir::Value,
    entry: ir::Value,
    /// The host termination flag's address, loaded at entry.
    interrupt: Option<ir::Value>,
    /// Each array's element pointer and length, by register.
    arrays: std::collections::BTreeMap<Register, (ir::Value, ir::Value)>,
    widenings: std::collections::BTreeMap<u32, Widening>,
}

fn flags() -> ir::MemFlagsData {
    ir::MemFlagsData::trusted()
}

fn machine_type(representation: Representation) -> ir::Type {
    match representation {
        Representation::Int32 => types::I32,
        Representation::Float64 => types::F64,
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
    ) -> Self {
        let start = builder.create_block();
        builder.append_block_params_for_function_params(start);
        builder.switch_to_block(start);
        let (registers, entry) = (builder.block_params(start)[0], builder.block_params(start)[1]);
        let blocks = (region.header..=region.backedge).map(|_| builder.create_block()).collect();
        let preheaders = counted.iter().map(|_| builder.create_block()).collect();
        let proved_blocks = counted
            .iter()
            .map(|counted| (counted.header..=counted.backedge).map(|_| builder.create_block()).collect())
            .collect();
        Self {
            builder,
            region,
            observation,
            pointer,
            webs,
            representations,
            variables: std::collections::BTreeMap::new(),
            pc: region.header,
            captures: Vec::new(),
            capture_representations,
            blocks,
            counted,
            preheaders,
            proved_blocks,
            copy: None,
            leaves: std::collections::BTreeMap::new(),
            registers,
            entry,
            interrupt: None,
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
            self.builder.switch_to_block(block);
            self.pc = pc;
            self.instruction(pc)?;
        }
        for index in 0..self.counted.len() {
            self.copy = Some(index);
            let (header, backedge) = (self.counted[index].header, self.counted[index].backedge);
            for pc in header..=backedge {
                self.builder.switch_to_block(self.proved_blocks[index][(pc - header) as usize]);
                self.pc = pc;
                self.instruction(pc)?;
            }
            self.copy = None;
            self.preheader(index);
        }
        self.write_back();
        self.builder.seal_all_blocks();
        self.builder.finalize(frontend);
        Some(self.widenings)
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
    fn start(&mut self, header: u32, base: Base) -> ir::Value {
        let value = match base {
            Base::Constant => return self.builder.ins().iconst(types::I64, 0),
            Base::Capture(index) => self.captures[index].value,
            Base::Start(register) => {
                let web = self.webs.read(self.region, header, register);
                self.web(web).value
            }
        };
        self.builder.ins().sextend(types::I64, value)
    }

    fn affine_start(&mut self, header: u32, value: Affine) -> ir::Value {
        let start = self.start(header, value.base);
        self.builder.ins().iadd_imm_s(start, value.offset)
    }

    /// The counted loop's preheader: runs its proof from the values arriving at the header
    /// and continues in the proved copy when every fact holds, else in the guarded body.
    fn preheader(&mut self, index: usize) {
        let counted = self.counted[index].clone();
        self.builder.switch_to_block(self.preheaders[index]);
        let holds = self.proof(&counted);
        let proved = self.proved_blocks[index][0];
        let guarded = self.blocks[(counted.header - self.region.header) as usize];
        self.builder.ins().brif(holds, proved, &[], guarded, &[]);
    }

    /// Whether every fact of `counted` holds for the iterations its arrival runs. The test
    /// passes `trips` times and fails on the next arrival, so an instruction before the test
    /// runs for iterations `0..=trips` and one after it for `0..trips`; an affine value is
    /// monotonic in the iteration, so its extremes are at the first and last of them.
    fn proof(&mut self, counted: &CountedLoop) -> ir::Value {
        let header = counted.header;
        let tested = self.affine_start(header, counted.induction);
        let bound = self.affine_start(header, counted.bound);
        let step = counted.base_step(counted.induction.base);
        let (trips, mut holds) = match (counted.relation, step > 0) {
            // Moving toward the bound, the test passes until the value crosses it.
            (Continue::AtMost, true) | (Continue::AtLeast, false) => {
                let distance = match counted.relation {
                    Continue::AtMost => self.builder.ins().isub(bound, tested),
                    Continue::AtLeast => self.builder.ins().isub(tested, bound),
                };
                let span = if step.abs() == 1 {
                    distance
                } else {
                    let magnitude = self.builder.ins().iconst(types::I64, step.abs());
                    self.builder.ins().udiv(distance, magnitude)
                };
                let passing = self.builder.ins().iadd_imm_s(span, 1);
                let zero = self.builder.ins().iconst(types::I64, 0);
                let crossed = self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, distance, 0);
                let trips = self.builder.ins().select(crossed, zero, passing);
                (trips, self.builder.ins().iconst(types::I8, 1))
            }
            // Moving away from it, the test passes until the value leaves `i32`, which the
            // guarded body meets; the proof holds only when the first test fails.
            (Continue::AtMost, false) | (Continue::AtLeast, true) => {
                let condition = match counted.relation {
                    Continue::AtMost => IntCC::SignedGreaterThan,
                    Continue::AtLeast => IntCC::SignedLessThan,
                };
                let fails = self.builder.ins().icmp(condition, tested, bound);
                (self.builder.ins().iconst(types::I64, 0), fails)
            }
        };
        let none = self.builder.ins().icmp_imm_s(IntCC::Equal, trips, 0);
        let trips_after = self.builder.ins().iadd_imm_s(trips, -1);
        for fact in &counted.facts {
            let first = self.affine_start(header, fact.value);
            let step = counted.base_step(fact.value.base);
            let last_iteration = if fact.pc < counted.test { trips } else { trips_after };
            let travel = self.builder.ins().imul_imm_s(last_iteration, step);
            let last = self.builder.ins().iadd(first, travel);
            let (low, high) = if step >= 0 { (first, last) } else { (last, first) };
            let (above, below) = match fact.bound {
                Bound::Index(array) => {
                    let length = self.arrays[&array].1;
                    (
                        self.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, low, 0),
                        self.builder.ins().icmp(IntCC::SignedLessThan, high, length),
                    )
                }
                Bound::Int32 => (
                    self.builder.ins().icmp_imm_s(IntCC::SignedGreaterThanOrEqual, low, i64::from(i32::MIN)),
                    self.builder.ins().icmp_imm_s(IntCC::SignedLessThanOrEqual, high, i64::from(i32::MAX)),
                ),
            };
            let within = self.builder.ins().band(above, below);
            let within = if fact.pc < counted.test { within } else { self.builder.ins().bor(within, none) };
            holds = self.builder.ins().band(holds, within);
        }
        holds
    }

    fn outcome(&mut self, outcome: u64, low: u64) -> ir::Value {
        self.builder
            .ins()
            .iconst(types::I64, ((outcome << OUTCOME_SHIFT) | low) as i64)
    }

    fn word(&mut self, index: usize) -> ir::Value {
        self.builder
            .ins()
            .load(types::I64, flags(), self.entry, index as i32 * WORD_BYTES)
    }

    /// A block laid out after the hot code: an exit, or a representation a region rarely
    /// meets.
    fn cold_block(&mut self) -> ir::Block {
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        block
    }

    /// A block that returns not entered, naming `widening`.
    fn not_entered(&mut self, widening: Option<Widening>) -> ir::Block {
        let current = self.builder.current_block().expect("lowering inside a block");
        let block = self.cold_block();
        self.builder.switch_to_block(block);
        let word = self.outcome(OUTCOME_NOT_ENTERED, widening.map_or(0, Widening::code));
        self.builder.ins().return_(&[word]);
        self.builder.switch_to_block(current);
        block
    }

    /// Unboxes every Number register and capture in its representation, entering only when
    /// all hold such values.
    fn enter(&mut self) -> Option<()> {
        self.interrupt = Some(self.word(ENTRY_INTERRUPT));
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
        // Only an entry value some read reaches is loaded and guarded.
        let registers: Vec<(Register, Definition)> = (0..self.region.numbers.len())
            .filter(|entry| self.webs.entry_read[*entry])
            .map(|entry| (self.region.numbers[entry], self.webs.find(entry)))
            .collect();
        for (register, web) in registers {
            let representation = self.representations[web];
            let bits = self.builder.ins().load(
                types::I64,
                flags(),
                self.registers,
                i32::from(register) * WORD_BYTES,
            );
            let otherwise = self.not_entered(Some(Widening::Register(register)));
            let value = self.unbox(bits, representation, otherwise);
            let variable = self.variable(web);
            self.builder.def_var(variable, value);
        }
        let header = self.target_block(None, self.region.header)?;
        self.builder.ins().jump(header, &[]);
        Some(())
    }

    /// The value `bits` encode in `representation`: an encoded `i32` for `Int32`, any Number
    /// for `Float64`, testing for a double first; anything else branches to `otherwise`.
    fn unbox(&mut self, bits: ir::Value, representation: Representation, otherwise: ir::Block) -> ir::Value {
        if representation == Representation::Int32 {
            let tag = self.builder.ins().band_imm_u(bits, ValueLayout::TAG_MASK as i64);
            let is_int = self.builder.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::INT_TAG as i64);
            let integer_block = self.builder.create_block();
            self.builder.ins().brif(is_int, integer_block, &[], otherwise, &[]);
            self.builder.switch_to_block(integer_block);
            return self.builder.ins().ireduce(types::I32, bits);
        }
        // A tag field within the tag range marks a non-double.
        let field = self.builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
        let offset = self.builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
        let tagged = self
            .builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedLessThanOrEqual, offset, ValueLayout::TAG_FIELD_SPAN as i64);
        let double_block = self.builder.create_block();
        let tagged_block = self.cold_block();
        let integer_block = self.cold_block();
        let join = self.builder.create_block();
        self.builder.append_block_param(join, types::F64);
        self.builder.ins().brif(tagged, tagged_block, &[], double_block, &[]);
        self.builder.switch_to_block(double_block);
        let double = self.builder.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(double)]);
        self.builder.switch_to_block(tagged_block);
        let int_field = (ValueLayout::INT_TAG >> ValueLayout::TAG_SHIFT) as i64;
        let is_int = self.builder.ins().icmp_imm_u(IntCC::Equal, field, int_field);
        self.builder.ins().brif(is_int, integer_block, &[], otherwise, &[]);
        self.builder.switch_to_block(integer_block);
        let integer = self.builder.ins().ireduce(types::I32, bits);
        let number = self.builder.ins().fcvt_from_sint(types::F64, integer);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(number)]);
        self.builder.switch_to_block(join);
        self.builder.block_params(join)[0]
    }

    /// The encoded bits of an `i32` Number.
    fn boxed_int32(&mut self, integer: ir::Value) -> ir::Value {
        let payload = self.builder.ins().uextend(types::I64, integer);
        self.builder.ins().bor_imm_u(payload, ValueLayout::INT_TAG as i64)
    }

    /// `Value::number(number)`'s bits: an integer when the truncation converts back to the
    /// same bits, the canonical NaN for any NaN, else the double's bits. A double its
    /// truncation differs from (any non-integral value, and NaN) takes the short path, which
    /// only canonicalizes NaN; integral values and infinities take the conversion.
    fn boxed_float64(&mut self, number: ir::Value) -> ir::Value {
        let truncated = self.builder.ins().trunc(number);
        let integral = self.builder.ins().fcmp(FloatCC::Equal, truncated, number);
        let fractional_block = self.builder.create_block();
        let integral_block = self.cold_block();
        let join = self.builder.create_block();
        self.builder.append_block_param(join, types::I64);
        self.builder.ins().brif(integral, integral_block, &[], fractional_block, &[]);
        self.builder.switch_to_block(fractional_block);
        let bits = self.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
        let nan = self.builder.ins().fcmp(FloatCC::Unordered, number, number);
        let canonical = self.builder.ins().iconst(types::I64, ValueLayout::CANONICAL_NAN as i64);
        let fractional = self.builder.ins().select(nan, canonical, bits);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(fractional)]);
        self.builder.switch_to_block(integral_block);
        let integer = self.builder.ins().fcvt_to_sint_sat(types::I32, number);
        let back = self.builder.ins().fcvt_from_sint(types::F64, integer);
        let back_bits = self.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), back);
        let bits = self.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
        let exact = self.builder.ins().icmp(IntCC::Equal, back_bits, bits);
        let encoded = self.boxed_int32(integer);
        let integral = self.builder.ins().select(exact, encoded, bits);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(integral)]);
        self.builder.switch_to_block(join);
        self.builder.block_params(join)[0]
    }

    fn boxed(&mut self, value: Typed) -> ir::Value {
        match value.representation {
            Representation::Int32 => self.boxed_int32(value.value),
            Representation::Float64 => self.boxed_float64(value.value),
        }
    }

    /// Fills each leave block: the interpreter resumes at its PC, so only the registers live
    /// there (`LoopRegion::live`) take their native values; any other keeps the value it
    /// held, which nothing reads before writing.
    fn write_back(&mut self) {
        let leaves: Vec<(u32, ir::Block)> = self.leaves.iter().map(|(pc, block)| (*pc, *block)).collect();
        for (pc, block) in leaves {
            self.builder.switch_to_block(block);
            let word = self.builder.block_params(block)[0];
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
            self.builder.ins().return_(&[word]);
        }
    }

    fn store_registers(&mut self, registers: &[(Register, Definition)]) {
        for &(register, web) in registers {
            let value = self.web(web);
            let bits = self.boxed(value);
            self.builder.ins().store(
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
                self.builder.append_block_param(block, types::I64);
                self.leaves.insert(pc, block);
                block
            }
        };
        self.builder.ins().jump(block, &[ir::BlockArg::Value(word)]);
    }

    /// A block that exits at `pc` for the general path.
    fn exit_block(&mut self, pc: u32) -> ir::Block {
        let current = self.builder.current_block().expect("lowering inside a block");
        let exit = self.cold_block();
        self.builder.switch_to_block(exit);
        self.leave(OUTCOME_EXIT, pc);
        self.builder.switch_to_block(current);
        exit
    }

    /// Continues past a guard that `failed` decides, exiting at `pc` when it holds.
    fn guard(&mut self, pc: u32, failed: ir::Value) {
        let exit = self.exit_block(pc);
        let pass = self.builder.create_block();
        self.builder.ins().brif(failed, exit, &[], pass, &[]);
        self.builder.switch_to_block(pass);
    }

    /// A block that exits at `pc` for the general path, naming `widening` for the next
    /// compilation.
    fn widen_block(&mut self, pc: u32, widening: Widening) -> ir::Block {
        self.widenings.insert(pc, widening);
        let current = self.builder.current_block().expect("lowering inside a block");
        let exit = self.cold_block();
        self.builder.switch_to_block(exit);
        self.leave(OUTCOME_WIDEN, pc);
        self.builder.switch_to_block(current);
        exit
    }

    /// `guard` of an `i32` assumption: `widening` was no `i32` when it fails.
    fn guard_int32(&mut self, pc: u32, failed: ir::Value, widening: Widening) {
        let exit = self.widen_block(pc, widening);
        let pass = self.builder.create_block();
        self.builder.ins().brif(failed, exit, &[], pass, &[]);
        self.builder.switch_to_block(pass);
    }

    /// The SSA variable of the web rooted at `web`.
    fn variable(&mut self, web: Definition) -> Variable {
        let representation = self.representations[web];
        *self
            .variables
            .entry(web)
            .or_insert_with(|| self.builder.declare_var(machine_type(representation)))
    }

    fn web(&mut self, web: Definition) -> Typed {
        let variable = self.variable(web);
        Typed {
            value: self.builder.use_var(variable),
            representation: self.representations[web],
        }
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
                value: self.builder.ins().iconst(types::I32, i64::from(number as i32)),
                representation: Representation::Int32,
            },
            Source::Number(number) => Typed {
                value: self.builder.ins().f64const(number),
                representation: Representation::Float64,
            },
        }
    }

    fn float(&mut self, value: Typed) -> ir::Value {
        match value.representation {
            Representation::Int32 => self.builder.ins().fcvt_from_sint(types::F64, value.value),
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
        let value = match (self.representations[web], value.representation) {
            (Representation::Float64, Representation::Int32) => self.float(value),
            _ => value.value,
        };
        let variable = self.variable(web);
        self.builder.def_var(variable, value);
    }

    /// Continues at `target`: its block within the region, else the lane resumes there. A
    /// backward jump polls termination first.
    fn jump(&mut self, pc: u32, target: u32) -> Option<()> {
        if target <= pc {
            let flag = self.interrupt?;
            let requested = self.builder.ins().atomic_load(types::I8, flags(), flag);
            self.guard(pc, requested);
        }
        match self.target_block(Some(pc), target) {
            Some(block) => {
                self.builder.ins().jump(block, &[]);
            }
            None => self.leave(OUTCOME_RESUME, target),
        }
        Some(())
    }

    fn fall_through(&mut self, pc: u32) -> Option<()> {
        match self.target_block(Some(pc), pc + 1) {
            Some(block) => {
                self.builder.ins().jump(block, &[]);
            }
            None => self.leave(OUTCOME_RESUME, pc + 1),
        }
        Some(())
    }

    /// The element index `key` names in an array of `length` elements, exiting at `pc`
    /// unless it is an integral Number below the length; an `i32` key a proof covers is.
    fn index(&mut self, pc: u32, key: Typed, length: ir::Value) -> ir::Value {
        let index = match key.representation {
            // A negative index zero-extends past every length (`MAX_INDEXED_LENGTH`).
            Representation::Int32 => self.builder.ins().uextend(types::I64, key.value),
            Representation::Float64 => {
                let index = self.builder.ins().fcvt_to_sint_sat(types::I64, key.value);
                let back = self.builder.ins().fcvt_from_sint(types::F64, index);
                let fractional = self.builder.ins().fcmp(FloatCC::NotEqual, back, key.value);
                self.guard(pc, fractional);
                index
            }
        };
        if !self.proved(pc) {
            let beyond = self.builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, index, length);
            self.guard(pc, beyond);
        }
        index
    }

    fn element_address(&mut self, elements: ir::Value, index: ir::Value) -> ir::Value {
        let offset = self.builder.ins().imul_imm_u(index, i64::from(WORD_BYTES));
        let offset = if self.pointer == types::I64 {
            offset
        } else {
            self.builder.ins().ireduce(self.pointer, offset)
        };
        self.builder.ins().iadd(elements, offset)
    }

    /// ToInt32 of `value`. A Float converts saturating, which is ToInt32 for NaN (zero) and
    /// for every truncation strictly inside `i32`; a result at either bound may have
    /// saturated, so it exits at `pc` for the general path.
    fn int32(&mut self, pc: u32, value: Typed) -> ir::Value {
        if value.representation == Representation::Int32 {
            return value.value;
        }
        let integer = self.builder.ins().fcvt_to_sint_sat(types::I32, value.value);
        // `integer - (MIN + 1)` wraps both bounds, and only them, to the top two values.
        let shifted = self.builder.ins().iadd_imm_s(integer, -(i64::from(i32::MIN) + 1));
        let bound = self
            .builder
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
        let (result, overflow) = match operator {
            FloatOperator::Add => self.builder.ins().sadd_overflow(left, right),
            FloatOperator::Subtract => self.builder.ins().ssub_overflow(left, right),
            FloatOperator::Multiply => {
                let (product, overflow) = self.builder.ins().smul_overflow(left, right);
                self.guard_int32(pc, overflow, Widening::Register(register));
                // A zero product is -0 when a factor is negative; the signs are tested only
                // for a zero product, off the common path.
                let zero = self.builder.ins().icmp_imm_u(IntCC::Equal, product, 0);
                let signs_block = self.cold_block();
                let done = self.builder.create_block();
                self.builder.ins().brif(zero, signs_block, &[], done, &[]);
                self.builder.switch_to_block(signs_block);
                let signs = self.builder.ins().bor(left, right);
                let negative = self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                self.guard_int32(pc, negative, Widening::Register(register));
                self.builder.ins().jump(done, &[]);
                self.builder.switch_to_block(done);
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
                let ins = self.builder.ins();
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
                let ins = self.builder.ins();
                // Shift counts are taken modulo 32, as Cranelift's shifts take them.
                let value = match operator {
                    Int32Operator::Or => ins.bor(left, right),
                    Int32Operator::And => ins.band(left, right),
                    Int32Operator::Xor => ins.bxor(left, right),
                    Int32Operator::ShiftLeft => ins.ishl(left, right),
                    Int32Operator::ShiftRight => ins.sshr(left, right),
                    Int32Operator::ShiftRightUnsigned => {
                        let shifted = ins.ushr(left, right);
                        let value = self.builder.ins().fcvt_from_uint(types::F64, shifted);
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
                let operator = arithmetic(instruction.binary_operator())?;
                let left = self.operand(instruction.operand_b())?;
                let right = self.operand(instruction.operand_c())?;
                let result = instruction.result_register();
                let value = self.arithmetic(pc, operator, result, left, right);
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
                            self.builder.ins().icmp_imm_s(IntCC::Equal, operand.value, i64::from(limit));
                        self.guard_int32(pc, overflow, Widening::Register(result));
                    }
                    let value = self.builder.ins().iadd_imm_s(operand.value, step);
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
                        let zero = self.builder.ins().icmp_imm_u(IntCC::Equal, operand.value, 0);
                        let minimum =
                            self.builder.ins().icmp_imm_s(IntCC::Equal, operand.value, i64::from(i32::MIN));
                        let failed = self.builder.ins().bor(zero, minimum);
                        self.guard_int32(pc, failed, Widening::Register(result));
                        let value = self.builder.ins().ineg(operand.value);
                        Typed { value, representation: Representation::Int32 }
                    }
                    Unary::Negate => {
                        let number = self.float(operand);
                        let value = self.builder.ins().fneg(number);
                        Typed { value, representation: Representation::Float64 }
                    }
                    Unary::BitwiseNot => {
                        let integer = self.int32(pc, operand);
                        let value = self.builder.ins().bnot(integer);
                        Typed { value, representation: Representation::Int32 }
                    }
                };
                self.define(result, value);
                self.fall_through(pc)
            }
            Op::RequireObjectCoercible => self.fall_through(pc),
            Op::GetIndex => {
                let base = instruction.operand_b().register_index()?;
                let (elements, length) = self.arrays[&base];
                let key = self.operand(instruction.operand_c())?;
                let index = self.index(pc, key, length);
                let address = self.element_address(elements, index);
                let bits = self.builder.ins().load(types::I64, flags(), address, 0);
                // A hole is tagged, so `unbox` sends it to the general path too.
                let representation = element_representation(&instruction, self.observation);
                let exit = match representation {
                    Representation::Int32 => self.widen_block(pc, Widening::Elements(base)),
                    Representation::Float64 => self.exit_block(pc),
                };
                let value = self.unbox(bits, representation, exit);
                self.define(instruction.result_register(), Typed { value, representation });
                self.fall_through(pc)
            }
            Op::SetIndex => {
                let (elements, length) = self.arrays[&instruction.register_b()];
                let key = self.register(instruction.register_c());
                let index = self.index(pc, key, length);
                let address = self.element_address(elements, index);
                let old = self.builder.ins().load(types::I64, flags(), address, 0);
                let tag = self.builder.ins().band_imm_u(old, ValueLayout::TAG_MASK as i64);
                let hole = self.builder.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::DELETED_TAG as i64);
                self.guard(pc, hole);
                let value = self.register(instruction.register_a());
                let bits = self.boxed(value);
                self.builder.ins().store(flags(), bits, address, 0);
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
                    self.builder.ins().icmp(condition, left.value, right.value)
                } else {
                    let condition = comparison(operator)?;
                    let (left, right) = (self.float(left), self.float(right));
                    self.builder.ins().fcmp(condition, left, right)
                };
                let taken = self.builder.create_block();
                let not_taken = self.builder.create_block();
                self.builder.ins().brif(holds, not_taken, &[], taken, &[]);
                self.builder.switch_to_block(taken);
                self.jump(pc, instruction.jump_target())?;
                self.builder.switch_to_block(not_taken);
                self.fall_through(pc)
            }
            _ => None,
        }
    }
}

/// `comparison` of two `i32` Numbers.
fn integer_comparison(operator: u32) -> Option<IntCC> {
    Some(match comparison(operator)? {
        FloatCC::LessThan => IntCC::SignedLessThan,
        FloatCC::LessThanOrEqual => IntCC::SignedLessThanOrEqual,
        FloatCC::GreaterThan => IntCC::SignedGreaterThan,
        FloatCC::GreaterThanOrEqual => IntCC::SignedGreaterThanOrEqual,
        FloatCC::Equal => IntCC::Equal,
        _ => IntCC::NotEqual,
    })
}
