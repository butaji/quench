use super::*;

/// The row kind of a binary operator, from one table for arithmetic, bitwise and relational
/// operators.
pub(super) const OPERATORS: [(BinaryOperator, Kind); 18] = [
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

pub(super) fn kind_of(operator: u32, relational: bool) -> Option<Kind> {
    let found = OPERATORS.iter().find(|(candidate, _)| *candidate as u32 == operator)?.1;
    (relational == (found >= Kind::Lt)).then_some(found)
}

/// An operand: a register of the activation or a constant `Value` word.
#[derive(Clone, Copy)]
pub(super) enum Source {
    Register(Register),
    Constant(u64),
}

/// An instruction as a row: its kind, destination, operands and branch target.
pub(super) struct Decoded {
    pub(super) kind: Kind,
    pub(super) dst: Option<Register>,
    pub(super) a: Option<Source>,
    pub(super) b: Option<Source>,
    pub(super) target: Option<u32>,
    /// A heap row (`base_rows`) in place of the numeric `kind`, with the array register a store
    /// writes through and the entry-record byte offset a field read loads.
    pub(super) heap: Option<Heap>,
    pub(super) base: Option<Register>,
    pub(super) hole: u64,
    /// How an instruction no row takes runs (`steps`), in place of the numeric `kind`.
    pub(super) step: Option<Step>,
    /// A `ToNumeric` copy: its operand is no Number until the row proves it (a loop's entry guards
    /// prove the registers it names; a function's registers are not proved).
    pub(super) checked: bool,
    /// The callee register and function word a row stands on where it replaces the call of that
    /// function (`function::inlined_call`): the row proves the register holds it.
    pub(super) identity: Option<(Register, u64)>,
}

impl Decoded {
    pub(super) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, a, b, target, heap: None, base: None, hole: 0, step: None, checked: false, identity: None }
    }

    pub(super) fn heap(heap: Heap, dst: Option<Register>, a: Option<Source>, b: Option<Source>) -> Self {
        Self { heap: Some(heap), ..Self::number(Kind::Move, dst, a, b, None) }
    }

    pub(super) fn step(step: Step, dst: Option<Register>) -> Self {
        Self { step: Some(step), ..Self::number(Kind::Move, dst, None, None, None) }
    }

    /// The registers the row names.
    pub(super) fn registers(&self) -> impl Iterator<Item = Register> {
        [self.dst, self.a.and_then(register_of), self.b.and_then(register_of), self.base, self.identity.map(|(register, _)| register)].into_iter().flatten()
    }
}

/// The Number a function's constant table holds at an index: the caller's for a region's own
/// instructions, the callee's for an inlined body (`inline_plan`).
pub(super) type Numbers<'a> = &'a dyn Fn(usize) -> Option<f64>;

pub(super) fn source(numbers: Numbers, operand: crate::bytecode::Operand) -> Result<Source, String> {
    match operand.kind() {
        Some(OperandKind::Register) => Ok(Source::Register(operand.payload())),
        Some(OperandKind::Constant) => numbers(usize::from(operand.payload()))
            .map(|number| Source::Constant(Value::number(number).to_bits()))
            .ok_or_else(|| "operand: constant that is no Number".to_string()),
        other => Err(format!("operand: {other:?}")),
    }
}

pub(super) fn binary(numbers: Numbers, instruction: WideInstruction) -> Result<Decoded, String> {
    let operator = instruction.binary_operator();
    let kind = kind_of(operator, false).ok_or_else(|| format!("{:?}:{operator}", instruction.op()))?;
    Ok(Decoded::number(
        kind,
        Some(instruction.result_register()),
        Some(source(numbers, instruction.operand_b())?),
        Some(source(numbers, instruction.operand_c())?),
        None,
    ))
}

pub(super) fn jump_binary(numbers: Numbers, instruction: WideInstruction) -> Result<Decoded, String> {
    let operator = instruction.binary_operator_field();
    let kind = kind_of(operator, true).ok_or_else(|| format!("{:?}:{operator}", instruction.op()))?;
    Ok(Decoded::number(
        kind,
        None,
        Some(source(numbers, instruction.operand_b())?),
        Some(source(numbers, instruction.operand_c())?),
        Some(instruction.jump_target()),
    ))
}

