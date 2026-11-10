//! A derived constructor whose body constructs its parent, then only initializes `this`
//! (`Function::super_initializer`).
use super::{
    AtomTable, Constant, ConstructArguments, Function, ImmediateLayout, InitializerConstant, InitializerStore,
    InitializerValue, Op, Register, SUPER_BASE_BINDING, WideInstruction,
};
use smallvec::SmallVec;

/// `constructor(a, b) { super(a, 0); this.f = b; this.g = null; }`: what the parent's
/// parameters take, in order, then the stores the body makes to the receiver the parent
/// returned. Every value is a parameter as passed or a primitive constant, so the body runs
/// no guest code of its own.
pub(crate) struct SuperInitializer {
    pub(crate) arguments: SmallVec<[InitializerValue; 4]>,
    pub(crate) stores: SmallVec<[InitializerStore; 4]>,
}

/// Whether `call` reads the active function's parent: the `SUPER_BASE_BINDING` intrinsic
/// `base` called with the `undefined` receiver `receiver` and the function `active` loaded,
/// alone in the call's window at `window`.
pub(super) fn calls_super_base(
    atoms: &AtomTable,
    constants: &[Constant],
    [active, base, receiver, call]: [&WideInstruction; 4],
    window: Option<Register>,
) -> bool {
    active.op() == Op::LoadCapture
        && base.op() == Op::LoadName
        && usize::try_from(base.atom_index()).ok().and_then(|atom| atoms.get(atom)) == Some(SUPER_BASE_BINDING)
        && receiver.op() == Op::LoadConst
        && matches!(constants.get(receiver.constant_index()), Some(Constant::Undefined))
        && call.op() == Op::Call
        && call.b() == base.result_register()
        && call.c() == receiver.result_register()
        && ImmediateLayout::argument_count(call.imm()) == 1
        && Some(ImmediateLayout::call_window_base(call.imm())) == window
}

/// The temporaries such a body wrote, latest last: a parameter's load or a primitive
/// constant's.
struct Temporaries<'a> {
    params: u16,
    constants: &'a [Constant],
    written: SmallVec<[(Register, InitializerValue); 8]>,
}

impl Temporaries<'_> {
    fn value(&self, register: Register) -> Option<InitializerValue> {
        self.written.iter().rev().find(|(written, _)| *written == register).map(|(_, value)| *value)
    }

    /// Records `instruction` when it writes a temporary these model, which writes nothing else.
    fn write(&mut self, instruction: &WideInstruction) -> bool {
        if instruction.result_flags() != 0 {
            return false;
        }
        let value = match instruction.op() {
            Op::LoadLocalPlain => u16::try_from(instruction.local_slot())
                .ok()
                .filter(|parameter| *parameter < self.params)
                .map(|parameter| InitializerValue::Parameter { parameter, fallback: None }),
            Op::LoadConst => InitializerConstant::of(self.constants, instruction.constant_index()).map(InitializerValue::Constant),
            _ => None,
        };
        value.map(|value| self.written.push((instruction.result_register(), value))).is_some()
    }

    /// Records the temporaries `records` writes next, and returns the record after them.
    fn written_until(&mut self, records: &mut impl Iterator<Item = WideInstruction>) -> Option<WideInstruction> {
        records.find(|record| !self.write(record))
    }
}

/// The register a body's first records leave the active function's parent in
/// (`calls_super_base`), whose window the rewrite may have filled with a copy.
fn super_base(records: &mut impl Iterator<Item = WideInstruction>, atoms: &AtomTable, constants: &[Constant]) -> Option<Register> {
    let (active, base, receiver, mut call) = (records.next()?, records.next()?, records.next()?, records.next()?);
    let mut window = active.result_register();
    if call.op() == Op::Move && call.result_flags() == 0 && call.register_b() == window {
        window = call.result_register();
        call = records.next()?;
    }
    calls_super_base(atoms, constants, [&active, &base, &receiver, &call], Some(window)).then(|| call.result_register())
}

impl Function {
    /// Whether the body is the return of `undefined` alone, as a class without a constructor
    /// or with an empty one has it.
    pub(crate) fn only_returns_undefined(&self, constants: &[Constant]) -> bool {
        let mut records = self.instructions();
        let mut temporaries = Temporaries { params: self.params, constants, written: SmallVec::new() };
        self.handlers.is_empty()
            && temporaries.written_until(&mut records).is_some_and(|record| returns_undefined(&temporaries, &record))
            && records.next().is_none()
    }

    /// The body as a `SuperInitializer`, for a derived constructor of simple parameters with
    /// no fields or private elements of its own (`instance_initializer`): the parent read
    /// (`super_base`), the `super(...)` construction of parameters and primitive constants,
    /// the receiver's binding, stores of such values to `this`, and the return of `undefined`.
    pub(crate) fn super_initializer(&self, atoms: &AtomTable, constants: &[Constant]) -> Option<SuperInitializer> {
        if !self.derived_constructor
            || !self.handlers.is_empty()
            || self.arguments_slot.is_some()
            || self.instance_initializer.is_some()
            || self.rest
            || !self.simple_parameters
        {
            return None;
        }
        let mut records = self.instructions();
        let parent = super_base(&mut records, atoms, constants)?;
        let mut temporaries = Temporaries { params: self.params, constants, written: SmallVec::new() };
        let construct = temporaries.written_until(&mut records).filter(|record| record.op() == Op::Construct)?;
        let ConstructArguments::Registers(window) = construct.construct_arguments() else {
            return None;
        };
        let (check, binds) = (records.next()?, records.next()?);
        let constructs = construct.is_super_construct()
            && construct.result_flags() == 0
            && construct.b() == parent
            && check.op() == Op::SuperCallCheck
            && binds.op() == Op::InitializeThis
            && binds.a() == construct.result_register();
        if !constructs {
            return None;
        }
        let arguments = (0..window.count).map(|index| temporaries.value(window.base + index)).collect::<Option<_>>()?;
        let stores = this_stores(&mut temporaries, &mut records)?;
        Some(SuperInitializer { arguments, stores })
    }
}

/// Whether `record` returns a temporary holding `undefined`.
fn returns_undefined(temporaries: &Temporaries<'_>, record: &WideInstruction) -> bool {
    record.op() == Op::Return && temporaries.value(record.register_a()) == Some(InitializerValue::Constant(InitializerConstant::Undefined))
}

/// The stores to `this` of the temporaries `records` writes, up to the return of `undefined`
/// that ends them.
fn this_stores(temporaries: &mut Temporaries<'_>, records: &mut impl Iterator<Item = WideInstruction>) -> Option<SmallVec<[InitializerStore; 4]>> {
    let mut stores = SmallVec::new();
    loop {
        let record = temporaries.written_until(records)?;
        if !record.stores_to_this() {
            return (returns_undefined(temporaries, &record) && records.next().is_none()).then_some(stores);
        }
        stores.push(InitializerStore { atom: record.imm(), site: record.c(), value: temporaries.value(record.register_a())? });
    }
}
