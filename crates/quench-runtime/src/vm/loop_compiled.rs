//! The loop tier: Cranelift code for a hot loop whose body makes no call and allocates
//! nothing, entered at its backward jump in compiled execution.
//!
//! A region is a loop's instructions, header to backward jump, every one of which reads and
//! writes Numbers in registers, depth-0 captures and present dense elements of record arrays.
//! Native code keeps each Number register in an `f64` SSA value. Registers are the
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
use oxc_ast::ast::BinaryOperator;

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
    NotEntered,
    /// Control left the region by a jump to this PC, which the lane runs next.
    Resume(u32),
    /// The instruction at this PC needs the general path, which executes it.
    Exit(u32),
}

const OUTCOME_NOT_ENTERED: u64 = 0;
const OUTCOME_RESUME: u64 = 1;
const OUTCOME_EXIT: u64 = 2;

impl LoopOutcome {
    fn decode(word: u64) -> Self {
        let pc = (word & PC_MASK) as u32;
        match word >> OUTCOME_SHIFT {
            OUTCOME_RESUME => Self::Resume(pc),
            OUTCOME_EXIT => Self::Exit(pc),
            _ => Self::NotEntered,
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
#[derive(Debug)]
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
}

/// The entry record native code reads: the termination flag, then each capture's value,
/// then each array's element pointer and length.
pub(super) struct LoopEntry {
    words: [u64; ENTRY_WORDS],
    length: usize,
}

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

    /// Arrays follow the captures, in the region's order.
    pub(super) fn push_array(&mut self, elements: *mut Value, length: usize) {
        self.push(elements.addr() as u64);
        self.push(length as u64);
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
        Op::IncDec => {
            instruction.boolean_flag()?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
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

/// A compiled region: one W^X mapping and its entry.
pub(super) struct CompiledLoop {
    _code: super::compiled::CodeMemory,
    entry: unsafe extern "C" fn(*mut Value, *const u64) -> u64,
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
}

/// Native code for `region`, or `None` when Cranelift declines it.
pub(super) fn compile(region: &LoopRegion) -> Option<CompiledLoop> {
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
    let mut context = FunctionBuilderContext::new();
    let builder = FunctionBuilder::new(&mut native, &mut context);
    Lowering::new(builder, region, pointer).lower(isa.frontend_config())?;
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
    Some(CompiledLoop { _code: memory, entry })
}

/// The Cranelift user-function namespace of loop regions ("QLOP").
const LOOP_NAMESPACE: u32 = 0x514c_4f50;

struct Lowering<'a, 'b> {
    builder: FunctionBuilder<'b>,
    region: &'a LoopRegion,
    pointer: ir::Type,
    variables: std::collections::BTreeMap<Register, Variable>,
    blocks: Vec<ir::Block>,
    /// Writes every Number register back and returns its parameter, the outcome word.
    leave: ir::Block,
    registers: ir::Value,
    entry: ir::Value,
    /// Each array's element pointer and length, by register.
    arrays: std::collections::BTreeMap<Register, (ir::Value, ir::Value)>,
}

fn flags() -> ir::MemFlagsData {
    ir::MemFlagsData::trusted()
}

impl<'a, 'b> Lowering<'a, 'b> {
    fn new(mut builder: FunctionBuilder<'b>, region: &'a LoopRegion, pointer: ir::Type) -> Self {
        let start = builder.create_block();
        builder.append_block_params_for_function_params(start);
        builder.switch_to_block(start);
        let (registers, entry) = (builder.block_params(start)[0], builder.block_params(start)[1]);
        let blocks = (region.header..=region.backedge).map(|_| builder.create_block()).collect();
        let leave = builder.create_block();
        builder.append_block_param(leave, types::I64);
        let variables = region
            .numbers
            .iter()
            .map(|register| (*register, builder.declare_var(types::F64)))
            .collect();
        Self {
            builder,
            region,
            pointer,
            variables,
            blocks,
            leave,
            registers,
            entry,
            arrays: std::collections::BTreeMap::new(),
        }
    }

    fn lower(mut self, frontend: cranelift_codegen::isa::TargetFrontendConfig) -> Option<()> {
        self.enter()?;
        for pc in self.region.header..=self.region.backedge {
            let block = self.block(pc)?;
            self.builder.switch_to_block(block);
            self.instruction(pc)?;
        }
        self.write_back();
        self.builder.seal_all_blocks();
        self.builder.finalize(frontend);
        Some(())
    }

    fn block(&self, pc: u32) -> Option<ir::Block> {
        (self.region.header..=self.region.backedge)
            .contains(&pc)
            .then(|| self.blocks[(pc - self.region.header) as usize])
    }

    fn outcome(&mut self, outcome: u64, pc: u32) -> ir::Value {
        self.builder
            .ins()
            .iconst(types::I64, ((outcome << OUTCOME_SHIFT) | u64::from(pc)) as i64)
    }

    fn word(&mut self, index: usize) -> ir::Value {
        self.builder
            .ins()
            .load(types::I64, flags(), self.entry, index as i32 * WORD_BYTES)
    }

    /// Unboxes every Number register, entering only when all hold Numbers.
    fn enter(&mut self) -> Option<()> {
        let not_entered = self.builder.create_block();
        for (index, array) in self.region.arrays.iter().enumerate() {
            let base = ENTRY_CAPTURES + self.region.captures.len() + ARRAY_WORDS * index;
            let elements = self.word(base);
            let length = self.word(base + 1);
            self.arrays.insert(array.register, (elements, length));
        }
        let registers: Vec<(Register, Variable)> =
            self.variables.iter().map(|(register, variable)| (*register, *variable)).collect();
        for (register, variable) in registers {
            let bits = self.builder.ins().load(
                types::I64,
                flags(),
                self.registers,
                i32::from(register) * WORD_BYTES,
            );
            let number = self.unbox(bits, not_entered);
            self.builder.def_var(variable, number);
        }
        let header = self.block(self.region.header)?;
        self.builder.ins().jump(header, &[]);
        self.builder.switch_to_block(not_entered);
        let word = self.outcome(OUTCOME_NOT_ENTERED, self.region.header);
        self.builder.ins().return_(&[word]);
        Some(())
    }

    /// The Number `bits` encode, branching to `otherwise` for any other value.
    fn unbox(&mut self, bits: ir::Value, otherwise: ir::Block) -> ir::Value {
        let ins = self.builder.ins();
        let tag = ins.band_imm_u(bits, ValueLayout::TAG_MASK as i64);
        let is_int = self.builder.ins().icmp_imm_u(IntCC::Equal, tag, ValueLayout::INT_TAG as i64);
        let integer_block = self.builder.create_block();
        let other_block = self.builder.create_block();
        let join = self.builder.create_block();
        self.builder.append_block_param(join, types::F64);
        self.builder.ins().brif(is_int, integer_block, &[], other_block, &[]);
        self.builder.switch_to_block(integer_block);
        let integer = self.builder.ins().ireduce(types::I32, bits);
        let number = self.builder.ins().fcvt_from_sint(types::F64, integer);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(number)]);
        self.builder.switch_to_block(other_block);
        // A tag field within the tag range marks a non-Number.
        let field = self.builder.ins().ushr_imm_u(bits, i64::from(ValueLayout::TAG_SHIFT));
        let offset = self.builder.ins().iadd_imm_s(field, -(ValueLayout::FIRST_TAG_FIELD as i64));
        let tagged = self
            .builder
            .ins()
            .icmp_imm_u(IntCC::UnsignedLessThanOrEqual, offset, ValueLayout::TAG_FIELD_SPAN as i64);
        let double_block = self.builder.create_block();
        self.builder.ins().brif(tagged, otherwise, &[], double_block, &[]);
        self.builder.switch_to_block(double_block);
        let double = self.builder.ins().bitcast(types::F64, ir::MemFlagsData::new(), bits);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(double)]);
        self.builder.switch_to_block(join);
        self.builder.block_params(join)[0]
    }

