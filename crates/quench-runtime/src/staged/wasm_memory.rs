//! Wasm linear-memory semantics written once over `WasmMemory`: the bounds check of an access
//! and of a bulk operation, the order in which a copy checks its ranges, and what an access
//! moves (its width, whether a narrow load sign-extends, little-endian bytes). The interpreter
//! runs them on `ConcreteMemory` (`staged::wasm_memory_bytes`, `wasm_memory_vector`), the Wasm tier on
//! Cranelift values (`Emitter`, `vm/compiled/wasm_memory_stage.rs`). A stage supplies the
//! machine: the memory's length, the words an access moves and the bulk movers.

use super::machine::MachineStage;
use super::wasm::{WasmStage, Width};
use super::wasm_concrete::ConcreteWasm;
#[cfg(not(stencil_build))]
use super::wasm_machine::{ConcreteBytes, ConcreteMemory};
#[cfg(not(stencil_build))]
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::V128_BYTES;
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::integer::I64BinaryOperator as Binary;
#[cfg(not(stencil_build))]
use crate::wasm::memory::{MemoryLoad, MemoryStore};
use crate::{WasmTrap, WasmType};
#[cfg(not(stencil_build))]
use crate::WasmValue;

/// What one scalar access moves: the type of its register value, the bytes it touches and
/// whether a narrow load sign-extends them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Access {
    pub ty: WasmType,
    pub bytes: usize,
    pub signed: bool,
}

impl Access {
    /// The word an access moves, from the `bytes` little-endian bytes it read (`raw`, zero
    /// above them): sign-extended where a narrow load asks, in the width of its type. Every
    /// stage that reads bytes (the interpreter's slices, the b-jit's raw memory) decodes here.
    #[inline(always)]
    pub(crate) fn loaded(self, raw: u64) -> u64 {
        let unused = u64::BITS - self.bytes as u32 * u8::BITS;
        let value = if self.signed { ((raw << unused) as i64) >> unused } else { raw as i64 };
        match Width::of(self.ty) {
            Some(width) => ConcreteWasm.constant(width, value),
            None => raw,
        }
    }

    /// The value of this scalar access at `address` of `bytes`: out of bounds traps.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub(crate) fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
        let word = load(&mut ConcreteBytes { memory: bytes }, self, address, 0, false)?;
        Ok(WasmValue::from_bits(self.ty, word))
    }

    /// Writes `value` of this scalar access at `address` of `bytes`: out of bounds traps before
    /// any byte changes.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub(crate) fn write(self, bytes: &mut [u8], address: u64, value: WasmValue) -> Result<(), WasmTrap> {
        store(&mut ConcreteMemory { memory: bytes }, self, address, 0, value.word(), false)
    }

    /// The access a scalar load makes; a vector load has none. Always inlined: a handler's
    /// constant load folds to its width and sign, so the bytes move as one word, not by memcpy.
    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub(crate) const fn of_load(load: MemoryLoad) -> Option<Self> {
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
        Some(Self {
            ty,
            bytes: load.width(),
            signed,
        })
    }

    /// A 128-bit vector access: a v128 load or store, which the vector steps make and the
    /// stages move as one `Vector` word.
    pub(crate) const VECTOR: Self = Self {
        ty: WasmType::V128,
        bytes: V128_BYTES,
        signed: false,
    };

    #[cfg(not(stencil_build))]
    #[inline(always)]
    pub(crate) const fn of_store(store: MemoryStore) -> Self {
        Self {
            ty: store.value_type(),
            bytes: store.width(),
            signed: false,
        }
    }
}

/// The linear-memory opcodes a function body runs.
#[cfg(not(stencil_build))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Memory {
    /// `a = load[b][c + imm]`: the memory in `b`, a 32-bit address in `c`, a static offset.
    Load(Access),
    /// `store[b][c + imm] = a`.
    Store(Access),
    /// `a = memory.size b`, in pages.
    Size,
    /// `a = memory.grow b by c pages`: the size in pages before, or -1 where the memory cannot
    /// grow. Trap-free.
    Grow,
    /// `memory.fill`: the memory in `a`, the byte in `b`, the length in `c`, the destination
    /// in the register pair.
    Fill,
    /// `memory.copy`: both memories in `a` and `b`, the length in `c`, the destination and
    /// source in the register pair.
    Copy,
}