pub(super) fn load_const(numbers: Numbers, instruction: WideInstruction) -> Result<Decoded, String> {
    let number = numbers(instruction.constant_index()).ok_or_else(|| "LoadConst: not a Number".to_string())?;
    let word = Source::Constant(Value::number(number).to_bits());
    Ok(Decoded::number(Kind::Move, Some(instruction.result_register()), Some(word), None, None))
}

pub(super) fn inc_dec(instruction: WideInstruction) -> Result<Decoded, String> {
    let decrement = instruction.boolean_flag().ok_or("IncDec")?;
    Ok(Decoded::number(
        if decrement { Kind::Sub } else { Kind::Add },
        Some(instruction.result_register()),
        Some(Source::Register(instruction.register_b())),
        Some(Source::Constant(Value::integer(1).to_bits())),
        None,
    ))
}

/// Where a heap row's recorded facts come from: a loop region's entry record (its arrays read
/// through it where the views are `dense`), or nowhere, as a whole function's rows read the heap
/// by its tables (an array word's record, a field's cache entry).
#[derive(Clone, Copy)]
pub(super) enum Facts {
    Entry { dense: bool },
    Site,
}

/// The entry-record byte offset of the array `register` holds through the region, which an
/// indexing row reads the backing of from the record; 0 where the array is looked up per access.
fn entry_array(region: &LoopRegion, facts: Facts, register: Register) -> u64 {
    // A row's hole is a u32 patch: an offset past it is looked up per access instead.
    match facts {
        Facts::Entry { dense: true } => region.array_entry_offset(register).filter(|offset| u32::try_from(*offset).is_ok()).unwrap_or(0),
        Facts::Entry { dense: false } | Facts::Site => 0,
    }
}

/// The row of a field read: the value the loop's entry recorded, or the site's cache entry
/// (a named field of a register, not of `this`, whose receiver arrives outside the registers).
fn field_row(region: &LoopRegion, facts: Facts, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    let result = Some(instruction.result_register());
    if matches!(facts, Facts::Site) {
        let FieldLookup::Atom { base, cache_site, .. } = instruction.field_lookup()? else { return None };
        let site = u64::from(cache_site) * crate::vm::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64;
        return Some(Decoded { hole: site, ..Decoded::heap(Heap::Site, result, Some(Source::Register(base.register_index()?)), None) });
    }
    let index = *region.field_reads().get(&(pc - region.header))?;
    let word = super::super::osr_loop::field_word(region, index);
    let row = Decoded::heap(Heap::Field, result, None, None);
    (!region.is_dynamic_field(index) && u32::try_from(word as u64 * SLOT).is_ok()).then_some(Decoded { hole: word as u64 * SLOT, ..row })
}

/// The heap instructions: a capture, a dense element read or store, a recorded or cached field,
/// the proof that a base is an array, the activation's `this` and its local slots.
pub(super) fn heap_row(region: &LoopRegion, facts: Facts, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    let register = |register| Some(Source::Register(register));
    match instruction.op().class() {
        Class::LoadCapture if instruction.capture_depth() == 0 => {
            Some(Decoded::heap(Heap::Capture(instruction.capture_slot()), Some(instruction.result_register()), None, None))
        }
        Class::GetIndex if !instruction.returns_from_frame() => {
            let base = instruction.operand_b().register_index()?;
            let index = source(&|index| region.number_constant(index), instruction.operand_c()).ok()?;
            let row = Decoded::heap(Heap::Index, Some(instruction.result_register()), register(base), Some(index));
            Some(Decoded { hole: entry_array(region, facts, base), ..row })
        }
        Class::SetIndex => {
            let value = register(instruction.register_a());
            let row = Decoded::heap(Heap::Store, None, value, register(instruction.register_c()));
            Some(Decoded { base: Some(instruction.register_b()), hole: entry_array(region, facts, instruction.register_b()), ..row })
        }
        Class::RequireObjectCoercible => Some(Decoded::heap(Heap::Array, None, register(instruction.register_b()), None)),
        Class::LoadThis if !instruction.writes_current_this() => Some(Decoded::heap(Heap::This, Some(instruction.result_register()), None, None)),
        Class::LoadLocalPlain => {
            let slot = u16::try_from(instruction.local_slot()).ok()?;
            Some(Decoded::heap(Heap::LoadLocal(slot), Some(instruction.result_register()), None, None))
        }
        // A store that also defines a register (`optional_register_b`) is the interpreter's.
        Class::StoreLocalPlain if instruction.optional_register_b().is_none() => {
            let slot = u16::try_from(instruction.local_slot()).ok()?;
            Some(Decoded::heap(Heap::StoreLocal(slot), None, register(instruction.register_a()), None))
        }
        Class::GetField if !instruction.returns_from_frame() && !instruction.writes_current_this() => field_row(region, facts, pc, instruction),
        _ => None,
    }
}

