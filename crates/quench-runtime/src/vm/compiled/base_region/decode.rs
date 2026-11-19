use super::*;
use crate::vm::global_facts::LexicalSlot;
use crate::staged::property::StoreArm;
mod fields;
use fields::{field_row, field_store_row};
pub(super) use fields::stores_field;

/// The row kind of a binary operator: the shared operation rows decide which Number operation an
/// operator is (`vm::operations::number_rows`, the table every tier consumes), and the row
/// vocabulary names each operation once. A relational jump takes only a relation, a `Binary` only an
/// arithmetic or bitwise operation.
pub(super) fn kind_of(operator: u32, relational: bool) -> Option<Kind> {
    use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};
    if relational {
        return NumberRelation::from_operator(operator).map(|relation| match relation {
            NumberRelation::LessThan => Kind::Lt,
            NumberRelation::LessEqual => Kind::Le,
            NumberRelation::GreaterThan => Kind::Gt,
            NumberRelation::GreaterEqual => Kind::Ge,
            NumberRelation::Equal => Kind::Eq,
            NumberRelation::NotEqual => Kind::Ne,
        });
    }
    let arithmetic = NumberArithmetic::from_operator(operator).map(|operation| match operation {
        NumberArithmetic::Add => Kind::Add,
        NumberArithmetic::Subtract => Kind::Sub,
        NumberArithmetic::Multiply => Kind::Mul,
        NumberArithmetic::Divide => Kind::Div,
    });
    let remainder = (operator == crate::vm::operations::REMAINDER).then_some(Kind::Rem);
    arithmetic.or(remainder).or_else(|| NumberBitwise::from_operator(operator).map(|operation| match operation {
        NumberBitwise::And => Kind::And,
        NumberBitwise::Or => Kind::Or,
        NumberBitwise::Xor => Kind::Xor,
        NumberBitwise::ShiftLeft => Kind::Shl,
        NumberBitwise::ShiftRight => Kind::Shr,
        NumberBitwise::ShiftRightUnsigned => Kind::UShr,
    }))
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
    /// The observed type of the own slot a field store writes, where its site proves one own load
    /// (`NativeFieldLoad::ty`).
    pub(super) slot: Option<crate::typed_ir::Type>,
    /// The exact shape whose slot `slot` is the type of, for a field store: an ungated store row
    /// writes only into a receiver of this shape (`plan::site_holes`, hole 4), as the site's live
    /// cache entry may name another shape, whose slot type nothing proved.
    pub(super) slot_shape: Option<u32>,
    /// A site read's or store's one own flat entry (`decode::fields::ic_hole`: the exact shape, and the
    /// slot's byte offset above 32 bits), whose inline-cache row runs first and falls to the site row
    /// on a miss (`plan::ic_plan`).
    pub(super) ic: Option<u64>,
    /// The step a store row's instruction runs as where the row declines its operand (`declines`):
    /// the instruction's own (`steps::lowering`).
    pub(super) declined: Option<Step>,
    /// The exact shape of the holder whose own native function the row stands on (`math_call`'s
    /// identity guard): a Stable fact the code takes (`held_holders`), so the function's identity is
    /// not recycled while the code runs.
    pub(super) holder: Option<u32>,
    /// The slot type the fact store proves of a site read's one own flat entry (`decode::fields::cached_read`): a
    /// Number type the code holds as a dependency (`held_slot_types`), which a widening of the slot retires it
    /// over, so the read's inline-cache row delivers the word to a pinned double with no tag test
    /// (`plan::ic_plan`). Kept only where that row runs and the code makes no call (`repr::keep_typed_reads`).
    pub(super) typed: Option<crate::code_dependencies::SlotDependency>,
    /// Every path proves the inline-cache access's receiver of its entry's shape (`proofs::prove`): the row
    /// reads or writes the slot with no guard.
    pub(super) proven: bool,
    /// Some receiver proof reaches the row (`proofs::prove`), which a later row may stand on: the code
    /// is not resumed here.
    pub(super) relies: bool,
}

