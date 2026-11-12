//! Wasm linear-memory semantics written once over `WasmMemory`: the bounds check of an access
//! and of a bulk operation, the order in which a copy checks its ranges, and what an access
//! moves (its width, whether a narrow load sign-extends, little-endian bytes). The interpreter
//! runs them on `ConcreteMemory` (`MemoryLoad::read`, `MemoryStore::write`), the Wasm tier on
//! Cranelift values (`Emitter`, `vm/compiled/wasm_memory_stage.rs`). A stage supplies the
//! machine: the memory's length, the words an access moves and the bulk movers.

use super::wasm::{WasmStage, Width};
use super::wasm_machine::{ConcreteBytes, ConcreteMemory};
use crate::bytecode::{Class, Op, WideInstruction};
use crate::wasm::V128_BYTES;
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::integer::I64BinaryOperator as Binary;
use crate::wasm::memory::{MemoryLoad, MemoryStore, WASM_PAGE_BITS, checked_range};
use crate::{WasmTrap, WasmType, WasmValue};

/// What one scalar access moves: the type of its register value, the bytes it touches and
/// whether a narrow load sign-extends them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Access {
    pub(crate) ty: WasmType,
    pub(crate) bytes: usize,
    pub(crate) signed: bool,
}

impl Access {
    /// The access a scalar load makes; a vector load has none.
    pub(crate) fn of_load(load: MemoryLoad) -> Option<Self> {
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

    /// A 128-bit vector access, which only the vector steps make.
    pub(crate) const VECTOR: Self = Self {
        ty: WasmType::V128,
        bytes: V128_BYTES,
        signed: false,
    };

    pub(crate) fn of_store(store: MemoryStore) -> Self {
        Self {
            ty: store.value_type(),
            bytes: store.width(),
            signed: false,
        }
    }
}

/// The linear-memory opcodes a function body runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Memory {
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

impl Memory {
    pub(crate) fn of(op: Op) -> Option<Self> {
        if let Some(load) = MemoryLoad::from_direct_op(op).and_then(Access::of_load) {
            return Some(Self::Load(load));
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
    pub(crate) const fn needs_compiled_memory(self) -> bool {
        matches!(self, Self::Size | Self::Grow | Self::Fill | Self::Copy)
    }

    /// Whether the operation calls out to the VM, which changes the memory's length: the
    /// program's callouts must be available, and the tier reads the length again after it.
    pub(crate) const fn needs_callouts(self) -> bool {
        matches!(self, Self::Grow)
    }
}

/// The linear memory of the instance a function runs in, as far as reading it goes.
pub(crate) trait WasmMemory: WasmStage {
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
pub(crate) trait WasmMemoryWrite: WasmMemory {
    fn memory_store(&mut self, access: Access, index: Self::Word, offset: u32, value: Self::Word);
    /// `length` copies of the low byte of the `I32` word `byte` from `index`, in range.
    fn memory_fill(&mut self, index: Self::Word, byte: Self::Word, length: Self::Word);
    /// `length` bytes from `source` to `destination`, both in range; overlapping ranges copy
    /// as if through a temporary buffer.
    fn memory_copy(&mut self, destination: Self::Word, source: Self::Word, length: Self::Word);
}

/// The linear memory a function grows: the machine grows it, as the VM does for the
/// interpreter (`LinearBytes::grow_pages`), and the length every later access reads is the new
/// one.
pub(crate) trait WasmMemoryGrow: WasmStage {
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
pub(crate) fn load<S: WasmMemory>(
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
pub(crate) fn store<S: WasmMemoryWrite>(
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

/// The effective address of a vector access: the 32-bit `address` plus its static `offset`, as
/// an `I64` word that cannot wrap.
#[inline(always)]
pub(crate) fn effective_address<S: WasmMemory>(
    stage: &mut S,
    address: S::Word,
    offset: u64,
) -> S::Word {
    let index = stage.memory_index(address);
    let offset = stage.constant(Width::I64, offset as i64);
    stage.operate(Binary::Add, Width::I64, index, offset)
}

/// `memory.size`: the length in pages, an `I32` word.
#[inline(always)]
pub(crate) fn size<S: WasmMemory>(stage: &mut S) -> S::Word {
    let length = stage.memory_length();
    let bits = stage.constant(Width::I64, i64::from(WASM_PAGE_BITS));
    let pages = stage.operate(Binary::ShiftRightUnsigned, Width::I64, length, bits);
    stage.convert(ScalarConversionOperator::WrapI64, pages)
}

/// `memory.fill`: the range checks before any byte is written.
#[inline(always)]
pub(crate) fn fill<S: WasmMemoryWrite>(
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
pub(crate) fn copy<S: WasmMemoryWrite>(
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

impl Memory {
    /// The registers naming the memory the operation reads: its binding register.
    pub(crate) fn memories(self, instruction: WideInstruction) -> [Option<u16>; 2] {
        match self {
            Self::Load(_) | Self::Store(_) | Self::Size | Self::Grow => {
                [Some(instruction.b()), None]
            }
            Self::Fill => [Some(instruction.a()), None],
            Self::Copy => [Some(instruction.a()), Some(instruction.b())],
        }
    }

    /// The registers holding 32-bit addresses, lengths and bytes the operation reads.
    pub(crate) fn operands(self, instruction: WideInstruction) -> [Option<u16>; 3] {
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

impl MemoryLoad {
    /// The value at `address` of `bytes`: out of bounds traps. A scalar load is the staged
    /// access (`load`) on the bytes; a vector load decodes its own.
    #[inline(always)]
    pub(crate) fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
        let Some(access) = Access::of_load(self) else {
            let range = checked_range(address, self.width(), bytes.len())?;
            // SAFETY: the range is `width` bytes inside `bytes`.
            return Ok(unsafe { self.read_vector_at(bytes.as_ptr().add(range.start)) });
        };
        let mut stage = ConcreteBytes { memory: bytes };
        let word = load(&mut stage, access, address, 0, false)?;
        Ok(WasmValue::from_bits(access.ty, word))
    }

    /// The value at `at`, unchecked.
    ///
    /// # Safety
    /// `width` bytes from `at` must be readable.
    #[inline(always)]
    pub(crate) unsafe fn read_at(self, at: *const u8) -> WasmValue {
        let Some(access) = Access::of_load(self) else {
            // SAFETY: the caller keeps the word's bytes readable.
            return unsafe { self.read_vector_at(at) };
        };
        // SAFETY: the caller keeps `width` bytes readable.
        let memory = unsafe { std::slice::from_raw_parts(at, access.bytes) };
        let word = ConcreteBytes { memory }.memory_load(access, 0, 0);
        WasmValue::from_bits(access.ty, word)
    }
}

impl MemoryStore {
    /// Write `value` at `address` of `bytes`: out of bounds traps before any byte changes. A
    /// scalar store is the staged access (`store`) on the bytes; a vector store writes its own.
    #[inline(always)]
    pub(crate) fn write(
        self,
        bytes: &mut [u8],
        address: u64,
        value: WasmValue,
    ) -> Result<(), WasmTrap> {
        if self.value_type() == WasmType::V128 {
            let range = checked_range(address, self.width(), bytes.len())?;
            // SAFETY: the range is `width` bytes inside `bytes`.
            unsafe { self.write_vector_at(bytes.as_mut_ptr().add(range.start), value) };
            return Ok(());
        }
        let mut stage = ConcreteMemory { memory: bytes };
        store(&mut stage, Access::of_store(self), address, 0, value.word(), false)
    }

    /// Write `value` at `at`, unchecked.
    ///
    /// # Safety
    /// `width` bytes from `at` must be writable.
    #[inline(always)]
    pub(crate) unsafe fn write_at(self, at: *mut u8, value: WasmValue) {
        if self.value_type() == WasmType::V128 {
            // SAFETY: the caller keeps the word's bytes writable.
            return unsafe { self.write_vector_at(at, value) };
        }
        let access = Access::of_store(self);
        // SAFETY: the caller keeps `width` bytes writable.
        let memory = unsafe { std::slice::from_raw_parts_mut(at, access.bytes) };
        ConcreteMemory { memory }.memory_store(access, 0, 0, value.word());
    }
}
