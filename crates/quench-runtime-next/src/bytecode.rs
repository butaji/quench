pub type Atom = u32;
pub type Register = u16;

mod atoms;
mod instruction;
mod numeric_ops;
pub(crate) use atoms::AtomTable;
pub use instruction::Instr;
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
    pub locals: u16,
    pub code: Vec<Instr>,
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
fn local_loads_in_bounds(code: &[Instr], locals: u16) -> bool {
    code.iter().all(|instruction| {
        instruction.op() != Op::LoadLocal || instruction.imm() < u32::from(locals)
    })
}

impl ResidualProgram {
    pub const FORMAT_VERSION: u8 = 6;
    pub const RUNTIME_ABI_FINGERPRINT: u64 = 0x5251_4a00_0006_0002;

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
        let mut out = BinaryWriter::new();
        out.bytes.extend_from_slice(b"RQJ\0\x06");
        out.u64(Self::RUNTIME_ABI_FINGERPRINT);
        out.strings(&self.atoms);
        out.u32(self.constants.len() as u32);
        for value in &self.constants {
            match value {
                Constant::Number(value) => {
                    out.u8(0);
                    out.u64(value.to_bits());
                }
                Constant::String(value) => {
                    out.u8(1);
                    out.string(value);
                }
                Constant::BigInt(value) => {
                    out.u8(6);
                    out.string(value);
                }
                Constant::Boolean(value) => {
                    out.u8(if *value { 2 } else { 3 });
                }
                Constant::Null => out.u8(4),
                Constant::Undefined => out.u8(5),
            }
        }
        out.u32(self.functions.len() as u32);
        for function in &self.functions {
            out.option_u32(function.parent);
            out.option_u32(function.name);
            out.u16(function.params);
            out.u8(u8::from(function.rest));
            out.u16(function.locals);
            out.u16(function.registers);
            out.u8(function.dispatch as u8);
            out.u32(function.register_root_offset);
            out.u32(function.code.len() as u32);
            for instruction in &function.code {
                out.u8(instruction.op() as u8);
                out.u16(instruction.a());
                out.u16(instruction.b());
                out.u16(instruction.c());
                out.u32(instruction.imm());
            }
            out.u32(function.handlers.len() as u32);
            for handler in &function.handlers {
                out.u32(handler.start);
                out.u32(handler.end);
                out.u32(handler.target);
                out.u16(handler.slot.unwrap_or(u16::MAX));
            }
        }
        out.u16(self.cache_sites);
        out.u32(self.method_sites.len() as u32);
        for site in &self.method_sites {
            out.u32(site.atom);
            out.u16(site.cache);
            out.u32(site.argument_start);
            out.u16(site.argument_count);
            out.optional_pair(site.receiver_path);
        }
        out.u16s(&self.method_arguments);
        out.u32(self.field_sites.len() as u32);
        for site in &self.field_sites {
            out.u16(site.base.0);
            out.pair(site.first);
            out.optional_pair(site.second);
            out.optional_pair(site.sink);
        }
        out.u32(self.object_sites.len() as u32);
        for site in &self.object_sites {
            out.u32(site.atoms[0]);
            out.u32(site.atoms[1]);
        }
        out.u32(self.superinstructions.len() as u32);
        for site in &self.superinstructions {
            for instruction in site.code {
                out.u8(instruction.op() as u8);
                out.u16(instruction.a());
                out.u16(instruction.b());
                out.u16(instruction.c());
                out.u32(instruction.imm());
            }
        }
        out.u32(self.register_roots.len() as u32);
        for value in &self.register_roots {
            out.u64(*value);
        }
        std::fs::write(path, out.bytes).map_err(|error| error.to_string())
    }

    pub fn read_binary(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let mut input = BinaryReader::new(&bytes);
        input.magic(b"RQJ\0\x06")?;
        let abi = input.u64()?;
        if abi != Self::RUNTIME_ABI_FINGERPRINT {
            return Err("residual runtime ABI mismatch".into());
        }
        let atoms = input.strings()?;
        let constants = input.list(|input| match input.u8()? {
            0 => Ok(Constant::Number(f64::from_bits(input.u64()?))),
            1 => Ok(Constant::String(input.string()?)),
            6 => Ok(Constant::BigInt(input.string()?)),
            2 => Ok(Constant::Boolean(true)),
            3 => Ok(Constant::Boolean(false)),
            4 => Ok(Constant::Null),
            5 => Ok(Constant::Undefined),
            _ => Err("invalid residual constant".into()),
        })?;
        let functions = input.list(|input| {
            let parent = input.option_u32()?;
            let name = input.option_u32()?;
            let params = input.u16()?;
            let rest = match input.u8()? {
                0 => false,
                1 => true,
                _ => return Err("invalid residual rest flag".into()),
            };
            let locals = input.u16()?;
            let registers = input.u16()?;
            let dispatch = match input.u8()? {
                0 => DispatchClass::General,
                1 => DispatchClass::Numeric,
                _ => return Err("invalid residual dispatch class".into()),
            };
            let register_root_offset = input.u32()?;
            let code = input.list(|input| {
                let opcode = input.u8()?;
                if usize::from(opcode) >= Op::COUNT {
                    return Err("invalid residual opcode".into());
                }
                // SAFETY: `Op` is a contiguous repr(u16) enum generated by `opcodes!`.
                let op = unsafe { std::mem::transmute::<u16, Op>(u16::from(opcode)) };
                Instr::try_new(op, input.u16()?, input.u16()?, input.u16()?, input.u32()?)
                    .ok_or_else(|| String::from("residual instruction exceeds packed domain"))
            })?;
            if !local_loads_in_bounds(&code, locals) {
                return Err("invalid residual local load".into());
            }
            let handlers = input.list(|input| {
                let start = input.u32()?;
                let end = input.u32()?;
                let target = input.u32()?;
                let slot = input.u16()?;
                Ok(Handler {
                    start,
                    end,
                    target,
                    slot: (slot != u16::MAX).then_some(slot),
                })
            })?;
            Ok(Function {
                parent,
                name,
                params,
                rest,
                locals,
                code,
                registers,
                dispatch,
                handlers,
                register_root_offset,
            })
        })?;
        let cache_sites = input.u16()?;
        let method_sites = input.list(|input| {
            Ok(MethodSite {
                atom: input.u32()?,
                cache: input.u16()?,
                argument_start: input.u32()?,
                argument_count: input.u16()?,
                receiver_path: input.optional_pair()?,
            })
        })?;
        let method_arguments = input.u16s()?;
        let field_sites = input.list(|input| {
            Ok(FieldSite {
                base: FieldBase(input.u16()?),
                first: input.pair()?,
                second: input.optional_pair()?,
                sink: input.optional_pair()?,
            })
        })?;
        let object_sites = input.list(|input| {
            Ok(ObjectSite {
                atoms: [input.u32()?, input.u32()?],
            })
        })?;
        let superinstructions = input.list(|input| {
            let mut code = [Instr::new(Op::Nop, 0, 0, 0, 0); 4];
            for instruction in &mut code {
                let opcode = input.u8()?;
                if usize::from(opcode) >= Op::COUNT {
                    return Err("invalid residual superinstruction opcode".into());
                }
                // SAFETY: same contiguous repr(u16) invariant as normal code.
                let op = unsafe { std::mem::transmute::<u16, Op>(u16::from(opcode)) };
                *instruction =
                    Instr::try_new(op, input.u16()?, input.u16()?, input.u16()?, input.u32()?)
                        .ok_or_else(|| {
                            String::from("residual superinstruction exceeds packed domain")
                        })?;
            }
            if code.map(|instruction| instruction.op())
                != [Op::MakeConstArray, Op::Binary, Op::Binary, Op::MakeObject2]
            {
                return Err("invalid residual superinstruction pattern".into());
            }
            Ok(Superinstruction { code })
        })?;
        let register_roots = input.list(|input| input.u64())?;
        input.finish()?;
        let program = Self {
            atoms,
            constants,
            functions,
            cache_sites,
            method_sites,
            method_arguments,
            field_sites,
            object_sites,
            superinstructions,
            register_roots,
        };
        program.validate()?;
        Ok(program)
    }
}

use binary::{BinaryReader, BinaryWriter};

mod binary;
mod disassemble;
#[cfg(test)]
mod tests;
mod validate;
