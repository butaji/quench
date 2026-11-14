use super::*;
use crate::vm::global_facts::LexicalSlot;

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
    /// A constant that is no Number: a tagged word (`undefined`, a boolean, `null`, a string or a
    /// BigInt the program holds) a row writes as it is, whose register is tagged whatever else holds.
    Value(u64),
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
    /// The second register a step writes: the `this` a name call defines beside its callee
    /// (`STEP_WRITES`). Its value is a tagged word the step made in its frame slot.
    pub(super) also: Option<Register>,
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
    /// The recorded transition an add row runs (`adds`), with the form its storage takes and whether
    /// the receiver is the activation's `this`.
    pub(super) add: Option<adds::Add>,
    /// The recorded naming a `SetFunctionName` row runs (`names`).
    pub(super) name: Option<names::Name>,
    /// A `ToNumeric` copy: its operand is no Number until the row proves it (a loop's entry guards
    /// prove the registers it names; a function's registers are not proved).
    pub(super) checked: bool,
    /// The instruction carried the return flag: the activation completes with the result the row
    /// writes (`steps::completing`), so control reaches nothing after it.
    pub(super) completes: bool,
    /// The constructor an `instanceof` fast arm stands on, with what the VM proved of it (`Known::instanceofs`): its
    /// identity is the row's `hole`, which the constructor register must hold, and the prototype link the walk looks for
    /// is the fact's, a Stable fact the code depends on (`held_instanceofs`).
    pub(super) instanceof: Option<(Value, InstanceofFact)>,
    /// The global object's slot the row holds under the object's layout and the slot's observed type
    /// (`globals`): Stable facts the code takes.
    pub(super) global: Option<Access>,
    /// The global lexical binding's word the row reads or stores live (`globals`), at the address in `hole`, under the global
    /// object's layout: a Stable fact the code takes.
    pub(super) lexical: Option<LexicalSlot>,
}

