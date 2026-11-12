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

/// How a row ends a whole function's run of rows: it leaves at its PC with the registers
/// canonical (an instruction with no row), or completes the activation with the register it reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    Exit,
    Return,
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
    /// The atom a field store holds unnamed by method caches (`Known::stores`), a Stable fact.
    pub(super) atom: Option<Atom>,
    /// Set for a row that leaves the function: it has no successor.
    pub(super) end: Option<End>,
    /// A `ToNumeric` copy: its operand is no Number until the row proves it (a loop's entry guards
    /// prove the registers it names; a function's registers are not proved).
    pub(super) checked: bool,
}

impl Decoded {
    pub(super) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, a, b, target, heap: None, base: None, hole: 0, atom: None, end: None, checked: false }
    }

    /// A row that leaves the function (`End`), reading `a`.
    pub(super) fn ending(end: End, a: Option<Source>) -> Self {
        Self { end: Some(end), ..Self::number(Kind::Jump, None, a, None, None) }
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

/// The byte offset of the field cache entry of `site` in the program's table.
fn entry_offset(site: u16) -> u64 {
    u64::from(site) * crate::vm::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64
}

/// The row of a named field read of a register through the site's cache entry, or a nested
/// lookup's two (`FieldSite`: `base.first.second`, with no sink field): a read of `this` has
/// its receiver outside the registers.
fn site_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let (base, first, second) = match instruction.field_lookup()? {
        FieldLookup::Atom { base, cache_site, .. } => (base, cache_site, None),
        FieldLookup::Site(index) => {
            let site = known.program.field_sites.get(index).filter(|site| site.sink.is_none())?;
            (site.base, site.first.1, site.second.map(|(_, cache_site)| cache_site))
        }
    };
    let (heap, hole) = match second {
        Some(second) => (Heap::SiteChain, entry_offset(first) | entry_offset(second) << numeric::library::CHAIN_SECOND_AT),
        None => (Heap::Site, entry_offset(first)),
    };
    let object = Some(Source::Register(base.register_index()?));
    Some(Decoded { hole, ..Decoded::heap(heap, Some(instruction.result_register()), object, None) })
}

/// The row of a field the loop's entry recorded: its value, which the loop does not write (`Facts::Entry`).
fn recorded_field(region: &LoopRegion, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    let index = *region.field_reads().get(&(pc - region.header))?;
    let hole = super::super::osr_loop::field_word(region, index) as u64 * SLOT;
    let row = Decoded::heap(Heap::Field, Some(instruction.result_register()), None, None);
    (!region.is_dynamic_field(index) && u32::try_from(hole).is_ok()).then_some(Decoded { hole, ..row })
}

/// The row of a field read: an array's `length`, the value a loop's entry recorded, or the site's
/// cache entry (`site_row`), which also answers a loop's read its entry did not record, or that
/// reads through a register the loop redefines.
fn field_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    if let Some(FieldLookup::Atom { atom, base, .. }) = instruction.field_lookup()
        && known.names_length(atom)
    {
        let array = Some(Source::Register(base.register_index()?));
        return Some(Decoded::heap(Heap::Length, Some(instruction.result_register()), array, None));
    }
    match facts {
        Facts::Entry { .. } => recorded_field(region, pc, instruction).or_else(|| site_row(known, instruction)),
        Facts::Site => site_row(known, instruction),
    }
}

/// The row of a named field store to a register's object through the site's cache entry
/// (`entry::site_store`): a function's, where the store may run in place (`Known::stores_in_place`).
/// A loop region's reads may be entry snapshots of a field its stores write, so it has none.
fn field_store_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let atom = instruction.atom_index();
    if !known.stores_in_place(atom) || instruction.returns_from_frame() || instruction.writes_current_this() {
        return None;
    }
    let value = Some(Source::Register(instruction.register_a()));
    let row = Decoded::heap(Heap::SiteStore, None, value, None);
    Some(Decoded { base: Some(instruction.field_store_register()?), hole: entry_offset(instruction.cache_site_index()), atom: Some(atom), ..row })
}

