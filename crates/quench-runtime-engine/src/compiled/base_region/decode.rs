#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::vm::global_facts::LexicalSlot;
use quench_runtime::staged::property::StoreArm;
use quench_runtime::staged::{BinaryClass, Constant, DecodeContext, Operation};
pub(crate) mod fields;
use fields::{field_row, field_store_row};
pub(crate) use fields::{site_read_row, stores_field};
pub(crate) mod number_rows;
pub(crate) use number_rows::{numeric_row, staged_binary, staged_unary};

/// The row kind of a `Binary`'s operator class (`staged::BinaryClass`, the one classification of the operators):
/// an arithmetic, bitwise or remainder operator's row. A relation as a value, or any other operator, has no Number row.
pub(crate) fn kind_of(class: BinaryClass) -> Option<Kind> {
    use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise};
    Some(match class {
        BinaryClass::Arithmetic(NumberArithmetic::Add) => Kind::Add,
        BinaryClass::Arithmetic(NumberArithmetic::Subtract) => Kind::Sub,
        BinaryClass::Arithmetic(NumberArithmetic::Multiply) => Kind::Mul,
        BinaryClass::Arithmetic(NumberArithmetic::Divide) => Kind::Div,
        BinaryClass::Remainder => Kind::Rem,
        BinaryClass::Bitwise(NumberBitwise::And) => Kind::And,
        BinaryClass::Bitwise(NumberBitwise::Or) => Kind::Or,
        BinaryClass::Bitwise(NumberBitwise::Xor) => Kind::Xor,
        BinaryClass::Bitwise(NumberBitwise::ShiftLeft) => Kind::Shl,
        BinaryClass::Bitwise(NumberBitwise::ShiftRight) => Kind::Shr,
        BinaryClass::Bitwise(NumberBitwise::ShiftRightUnsigned) => Kind::UShr,
        BinaryClass::Relation(_) | BinaryClass::Other => return None,
    })
}

/// The row kind of a Number relation: the compare-and-jump rows and the relational `kind_of` both take it.
pub(crate) fn relation_kind(relation: quench_runtime::vm::operations::NumberRelation) -> Kind {
    use quench_runtime::vm::operations::NumberRelation;
    match relation {
        NumberRelation::LessThan => Kind::Lt,
        NumberRelation::LessEqual => Kind::Le,
        NumberRelation::GreaterThan => Kind::Gt,
        NumberRelation::GreaterEqual => Kind::Ge,
        NumberRelation::Equal => Kind::Eq,
        NumberRelation::NotEqual => Kind::Ne,
    }
}

/// An operand: a register of the activation or a constant `Value` word.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    Register(Register),
    Constant(u64),
    /// A constant that is no Number: a tagged word (`undefined`, a boolean, `null`, a string or a
    /// BigInt the program holds) a row writes as it is, whose register is tagged whatever else holds.
    Value(u64),
}

/// The receiver a folded prototype-held read stands on (`Heap::Held`): the exact shape and the
/// prototype link the row proves of the receiver before it answers the value the holder held (the
/// row's `hole`), the holder's exact shape, whose values the code holds, and the `depth` prototypes
/// from the link the answer was read through, whose layouts it holds (Stable facts).
#[derive(Clone, Copy)]
pub(crate) struct Folded {
    pub(crate) shape: u32,
    pub(crate) link: u32,
    pub(crate) holder: u32,
    pub(crate) depth: u8,
}

impl Folded {
    /// The chain the answer was read through (`StableFacts::chains`).
    pub(crate) fn chain(self) -> quench_runtime::code_dependencies::PrototypeChain {
        quench_runtime::code_dependencies::PrototypeChain { link: self.link, depth: self.depth }
    }

    /// The row's receiver word: the shape, and the link above `HELD_LINK_AT` bits.
    pub(crate) fn word(self) -> u64 {
        u64::from(self.shape) | u64::from(self.link) << numeric::site_bits::HELD_LINK_AT
    }
}

