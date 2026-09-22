pub type Atom = u32;
pub type Register = u16;

mod atoms;
mod control_flow;
mod instruction;
mod numeric_ops;
pub(crate) use atoms::AtomTable;
pub use instruction::Instr;
pub(crate) use instruction::WideInstruction;
pub(crate) use numeric_ops::specialized_numeric_op;
pub(crate) const RETURN_REGISTER: Register = 1 << 15;
pub(crate) const SET_THIS_REGISTER: Register = 1 << 14;
pub(crate) const REGISTER_MASK: Register = SET_THIS_REGISTER - 1;
pub(crate) const NUMERIC_LOCAL_INC_STORE: u16 = 1;
pub(crate) const NUMERIC_LOCAL_TARGET: u16 = SET_THIS_REGISTER;
#[derive(Clone, Debug)]
pub enum Constant {
    Number(f64),
    String(String),
    StringUnits(Vec<u16>),
    BigInt(String),
    Boolean(bool),
    Null,
    Undefined,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Effect(u8);

impl Effect {
    const PURE: Self = Self(0);
    const READS_HEAP: Self = Self(1 << 0);
    const WRITES_HEAP: Self = Self(1 << 1);
    const THROWS: Self = Self(1 << 2);
    const CONTROL: Self = Self(1 << 3);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

macro_rules! opcodes {
    ($($name:ident => $effect:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u16)]
        pub enum Op { $($name),+ }

        #[allow(dead_code)]
        impl Op {
            pub const COUNT: usize = [$(stringify!($name)),+].len();
            pub const NAMES: [&'static str; Self::COUNT] = [$(stringify!($name)),+];
            const EFFECTS: [Effect; Self::COUNT] = [$($effect),+];

            pub(crate) const fn effect(self) -> Effect {
                Self::EFFECTS[self as usize]
            }
        }
    };
}
const READ_THROW: Effect = Effect::READS_HEAP.union(Effect::THROWS);
const WRITE_THROW: Effect = Effect::WRITES_HEAP.union(Effect::THROWS);
const CALL_EFFECT: Effect = READ_THROW.union(Effect::WRITES_HEAP);

opcodes!(
    Nop => Effect::PURE,
    Wide => Effect::PURE,
    LoadConst => Effect::PURE,
    LoadLocal => Effect::PURE,
    StoreLocal => Effect::PURE,
    LoadEnvLocal => Effect::READS_HEAP,
    StoreEnvLocal => Effect::WRITES_HEAP,
    LoadCapture => Effect::READS_HEAP,
    StoreCapture => Effect::WRITES_HEAP,
    LoadName => READ_THROW,
    StoreName => WRITE_THROW,
    LoadThis => Effect::PURE,
    MakeClosure => CALL_EFFECT,
    MakeArray => CALL_EFFECT,
    MakeConstArray => CALL_EFFECT,
    MakeObject => CALL_EFFECT,
    MakeObject2 => CALL_EFFECT,
    SuperConstArrayObject2 => CALL_EFFECT,
    GetIterator => READ_THROW,
    GetAsyncIterator => READ_THROW,
    Await => READ_THROW.union(Effect::CONTROL),
    Yield => READ_THROW.union(Effect::CONTROL),
    GetField => READ_THROW,
    GetIndex => READ_THROW,
    SetField => WRITE_THROW,
    SetThisField => WRITE_THROW,
    SetIndex => WRITE_THROW,
    Binary => READ_THROW,
    IncDec => READ_THROW,
    Unary => READ_THROW,
    Move => Effect::PURE,
    Call => CALL_EFFECT,
    CallKnown => CALL_EFFECT,
    CallMethod => CALL_EFFECT,
    CallThisMethod => CALL_EFFECT,
    Construct => CALL_EFFECT,
    Jump => Effect::CONTROL,
    JumpFalse => Effect::CONTROL,
    JumpBinaryFalse => READ_THROW.union(Effect::CONTROL),
    Return => Effect::CONTROL,
    Throw => Effect::THROWS.union(Effect::CONTROL),
    NumericAdd => READ_THROW,
    NumericMultiply => READ_THROW,
);
#[derive(Clone, Debug)]
pub struct Function {
    pub parent: Option<u32>,
    pub name: Option<Atom>,
    pub params: u16,
    pub rest: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub arguments_slot: Option<u16>,
    pub locals: u16,
    pub code: Vec<Instr>,
    pub(crate) wide: Vec<WideInstruction>,
    pub registers: u16,
    pub(crate) dispatch: DispatchClass,
    pub(crate) handlers: Vec<Handler>,
    pub(crate) register_root_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DispatchClass {
    General,
    Numeric,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MethodSite {
    pub atom: Atom,
    pub cache: u16,
    pub argument_start: u32,
    pub argument_count: u16,
    pub receiver_path: Option<(Atom, u16)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ObjectSite {
    pub atoms: [Atom; 2],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Superinstruction {
    pub code: [Instr; 4],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Handler {
    pub start: u32,
    pub end: u32,
    pub target: u32,
    pub slot: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct FieldBase(pub(crate) u16);

impl FieldBase {
    pub(crate) const THIS: Self = Self(u16::MAX);
    pub(crate) const NESTED: u16 = u16::MAX - 1;

    pub(crate) fn register(value: Register) -> Self {
        debug_assert!(value < Self::NESTED);
        Self(value)
    }

    pub(crate) fn register_index(self) -> Option<Register> {
        (self != Self::THIS).then_some(self.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldSite {
    pub base: FieldBase,
    pub first: (Atom, u16),
    pub second: Option<(Atom, u16)>,
    pub sink: Option<(Atom, u16)>,
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub(crate) struct Operand(pub(crate) u16);

impl Operand {
    const TAG_SHIFT: u16 = 14;
    const PAYLOAD_MASK: u16 = (1 << Self::TAG_SHIFT) - 1;

    pub(crate) fn register(value: Register) -> Self {
        debug_assert!(value <= Self::PAYLOAD_MASK);
        Self(value)
    }

    pub(crate) fn constant(value: u32) -> Self {
        debug_assert!(value <= u32::from(Self::PAYLOAD_MASK));
        Self((1 << Self::TAG_SHIFT) | value as u16)
    }

    pub(crate) fn field(value: u32) -> Self {
        debug_assert!(value <= u32::from(Self::PAYLOAD_MASK));
        Self((2 << Self::TAG_SHIFT) | value as u16)
    }

    pub(crate) fn local(slot: u16) -> Self {
        debug_assert!(slot <= Self::PAYLOAD_MASK);
        Self((3 << Self::TAG_SHIFT) | slot)
    }

    pub(crate) fn tag(self) -> u16 {
        self.0 >> Self::TAG_SHIFT
    }

    pub(crate) fn payload(self) -> u16 {
        self.0 & Self::PAYLOAD_MASK
    }

    pub(crate) fn register_index(self) -> Option<Register> {
        (self.tag() == 0).then_some(self.payload())
    }
}

#[derive(Clone, Debug)]
pub struct ResidualProgram {
    pub(crate) specialized: bool,
    pub(crate) atoms: AtomTable,
    pub(crate) constants: Vec<Constant>,
    pub(crate) functions: Vec<Function>,
    pub(crate) cache_sites: u16,
    pub(crate) method_sites: Vec<MethodSite>,
    pub(crate) method_arguments: Vec<Register>,
    pub(crate) field_sites: Vec<FieldSite>,
    pub(crate) object_sites: Vec<ObjectSite>,
    pub(crate) superinstructions: Vec<Superinstruction>,
    pub(crate) register_roots: Vec<u64>,
}

#[cold]
#[inline(never)]
fn local_loads_in_bounds(code: &[Instr], wide: &[WideInstruction], locals: u16) -> bool {
    code.iter().all(|instruction| {
        instruction.op() != Op::LoadLocal || instruction.imm() < u32::from(locals)
    }) && wide.iter().all(|instruction| {
        instruction.op() != Op::LoadLocal || instruction.imm() < u32::from(locals)
    })
}

impl ResidualProgram {
    pub const FORMAT_VERSION: u8 = 13;
    pub const RUNTIME_ABI_FINGERPRINT: u64 = 0x5251_4a00_000d_0006;

    pub fn function_count(&self) -> usize {
        self.functions.len()
    }
    pub fn instruction_count(&self) -> usize {
        self.functions
            .iter()
            .map(|function| function.code.len())
            .sum()
    }
    pub fn write_binary(&self, path: &std::path::Path) -> Result<(), String> {
        binary::write_program(self, path)
    }

    pub fn read_binary(path: &std::path::Path) -> Result<Self, String> {
        binary::read_program(path)
    }
}
mod binary;
mod disassemble;
#[cfg(test)]
mod tests;
mod validate;
