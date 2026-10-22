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
pub(super) enum LoopOutcome {
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
pub(super) enum Widening {
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
pub(super) struct ArrayUse {
    pub(super) register: Register,
    pub(super) written: bool,
}

/// A loop's admissible region and the facts its entry needs.
pub(super) struct LoopRegion {
    pub(super) header: u32,
    pub(super) backedge: u32,
    /// The Number registers, in variable order.
    numbers: Vec<Register>,
    /// The arrays, in entry-record order.
    pub(super) arrays: Vec<ArrayUse>,
    /// The depth-0 capture slots `LoadCapture` reads, in entry-record order.
    pub(super) captures: Vec<u16>,
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
pub(super) struct LoopEntry {
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
    pub(super) fn new(interrupt: &std::sync::atomic::AtomicBool) -> Self {
        let mut words = [0; ENTRY_WORDS];
        words[ENTRY_INTERRUPT] = std::ptr::from_ref(interrupt).addr() as u64;
        Self { words, length: ENTRY_CAPTURES }
    }

    fn push(&mut self, word: u64) {
        self.words[self.length] = word;
        self.length += 1;
    }

    /// Captures come first, in the region's order.
    pub(super) fn push_capture(&mut self, value: Value) {
        self.push(value.to_bits());
    }

    /// The first `count` capture values, as pushed.
    pub(super) fn captures(&self, count: usize) -> Vec<Value> {
        self.words[ENTRY_CAPTURES..ENTRY_CAPTURES + count].iter().map(|bits| Value::from_bits(*bits)).collect()
    }