/// An instruction as a row: its kind, destination, operands and branch target.
pub(crate) struct Decoded {
    pub(crate) kind: Kind,
    pub(crate) dst: Option<Register>,
    /// The second register a step writes: the `this` a name call defines beside its callee
    /// (`STEP_WRITES`). Its value is a tagged word the step made in its frame slot.
    pub(crate) also: Option<Register>,
    pub(crate) a: Option<Source>,
    pub(crate) b: Option<Source>,
    pub(crate) target: Option<u32>,
    /// A heap row (`base_rows`) in place of the numeric `kind`, with the array register a store
    /// writes through and the entry-record byte offset a field read loads.
    pub(crate) heap: Option<Heap>,
    pub(crate) base: Option<Register>,
    pub(crate) hole: u64,
    /// How an instruction no row takes runs (`steps`), in place of the numeric `kind`.
    pub(crate) step: Option<Step>,
    /// The instruction a helper step runs packed (`WideInstruction::words`), where a family covers its opcode
    /// and the helper table has the packed helper (`steps::packed_words`).
    pub(crate) packed: Option<[u64; 2]>,
    /// The atom a field store holds unnamed by method caches (`Known::stores`), a Stable fact.
    pub(crate) atom: Option<Atom>,
    /// The receiver a folded prototype-held read stands on, with its value in `hole`.
    pub(crate) folded: Option<Folded>,
    /// A named read of a class-typed parameter's prototype value (`declared::class_member_read`): a Linked held read
    /// that `proofs::prove` makes a constant load where the receiver's shape is proven at the row.
    pub(crate) class_read: bool,
    /// The recorded transition an add row runs (`adds`), with the form its storage takes and whether
    /// the receiver is the activation's `this`.
    pub(crate) add: Option<adds::Add>,
    /// The recorded naming a `SetFunctionName` row runs (`names`).
    pub(crate) name: Option<names::Name>,
    /// A `ToNumeric` copy: its operand is no Number until the row proves it (a loop's entry guards
    /// prove the registers it names; a function's registers are not proved).
    pub(crate) checked: bool,
    /// The instruction carried the return flag: the activation completes with the result the row
    /// writes (`steps::completing`), so control reaches nothing after it.
    pub(crate) completes: bool,
    /// The constructor an `instanceof` fast arm stands on, with what the VM proved of it (`Known::instanceofs`): its
    /// identity is the row's `hole`, which the constructor register must hold, and the prototype link the walk looks for
    /// is the fact's, a Stable fact the code depends on (`held_instanceofs`).
    pub(crate) instanceof: Option<(Value, InstanceofFact)>,
    /// The global object's slot the row holds under the object's layout and the slot's observed type
    /// (`globals`): Stable facts the code takes.
    pub(crate) global: Option<Access>,
    /// The global lexical binding's word the row reads or stores live (`globals`), at the address in `hole`, under the global
    /// object's layout: a Stable fact the code takes.
    pub(crate) lexical: Option<LexicalSlot>,
    /// The observed type of the own slot a field store writes, where its site proves one own load
    /// (`NativeFieldLoad::ty`).
    pub(crate) slot: Option<quench_runtime::typed_ir::Type>,
    /// The exact shape whose slot `slot` is the type of, for a field store: an ungated store row
    /// writes only into a receiver of this shape (`plan::site_holes`, hole 4), as the site's live
    /// cache entry may name another shape, whose slot type nothing proved.
    pub(crate) slot_shape: Option<u32>,
    /// A site read's or store's one own flat entry (`decode::fields::ic_hole`: the exact shape, and the
    /// slot's byte offset above 32 bits), whose inline-cache row runs first and falls to the site row
    /// on a miss (`plan::ic_plan`).
    pub(crate) ic: Option<u64>,
    /// The step a store row's instruction runs as where the row declines its operand (`declines`):
    /// the instruction's own (`steps::lowering`).
    pub(crate) declined: Option<Step>,
    /// The exact shape of the holder whose own native function the row stands on (`math_call`'s
    /// identity guard): a Stable fact the code takes (`held_holders`), so the function's identity is
    /// not recycled while the code runs.
    pub(crate) holder: Option<u32>,
    /// The Stable cell of the global member the row folds (`GlobalRead::NativeMember`'s `cell`, or a held object's read): its fact is
    /// subscribed by the body (`Stitched::push`), and its realm proved once (`globals::held_realm`).
    /// The slot type the fact store proves of a site read's one own flat entry (`decode::fields::cached_read`): a
    /// Number type or the direct-object family, which the code holds as a dependency (`held_slot_types`), and a
    /// widening of the slot retires it over. A Number read's inline-cache row then delivers the word to a pinned
    /// double with no tag test (`plan::ic_plan`); an object read's destination is a direct object after it
    /// (`proofs::typed_object`). Kept only where that row runs and the code makes no call (`repr::keep_typed_reads`).
    pub(crate) typed: Option<quench_runtime::code_dependencies::SlotDependency>,
    pub(crate) cell: Option<quench_runtime::vm::global_facts::StableCell>,
    /// Every path proves the inline-cache access's receiver of its entry's shape (`proofs::prove`): the row
    /// reads or writes the slot with no guard.
    pub(crate) proven: bool,
    /// Where `proven`, the receiver's storage form is proven flat too (`proofs::prove`): the row's slot is the arena run's,
    /// with no compare of the auxiliary word (`Heap::IcKnownGetFlat`, `Heap::IcKnownSetFlat`).
    pub(crate) flat: bool,
    /// Some receiver proof reaches the row (`proofs::prove`), which a later row may stand on: the code
    /// is not resumed here.
    pub(crate) relies: bool,
    /// A proven store whose run-level stores test the region's entry made, and no call since (`proofs::prove`): the row
    /// tests nothing of its own (`Heap::IcKnownSet`'s `admitted`), or the test `recheck` makes before it.
    pub(crate) admitted: bool,
    /// A proven store a call cleared the run-level stores test for, which re-reads the run's stores word
    /// before it (`Heap::Stores`, a mid-body guard), so the stores after it are admitted (`proofs::prove`).
    pub(crate) recheck: bool,
    /// A generic `Binary` (its operator is data, so its step helper runs any operands): a numeric row whose site is a
    /// string `+` (`Variant::Concat`) keeps the region running through that helper as its miss (`plan::concat_run`).
    pub(crate) generic: bool,
    /// Operand a's frame slot holds an int32 on every path into this row (`numbers::proven_int_operands`).
    pub(crate) int_a: bool,
    /// Operand b's frame slot holds an int32 on every path into this row (`numbers::proven_int_operands`).
    pub(crate) int_b: bool,
}