impl Decoded {
    pub(super) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, also: None, a, b, target, heap: None, base: None, hole: 0, atom: None, folded: None, step: None, checked: false, completes: false, instanceof: None, global: None, lexical: None, add: None, name: None, slot: None, slot_shape: None, ic: None, declined: None, holder: None, typed: None, proven: false, relies: false }
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

    /// The observed type of the slot a store row writes in place: a global object's slot or a field's.
    fn stored_slot_type(&self) -> Option<crate::typed_ir::Type> {
        match self.heap {
            Some(Heap::GlobalStore) => self.global.map(|access| access.slot.ty),
            Some(Heap::SiteStore | Heap::ThisStore) => self.slot,
            _ => None,
        }
    }

    /// How the store row writes its operand in `place` to its slot (`StoreArm::of`): straight, where
    /// the place's type is the slot's observed type; once its frame word is an int32, for a field
    /// store of a frame slot whose type holds int32 (`StoreArm::holds`); else not at all, as a wider
    /// value would widen the observation, which the VM must notice.
    fn store_arm(&self, place: Place) -> Option<StoreArm> {
        let arm = StoreArm::of((0, 0), self.stored_slot_type()?, value_fact(place))?;
        let tested = place == Place::Mem && matches!(self.heap, Some(Heap::SiteStore | Heap::ThisStore));
        (arm.holds.is_none() || tested && arm.holds == Some(crate::typed_ir::Type::INT32)).then_some(arm)
    }

    /// What a tested store (`store_arm` of a frame word) admits besides an int32: a double, where
    /// the slot's observed type holds Number, so the store widens no observation and logs nothing.
    pub(super) fn admitted_tags(&self) -> u64 {
        if self.stored_slot_type().is_some_and(|ty| crate::typed_ir::Type::NUMBER.is_subtype(ty)) {
            numeric::library::STORE_ADMITS_DOUBLES
        } else {
            numeric::library::STORE_ADMITS_INT32
        }
    }

    /// The step a store row runs as where it cannot write its operand in `place` (`store_arm`): the
    /// instruction's own.
    pub(super) fn declines(&self, place: Place) -> Option<Step> {
        self.stored_slot_type().filter(|_| self.store_arm(place).is_none()).and(self.declined)
    }

    /// Whether the store row writes its operand in `place` through the store gate.
    ///
    /// Invariant: an ungated row writes with no slot-type join, so it is chosen only where the plan
    /// proved the slot's observed type admits every value the row can write (`store_arm`: the place's
    /// type is the slot's, or the slot's int32 type holds the frame word once tested), and the value
    /// needs no barrier log (immediate, or the tested int32). Every other store, including an
    /// immediate (`Place::Const`, `Place::PinF`, a pinned int32) into a slot whose type the site does
    /// not prove (no single own load, `Decoded::slot` `None`) or does not admit, is gated: the gate
    /// refuses every store while slot types are tracked, so the interpreter's store joins the type.
    /// Slot types only widen, so an admission proved here stays true while the code runs.
    pub(super) fn gated(&self, place: Place) -> bool {
        match self.store_arm(place) {
            None => true,
            Some(arm) => arm.holds.is_none() && !value_fact(place).is_subtype(crate::typed_ir::Type::IMMEDIATE),
        }
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

/// The row of a comparison jump that fails to its target (`Operation::CompareJump` with
/// `negate`): its staged comparison as the row's relation; any other comparison is the step's.
pub(super) fn relation_jump(numbers: Numbers, instruction: WideInstruction, comparison: crate::staged::compare::Comparison,
    (left, right): (crate::bytecode::Operand, crate::bytecode::Operand)) -> Result<Decoded, String> {
    use crate::staged::compare::Comparison;
    use crate::vm::operations::NumberRelation;
    let kind = match comparison {
        Comparison::Relation { relation, .. } => match relation {
            NumberRelation::LessThan => Kind::Lt,
            NumberRelation::LessEqual => Kind::Le,
            NumberRelation::GreaterThan => Kind::Gt,
            NumberRelation::GreaterEqual => Kind::Ge,
            NumberRelation::Equal => Kind::Eq,
            NumberRelation::NotEqual => Kind::Ne,
        },
        Comparison::Equality { negated: false, .. } => Kind::Eq,
        Comparison::Equality { negated: true, .. } => Kind::Ne,
        _ => return Err("comparison: not a Number relation".to_string()),
    };
    Ok(Decoded::number(kind, None, Some(source(numbers, left)?), Some(source(numbers, right)?), Some(instruction.jump_target())))
}

pub(super) fn load_const(numbers: Numbers, destination: Register, index: usize) -> Result<Decoded, String> {
    let number = numbers(index).ok_or_else(|| "LoadConst: not a Number".to_string())?;
    let word = Source::Constant(Value::number(number).to_bits());
    Ok(Decoded::number(Kind::Move, Some(destination), Some(word), None, None))
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

/// The row of a depth-0 capture read: the activation's slots, which a region that makes calls
/// reads live (`osr_lane::bind_captures`), as the interpreter's `lane_load_capture` does.
fn capture_load_row(facts: Facts, instruction: WideInstruction) -> Option<Decoded> {
    (instruction.capture_depth() == 0)
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
    use crate::staged::Operation;
    // The staged operation the instruction denotes (`Operation::decode`) names its row: the one
    // decode every tier reads, so this tier names no opcode of its own.
    match Operation::<(), ()>::decode(instruction, crate::staged::DecodeContext::EMPTY) {
        Some(Operation::LoadCapture { .. }) => capture_load_row(facts, instruction),
        Some(Operation::StoreCapture { .. }) => capture_store_row(facts, instruction),
        Some(Operation::ElementRead { .. }) => index_read_row(region, facts, instruction),
        Some(Operation::ElementWrite { .. }) => Some(index_write_row(region, facts, instruction)),
        Some(Operation::RequireObjectCoercible { register }) => Some(Decoded::heap(Heap::Array, None, Some(Source::Register(register)), None)),
        Some(Operation::LoadThis { .. }) => this_row(instruction),
        Some(Operation::LoadLocalPlain { .. }) => load_local_row(instruction),
        Some(Operation::StoreLocalPlain { .. }) => store_local_row(instruction),
        _ => named_row(region, facts, known, pc, instruction),
    }
}

/// The heap instructions that name a field or a global: a recorded or cached field read or store, a
/// membership test, and a global name's access (`globals`).
fn named_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    use crate::staged::Operation;
    match crate::vm::compiled::decoded(instruction) {
        Some(Operation::GetField { .. }) if !instruction.returns_from_frame() && !instruction.writes_current_this() =>
            return field_row(region, facts, known, pc, instruction),
        Some(Operation::SetField { .. }) => return field_store_row(known, instruction),
        _ => {}
    }
    match instruction.op().class() {
        Class::Binary => membership_row(known, pc, instruction, instruction.result_register()),
        _ if instruction.op().traits().has(crate::bytecode::Traits::GLOBAL_NAME_ACCESS) => global_row(known, pc, instruction),
        _ => None,
    }
}

/// The row of a `LoadConst` of a constant that is no Number (`undefined`, `null`, a boolean, a
/// string, a BigInt): the tagged word the compiling VM holds for it, written as it is
/// (`Source::Value`), where the VM has the program's constants (`Known::constants`).
fn value_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    let (destination, index) = crate::staged::Operation::<(), ()>::decode_constant_load(instruction).filter(|_| instruction.result_flags() == 0)?;
    let value = known.constants.get(index)?;
    Some(Decoded::number(Kind::Move, Some(destination), Some(Source::Value(value.to_bits())), None, None))
}