#[cfg(not(stencil_build))]
impl Memory {
    pub(crate) const fn of(op: Op) -> Option<Self> {
        if let Some(load) = MemoryLoad::from_direct_op(op) {
            if let Some(access) = Access::of_load(load) {
                return Some(Self::Load(access));
            }
        }
        if let Some(store) = MemoryStore::from_direct_op(op) {
            return Some(Self::Store(Access::of_store(store)));
        }
        match op.class() {
            Class::WasmMemorySize => Some(Self::Size),
            Class::WasmMemoryGrow => Some(Self::Grow),
            Class::WasmMemoryFill => Some(Self::Fill),
            Class::WasmMemoryCopy => Some(Self::Copy),
            _ => None,
        }
    }

    /// Whether the operation runs a native bulk or size operation, which needs the memory's
    /// representation to be the one compiled code supports.
    pub const fn needs_compiled_memory(self) -> bool {
        matches!(self, Self::Size | Self::Grow | Self::Fill | Self::Copy)
    }

    /// Whether the operation calls out to the VM, which changes the memory's length: the
    /// program's callouts must be available, and the tier reads the length again after it.
    pub const fn needs_callouts(self) -> bool {
        matches!(self, Self::Grow)
    }
}

/// The register of the one memory `instruction` addresses through a binding the frame resolves
/// once: a direct access's and `memory.size`'s `b`, `memory.fill`'s `a`. `None` for any other
/// instruction, a grow (whose call-out reads the memory again) and a copy (which names two)
/// included.
#[cfg(not(stencil_build))]
pub fn bound_memory_register(instruction: WideInstruction) -> Option<crate::bytecode::Register> {
    let op = instruction.op();
    if MemoryLoad::from_direct_op(op).is_some() || MemoryStore::from_direct_op(op).is_some() {
        return Some(instruction.b());
    }
    match Memory::of(op)? {
        Memory::Size => Some(instruction.b()),
        Memory::Fill => Some(instruction.a()),
        Memory::Load(_) | Memory::Store(_) | Memory::Grow | Memory::Copy => None,
    }
}

/// The linear memory of the instance a function runs in, as far as reading it goes.
pub trait WasmMemory: WasmStage {
    /// The word a v128 access moves: the 16 bytes, little-endian.
    type Vector: Copy;
    /// Whether the machine faults on an access past the memory by itself, a guard region
    /// beyond the reservation, so an access whose index plus extent stays inside the
    /// reservation needs no check.
    fn guarded(&self) -> bool;
    /// The memory's length in bytes: an `I64` word.
    fn memory_length(&mut self) -> Self::Word;
    /// The `I64` index a 32-bit address word names.
    fn memory_index(&mut self, address: Self::Word) -> Self::Word;
    /// The `access` at `index + offset`, which the rule has checked.
    fn memory_load(&mut self, access: Access, index: Self::Word, offset: u32) -> Self::Word;
    /// The vector of the 16 bytes at `index + offset`, which the rule has checked.
    fn memory_load_vector(&mut self, index: Self::Word, offset: u32) -> Self::Vector;
    /// The flag that the `extent` bytes from `index` pass the end of the memory: the length
    /// compared with the end of the range, which a stage whose index is not a bounded word
    /// answers by its own range check (`checked_range`).
    #[inline(always)]
    fn range_exceeds(&mut self, index: Self::Word, extent: Self::Word) -> Self::Word {
        let end = self.operate(Binary::Add, Width::I64, index, extent);
        let length = self.memory_length();
        self.operate(Binary::GreaterUnsigned, Width::I64, end, length)
    }
}

/// The linear memory a function writes: stores and the bulk movers.
pub trait WasmMemoryWrite: WasmMemory {
    fn memory_store(&mut self, access: Access, index: Self::Word, offset: u32, value: Self::Word);
    /// Writes the vector `value` over the 16 bytes at `index + offset`, which the rule has checked.
    fn memory_store_vector(&mut self, index: Self::Word, offset: u32, value: Self::Vector);
    /// `length` copies of the low byte of the `I32` word `byte` from `index`, in range.
    fn memory_fill(&mut self, index: Self::Word, byte: Self::Word, length: Self::Word);
    /// `length` bytes from `source` to `destination`, both in range; overlapping ranges copy
    /// as if through a temporary buffer.
    fn memory_copy(&mut self, destination: Self::Word, source: Self::Word, length: Self::Word);
}

