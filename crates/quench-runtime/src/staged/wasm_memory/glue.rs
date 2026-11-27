#![allow(unused_imports)]
use super::*;
use crate::staged::wasm_machine::{ConcreteBytes, ConcreteMemory};
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::WasmValue;

/// The scalar access's reads and writes over bytes: the interpreter's `ConcreteBytes` and
/// `ConcreteMemory` stages, runtime only.
pub trait AccessWasmMemoryExt: Sized {
    /// The value of this scalar access at `address` of `bytes`: out of bounds traps.
    fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap>;
    /// Writes `value` of this scalar access at `address` of `bytes`: out of bounds traps before
    /// any byte changes.
    fn write(self, bytes: &mut [u8], address: u64, value: WasmValue) -> Result<(), WasmTrap>;
}

impl AccessWasmMemoryExt for Access {
    fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
        let word = load(&mut ConcreteBytes { memory: bytes }, self, address, 0, false)?;
        Ok(WasmValue::from_bits(self.ty, word))
    }

    fn write(self, bytes: &mut [u8], address: u64, value: WasmValue) -> Result<(), WasmTrap> {
        store(&mut ConcreteMemory { memory: bytes }, self, address, 0, value.word(), false)
    }
}

/// The access a scalar load makes; a vector load has none. Always inlined: a handler's constant
/// load folds to its width and sign, so the bytes move as one word, not by memcpy. A const fn, so
/// a free fn: an inherent impl of `Access` cannot live outside quench-staged.
#[inline(always)]
pub const fn access_of_load(load: MemoryLoad) -> Option<Access> {
    let ty = match load {
        MemoryLoad::I32Load
        | MemoryLoad::I32Load8S
        | MemoryLoad::I32Load8U
        | MemoryLoad::I32Load16S
        | MemoryLoad::I32Load16U => WasmType::I32,
        MemoryLoad::F32Load => WasmType::F32,
        MemoryLoad::I64Load
        | MemoryLoad::I64Load8S
        | MemoryLoad::I64Load8U
        | MemoryLoad::I64Load16S
        | MemoryLoad::I64Load16U
        | MemoryLoad::I64Load32S
        | MemoryLoad::I64Load32U => WasmType::I64,
        MemoryLoad::F64Load => WasmType::F64,
        _ => return None,
    };
    let signed = matches!(
        load,
        MemoryLoad::I32Load8S
            | MemoryLoad::I32Load16S
            | MemoryLoad::I64Load8S
            | MemoryLoad::I64Load16S
            | MemoryLoad::I64Load32S
    );
    Some(Access {
        ty,
        bytes: load.width(),
        signed,
    })
}

/// The access a scalar store makes. A const fn, so a free fn (see `access_of_load`).
#[inline(always)]
pub const fn access_of_store(store: MemoryStore) -> Access {
    Access {
        ty: store.value_type(),
        bytes: store.width(),
        signed: false,
    }
}

/// The linear-memory step `op` is (`Decoder::WasmMemory`), or none for an opcode it does not name.
/// A const fn, so a free fn (see `access_of_load`).
pub const fn memory_of(op: Op) -> Option<Memory> {
    if let Some(load) = memory_load_from_direct_op(op) {
        if let Some(access) = access_of_load(load) {
            return Some(Memory::Load(access));
        }
    }
    if let Some(store) = memory_store_from_direct_op(op) {
        return Some(Memory::Store(access_of_store(store)));
    }
    match op.class() {
        Class::WasmMemorySize => Some(Memory::Size),
        Class::WasmMemoryGrow => Some(Memory::Grow),
        Class::WasmMemoryFill => Some(Memory::Fill),
        Class::WasmMemoryCopy => Some(Memory::Copy),
        _ => None,
    }
}

/// The registers a memory step names, read from its bytecode: runtime only.
pub trait MemoryWasmMemoryExt: Sized {
    /// The registers naming the memory the operation reads: its binding register.
    fn memories(self, instruction: WideInstruction) -> [Option<u16>; 2];
    /// The registers holding 32-bit addresses, lengths and bytes the operation reads.
    fn operands(self, instruction: WideInstruction) -> [Option<u16>; 3];
}