/// The row of `instruction` at `pc`: a numeric or heap row where one takes it, else the step the
/// interpreter's own operation makes (`steps`), so no instruction is refused.
pub(super) fn decode(region: &LoopRegion, (facts, known): (Facts, Known), pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    numeric_row(&|index| region.number_constant(index), instruction)
        .flatten()
        .or_else(|| heap_row(region, facts, known, pc, instruction).filter(|_| instruction.result_flags() == 0)
            .map(|row| Decoded { declined: row.stored_slot_type().map(|_| lowering(instruction, known, helpers)), ..row }))
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
    use crate::staged::{NumberOperation, Operation};
    let copy = || Decoded::number(Kind::Move, Some(instruction.result_register()), Some(Source::Register(instruction.register_b())), None, None);
    let plain = instruction.result_flags() == 0;
    // The moves and jumps by the staged operation the instruction denotes (`Operation::decode`).
    match Operation::<(), ()>::decode(instruction, crate::staged::DecodeContext::EMPTY) {
        Some(Operation::Move { .. }) => return Some(plain.then(copy)),
        // A Number operand is its own numeric value: the loop's entry guards and its own rows prove
        // it, and a function's row proves it itself (`checked`).
        Some(Operation::Number(NumberOperation::ToNumeric { .. })) => return Some(plain.then(|| Decoded { checked: true, ..copy() })),
        Some(Operation::Jump) => return Some(Some(Decoded::number(Kind::Jump, None, None, None, Some(instruction.jump_target())))),
        Some(Operation::JumpIf { register, taken }) => return Some(Some(truthiness_jump(instruction, register, taken))),
        // A comparison jump (`JumpBinaryFalse`): a Number relation, or `==`/`===` (`!=`/`!==`)
        // of Numbers, which the loop's entry guards and its rows prove its operands are.
        Some(Operation::CompareJump { comparison, left, right, negate: true }) => return Some(relation_jump(numbers, instruction, comparison, (left, right)).ok()),
        // `!x`: the boolean of its truthiness.
        Some(Operation::LogicalNot { destination, source }) =>
            return Some(plain.then(|| Decoded::number(Kind::Not, Some(destination), Some(Source::Register(source)), None, None))),
        // `++`/`--`: the int32 update by one (the row's overflow leaves, as the staged one does).
        Some(Operation::Number(NumberOperation::IncDec { destination, source, decrement, .. })) => return Some(plain.then(|| Decoded::number(
            if decrement { Kind::Sub } else { Kind::Add }, Some(destination), Some(Source::Register(source)),
            Some(Source::Constant(Value::integer(1).to_bits())), None))),
        _ => {}
    }
    // `-x` and `+x`: the Number unary the interpreter's operations name (`NumberUnary`).
    if let Some(operation) = crate::vm::operations::NumberUnary::from_js_instruction(instruction.op(), instruction.unary_operator()) {
        return Some(unary_row(instruction, operation).filter(|_| plain));
    }
    if let Some((destination, index)) = Operation::<(), ()>::decode_constant_load(instruction) {
        return Some(load_const(numbers, destination, index).ok().filter(|_| plain));
    }
    Some(match instruction.op().class() {
        Class::Binary | Class::NumericAdd | Class::NumericMultiply => binary(numbers, instruction).ok().filter(|_| plain),
        _ => return None,
    })
}