impl Decoded {
    pub(crate) fn number(kind: Kind, dst: Option<Register>, a: Option<Source>, b: Option<Source>, target: Option<u32>) -> Self {
        Self { kind, dst, also: None, a, b, target, heap: None, base: None, hole: 0, atom: None, folded: None, class_read: false, step: None, packed: None, checked: false, completes: false, instanceof: None, global: None, lexical: None, add: None, name: None, slot: None, slot_shape: None, ic: None, declined: None, holder: None, typed: None, cell: None, proven: false, flat: false, relies: false, admitted: false, recheck: false, generic: false, int_a: false, int_b: false }
    }

    pub(crate) fn heap(heap: Heap, dst: Option<Register>, a: Option<Source>, b: Option<Source>) -> Self {
        Self { heap: Some(heap), ..Self::number(Kind::Move, dst, a, b, None) }
    }

    pub(crate) fn step(step: Step, dst: Option<Register>) -> Self {
        Self { step: Some(step), ..Self::number(Kind::Move, dst, None, None, None) }
    }

    /// Whether the row holds a Stable fact its code took: a folded prototype-held read, an
    /// `instanceof` of a constructor's `prototype`, a global object's slot, an add, or a store in place for an atom no method cache
    /// names. A change of the fact retires the code, and a run already inside it learns that from the invalidation word (`Heap::Live`).
    pub(crate) fn holds_stable(&self) -> bool {
        self.add.is_some() || self.folded.is_some() || self.holder.is_some() || matches!(self.heap, Some(Heap::SiteStore | Heap::ThisStore)) || self.instanceof.is_some() || self.global.is_some() || self.lexical.is_some()
    }

