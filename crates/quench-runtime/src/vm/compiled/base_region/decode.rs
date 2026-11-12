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

/// The receiver a folded prototype-held read stands on (`Heap::Held`): the exact shape and the
/// prototype link the row proves of the receiver before it answers the value the holder held (the
/// row's `hole`), and the holder's exact shape, whose values the code holds (a Stable fact).
#[derive(Clone, Copy)]
pub(super) struct Folded {
    pub(super) shape: u32,
    pub(super) link: u32,
    pub(super) holder: u32,
}

impl Folded {
    /// The row's receiver word: the shape, and the link above `HELD_LINK_AT` bits.
    pub(super) fn word(self) -> u64 {
        u64::from(self.shape) | u64::from(self.link) << numeric::library::HELD_LINK_AT
    }
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
    /// The atom a field store holds unnamed by method caches (`Known::stores`), a Stable fact.
    pub(super) atom: Option<Atom>,
    /// The receiver a folded prototype-held read stands on, with its value in `hole`.
    pub(super) folded: Option<Folded>,
    /// A `ToNumeric` copy: its operand is no Number until the row proves it (a loop's entry guards
    /// prove the registers it names; a function's registers are not proved).
    pub(super) checked: bool,
}

impl Decoded {
    pub(super) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, a, b, target, heap: None, base: None, hole: 0, atom: None, folded: None, step: None, checked: false }
    }

    pub(super) fn heap(heap: Heap, dst: Option<Register>, a: Option<Source>, b: Option<Source>) -> Self {
        Self { heap: Some(heap), ..Self::number(Kind::Move, dst, a, b, None) }
    }

    pub(super) fn step(step: Step, dst: Option<Register>) -> Self {
        Self { step: Some(step), ..Self::number(Kind::Move, dst, None, None, None) }
    }

    /// Whether the row holds a Stable fact its code took: a folded prototype-held read, or a store in
    /// place for an atom no method cache names. A change of the fact retires the code, and a run
    /// already inside it learns that from the invalidation word (`Heap::Live`).
    pub(super) fn holds_stable(&self) -> bool {
        matches!(self.heap, Some(Heap::Held | Heap::ThisHeld | Heap::SiteStore | Heap::ThisStore))
    }

    /// Whether the row makes a call or a step the interpreter's operation runs, either of which may
    /// run guest code that changes a Stable fact.
    pub(super) fn makes_calls(&self) -> bool {
        matches!(self.step, Some(Step::Call(_) | Step::Helper))
    }

    /// The registers the row names.
    pub(super) fn registers(&self) -> impl Iterator<Item = Register> {
        [self.dst, self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
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
    /// A loop region: `dense` where its arrays' views are the entry record's, `writes` where it
    /// stores a field through a row, which may stale a field value the entry record holds
    /// (`stores_field`), so its field reads are the site's, live; `calls` where it makes a call by a
    /// row (`calls_in_place`), whose callee may change anything the entry recorded: such a region
    /// reads its arrays through their records, its fields through the site's entry, and leaves for
    /// the captures its entry copied (the interpreter reads them live).
    Entry { dense: bool, writes: bool, calls: bool },
    Site,
}

/// The entry-record byte offset of the array `register` holds through the region, which an
/// indexing row reads the backing of from the record; 0 where the array is looked up per access.
fn entry_array(region: &LoopRegion, facts: Facts, register: Register) -> u64 {
    // A row's hole is a u32 patch: an offset past it is looked up per access instead.
    match facts {
        Facts::Entry { dense: true, .. } => region.array_entry_offset(register).filter(|offset| u32::try_from(*offset).is_ok()).unwrap_or(0),
        Facts::Entry { dense: false, .. } | Facts::Site => 0,
    }
}

/// The byte offset of the field cache entry of `site` in the program's table.
fn entry_offset(site: u16) -> u64 {
    u64::from(site) * crate::vm::NATIVE_FIELD_CACHE_LAYOUT.entry_size as u64
}

/// The folded form of a field read at cache `site`: where the site proves exactly one load, held by
/// the receiver's immediate prototype, whose value the holder holds now (`InheritedConstant`), the
/// receiver it stands on and the value. A change of a value any prototype of the holder's shape holds
/// retires the code (`Body::holders`).
fn folded_read(known: Known, site: u16) -> Option<(Folded, u64)> {
    let reads = known.reads?;
    let loads = reads(site);
    let [load] = loads.as_slice() else { return None };
    let inherited = load.inherited?;
    let [(link, value)] = inherited.values() else { return None };
    (!load.function_shape).then_some((Folded { shape: load.shape, link: *link, holder: load.holder? }, value.to_bits()))
}

/// The row of a named field read of a register (or of the receiver, `this`, which the payload
/// holds) through the site's cache entry, or a nested lookup's two (`FieldSite`: `base.first.second`,
/// with no sink field); a read the prototype holds is folded to its value where the code may hold
/// that (`folded_read`).
fn site_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let (base, first, second) = match instruction.field_lookup()? {
        FieldLookup::Atom { base, cache_site, .. } => (base, cache_site, None),
        FieldLookup::Site(index) => {
            let site = known.program.field_sites.get(index).filter(|site| site.sink.is_none())?;
            (site.base, site.first.1, site.second.map(|(_, cache_site)| cache_site))
        }
    };
    let receiver = base == FieldBase::THIS;
    let object = if receiver { None } else { Some(Source::Register(base.register_index()?)) };
    let folded = second.is_none().then(|| folded_read(known, first)).flatten();
    let heap = match (receiver, second.is_some(), folded.is_some()) {
        (false, false, false) => Heap::Site,
        (false, true, _) => Heap::SiteChain,
        (true, false, false) => Heap::ThisSite,
        (true, true, _) => Heap::ThisChain,
        (false, false, true) => Heap::Held,
        (true, false, true) => Heap::ThisHeld,
    };
    let hole = entry_offset(first) | second.map_or(0, |second| entry_offset(second) << numeric::library::CHAIN_SECOND_AT);
    let row = Decoded::heap(heap, Some(instruction.result_register()), object, None);
    Some(match folded {
        Some((folded, value)) => Decoded { hole: value, folded: Some(folded), ..row },
        None => Decoded { hole, ..row },
    })
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
        Facts::Entry { writes: false, .. } => recorded_field(region, pc, instruction).or_else(|| site_row(known, instruction)),
        Facts::Entry { writes: true, .. } | Facts::Site => site_row(known, instruction),
    }
}