    /// Arrays follow the captures, in the region's order. An array longer than
    /// `MAX_INDEXED_LENGTH` declines the entry.
    pub(super) fn push_array(&mut self, elements: *mut Value, length: usize) -> Option<()> {
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
pub(super) struct Observation<'a> {
    pub(super) register: &'a dyn Fn(Register) -> Value,
    /// Whether the array in a register looks to hold `i32` elements.
    pub(super) int32_elements: &'a dyn Fn(Register) -> bool,
    pub(super) captures: &'a [Value],
    pub(super) widened: &'a std::collections::BTreeSet<Widening>,
}

fn int32_number(number: f64) -> bool {
    Value::number(number).as_int().is_some()
}

/// Each Number register's and capture's representation: an `i32` where the observed value
/// is one, no widening names it, and every definition in the region yields an `i32` from
/// `i32` operands; the least fixpoint from that start, widening a register any definition
/// gives an `f64`.
fn representations(
    region: &LoopRegion,
    observation: &Observation<'_>,
) -> (
    std::collections::BTreeMap<Register, Representation>,
    Vec<Representation>,
) {
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
    let mut registers: std::collections::BTreeMap<Register, Representation> = region
        .numbers
        .iter()
        .map(|register| (*register, of((observation.register)(*register), Widening::Register(*register))))
        .collect();
    loop {
        let mut changed = false;
        for instruction in &region.instructions {
            let Some((register, representation)) =
                definition(region, instruction, &registers, &captures, observation)
            else {
                continue;
            };
            if representation == Representation::Float64
                && registers.get(&register) == Some(&Representation::Int32)
            {
                registers.insert(register, Representation::Float64);
                changed = true;
            }
        }
        if !changed {
            return (registers, captures);
        }
    }
}

/// The register an instruction defines and the representation its result has.
fn definition(
    region: &LoopRegion,
    instruction: &WideInstruction,
    registers: &std::collections::BTreeMap<Register, Representation>,
    captures: &[Representation],
    observation: &Observation<'_>,
) -> Option<(Register, Representation)> {
    let source = |operand: Operand| match operand.kind() {
        Some(OperandKind::Register) => registers
            .get(&operand.payload())
            .copied()
            .unwrap_or(Representation::Float64),
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
        Op::Move | Op::IncDec | Op::ToNumeric => registers
            .get(&instruction.register_b())
            .copied()
            .unwrap_or(Representation::Float64),
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
            Unary::Plus | Unary::Negate => registers
                .get(&instruction.register_b())
                .copied()
                .unwrap_or(Representation::Float64),
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
    let (registers, captures) = representations(region, observation);
    let mut context = FunctionBuilderContext::new();
    let builder = FunctionBuilder::new(&mut native, &mut context);
    let widenings = Lowering::new(builder, region, observation, pointer, registers, captures)
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
    representations: std::collections::BTreeMap<Register, Representation>,
    variables: std::collections::BTreeMap<Register, Variable>,
    /// Each capture's value, unboxed at entry, in region order.
    captures: Vec<Typed>,
    capture_representations: Vec<Representation>,
    blocks: Vec<ir::Block>,
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
        representations: std::collections::BTreeMap<Register, Representation>,
        capture_representations: Vec<Representation>,
    ) -> Self {
        let start = builder.create_block();
        builder.append_block_params_for_function_params(start);
        builder.switch_to_block(start);
        let (registers, entry) = (builder.block_params(start)[0], builder.block_params(start)[1]);
        let blocks = (region.header..=region.backedge).map(|_| builder.create_block()).collect();
        let variables = representations
            .iter()
            .map(|(register, representation)| (*register, builder.declare_var(machine_type(*representation))))
            .collect();
        Self {
            builder,
            region,
            observation,
            pointer,
            representations,
            variables,
            captures: Vec::new(),
            capture_representations,
            blocks,
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
            self.instruction(pc)?;
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
        let registers: Vec<(Register, Representation)> =
            self.representations.iter().map(|(register, representation)| (*register, *representation)).collect();
        for (register, representation) in registers {
            let bits = self.builder.ins().load(
                types::I64,
                flags(),
                self.registers,
                i32::from(register) * WORD_BYTES,
            );
            let otherwise = self.not_entered(Some(Widening::Register(register)));
            let value = self.unbox(bits, representation, otherwise);
            self.builder.def_var(self.variables[&register], value);
        }
        let header = self.block(self.region.header)?;
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
            let registers: Vec<Register> = self
                .representations
                .keys()
                .copied()
                .filter(|register| self.region.is_live(pc, *register))
                .collect();
            self.store_registers(&registers);
            self.builder.ins().return_(&[word]);
        }
    }

    fn store_registers(&mut self, registers: &[Register]) {
        for &register in registers {
            let value = self.register(register);
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

    fn register(&mut self, register: Register) -> Typed {
        Typed {
            value: self.builder.use_var(self.variables[&register]),
            representation: self.representations[&register],
        }
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

    /// Writes `value` to `register` in the register's representation; an `i32` result only
    /// reaches an `i32` register (`representations`).
    fn define(&mut self, register: Register, value: Typed) {
        let value = match (self.representations[&register], value.representation) {
            (Representation::Float64, Representation::Int32) => self.float(value),
            _ => value.value,
        };
        self.builder.def_var(self.variables[&register], value);
    }

    /// Continues at `target`: its block within the region, else the lane resumes there. A
    /// backward jump polls termination first.
    fn jump(&mut self, pc: u32, target: u32) -> Option<()> {
        if target <= pc {
            let flag = self.interrupt?;
            let requested = self.builder.ins().atomic_load(types::I8, flags(), flag);
            self.guard(pc, requested);
        }
        match self.block(target) {
            Some(block) => {
                self.builder.ins().jump(block, &[]);
            }
            None => self.leave(OUTCOME_RESUME, target),
        }
        Some(())
    }

    fn fall_through(&mut self, pc: u32) -> Option<()> {
        match self.block(pc + 1) {
            Some(block) => {
                self.builder.ins().jump(block, &[]);
            }
            None => self.leave(OUTCOME_RESUME, pc + 1),
        }
        Some(())
    }

    /// The element index `key` names in an array of `length` elements, exiting at `pc`
    /// unless it is an integral Number below the length.
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
        let beyond = self.builder.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, index, length);
        self.guard(pc, beyond);
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

    /// ToInt32 of `value`, exiting at `pc` unless a Float's truncation fits `i32` (NaN's is
    /// zero).
    fn int32(&mut self, pc: u32, value: Typed) -> ir::Value {
        if value.representation == Representation::Int32 {
            return value.value;
        }
        let number = value.value;
        let low = self.builder.ins().f64const(f64::from(i32::MIN));
        let high = self.builder.ins().f64const(-f64::from(i32::MIN));
        let above = self.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, number, low);
        let below = self.builder.ins().fcmp(FloatCC::LessThan, number, high);
        let nan = self.builder.ins().fcmp(FloatCC::Unordered, number, number);
        let inside = self.builder.ins().band(above, below);
        let valid = self.builder.ins().bor(inside, nan);
        let invalid = self.builder.ins().bxor_imm_u(valid, 1);
        self.guard(pc, invalid);
        self.builder.ins().fcvt_to_sint_sat(types::I32, number)
    }

    /// `left OP right` as `i32` arithmetic into `register`, exiting at `pc` when the exact
    /// result is no `i32` (an overflow, or the -0 of a zero product with a negative factor).
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
                let zero = self.builder.ins().icmp_imm_u(IntCC::Equal, product, 0);
                let signs = self.builder.ins().bor(left, right);
                let negative = self.builder.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
                let negative_zero = self.builder.ins().band(zero, negative);
                let failed = self.builder.ins().bor(overflow, negative_zero);
                (product, failed)
            }
            FloatOperator::Divide => unreachable!("division has a Float64 result"),
        };
        self.guard_int32(pc, overflow, Widening::Register(register));
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
                if self.representations[&result] == Representation::Int32
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
                    && self.representations[&result] == Representation::Int32
                {
                    // Only the extreme the step leaves `i32` from overflows.
                    let (limit, step) = if decrement { (i32::MIN, -1) } else { (i32::MAX, 1) };
                    let overflow = self.builder.ins().icmp_imm_s(IntCC::Equal, operand.value, i64::from(limit));
                    self.guard_int32(pc, overflow, Widening::Register(result));
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
                            && self.representations[&result] == Representation::Int32 =>
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