impl MemoryWasmMemoryExt for Memory {
    fn memories(self, instruction: WideInstruction) -> [Option<u16>; 2] {
        match self {
            Self::Load(_) | Self::Store(_) | Self::Size | Self::Grow => {
                [Some(instruction.b()), None]
            }
            Self::Fill => [Some(instruction.a()), None],
            Self::Copy => [Some(instruction.a()), Some(instruction.b())],
        }
    }

    fn operands(self, instruction: WideInstruction) -> [Option<u16>; 3] {
        match self {
            Self::Load(_) | Self::Store(_) | Self::Grow => [Some(instruction.c()), None, None],
            Self::Size => [None, None, None],
            Self::Fill => {
                let (destination, _) = instruction.register_pair();
                [
                    Some(instruction.b()),
                    Some(instruction.c()),
                    Some(destination),
                ]
            }
            Self::Copy => {
                let (destination, source) = instruction.register_pair();
                [Some(destination), Some(source), Some(instruction.c())]
            }
        }
    }
}

/// The register of the one memory `instruction` addresses through a binding the frame resolves
/// once: a direct access's and `memory.size`'s `b`, `memory.fill`'s `a`. `None` for any other
/// instruction, a grow (whose call-out reads the memory again) and a copy (which names two)
/// included.
pub fn bound_memory_register(instruction: WideInstruction) -> Option<crate::bytecode::Register> {
    let op = instruction.op();
    if memory_load_from_direct_op(op).is_some() || memory_store_from_direct_op(op).is_some() {
        return Some(instruction.b());
    }
    match memory_of(op)? {
        Memory::Size => Some(instruction.b()),
        Memory::Fill => Some(instruction.a()),
        Memory::Load(_) | Memory::Store(_) | Memory::Grow | Memory::Copy => None,
    }
}

// Scalar memory32 accesses with a static offset have one opcode each. The access semantics
// remain the MemoryLoad/MemoryStore rules in quench-staged; the opcode only names the operator so
// dispatch needs no selector decode. The two maps are free fns: an inherent impl of a type that
// quench-staged defines cannot live here.
macro_rules! direct_accesses {
    ($kind:ident, $direct:ident, $from:ident; $($name:ident => $op:ident;)+) => {
        /// The opcode of a direct access of `operator`, or none for an operator with no opcode.
        pub const fn $direct(operator: $kind) -> Option<crate::bytecode::Op> {
            match operator { $($kind::$name => Some(crate::bytecode::Op::$op),)+ #[allow(unreachable_patterns)] _ => None }
        }
        /// The operator a direct access opcode names, or none for any other opcode.
        pub const fn $from(op: crate::bytecode::Op) -> Option<$kind> {
            match op { $(crate::bytecode::Op::$op => Some($kind::$name),)+ _ => None }
        }
    }
}
direct_accesses! { MemoryLoad, memory_load_direct_op, memory_load_from_direct_op;
    I32Load => WasmI32Load;
    I64Load => WasmI64Load;
    F32Load => WasmF32Load;
    F64Load => WasmF64Load;
    I32Load8S => WasmI32Load8S;
    I32Load8U => WasmI32Load8U;
    I32Load16S => WasmI32Load16S;
    I32Load16U => WasmI32Load16U;
    I64Load8S => WasmI64Load8S;
    I64Load8U => WasmI64Load8U;
    I64Load16S => WasmI64Load16S;
    I64Load16U => WasmI64Load16U;
    I64Load32S => WasmI64Load32S;
    I64Load32U => WasmI64Load32U;
}
direct_accesses! { MemoryStore, memory_store_direct_op, memory_store_from_direct_op;
    I32Store => WasmI32Store;
    I64Store => WasmI64Store;
    F32Store => WasmF32Store;
    F64Store => WasmF64Store;
    I32Store8 => WasmI32Store8;
    I32Store16 => WasmI32Store16;
    I64Store8 => WasmI64Store8;
    I64Store16 => WasmI64Store16;
    I64Store32 => WasmI64Store32;
}