    /// The observed type of the slot a store row writes in place: a global object's slot or a field's.
    fn stored_slot_type(&self) -> Option<quench_runtime::typed_ir::Type> {
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
        (arm.holds.is_none() || tested && arm.holds == Some(quench_runtime::typed_ir::Type::INT32)).then_some(arm)
    }

    /// What a tested store (`store_arm` of a frame word) admits besides an int32: a double, where
    /// the slot's observed type holds Number, so the store widens no observation and logs nothing.
    pub(crate) fn admitted_tags(&self) -> u64 {
        if self.stored_slot_type().is_some_and(|ty| quench_runtime::typed_ir::Type::NUMBER.is_subtype(ty)) {
            numeric::site_bits::STORE_ADMITS_DOUBLES
        } else {
            numeric::site_bits::STORE_ADMITS_INT32
        }
    }

    /// The step a store row runs as where it cannot write its operand in `place` (`store_arm`): the
    /// instruction's own.
    pub(crate) fn declines(&self, place: Place) -> Option<Step> {
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
    pub(crate) fn gated(&self, place: Place) -> bool {
        match self.store_arm(place) {
            None => true,
            Some(arm) => arm.holds.is_none() && !value_fact(place).is_subtype(quench_runtime::typed_ir::Type::IMMEDIATE),
        }
    }

    /// Whether the row makes a call or a step the interpreter's operation runs, either of which may
    /// run guest code that changes a Stable fact.
    pub(crate) fn makes_calls(&self) -> bool {
        matches!(self.step, Some(Step::Call(_))) || self.step.is_some_and(Step::runs_helper)
    }

    /// Whether the plan gives the row an inline-cache row ahead of its site row (`plan::ic_plan`): a site read or store
    /// with a cache entry, naming no second register and not completing. The proofs and the plan both ask this, so the
    /// inline-cache row is taken exactly where the proofs credit its hit.
    pub(crate) fn takes_ic_row(&self) -> bool {
        self.ic.is_some() && matches!(self.heap, Some(Heap::Site | Heap::SiteStore)) && self.also.is_none() && !self.completes
    }

    /// The registers the row names.
    pub(crate) fn registers(&self) -> impl Iterator<Item = Register> {
        [self.dst, self.also, self.a.and_then(register_of), self.b.and_then(register_of), self.base].into_iter().flatten()
    }
}

/// The Number a function's constant table holds at an index: the caller's for a region's own
/// instructions, the callee's for an inlined body (`inline_plan`).
pub(crate) type Numbers<'a> = &'a dyn Fn(usize) -> Option<f64>;

pub(crate) fn source(numbers: Numbers, operand: quench_runtime::bytecode::Operand) -> Result<Source, String> {
    match operand.kind() {
        Some(OperandKind::Register) => Ok(Source::Register(operand.payload())),
        Some(OperandKind::Constant) => numbers(usize::from(operand.payload()))
            .map(|number| Source::Constant(Value::number(number).to_bits()))
            .ok_or_else(|| "operand: constant that is no Number".to_string()),
        other => Err(format!("operand: {other:?}")),
    }
}

/// The staged operation `instruction` denotes, decoded from its own fields alone (`DecodeContext::EMPTY`):
/// the dispatch key of the planner's rows. `None` where the opcode has no such operation without a table
/// or no decoder; those rows keep their class arm (a derivation debt, marked where they are).
pub(crate) fn staged_of(instruction: WideInstruction) -> Option<Operation<(), ()>> {
    Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY)
}