/// Whether `instruction` stores to a depth-0 capture and nothing else (`general_capture::lower_capture`'s
/// conditions): it completes no activation, sets no `this`, and is no numeric local's update.
fn own_capture_store(instruction: WideInstruction) -> bool {
    instruction.capture_depth() == 0 && !instruction.returns_from_frame() && !instruction.writes_current_this() && instruction.numeric_local_target().is_none()
}

/// The row of a depth-0 capture access: a read, or a function's store (a loop region's payload
/// never admits direct capture stores).
fn capture_row(facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    match instruction.op().class() {
        Class::LoadCapture if instruction.capture_depth() == 0 => {
            Some(Decoded::heap(Heap::Capture(instruction.capture_slot()), Some(instruction.result_register()), None, None))
        }
        Class::StoreCapture if matches!(facts, Facts::Site) && own_capture_store(instruction) => {
            Some(Decoded::heap(Heap::CaptureSet(instruction.capture_slot()), None, Some(Source::Register(instruction.register_a())), None))
        }
        _ => None,
    }
}

/// The row of a dense element read or store, or of the proof that a base is an array.
fn array_row(region: &LoopRegion, facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    let register = |register| Some(Source::Register(register));
    match instruction.op().class() {
        Class::GetIndex if !instruction.returns_from_frame() => {
            let base = instruction.operand_b().register_index()?;
            let index = source(region, instruction.operand_c()).ok()?;
            let row = Decoded::heap(Heap::Index, Some(instruction.result_register()), register(base), Some(index));
            Some(Decoded { hole: entry_array(region, facts, base), ..row })
        }
        Class::SetIndex => {
            let value = register(instruction.register_a());
            let row = Decoded::heap(Heap::Store, None, value, register(instruction.register_c()));
            Some(Decoded { base: Some(instruction.register_b()), hole: entry_array(region, facts, instruction.register_b()), ..row })
        }
        Class::RequireObjectCoercible => Some(Decoded::heap(Heap::Array, None, register(instruction.register_b()), None)),
        _ => None,
    }
}

/// The heap instructions: a capture, a dense element read or store, a recorded or cached field
/// read or store, the proof that a base is an array.
pub(super) fn heap_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match instruction.op().class() {
        Class::LoadCapture | Class::StoreCapture => capture_row(facts, instruction),
        Class::GetIndex | Class::SetIndex | Class::RequireObjectCoercible => array_row(region, facts, instruction),
        Class::GetField if !instruction.returns_from_frame() && !instruction.writes_current_this() => field_row(region, facts, known, pc, instruction),
        _ if instruction.op().is_field_store() && matches!(facts, Facts::Site) => field_store_row(known, instruction),
        _ => None,
    }
}

pub(super) fn decode(region: &LoopRegion, (facts, known): (Facts, Known), pc: u32, instruction: WideInstruction) -> Result<Decoded, String> {
    let decoded = match numeric_row(region, instruction) {
        Some(decoded) => decoded,
        None => heap_row(region, facts, known, pc, instruction).filter(|_| instruction.result_flags() == 0),
    };
    decoded.ok_or_else(|| reject(region, instruction))
}

/// The row of a numeric instruction: `None` when the instruction is not one, `Some(None)` when it
/// is one a row cannot take.
pub(super) fn numeric_row(region: &LoopRegion, instruction: WideInstruction) -> Option<Option<Decoded>> {
    let copy = || Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None);
    let plain = instruction.result_flags() == 0;
    Some(match instruction.op().class() {
        Class::Move => plain.then(copy),
        // A Number operand is its own numeric value: the loop's entry guards and its own rows prove
        // it, and a function's row proves it itself (`checked`).
        Class::ToNumeric => plain.then(|| Decoded { checked: true, ..copy() }),
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
        Class::GetField => "GetField: dynamic or unrecorded".to_string(),
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
