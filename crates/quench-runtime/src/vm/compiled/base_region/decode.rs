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
    /// The inline-cache entries of a field access (`field_entry`), which `plan` makes a chain of rows.
    pub(super) field_entry: Option<Cache>,
}

impl Decoded {
    pub(super) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, a, b, target, heap: None, base: None, hole: 0, field_entry: None }
    }

    pub(super) fn heap(heap: Heap, dst: Option<Register>, a: Option<Source>, b: Option<Source>) -> Self {
        Self { heap: Some(heap), ..Self::number(Kind::Move, dst, a, b, None) }
    }

    /// The registers the row names.
    pub(super) fn registers(&self) -> impl Iterator<Item = Register> {
        [self.dst, self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
    }
}

pub(super) fn source(region: &LoopRegion, operand: crate::bytecode::Operand) -> Result<Source, String> {
    match operand.kind() {
        Some(OperandKind::Register) => Ok(Source::Register(operand.payload())),
        Some(OperandKind::Constant) => region
            .number_constant(usize::from(operand.payload()))
            .map(|number| Source::Constant(Value::number(number).to_bits()))
            .ok_or_else(|| "operand: constant that is no Number".to_string()),
        other => Err(format!("operand: {other:?}")),
    }
}

pub(super) fn binary(region: &LoopRegion, instruction: WideInstruction) -> Result<Decoded, String> {
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

pub(super) fn jump_binary(region: &LoopRegion, instruction: WideInstruction) -> Result<Decoded, String> {
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

pub(super) fn load_const(region: &LoopRegion, instruction: WideInstruction) -> Result<Decoded, String> {
    let number = region.number_constant(instruction.constant_index()).ok_or_else(|| "LoadConst: not a Number".to_string())?;
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

/// The heap instructions: a capture, a dense element read or store, a recorded field, the proof
/// that a base is an array.
/// The entry-record byte offset of the array `register` holds through the region, which an
/// indexing row reads the backing of from the record; 0 where the array is looked up per access.
fn entry_array(region: &LoopRegion, dense: bool, register: Register) -> u64 {
    // A row's hole is a u32 patch: an offset past it is looked up per access instead.
    if dense { region.array_entry_offset(register).filter(|offset| u32::try_from(*offset).is_ok()).unwrap_or(0) } else { 0 }
}

pub(super) fn heap_row(region: &LoopRegion, dense: bool, pc: u32, instruction: WideInstruction, loads: &[Vec<NativeFieldLoad>]) -> Option<Decoded> {
    let register = |register| Some(Source::Register(register));
    match instruction.op().class() {
        Class::LoadCapture if instruction.capture_depth() == 0 => {
            Some(Decoded::heap(Heap::Capture(instruction.capture_slot()), Some(instruction.result_register()), None, None))
        }
        Class::GetIndex if !instruction.returns_from_frame() => {
            let base = instruction.operand_b().register_index()?;
            let index = source(region, instruction.operand_c()).ok()?;
            let row = Decoded::heap(Heap::Index, Some(instruction.result_register()), register(base), Some(index));
            Some(Decoded { hole: entry_array(region, dense, base), ..row })
        }
        Class::SetIndex => {
            let value = register(instruction.register_a());
            let row = Decoded::heap(Heap::Store, None, value, register(instruction.register_c()));
            Some(Decoded { base: Some(instruction.register_b()), hole: entry_array(region, dense, instruction.register_b()), ..row })
        }
        Class::RequireObjectCoercible => Some(Decoded::heap(Heap::Array, None, register(instruction.register_b()), None)),
        Class::GetField if !instruction.returns_from_frame() && !instruction.writes_current_this() => {
            let index = *region.field_reads().get(&(pc - region.header))?;
            if region.is_dynamic_field(index) {
                return cached_get(instruction, loads.get(index)?);
            }
            let word = super::super::osr_loop::field_word(region, index);
            let row = Decoded::heap(Heap::Field, Some(instruction.result_register()), None, None);
            u32::try_from(word as u64 * SLOT).is_ok().then_some(Decoded { hole: word as u64 * SLOT, ..row })
        }
        _ if instruction.op().is_field_store() => cached_set(region, instruction, loads),
        _ => None,
    }
}

pub(super) fn decode(region: &LoopRegion, dense: bool, pc: u32, instruction: WideInstruction, loads: &[Vec<NativeFieldLoad>]) -> Result<Decoded, String> {
    let decoded = match numeric_row(region, instruction) {
        Some(decoded) => decoded,
        None => heap_row(region, dense, pc, instruction, loads).filter(|_| instruction.result_flags() == 0),
    };
    decoded.ok_or_else(|| reject(region, instruction))
}

/// The row of a numeric instruction: `None` when the instruction is not one, `Some(None)` when it
/// is one a row cannot take.
fn numeric_row(region: &LoopRegion, instruction: WideInstruction) -> Option<Option<Decoded>> {
    let copy = || Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None);
    let plain = instruction.result_flags() == 0;
    Some(match instruction.op().class() {
        Class::Move => plain.then(copy),
        // A Number operand (the entry guards and the region's own rows prove it) is its own numeric value.
        Class::ToNumeric => plain.then(copy),
        Class::LoadConst => load_const(region, instruction).ok().filter(|_| plain),
        Class::Binary | Class::NumericAdd | Class::NumericMultiply => binary(region, instruction).ok().filter(|_| plain),
        Class::IncDec => inc_dec(instruction).ok().filter(|_| plain),
        Class::JumpBinaryFalse => jump_binary(region, instruction).ok(),
        Class::Jump => Some(Decoded::number(Kind::Jump, None, None, None, Some(instruction.jump_target()))),
        _ => return None,
    })
}

/// Why no row takes `instruction`.
fn reject(region: &LoopRegion, instruction: WideInstruction) -> String {
    let op = instruction.op();
    let fallback = |error: Option<String>| error.unwrap_or_else(|| format!("{op:?}"));
    match op.class() {
        Class::GetField => "GetField: unrecorded, or no flat entry the cache rows take".to_string(),
        Class::Binary | Class::NumericAdd | Class::NumericMultiply => fallback(binary(region, instruction).err()),
        Class::JumpBinaryFalse => fallback(jump_binary(region, instruction).err()),
        Class::LoadConst => fallback(load_const(region, instruction).err()),
        _ => fallback(None),
    }
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