impl Decoded {
    pub(super) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, also: None, a, b, target, heap: None, base: None, hole: 0, atom: None, folded: None, step: None, checked: false, completes: false, instanceof: None, global: None, lexical: None, add: None, name: None }
    }

    pub(super) fn heap(heap: Heap, dst: Option<Register>, a: Option<Source>, b: Option<Source>) -> Self {
        Self { heap: Some(heap), ..Self::number(Kind::Move, dst, a, b, None) }
    }

    pub(super) fn step(step: Step, dst: Option<Register>) -> Self {
        Self { step: Some(step), ..Self::number(Kind::Move, dst, None, None, None) }
    }

    /// Whether the row holds a Stable fact its code took: a folded prototype-held read, an
    /// `instanceof` of a constructor's `prototype`, a global object's slot, an add, or a store in place for an atom no method cache
    /// names. A change of the fact retires the code, and a run already inside it learns that from the invalidation word (`Heap::Live`).
    pub(super) fn holds_stable(&self) -> bool {
        self.add.is_some() || self.folded.is_some() || matches!(self.heap, Some(Heap::SiteStore | Heap::ThisStore)) || self.instanceof.is_some() || self.global.is_some() || self.lexical.is_some()
    }

    /// Whether the row makes a call or a step the interpreter's operation runs, either of which may
    /// run guest code that changes a Stable fact.
    pub(super) fn makes_calls(&self) -> bool {
        matches!(self.step, Some(Step::Call(_))) || self.step.is_some_and(Step::runs_helper)
    }

    /// The registers the row names.
    pub(super) fn registers(&self) -> impl Iterator<Item = Register> {
        [self.dst, self.also, self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
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
    let object = if base == FieldBase::THIS { None } else { Some(base.register_index()?) };
    Some(cached_read(known, (object, first, second), instruction.result_register()))
}

/// The row that reads cache site `first` (and, chained, `second`) of the object in register `object`, or of the receiver where there is
/// none, into the register `dst`: folded to the prototype's value where the site proves exactly that (`folded_read`).
fn cached_read(known: Known, (object, first, second): (Option<Register>, u16, Option<u16>), dst: Register) -> Decoded {
    let folded = second.is_none().then(|| folded_read(known, first)).flatten();
    let heap = match (object.is_none(), second.is_some(), folded.is_some()) {
        (false, false, false) => Heap::Site,
        (false, true, _) => Heap::SiteChain,
        (true, false, false) => Heap::ThisSite,
        (true, true, _) => Heap::ThisChain,
        (false, false, true) => Heap::Held,
        (true, false, true) => Heap::ThisHeld,
    };
    let hole = entry_offset(first) | second.map_or(0, |second| entry_offset(second) << numeric::library::CHAIN_SECOND_AT);
    let row = Decoded::heap(heap, Some(dst), object.map(Source::Register), None);
    match folded {
        Some((folded, value)) => Decoded { hole: value, folded: Some(folded), ..row },
        None => Decoded { hole, ..row },
    }
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
        let register = base.register_index()?;
        let (heap, hole) = element_heap(region, facts, register, (Heap::Length, |_| Heap::TypedLength));
        let row = Decoded::heap(heap, Some(instruction.result_register()), Some(Source::Register(register)), None);
        return Some(Decoded { hole: if heap == Heap::TypedLength { hole } else { 0 }, ..row });
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
    if !known.stores_in_place(atom) || instruction.returns_from_frame() || instruction.writes_current_this() || adds::of(known, instruction).is_some() {
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

/// Whether `instruction` stores to a depth-0 capture and nothing else (`general_capture::lower_capture`'s
/// conditions): it completes no activation, sets no `this`, and is no numeric local's update.
fn own_capture_store(instruction: WideInstruction) -> bool {
    instruction.capture_depth() == 0 && !instruction.returns_from_frame() && !instruction.writes_current_this() && instruction.numeric_local_target().is_none()
}

/// The row of a depth-0 capture access `heap` over `result` and `value`: a read, or a store, which the row
/// makes where the run's slots admit every direct store (`JS_STEP_CAPTURE_STORES`). Where they do not, the slot
/// holds a marker (a loop's snapshot of a stored slot) or the binding is a root one (a global `var`'s shared
/// slot, which the activation's dense view omits), a body has the interpreter's step behind the row
/// (`Step::Fast`) and a loop region leaves into it.
fn capture_arm(facts: Facts, heap: Heap, result: Option<Register>, value: Option<Source>) -> Decoded {
    match facts {
        Facts::Site => Decoded { a: value, ..Decoded::step(Step::Fast(heap), result) },
        Facts::Entry { .. } => Decoded::heap(heap, result, value, None),
    }
}

/// The row of a depth-0 capture read, which a loop region that makes calls leaves for the captures its entry copied.
fn capture_load_row(facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    (instruction.capture_depth() == 0 && !matches!(facts, Facts::Entry { calls: true, .. }))
        .then(|| capture_arm(facts, Heap::Capture(instruction.capture_slot()), Some(instruction.result_register()), None))
}

/// The row of a depth-0 capture store.
fn capture_store_row(facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    own_capture_store(instruction).then(|| {
        capture_arm(facts, Heap::CaptureSet(instruction.capture_slot()), None, Some(Source::Register(instruction.register_a())))
    })
}

/// The row of an element read.
fn index_read_row(region: &LoopRegion, facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    if instruction.returns_from_frame() {
        return None;
    }
    let base = instruction.operand_b().register_index()?;
    let index = source(&|index| region.number_constant(index), instruction.operand_c()).ok()?;
    let (heap, hole) = element_heap(region, facts, base, (Heap::Index, Heap::TypedIndex));
    let row = Decoded::heap(heap, Some(instruction.result_register()), Some(Source::Register(base)), Some(index));
    Some(Decoded { hole, ..row })
}

/// The row of an element store.
fn index_write_row(region: &LoopRegion, facts: Facts, instruction: WideInstruction) -> Decoded {
    let (value, key) = (Some(Source::Register(instruction.register_a())), Some(Source::Register(instruction.register_c())));
    let (heap, hole) = element_heap(region, facts, instruction.register_b(), (Heap::Store, Heap::TypedStore));
    Decoded { base: Some(instruction.register_b()), hole, ..Decoded::heap(heap, None, value, key) }
}

/// The row of the activation's `this`, which every run reads live.
fn this_row(instruction: WideInstruction) -> Option<Decoded> {
    (!instruction.writes_current_this()).then(|| Decoded::heap(Heap::This, Some(instruction.result_register()), None, None))
}

/// The row of a local slot's read, which every run reads live.
fn load_local_row(instruction: WideInstruction) -> Option<Decoded> {
    let slot = u16::try_from(instruction.local_slot()).ok()?;
    Some(Decoded::heap(Heap::LoadLocal(slot), Some(instruction.result_register()), None, None))
}

/// The row of a local slot's store; a store that also defines a register (`optional_register_b`) is the
/// interpreter's.
fn store_local_row(instruction: WideInstruction) -> Option<Decoded> {
    let slot = u16::try_from(instruction.local_slot()).ok().filter(|_| instruction.optional_register_b().is_none())?;
    Some(Decoded::heap(Heap::StoreLocal(slot), None, Some(Source::Register(instruction.register_a())), None))
}

/// The heap instructions: a capture, a dense element read or store, the proof that a base is an
/// array, the activation's `this` and its local slots, and the named ones (`named_row`).
pub(super) fn heap_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match instruction.op().class() {
        Class::LoadCapture => capture_load_row(facts, instruction),
        Class::StoreCapture => capture_store_row(facts, instruction),
        Class::GetIndex => index_read_row(region, facts, instruction),
        Class::SetIndex => Some(index_write_row(region, facts, instruction)),
        Class::RequireObjectCoercible => Some(Decoded::heap(Heap::Array, None, Some(Source::Register(instruction.register_b())), None)),
        Class::LoadThis => this_row(instruction),
        Class::LoadLocalPlain => load_local_row(instruction),
        Class::StoreLocalPlain => store_local_row(instruction),
        _ => named_row(region, facts, known, pc, instruction),
    }
}

/// The heap instructions that name a field or a global: a recorded or cached field read or store, a
/// membership test, and a global name's access (`globals`).
fn named_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match instruction.op().class() {
        Class::GetField if !instruction.returns_from_frame() && !instruction.writes_current_this() => field_row(region, facts, known, pc, instruction),
        Class::Binary => membership_row(known, pc, instruction, instruction.result_register()),
        _ if instruction.op().traits().has(crate::bytecode::Traits::GLOBAL_NAME_ACCESS) => global_row(known, pc, instruction),
        _ if instruction.op().is_field_store() => field_store_row(known, instruction),
        _ => None,
    }
}

/// The row of a `LoadConst` of a constant that is no Number (`undefined`, `null`, a boolean, a
/// string, a BigInt): the tagged word the compiling VM holds for it, written as it is
/// (`Source::Value`), where the VM has the program's constants (`Known::constants`).
fn value_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    if instruction.op().class() != Class::LoadConst || instruction.result_flags() != 0 {
        return None;
    }
    let value = known.constants.get(instruction.constant_index())?;
    Some(Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Value(value.to_bits())), None, None))
}