/// The linear memory a function grows: the machine grows it, as the VM does for the
/// interpreter (`LinearBytes::grow_pages`), and the length every later access reads is the new
/// one.
pub trait WasmMemoryGrow: WasmStage {
    /// Grows the memory by the `I32` word `pages` of pages: the `I32` word of its size in
    /// pages before, or -1 where it cannot grow (past its maximum, or refused by the host).
    /// Trap-free; a failed growth changes nothing.
    fn memory_grow(&mut self, pages: Self::Word) -> Self::Word;
}

/// A trap where the `extent` bytes from `index` pass the end of the memory.
#[inline(always)]
fn check_range<S: WasmMemory>(
    stage: &mut S,
    index: S::Word,
    extent: S::Word,
) -> Result<(), S::Exit> {
    let past = stage.range_exceeds(index, extent);
    stage.trap_when(past, WasmTrap::OutOfBoundsMemory)
}

/// The check of an access of `access` at `index + offset`. `proven` says the caller has shown
/// the index plus extent inside a guarded reservation, so the machine faults where it must.
#[inline(always)]
pub(crate) fn check_access<S: WasmMemory>(
    stage: &mut S,
    access: Access,
    index: S::Word,
    offset: u32,
    proven: bool,
) -> Result<(), S::Exit> {
    if proven && stage.guarded() {
        return Ok(());
    }
    let extent = stage.constant(Width::I64, i64::from(offset) + access.bytes as i64);
    check_range(stage, index, extent)
}

/// The `access` at `index + offset`, `index` being the `I64` index of the address
/// (`WasmMemory::memory_index`): out of bounds traps. The one place the check of an access is
/// written.
#[inline(always)]
pub fn load<S: WasmMemory>(
    stage: &mut S,
    access: Access,
    index: S::Word,
    offset: u32,
    proven: bool,
) -> Result<S::Word, S::Exit> {
    check_access(stage, access, index, offset, proven)?;
    Ok(stage.memory_load(access, index, offset))
}

#[inline(always)]
pub fn store<S: WasmMemoryWrite>(
    stage: &mut S,
    access: Access,
    index: S::Word,
    offset: u32,
    value: S::Word,
    proven: bool,
) -> Result<(), S::Exit> {
    check_access(stage, access, index, offset, proven)?;
    stage.memory_store(access, index, offset, value);
    Ok(())
}

/// The effective address of a vector access: the 32-bit `address` plus its static `offset`, as an
/// `I64` word that cannot wrap.
#[inline(always)]
pub fn effective_address<S: WasmMemory>(
    stage: &mut S,
    address: S::Word,
    offset: u64,
) -> S::Word {
    let index = stage.memory_index(address);
    let offset = stage.constant(Width::I64, offset as i64);
    stage.operate(Binary::Add, Width::I64, index, offset)
}

/// The length in pages of `1 << page_bits` bytes each, as an `I64` word: `page_bits` is the
/// memory type's page size (`page_size_log2`), 0 for one-byte pages.
#[inline(always)]
pub(crate) fn pages<S: WasmMemory>(stage: &mut S, page_bits: u32) -> S::Word {
    let length = stage.memory_length();
    let bits = stage.constant(Width::I64, i64::from(page_bits));
    stage.operate(Binary::ShiftRightUnsigned, Width::I64, length, bits)
}

/// `memory.size`: the length in pages of `1 << page_bits` bytes, an `I32` word.
#[inline(always)]
pub fn size<S: WasmMemory>(stage: &mut S, page_bits: u32) -> S::Word {
    let count = pages(stage, page_bits);
    stage.convert(ScalarConversionOperator::WrapI64, count)
}

/// `WasmMemoryAddress`: the effective address of a scalar access, the `I64` `index` of the
/// address plus its static `offset`. A sum that wraps traps, as the arm's checked add did.
#[inline(always)]
pub(crate) fn scalar_address<S: WasmStage>(
    stage: &mut S,
    index: S::Word,
    offset: u64,
) -> Result<S::Word, S::Exit> {
    let offset = stage.constant(Width::I64, offset as i64);
    let effective = stage.operate(Binary::Add, Width::I64, index, offset);
    // Adding a non-negative offset wraps exactly when the sum falls below the index.
    let wrapped = stage.operate(Binary::GreaterUnsigned, Width::I64, index, effective);
    stage.trap_when(wrapped, WasmTrap::OutOfBoundsMemory)?;
    Ok(effective)
}