/// A jump on `register`'s truthiness: taken where it is truthy (`taken`, `JumpTrue`) or falsy.
fn truthiness_jump(instruction: WideInstruction, register: crate::bytecode::Register, taken: bool) -> Decoded {
    let kind = if taken { Kind::JumpTrue } else { Kind::JumpFalse };
    Decoded::number(kind, None, Some(Source::Register(register)), None, Some(instruction.jump_target()))
}

/// The row of a Number unary over a register: `-` the negation of a Number, `+` a Number as it
/// is (proved, as `ToNumeric` is).
fn unary_row(instruction: WideInstruction, operation: crate::vm::operations::NumberUnary) -> Option<Decoded> {
    use crate::vm::operations::NumberUnary;
    let (result, operand) = (instruction.result_register(), Some(Source::Register(instruction.register_b())));
    let row = |kind| Decoded::number(kind, Some(result), operand, None, None);
    Some(match operation {
        NumberUnary::Negate => row(Kind::Neg),
        NumberUnary::Plus => Decoded { checked: true, ..row(Kind::Move) },
    })
}

/// Bits in a word's high half.
pub(super) const HALF: u32 = 32;
/// The high 32 bits of an int word.
pub(super) const INT_HIGH: u64 = crate::value::ValueLayout::INT_TAG >> HALF;

pub(super) fn register_of(source: Source) -> Option<Register> {
    match source {
        Source::Register(register) => Some(register),
        Source::Constant(_) | Source::Value(_) => None,
    }
}