/// The row of `instruction` at `pc`: a numeric or heap row where one takes it, else the step the
/// interpreter's own operation makes (`steps`), so no instruction is refused.
pub(super) fn decode(region: &LoopRegion, facts: Facts, pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    numeric_row(&|index| region.number_constant(index), instruction)
        .flatten()
        .or_else(|| heap_row(region, facts, pc, instruction).filter(|_| instruction.result_flags() == 0))
        .unwrap_or_else(|| decode_step(pc, instruction, helpers))
}

/// The row of a numeric instruction: `None` when the instruction is not one, `Some(None)` when it
/// is one a row cannot take.
pub(super) fn numeric_row(numbers: Numbers, instruction: WideInstruction) -> Option<Option<Decoded>> {
    let copy = || Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None);
    let plain = instruction.result_flags() == 0;
    Some(match instruction.op().class() {
        Class::Move => plain.then(copy),
        // A Number operand is its own numeric value: the loop's entry guards and its own rows prove
        // it, and a function's row proves it itself (`checked`).
        Class::ToNumeric => plain.then(|| Decoded { checked: true, ..copy() }),
        Class::LoadConst => load_const(numbers, instruction).ok().filter(|_| plain),
        Class::Binary | Class::NumericAdd | Class::NumericMultiply => binary(numbers, instruction).ok().filter(|_| plain),
        Class::IncDec => inc_dec(instruction).ok().filter(|_| plain),
        Class::JumpBinaryFalse => jump_binary(numbers, instruction).ok(),
        Class::Jump => Some(Decoded::number(Kind::Jump, None, None, None, Some(instruction.jump_target()))),
        Class::JumpTrue | Class::JumpFalse => Some(truthiness_jump(instruction)),
        Class::Unary => unary_row(instruction).filter(|_| plain),
        _ => return None,
    })
}

/// A jump on a register's truthiness: taken where it is truthy (`JumpTrue`) or falsy.
fn truthiness_jump(instruction: WideInstruction) -> Decoded {
    let kind = if instruction.op().class() == Class::JumpTrue { Kind::JumpTrue } else { Kind::JumpFalse };
    Decoded::number(kind, None, Some(Source::Register(instruction.register_a())), None, Some(instruction.jump_target()))
}

/// The row of a unary operator over a register: `!` is the boolean of its truthiness, `-` the
/// negation of a Number, `+` a Number as it is (proved, as `ToNumeric` is); any other operator is
/// the interpreter's.
fn unary_row(instruction: WideInstruction) -> Option<Decoded> {
    let (result, operand) = (instruction.result_register(), Some(Source::Register(instruction.register_b())));
    let operator = instruction.unary_operator();
    let row = |kind| Decoded::number(kind, Some(result), operand, None, None);
    [
        (UnaryOperator::LogicalNot, row(Kind::Not)),
        (UnaryOperator::UnaryNegation, row(Kind::Neg)),
        (UnaryOperator::UnaryPlus, Decoded { checked: true, ..row(Kind::Move) }),
    ].into_iter().find(|(candidate, _)| *candidate as u32 == operator).map(|(_, decoded)| decoded)
}

/// Bits in a word's high half.
pub(super) const HALF: u32 = 32;
/// What a compile observed of its registers: the ints and the doubles.
pub(super) const SEEN: usize = 2;
/// The high 32 bits of an int word.
pub(super) const INT_HIGH: u64 = crate::value::ValueLayout::INT_TAG >> HALF;

pub(super) fn register_of(source: Source) -> Option<Register> {
    match source {
        Source::Register(register) => Some(register),
        Source::Constant(_) => None,
    }
}