/// The row of a named field store to a register's object, or to `this`, through the site's cache
/// entry (`entry::site_store`), where the store may run in place (`Known::stores_in_place`).
fn field_store_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let atom = instruction.atom_index();
    if !known.stores_in_place(atom) || instruction.returns_from_frame() || instruction.writes_current_this() {
        return None;
    }
    let (heap, base) = match instruction.field_store_register() {
        Some(register) => (Heap::SiteStore, Some(register)),
        None => (Heap::ThisStore, None),
    };
    let value = Some(Source::Register(instruction.register_a()));
    Some(Decoded { base, hole: entry_offset(instruction.cache_site_index()), atom: Some(atom), ..Decoded::heap(heap, None, value, None) })
}

/// Whether `instruction` is a field store a row makes (`field_store_row`).
pub(super) fn stores_field(known: Known, instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0 && instruction.op().is_field_store() && field_store_row(known, instruction).is_some()
}

/// The facts rows hold, each once, in order.
pub(super) fn distinct<T: Ord>(facts: impl Iterator<Item = T>) -> Box<[T]> {
    let mut facts: Vec<T> = facts.collect();
    facts.sort_unstable();
    facts.dedup();
    facts.into()
}

/// The exact shapes of the holders whose values the rows hold folded (`Decoded::folded`): the Stable
/// facts the code takes, which a change of a value a prototype of that shape holds retires it.
pub(super) fn held_holders(decoded: &[Decoded]) -> Box<[u32]> {
    distinct(decoded.iter().filter_map(|row| row.folded.map(|folded| folded.holder)))
}

/// The atoms the rows hold unnamed by method caches (`Decoded::atom`): the Stable facts the code
/// takes, which a cache naming one retires it.
pub(super) fn held_atoms(decoded: &[Decoded]) -> Box<[Atom]> {
    distinct(decoded.iter().filter_map(|row| row.atom))
}

/// Whether `instruction` stores to a depth-0 capture and nothing else (`general_capture::lower_capture`'s
/// conditions): it completes no activation, sets no `this`, and is no numeric local's update.
fn own_capture_store(instruction: WideInstruction) -> bool {
    instruction.capture_depth() == 0 && !instruction.returns_from_frame() && !instruction.writes_current_this() && instruction.numeric_local_target().is_none()
}

/// The row of a depth-0 capture access: a read, or a store, which the row makes where the run's
/// slots admit every direct store (`JS_STEP_CAPTURE_STORES`) and leaves for the interpreter where
/// they do not or the slot holds a marker (a loop's snapshot of a stored slot).
fn capture_row(facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    match instruction.op().class() {
        Class::LoadCapture if instruction.capture_depth() == 0 && !matches!(facts, Facts::Entry { calls: true, .. }) => {
            Some(Decoded::heap(Heap::Capture(instruction.capture_slot()), Some(instruction.result_register()), None, None))
        }
        Class::StoreCapture if own_capture_store(instruction) => {
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
        _ => None,
    }
}

/// The row of the activation's `this` and its local slots, which every run reads live.
fn activation_row(instruction: WideInstruction) -> Option<Decoded> {
    let register = |register| Some(Source::Register(register));
    match instruction.op().class() {
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
        _ => None,
    }
}

/// The heap instructions: a capture, a dense element read or store, a recorded or cached field
/// read or store, the proof that a base is an array, the activation's `this` and its local slots.
pub(super) fn heap_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match instruction.op().class() {
        Class::LoadCapture | Class::StoreCapture => capture_row(facts, instruction),
        Class::GetIndex | Class::SetIndex | Class::RequireObjectCoercible => array_row(region, facts, instruction),
        Class::GetField if !instruction.returns_from_frame() && !instruction.writes_current_this() => field_row(region, facts, known, pc, instruction),
        Class::LoadThis | Class::LoadLocalPlain | Class::StoreLocalPlain => activation_row(instruction),
        _ if instruction.op().is_field_store() => field_store_row(known, instruction),
        _ => None,
    }
}

/// The row of `instruction` at `pc`: a numeric or heap row where one takes it, else the step the
/// interpreter's own operation makes (`steps`), so no instruction is refused.
pub(super) fn decode(region: &LoopRegion, (facts, known): (Facts, Known), pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    numeric_row(&|index| region.number_constant(index), instruction)
        .flatten()
        .or_else(|| heap_row(region, facts, known, pc, instruction).filter(|_| instruction.result_flags() == 0))
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