pub(crate) fn binary(numbers: Numbers, instruction: WideInstruction) -> Result<Decoded, String> {
    let operator = instruction.binary_operator();
    let kind = kind_of(BinaryClass::of_operator(operator)).ok_or_else(|| format!("{:?}:{operator}", instruction.op()))?;
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
pub(crate) fn relation_jump(numbers: Numbers, instruction: WideInstruction, comparison: quench_runtime::staged::compare::Comparison,
    (left, right): (quench_runtime::bytecode::Operand, quench_runtime::bytecode::Operand)) -> Result<Decoded, String> {
    use quench_runtime::staged::compare::Comparison;
    let kind = match comparison {
        Comparison::Relation { relation, .. } => relation_kind(relation),
        Comparison::Equality { negated: false, .. } => Kind::Eq,
        Comparison::Equality { negated: true, .. } => Kind::Ne,
        _ => return Err("comparison: not a Number relation".to_string()),
    };
    Ok(Decoded::number(kind, None, Some(source(numbers, left)?), Some(source(numbers, right)?), Some(instruction.jump_target())))
}

/// The row of a comparison jump no Number relation names (`BIT_TEST`, `NULLISH_EQ`/`NULLISH_NE`, `BOOL_TRUE`/`BOOL_FALSE`):
/// `if (a & b)`, a loose test against `null` or `undefined` (`nullish_comparison`, the one derivation the lane
/// shares), and `a === true` or `a === false` against a constant Boolean. The row runs the staged step of the same
/// comparison, so each test reads only the operands it names.
pub(crate) fn test_row(numbers: Numbers, known: Known, instruction: WideInstruction) -> Option<Decoded> {
    use quench_runtime::staged::compare::Comparison;
    use quench_runtime::vm::operations::NumberBitwise;
    let Some(Operation::CompareJump { comparison, left, right, negate: true }) = staged_of(instruction) else { return None };
    let target = Some(instruction.jump_target());
    match comparison {
        Comparison::Bitwise(NumberBitwise::And) =>
            Some(Decoded::number(Kind::BitTest, None, Some(source(numbers, left).ok()?), Some(source(numbers, right).ok()?), target)),
        Comparison::Equality { strict: false, .. } => {
            let nullish = crate::compiled::nullish_comparison(&known.program.constants, instruction, |_| quench_runtime::typed_ir::Type::ANY)?;
            let kind = if nullish.negated { Kind::NullishNe } else { Kind::NullishEq };
            Some(Decoded::number(kind, None, Some(Source::Register(nullish.source)), None, target))
        }
        Comparison::Equality { strict: true, negated: false } => boolean_test(known, (left, right)).map(|(kind, register)| {
            Decoded::number(kind, None, Some(Source::Register(register)), None, target)
        }),
        _ => None,
    }
}

/// The Boolean identity test a register `===` a constant Boolean is: its row kind and the register.
fn boolean_test(known: Known, (left, right): (quench_runtime::bytecode::Operand, quench_runtime::bytecode::Operand)) -> Option<(Kind, Register)> {
    let constant = |operand: quench_runtime::bytecode::Operand| (operand.kind() == Some(OperandKind::Constant))
        .then(|| known.constants.get(usize::from(operand.payload())).copied()).flatten().filter(|value| *value != Value::DELETED);
    let (register, expected) = match (left.register_index(), right.register_index()) {
        (Some(register), None) => (register, constant(right)?),
        (None, Some(register)) => (register, constant(left)?),
        _ => return None,
    };
    if expected == Value::TRUE {
        Some((Kind::BoolTrue, register))
    } else if expected == Value::FALSE {
        Some((Kind::BoolFalse, register))
    } else {
        None
    }
}

pub(crate) fn load_const(numbers: Numbers, destination: Register, index: usize) -> Result<Decoded, String> {
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
/// makes where the run's slots admit every direct store (the stores bit of the window's `CONTEXT_BODY` word). Where they do not, the slot
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

/// `WasmBranchTable` (`Step::Switch`): the selector register (`a`) and the table's slot count (`hole`). Its entries are
/// the `Jump`s after it, slots `0..=hole`; the planner lays a compare tree over them (`function::switch`).
fn switch_row(instruction: WideInstruction) -> Option<Decoded> {
    (instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::BranchTable).then(|| Decoded {
        hole: u64::from(instruction.imm()),
        a: Some(Source::Register(instruction.register_a())),
        ..Decoded::step(Step::Switch, None)
    })
}

/// The heap instructions: a capture, a dense element read or store, the proof that a base is an
/// array, the activation's `this` and its local slots, and the named ones (`named_row`).
pub(crate) fn heap_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match staged_of(instruction) {
        Some(Operation::LoadCapture { .. }) => capture_load_row(facts, instruction),
        Some(Operation::StoreCapture { .. }) => capture_store_row(facts, instruction),
        Some(Operation::ElementRead { .. }) => index_read_row(region, facts, instruction),
        Some(Operation::ElementWrite { .. }) => Some(index_write_row(region, facts, instruction)),
        Some(Operation::LoadThis { .. }) => this_row(instruction),
        Some(Operation::LoadLocalPlain { .. }) => load_local_row(instruction),
        Some(Operation::StoreLocalPlain { result: None, .. }) => store_local_row(instruction),
        _ => named_row(region, facts, known, pc, instruction),
    }
}

/// The heap instructions that name a field or a global: a recorded or cached field read or store, a
/// membership test, and a global name's access (`globals`).
fn named_row(region: &LoopRegion, facts: Facts, known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match staged_of(instruction) {
        Some(Operation::GetField { .. }) if !instruction.returns_from_frame() && !instruction.writes_current_this() => field_row(region, facts, known, pc, instruction),
        // derivation debt: no staged::Operation for a Binary with a Number constant operand (`Operation::Binary` takes registers only)
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Binary) => membership_row(known, pc, instruction, instruction.result_register()),
        _ if instruction.op().traits().has(quench_runtime::bytecode::Traits::GLOBAL_NAME_ACCESS) => global_row(known, pc, instruction),
        _ if instruction.op().is_field_store() => field_store_row(known, instruction),
        _ => None,
    }
}