/// `memory.fill`: the range checks before any byte is written.
#[inline(always)]
pub fn fill<S: WasmMemoryWrite>(
    stage: &mut S,
    destination: S::Word,
    byte: S::Word,
    length: S::Word,
) -> Result<(), S::Exit> {
    let (index, length) = (stage.memory_index(destination), stage.memory_index(length));
    check_range(stage, index, length)?;
    stage.memory_fill(index, byte, length);
    Ok(())
}

/// `memory.copy`: both ranges check, the destination first, before any byte moves.
#[inline(always)]
pub fn copy<S: WasmMemoryWrite>(
    stage: &mut S,
    destination: S::Word,
    source: S::Word,
    length: S::Word,
) -> Result<(), S::Exit> {
    let destination = stage.memory_index(destination);
    let source = stage.memory_index(source);
    let length = stage.memory_index(length);
    check_range(stage, destination, length)?;
    check_range(stage, source, length)?;
    stage.memory_copy(destination, source, length);
    Ok(())
}

/// The two memories of a cross-memory copy (multi-memory `memory.copy`): the one it reads and
/// the one it writes. Each has its own length, and the transfer between them is the stage's
/// primitive, since shared and buffer-backed bytes move through their own storage.
#[cfg(not(stencil_build))]
pub(crate) trait WasmMemoryPair: WasmStage {
    /// The flag that `extent` bytes from `index` pass the end of the source memory.
    fn source_range_exceeds(&mut self, index: Self::Word, extent: Self::Word) -> Self::Word;
    /// The flag that `extent` bytes from `index` pass the end of the destination memory.
    fn destination_range_exceeds(&mut self, index: Self::Word, extent: Self::Word) -> Self::Word;
    /// `length` bytes from `source` in the source memory to `destination` in the destination
    /// memory, both ranges in range; a zero length moves nothing.
    fn memory_transfer(&mut self, destination: Self::Word, source: Self::Word, length: Self::Word);
}

/// `memory.copy` between two memories: the source's range checks, then the destination's, and
/// only then do bytes move. A trap leaves both memories untouched.
#[cfg(not(stencil_build))]
#[inline(always)]
pub(crate) fn copy_across<S: WasmMemoryPair>(
    stage: &mut S,
    destination: S::Word,
    source: S::Word,
    length: S::Word,
) -> Result<(), S::Exit> {
    let past = stage.source_range_exceeds(source, length);
    stage.trap_when(past, WasmTrap::OutOfBoundsMemory)?;
    let past = stage.destination_range_exceeds(destination, length);
    stage.trap_when(past, WasmTrap::OutOfBoundsMemory)?;
    stage.memory_transfer(destination, source, length);
    Ok(())
}

#[cfg(not(stencil_build))]
impl Memory {
    /// The registers naming the memory the operation reads: its binding register.
    pub fn memories(self, instruction: WideInstruction) -> [Option<u16>; 2] {
        match self {
            Self::Load(_) | Self::Store(_) | Self::Size | Self::Grow => {
                [Some(instruction.b()), None]
            }
            Self::Fill => [Some(instruction.a()), None],
            Self::Copy => [Some(instruction.a()), Some(instruction.b())],
        }
    }

    /// The registers holding 32-bit addresses, lengths and bytes the operation reads.
    pub fn operands(self, instruction: WideInstruction) -> [Option<u16>; 3] {
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

// Scalar memory32 accesses with a static offset have one opcode each. The access semantics
// remain the MemoryLoad/MemoryStore rules above; the opcode only names the operator so dispatch
// needs no selector decode.
#[cfg(not(stencil_build))]
macro_rules! direct_accesses {
    ($kind:ty; $($name:ident => $op:ident;)+) => {
        impl $kind {
            pub(crate) const fn direct_op(self) -> Option<crate::bytecode::Op> {
                match self { $(Self::$name => Some(crate::bytecode::Op::$op),)+ #[allow(unreachable_patterns)] _ => None }
            }
            pub const fn from_direct_op(op: crate::bytecode::Op) -> Option<Self> {
                match op { $(crate::bytecode::Op::$op => Some(Self::$name),)+ _ => None }
            }
        }
    }
}
#[cfg(not(stencil_build))]
direct_accesses! { crate::wasm::memory::MemoryLoad;
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
#[cfg(not(stencil_build))]
direct_accesses! { crate::wasm::memory::MemoryStore;
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