    /// `Value::number(number)`'s bits: an integer when the truncation converts back to the
    /// same bits, the canonical NaN for any NaN, else the double's bits.
    fn boxed(&mut self, number: ir::Value) -> ir::Value {
        let ins = self.builder.ins();
        let integer = ins.fcvt_to_sint_sat(types::I32, number);
        let back = self.builder.ins().fcvt_from_sint(types::F64, integer);
        let back_bits = self.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), back);
        let bits = self.builder.ins().bitcast(types::I64, ir::MemFlagsData::new(), number);
        let integral = self.builder.ins().icmp(IntCC::Equal, back_bits, bits);
        let payload = self.builder.ins().uextend(types::I64, integer);
        let encoded = self.builder.ins().bor_imm_u(payload, ValueLayout::INT_TAG as i64);
        let nan = self.builder.ins().fcmp(FloatCC::Unordered, number, number);
        let canonical = self.builder.ins().iconst(types::I64, ValueLayout::CANONICAL_NAN as i64);
        let other = self.builder.ins().select(nan, canonical, bits);
        self.builder.ins().select(integral, encoded, other)
    }

    fn write_back(&mut self) {
        self.builder.switch_to_block(self.leave);
        let word = self.builder.block_params(self.leave)[0];
        let registers: Vec<(Register, Variable)> =
            self.variables.iter().map(|(register, variable)| (*register, *variable)).collect();
        for (register, variable) in registers {
            let number = self.builder.use_var(variable);
            let bits = self.boxed(number);
            self.builder.ins().store(
                flags(),
                bits,
                self.registers,
                i32::from(register) * WORD_BYTES,
            );
        }
        self.builder.ins().return_(&[word]);
    }

    /// Leaves with `outcome` at `pc`.
    fn leave(&mut self, outcome: u64, pc: u32) {
        let word = self.outcome(outcome, pc);
        self.builder.ins().jump(self.leave, &[ir::BlockArg::Value(word)]);
    }

    /// A block that exits at `pc` for the general path.
    fn exit_block(&mut self, pc: u32) -> ir::Block {
        let current = self.builder.current_block().expect("lowering inside a block");
        let exit = self.builder.create_block();
        self.builder.switch_to_block(exit);
        self.leave(OUTCOME_EXIT, pc);
        self.builder.switch_to_block(current);
        exit
    }

    fn read(&mut self, source: Source) -> ir::Value {
        match source {
            Source::Register(register) => self.builder.use_var(self.variables[&register]),
            Source::Number(number) => self.builder.ins().f64const(number),
        }
    }

    fn register(&mut self, register: Register) -> ir::Value {
        self.read(Source::Register(register))
    }

    fn source(&self, operand: Operand) -> Option<Source> {
        match operand.kind()? {
            OperandKind::Register => Some(Source::Register(operand.payload())),
            OperandKind::Constant => Some(Source::Number(*self.region.constants.get(&operand.payload())?)),
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    fn define(&mut self, register: Register, value: ir::Value) {
        self.builder.def_var(self.variables[&register], value);
    }

    /// Continues at `target`: its block within the region, else the lane resumes there. A
    /// backward jump polls termination first.
    fn jump(&mut self, pc: u32, target: u32) -> Option<()> {
        if target <= pc {
            let flag = self.word(ENTRY_INTERRUPT);
            let requested = self.builder.ins().atomic_load(types::I8, flags(), flag);
            let continue_block = self.builder.create_block();
            let exit = self.exit_block(pc);
            self.builder.ins().brif(requested, exit, &[], continue_block, &[]);
            self.builder.switch_to_block(continue_block);
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
    fn index(&mut self, pc: u32, key: ir::Value, length: ir::Value) -> ir::Value {
        let exit = self.exit_block(pc);
        let index = self.builder.ins().fcvt_to_sint_sat(types::I64, key);
        let back = self.builder.ins().fcvt_from_sint(types::F64, index);
        let integral = self.builder.ins().fcmp(FloatCC::Equal, back, key);
        let below = self.builder.ins().icmp(IntCC::UnsignedLessThan, index, length);
        let valid = self.builder.ins().band(integral, below);
        let inside = self.builder.create_block();
        self.builder.ins().brif(valid, inside, &[], exit, &[]);
        self.builder.switch_to_block(inside);
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

    /// ToInt32 of `number`, exiting at `pc` unless its truncation fits `i32` (NaN's is zero).
    fn int32(&mut self, pc: u32, number: ir::Value) -> ir::Value {
        let exit = self.exit_block(pc);
        let low = self.builder.ins().f64const(f64::from(i32::MIN));
        let high = self.builder.ins().f64const(-f64::from(i32::MIN));
        let above = self.builder.ins().fcmp(FloatCC::GreaterThanOrEqual, number, low);
        let below = self.builder.ins().fcmp(FloatCC::LessThan, number, high);
        let nan = self.builder.ins().fcmp(FloatCC::Unordered, number, number);
        let inside = self.builder.ins().band(above, below);
        let valid = self.builder.ins().bor(inside, nan);
        let converted = self.builder.create_block();
        self.builder.ins().brif(valid, converted, &[], exit, &[]);
        self.builder.switch_to_block(converted);
        self.builder.ins().fcvt_to_sint_sat(types::I32, number)
    }

    fn arithmetic(&mut self, pc: u32, operator: Arithmetic, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Arithmetic::Float(operator) => {
                let ins = self.builder.ins();
                match operator {
                    FloatOperator::Add => ins.fadd(left, right),
                    FloatOperator::Subtract => ins.fsub(left, right),
                    FloatOperator::Multiply => ins.fmul(left, right),
                    FloatOperator::Divide => ins.fdiv(left, right),
                }
            }
            Arithmetic::Int32(operator) => {
                let left = self.int32(pc, left);
                let right = self.int32(pc, right);
                let ins = self.builder.ins();
                // Shift counts are taken modulo 32, as Cranelift's shifts take them.
                let result = match operator {
                    Int32Operator::Or => ins.bor(left, right),
                    Int32Operator::And => ins.band(left, right),
                    Int32Operator::Xor => ins.bxor(left, right),
                    Int32Operator::ShiftLeft => ins.ishl(left, right),
                    Int32Operator::ShiftRight => ins.sshr(left, right),
                    Int32Operator::ShiftRightUnsigned => {
                        let shifted = ins.ushr(left, right);
                        return self.builder.ins().fcvt_from_uint(types::F64, shifted);
                    }
                };
                self.builder.ins().fcvt_from_sint(types::F64, result)
            }
        }
    }

    fn instruction(&mut self, pc: u32) -> Option<()> {
        let instruction = self.region.instructions[(pc - self.region.header) as usize];
        match instruction.op() {
            Op::LoadConst => {
                let index = u16::try_from(instruction.constant_index()).ok()?;
                let number = *self.region.constants.get(&index)?;
                let value = self.builder.ins().f64const(number);
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::Move => {
                let value = self.register(instruction.register_b());
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::LoadCapture => {
                let slot = instruction.capture_slot();
                let index = self.region.captures.iter().position(|capture| *capture == slot)?;
                let bits = self.word(ENTRY_CAPTURES + index);
                let exit = self.exit_block(pc);
                let number = self.unbox(bits, exit);
                self.define(instruction.result_register(), number);
                self.fall_through(pc)
            }
            Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
                let operator = arithmetic(instruction.binary_operator())?;
                let left = self.source(instruction.operand_b())?;
                let right = self.source(instruction.operand_c())?;
                let (left, right) = (self.read(left), self.read(right));
                let value = self.arithmetic(pc, operator, left, right);
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::IncDec => {
                let operand = self.register(instruction.register_b());
                let one = self.builder.ins().f64const(1.0);
                let value = if instruction.boolean_flag()? {
                    self.builder.ins().fsub(operand, one)
                } else {
                    self.builder.ins().fadd(operand, one)
                };
                self.define(instruction.result_register(), value);
                self.fall_through(pc)
            }
            Op::GetIndex => {
                let base = instruction.operand_b().register_index()?;
                let (elements, length) = self.arrays[&base];
                let key = self.source(instruction.operand_c())?;
                let key = self.read(key);
                let index = self.index(pc, key, length);
                let address = self.element_address(elements, index);
                let bits = self.builder.ins().load(types::I64, flags(), address, 0);
                // A hole is tagged, so `unbox` sends it to the general path too.
                let exit = self.exit_block(pc);
                let number = self.unbox(bits, exit);
                self.define(instruction.result_register(), number);
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
                let exit = self.exit_block(pc);
                let store = self.builder.create_block();
                self.builder.ins().brif(hole, exit, &[], store, &[]);
                self.builder.switch_to_block(store);
                let value = self.register(instruction.register_a());
                let bits = self.boxed(value);
                self.builder.ins().store(flags(), bits, address, 0);
                self.fall_through(pc)
            }
            Op::Jump => self.jump(pc, instruction.jump_target()),
            Op::JumpBinaryFalse => {
                let condition = comparison(instruction.binary_operator_field())?;
                let left = self.source(instruction.operand_b())?;
                let right = self.source(instruction.operand_c())?;
                let (left, right) = (self.read(left), self.read(right));
                let holds = self.builder.ins().fcmp(condition, left, right);
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