/// The row of a `LoadConst` of a constant that is no Number (`undefined`, `null`, a boolean, a
/// string, a BigInt): the tagged word the compiling VM holds for it, written as it is
/// (`Source::Value`), where the VM has the program's constants (`Known::constants`).
fn value_row(known: Known, instruction: WideInstruction) -> Option<Decoded> {
    // The deleted marker is no constant's value: an image's table holds it for a constant it cannot name (`image_body`).
    let table = |index: usize| known.constants.get(index).filter(|value| **value != Value::DELETED).map(|value| Constant::untyped(*value));
    let context = DecodeContext { constant_at: Some(&table), ..DecodeContext::EMPTY };
    let Some(Operation::LoadConst { destination, constant }) = Operation::<(), ()>::decode(instruction, context) else { return None };
    (instruction.result_flags() == 0).then(|| Decoded::number(Kind::Move, Some(destination), Some(Source::Value(constant.value.to_bits())), None, None))
}

/// The row of `instruction` at `pc`: a numeric or heap row where one takes it, else the step the
/// interpreter's own operation makes (`steps`), so no instruction is refused.
pub(crate) fn decode(region: &LoopRegion, (facts, known): (Facts, Known), pc: u32, instruction: WideInstruction, helpers: LaneHelpers) -> Decoded {
    numeric_row(&|index| region.number_constant(index), instruction)
        .flatten()
        .or_else(|| test_row(&|index| region.number_constant(index), known, instruction))
        .or_else(|| switch_row(instruction))
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

/// Bits in a word's high half.
pub(crate) const HALF: u32 = 32;
/// The high 32 bits of an int word.
pub(crate) const INT_HIGH: u64 = quench_runtime::value::ValueLayout::INT_TAG >> HALF;

pub(crate) fn register_of(source: Source) -> Option<Register> {
    match source {
        Source::Register(register) => Some(register),
        Source::Constant(_) | Source::Value(_) => None,
    }
}