/// The row of `instruction` at `pc`: a numeric or heap row where one takes it, else the step the
/// interpreter's own operation makes (`steps`), so no instruction is refused.
pub(super) fn decode(region: &LoopRegion, (facts, known): (Facts, Known), pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    numeric_row(&|index| region.number_constant(index), instruction)
        .flatten()
        .or_else(|| heap_row(region, facts, known, pc, instruction).filter(|_| instruction.result_flags() == 0))
        .or_else(|| value_row(known, instruction))
        .unwrap_or_else(|| names(known, instanceof_arm(known, pc, instruction, decode_step(pc, instruction, known, (helpers, matches!(facts, Facts::Site))))))
}

/// The step `row` with the table of `typeof` result strings its fast arm reads (hole 2) where the VM has
/// one, else the helper step alone.
fn names(known: Known, row: Decoded) -> Decoded {
    match (row.step, known.typeof_names.first()) {
        (Some(Step::Fast(Heap::TypeofString)), Some(first)) => Decoded { hole: std::ptr::from_ref(first).addr() as u64, ..row },
        (Some(Step::Fast(Heap::TypeofString)), None) => Decoded { step: Some(Step::Helper), a: None, ..row },
        _ => row,
    }
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
        Source::Constant(_) | Source::Value(_) => None,
    }
}
