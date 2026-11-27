//! Memory layout and scalar access rules. Heap ownership stays in the VM.

use super::{WasmTrap, WasmValue};
use crate::bytecode::Register;
use crate::staged::wasm_emit;
use crate::staged::wasm_memory::{
    memory_load_direct_op, memory_store_direct_op, memory_store_from_direct_op,
};
use std::ops::Range;

/// One linear memory's backing store. The memory type decides the variant:
/// an unshared memory belongs to one agent, so a borrow flag serializes its
/// accesses; agents sharing a backing store synchronize through its lock.
/// Imports retain the original cell, so both variants have one owner cell.
#[derive(Debug)]
pub enum MemoryStorage {
    Unshared(std::cell::RefCell<LinearBytes>),
    Shared(std::sync::Arc<SharedMemory>),
}

/// A backing store that several agents may access concurrently.
#[derive(Debug)]
pub struct SharedMemory(pub(crate) crate::shared_backing::SharedBacking);

/// A linear memory's bytes. Either an ordinary heap buffer, or a fixed
/// virtual reservation whose accessible prefix grows in place: the bytes
/// never move, and every address a memory32 access can form (an i32 index
/// plus a u32 offset plus the access width) lies inside the reservation, so
/// an access past the accessible prefix faults instead of reaching other
/// memory. Compiled code relies on that fault in place of a bounds check.
#[derive(Debug)]
pub struct LinearBytes {
    repr: LinearRepr,
}

#[derive(Debug)]
enum LinearRepr {
    Heap(Vec<u8>),
    Reserved { base: *mut u8, len: usize },
}

/// The largest memory32: the whole i32 address space, the largest accessible prefix of a
/// reservation.
pub(crate) const MEMORY32_LIMIT: usize = u32::MAX as usize + 1;

/// The page granularity of the reservation's accessible prefix.
pub(crate) const HOST_PAGE: usize = 4096;

/// The reservation a guarded memory32 needs: every index plus offset plus
/// a full SIMD access, rounded up to a host page.
pub(crate) const GUARDED_RESERVATION: usize = MEMORY32_LIMIT * 2 + HOST_PAGE;
const _: () = assert!(super::V128_BYTES <= HOST_PAGE);

/// Where a guarded reservation is asked to start: low enough that its base
/// fits a signed 32-bit displacement, so compiled code can address the
/// memory as index plus constant. The kernel places the reservation
/// elsewhere when the range is taken.
const RESERVATION_HINT: usize = 1 << 30;

impl LinearBytes {
    pub(crate) fn heap(bytes: Vec<u8>) -> Self {
        Self {
            repr: LinearRepr::Heap(bytes),
        }
    }

    /// `len` zero bytes in a guarded reservation; `None` when the host
    /// cannot reserve the address space.
    pub(crate) fn reserved(len: usize) -> Option<Self> {
        if len > MEMORY32_LIMIT {
            return None;
        }
        // SAFETY: an inaccessible, unbacked private reservation; checked.
        let base = unsafe {
            libc::mmap(
                RESERVATION_HINT as *mut libc::c_void,
                GUARDED_RESERVATION,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return None;
        }
        let mut bytes = Self {
            repr: LinearRepr::Reserved {
                base: base.cast(),
                len: 0,
            },
        };
        bytes.grow_to(len).then_some(bytes)
    }

    /// The reservation's base and size when the memory is guarded.
    pub(crate) fn guard(&self) -> Option<(*mut u8, usize)> {
        match self.repr {
            LinearRepr::Reserved { base, .. } => Some((base, GUARDED_RESERVATION)),
            LinearRepr::Heap(_) => None,
        }
    }

    /// Grow to `len` bytes, zero-filling the new suffix; false when the host
    /// refuses the memory.
    pub(crate) fn grow_to(&mut self, new_len: usize) -> bool {
        match &mut self.repr {
            LinearRepr::Heap(bytes) => {
                let extra = new_len.saturating_sub(bytes.len());
                if bytes.try_reserve_exact(extra).is_err() {
                    return false;
                }
                bytes.resize(new_len, 0);
                true
            }
            LinearRepr::Reserved { base, len } => {
                if new_len > MEMORY32_LIMIT {
                    return false;
                }
                let committed = len.next_multiple_of(HOST_PAGE);
                let wanted = new_len.next_multiple_of(HOST_PAGE);
                // SAFETY: a page-aligned range inside the reservation; fresh
                // anonymous pages read as zero.
                if wanted > committed
                    && unsafe {
                        libc::mprotect(
                            base.add(committed).cast(),
                            wanted - committed,
                            libc::PROT_READ | libc::PROT_WRITE,
                        )
                    } != 0
                {
                    return false;
                }
                *len = new_len.max(*len);
                true
            }
        }
    }

    /// Bytes the memory holds for external accounting.
    pub(crate) fn capacity(&self) -> usize {
        match &self.repr {
            LinearRepr::Heap(bytes) => bytes.capacity(),
            LinearRepr::Reserved { len, .. } => len.next_multiple_of(HOST_PAGE),
        }
    }
}

impl Drop for LinearBytes {
    fn drop(&mut self) {
        if let LinearRepr::Reserved { base, .. } = self.repr {
            // SAFETY: the reservation this value owns.
            unsafe { libc::munmap(base.cast(), GUARDED_RESERVATION) };
        }
    }
}

impl std::ops::Deref for LinearBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match &self.repr {
            LinearRepr::Heap(bytes) => bytes,
            // SAFETY: the accessible prefix of the reservation.
            LinearRepr::Reserved { base, len, .. } => unsafe {
                std::slice::from_raw_parts(*base, *len)
            },
        }
    }
}

impl std::ops::DerefMut for LinearBytes {
    fn deref_mut(&mut self) -> &mut [u8] {
        match &mut self.repr {
            LinearRepr::Heap(bytes) => bytes,
            // SAFETY: as for `deref`; the memory state owns the bytes.
            LinearRepr::Reserved { base, len, .. } => unsafe {
                std::slice::from_raw_parts_mut(*base, *len)
            },
        }
    }
}

// SAFETY: owned memory like a Vec's buffer; agents share it as `MemoryStorage::direct_view` says.
unsafe impl Send for LinearBytes {}
unsafe impl Sync for LinearBytes {}

pub enum MemoryGuard<'a> {
    Unshared(std::cell::RefMut<'a, LinearBytes>),
    Shared(std::sync::MutexGuard<'a, crate::shared_backing::State>),
}

impl std::ops::Deref for MemoryGuard<'_> {
    type Target = LinearBytes;
    fn deref(&self) -> &LinearBytes {
        match self {
            Self::Unshared(state) => state,
            Self::Shared(state) => &state.bytes,
        }
    }
}
impl std::ops::DerefMut for MemoryGuard<'_> {
    fn deref_mut(&mut self) -> &mut LinearBytes {
        match self {
            Self::Unshared(state) => state,
            Self::Shared(state) => &mut state.bytes,
        }
    }
}

impl MemoryStorage {
    pub(crate) fn new(bytes: LinearBytes, shared: bool) -> Self {
        if shared {
            Self::Shared(std::sync::Arc::new(SharedMemory::new(bytes)))
        } else {
            Self::Unshared(std::cell::RefCell::new(bytes))
        }
    }

    pub fn lock(&self) -> MemoryGuard<'_> {
        match self {
            // An unshared memory's accessors never nest, as with the shared lock.
            Self::Unshared(state) => MemoryGuard::Unshared(state.borrow_mut()),
            Self::Shared(memory) => memory.lock(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Unshared(state) => state.borrow().len(),
            Self::Shared(memory) => memory.len(),
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        self.lock().capacity()
    }

    pub(crate) fn copy_from(&self, source: &Self, input: Range<usize>, output: Range<usize>) {
        match (self, source) {
            (Self::Shared(destination), Self::Shared(source)) => {
                destination.copy_from(source, input, output)
            }
            _ if std::ptr::eq(self, source) => {
                self.lock().copy_within(input, output.start);
            }
            // At most one side is shared, so only one lock is taken.
            _ => {
                let source = source.lock();
                self.lock()[output].copy_from_slice(&source[input]);
            }
        }
    }
}

impl SharedMemory {
    pub(crate) fn new(bytes: LinearBytes) -> Self {
        Self(crate::shared_backing::SharedBacking::from_wasm(bytes))
    }

    pub(crate) fn lock(&self) -> MemoryGuard<'_> {
        MemoryGuard::Shared(self.0.lock())
    }

    pub(crate) fn len(&self) -> usize {
        let _order = crate::shared_backing::memory_order();
        self.lock().len()
    }

    pub(crate) fn copy_from(&self, source: &Self, input: Range<usize>, output: Range<usize>) {
        self.0.copy_from(&source.0, input, output);
    }
}

/// Immutable data segments and locked linear memory share a byte-read view.
pub enum MemoryRead<'a> {
    Buffer(crate::heap::BufferRead<'a>),
    Memory(MemoryGuard<'a>),
}

impl std::ops::Deref for MemoryRead<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Buffer(bytes) => bytes,
            Self::Memory(bytes) => bytes,
        }
    }
}

const BYTE_PAGE_BITS: u32 = 0;
#[cfg(test)]
pub const WASM_PAGE_BYTES: usize = 1 << WASM_PAGE_BITS;

#[derive(Clone, Debug)]
pub struct WasmMemory {
    pub ty: wasmparser::MemoryType,
    pub import: Option<super::WasmImportName>,
}

impl WasmMemory {
    pub fn byte_length(&self) -> Option<usize> {
        if !supported_memory(&self.ty) {
            return None;
        }
        usize::try_from(self.ty.initial)
            .ok()?
            .checked_mul(self.ty.page_size() as usize)
    }
}

pub(crate) fn memory_page_limit(ty: &wasmparser::MemoryType) -> u64 {
    let (bits, index_maximum) = if ty.memory64 {
        (u64::BITS, u64::MAX)
    } else {
        (u32::BITS, u64::from(u32::MAX))
    };
    let pages = (1_u128 << bits) / u128::from(ty.page_size());
    u64::try_from(pages).unwrap_or(u64::MAX).min(index_maximum)
}

pub(crate) fn supported_memory(ty: &wasmparser::MemoryType) -> bool {
    (!ty.shared || ty.maximum.is_some())
        && matches!(ty.page_size_log2(), BYTE_PAGE_BITS | WASM_PAGE_BITS)
        && ty.initial <= memory_page_limit(ty)
        && ty
            .maximum
            .is_none_or(|maximum| maximum >= ty.initial && maximum <= memory_page_limit(ty))
}

pub(crate) fn maximum_bytes(ty: &wasmparser::MemoryType) -> usize {
    let pages = ty.maximum.unwrap_or_else(|| memory_page_limit(ty));
    usize::try_from(u128::from(pages) * u128::from(ty.page_size())).unwrap_or(usize::MAX)
}

#[derive(Clone, Debug)]
pub struct WasmData {
    pub mode: WasmDataMode,
    pub bytes: std::rc::Rc<Vec<u8>>,
}

/// Active segments initialize memory once and start dropped; passive segments
/// remain available until data.drop in each independently rooted instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WasmDataMode {
    Active {
        memory: u32,
        offset: super::WasmConstantExpression,
    },
    Passive,
}

#[inline]
pub(crate) fn checked_range(
    address: u64,
    length: usize,
    available: usize,
) -> Result<Range<usize>, WasmTrap> {
    let start = usize::try_from(address).map_err(|_| WasmTrap::OutOfBoundsMemory)?;
    let end = start
        .checked_add(length)
        .filter(|end| *end <= available)
        .ok_or(WasmTrap::OutOfBoundsMemory)?;
    Ok(start..end)
}

/// The page size of a Wasm memory as a shift, from the one list the b-jit codes also read.
macro_rules! page_bits_const {
    ($bits:literal) => {
        pub const WASM_PAGE_BITS: u32 = $bits;
    };
}
wasm_page_bits!(page_bits_const);

pub use super::memory_ops::*;

/// The direct opcode for a scalar access, when the memory has a prologue
/// binding register and the access can use the direct memory32 form.
pub(super) fn direct_access(
    operator: &wasmparser::Operator<'_>,
) -> Option<(crate::bytecode::Op, wasmparser::MemArg)> {
    MemoryLoad::from_wasm(operator)
        .and_then(|(load, memarg)| Some((memory_load_direct_op(load)?, memarg)))
        .or_else(|| {
            MemoryStore::from_wasm(operator)
                .and_then(|(store, memarg)| Some((memory_store_direct_op(store)?, memarg)))
        })
}

/// Whether a memory access reads or writes; the emit functions name the opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Load,
    Store,
}

// Lane accesses compose existing scalar memory access with SIMD lane rules.
macro_rules! lane_accesses {
    ($($wasm:ident, $direction:ident, $access:path, $lane_op:ident;)+) => {
        fn lane_access(op: &wasmparser::Operator<'_>) -> Option<(Direction, u32, wasmparser::MemArg, super::simd::SimdOperator, u8)> {
            match op {
                $(wasmparser::Operator::$wasm { memarg, lane } => Some((Direction::$direction, $access as u32, *memarg, super::simd::SimdOperator::$lane_op, *lane)),)+
                _ => None,
            }
        }
    }
}
lane_accesses! {
    V128Load8Lane, Load, MemoryLoad::I32Load8U, I8x16ReplaceLane;
    V128Store8Lane, Store, MemoryStore::I32Store8, I8x16ExtractLaneU;
    V128Load16Lane, Load, MemoryLoad::I32Load16U, I16x8ReplaceLane;
    V128Store16Lane, Store, MemoryStore::I32Store16, I16x8ExtractLaneU;
    V128Load32Lane, Load, MemoryLoad::I32Load, I32x4ReplaceLane;
    V128Store32Lane, Store, MemoryStore::I32Store, I32x4ExtractLane;
    V128Load64Lane, Load, MemoryLoad::I64Load, I64x2ReplaceLane;
    V128Store64Lane, Store, MemoryStore::I64Store, I64x2ExtractLane;
}

/// The access an operator names: its direction, its selector, its memarg and, for a lane access,
/// the SIMD lane rule and the lane.
fn access_operator(
    operator: &wasmparser::Operator<'_>,
) -> Option<(Direction, u32, wasmparser::MemArg, Option<(super::simd::SimdOperator, u8)>)> {
    MemoryLoad::from_wasm(operator)
        .map(|(load, arg)| (Direction::Load, load as u32, arg, None))
        .or_else(|| {
            MemoryStore::from_wasm(operator)
                .map(|(store, arg)| (Direction::Store, store as u32, arg, None))
        })
        .or_else(|| {
            lane_access(operator).map(|(direction, selector, arg, lane_op, lane)| {
                (direction, selector, arg, Some((lane_op, lane)))
            })
        })
}

/// The Memory32 binding `operator` shares with the body's other loads and stores,
/// `memory.size`, `memory.grow` and `memory.fill`, by memory index: one register loaded once
/// per call serves vector and lane accesses as it serves the direct ones.
pub(super) fn direct_access_memory(
    memories: &[WasmMemory],
    operator: &wasmparser::Operator<'_>,
) -> Option<usize> {
    let index = MemoryLoad::from_wasm(operator)
        .map(|(_, memarg)| memarg)
        .or_else(|| MemoryStore::from_wasm(operator).map(|(_, memarg)| memarg))
        .or_else(|| lane_access(operator).map(|(_, _, memarg, _, _)| memarg))
        .map(|memarg| memarg.memory)
        .or(match operator {
            wasmparser::Operator::MemorySize { mem }
            | wasmparser::Operator::MemoryGrow { mem }
            | wasmparser::Operator::MemoryFill { mem } => Some(*mem),
            _ => None,
        })? as usize;
    memories
        .get(index)
        .is_some_and(|memory| !memory.ty.memory64)
        .then_some(index)
}

impl super::Lowering<'_> {
    pub(super) fn memory_binding(
        &mut self,
        index: u32,
    ) -> Result<crate::bytecode::Register, crate::Diagnostic> {
        self.memories.get(index as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
        })?;
        if let Some(Some(register)) = self.memory_registers.get(index as usize) {
            return Ok(*register);
        }
        self.instance_binding(self.layout.slot(super::WasmBinding::Memory(index as usize)))
    }

    pub(super) fn instance_binding(
        &mut self,
        slot: usize,
    ) -> Result<crate::bytecode::Register, crate::Diagnostic> {
        let register = self.push()?;
        self.emit_emission(wasm_emit::instance_binding(register, slot as u32))?;
        Ok(register)
    }

    pub(super) fn memory_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        if self.atomic_operator(operator)? || self.bulk_memory_operator(operator)? {
            return Ok(true);
        }
        if self.direct_memory_operator(operator)? || self.addressed_memory_operator(operator)? {
            return Ok(true);
        }
        self.memory_size_operator(operator)
    }

    /// A scalar access the direct opcode addresses through the memory's binding register.
    fn direct_memory_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        let Some((op, memarg)) = direct_access(operator) else {
            return Ok(false);
        };
        let Some(&Some(memory)) = self.memory_registers.get(memarg.memory as usize) else {
            return Ok(false);
        };
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let offset = u32::try_from(memarg.offset).map_err(|_| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory32 offset out of range")
        })?;
        if memory_store_from_direct_op(op).is_some() {
            let value = self.pop()?;
            let address = self.pop()?;
            self.emit(op, value, memory, address, offset)?;
        } else {
            let address = self.pop()?;
            let result = self.push()?;
            self.emit(op, result, memory, address, offset)?;
            self.produced(result);
        }
        Ok(true)
    }

    /// A scalar or lane access through an effective address, loaded from the memory's binding.
    fn addressed_memory_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        let Some((direction, selector, arg, lane)) = access_operator(operator) else {
            return Ok(false);
        };
        self.memories.get(arg.memory as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
        })?;
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let value = if direction == Direction::Store || lane.is_some() {
            Some(self.pop()?)
        } else {
            None
        };
        let address = self.pop()?;
        let depth = self.depth;
        // Reserve the consumed registers until both operands have been read.
        self.depth = address + if value.is_some() { 2 } else { 1 };
        let memory = self.memory_binding(arg.memory)?;
        let effective = self.push()?;
        let offset = self.scalar_constant(WasmValue::I64(arg.offset as i64))?;
        self.emit_emission(wasm_emit::memory_address(effective, address, memory, offset))?;
        let operands = AddressedAccess { direction, selector, value, address, memory, effective };
        self.emit_addressed_access(operands, lane)?;
        self.depth = depth;
        if direction == Direction::Load {
            self.push()?;
        }
        Ok(true)
    }

    /// The access of an addressed operand: a scalar access, or a lane access when `lane` names one.
    fn emit_addressed_access(
        &mut self,
        operands: AddressedAccess,
        lane: Option<(super::simd::SimdOperator, u8)>,
    ) -> Result<(), crate::Diagnostic> {
        let AddressedAccess { direction, selector, value, address, memory, effective } = operands;
        let Some((lane_op, lane)) = lane else {
            let target = value.unwrap_or(address);
            let emission = match direction {
                Direction::Load => wasm_emit::memory_load(target, memory, effective, selector),
                Direction::Store => wasm_emit::memory_store(target, memory, effective, selector),
            };
            return self.emit_emission(emission);
        };
        let vector = value.expect("lane access vector operand");
        let access = LaneAccess { direction, lane_op, lane, selector, address, vector, memory, effective };
        self.lane_memory_access(access)
    }

    /// A lane access: a scalar access of the lane's word composed with the SIMD lane rule.
    fn lane_memory_access(&mut self, lane: LaneAccess) -> Result<(), crate::Diagnostic> {
        let lane_selector = lane.lane_op.selector(lane.lane);
        if super::simd::SimdOperator::from_selector(lane_selector).is_none() {
            return Err(crate::Diagnostic::unsupported(self.name, "invalid SIMD memory lane"));
        }
        let scalar = self.push()?;
        let emissions = match lane.direction {
            Direction::Load => [
                wasm_emit::memory_load(scalar, lane.memory, lane.effective, lane.selector),
                wasm_emit::lane_replace(lane.address, lane.vector, scalar, lane_selector),
            ],
            Direction::Store => [
                wasm_emit::lane_extract(scalar, lane.vector, lane_selector),
                wasm_emit::memory_store(scalar, lane.memory, lane.effective, lane.selector),
            ],
        };
        for emission in emissions {
            self.emit_emission(emission)?;
        }
        Ok(())
    }

    fn memory_size_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        let (index, grow) = match operator {
            wasmparser::Operator::MemorySize { mem } => (*mem, false),
            wasmparser::Operator::MemoryGrow { mem } => (*mem, true),
            _ => return Ok(false),
        };
        self.memories.get(index as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
        })?;
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let delta = if grow { Some(self.pop()?) } else { None };
        let result = self.push()?;
        let depth = self.depth;
        let memory = self.memory_binding(index)?;
        let emission = match delta {
            Some(delta) => wasm_emit::memory_grow(result, memory, delta),
            None => wasm_emit::memory_size(result, memory),
        };
        self.emit_emission(emission)?;
        self.depth = depth;
        Ok(true)
    }
}

/// The operands of one addressed access: its direction, its selector, the stored value when it
/// has one, the address, the memory and the effective address.
struct AddressedAccess {
    direction: Direction,
    selector: u32,
    value: Option<Register>,
    address: Register,
    memory: Register,
    effective: Register,
}

/// The operands of one lane access: its direction and the SIMD lane rule with its lane, the
/// memory access's selector, the address, the vector, the memory and the effective address.
struct LaneAccess {
    direction: Direction,
    lane_op: super::simd::SimdOperator,
    lane: u8,
    selector: u32,
    address: Register,
    vector: Register,
    memory: Register,
    effective: Register,
}

impl super::Lowering<'_> {
    pub(super) fn data_slot(&self, index: u32) -> Result<u16, crate::Diagnostic> {
        self.data.get(index as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm data segment out of bounds")
        })?;
        Ok(self.layout.slot(super::WasmBinding::Data(index as usize)) as u16)
    }

    fn bulk_memory_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        if let wasmparser::Operator::DataDrop { data_index } = *operator {
            let slot = self.data_slot(data_index)?;
            self.drop_instance_binding(slot)?;
            return Ok(true);
        }
        let Some((bulk, destination, source)) = self.bulk_operands(operator)? else {
            return Ok(false);
        };
        self.memories.get(destination as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
        })?;
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let length = self.pop()?;
        let input = self.pop()?;
        let output = self.pop()?;
        let depth = self.depth;
        // Keep the consumed three operands intact while loading heap bindings.
        self.depth = length + 1;
        let memory = self.memory_binding(destination)?;
        let source = self.bulk_source(bulk, source, input)?;
        let emission = match bulk {
            BulkOperator::Init => wasm_emit::memory_init(memory, source, length, output, input),
            BulkOperator::Transfer => {
                wasm_emit::memory_copy(memory, source, length, output, input)
            }
            BulkOperator::Fill => wasm_emit::memory_fill(memory, input, length, output),
        };
        self.emit_emission(emission)?;
        self.depth = depth;
        Ok(true)
    }

    /// The bulk operator, its destination memory and its source (a data segment or a memory),
    /// when `operator` is one of the bulk operators that move or fill bytes.
    fn bulk_operands(
        &self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<Option<(BulkOperator, u32, Option<u32>)>, crate::Diagnostic> {
        Ok(match *operator {
            wasmparser::Operator::MemoryInit { data_index, mem } => {
                self.data_slot(data_index)?;
                Some((BulkOperator::Init, mem, Some(data_index)))
            }
            wasmparser::Operator::MemoryCopy { dst_mem, src_mem } => {
                self.memories.get(src_mem as usize).ok_or_else(|| {
                    crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
                })?;
                Some((BulkOperator::Transfer, dst_mem, Some(src_mem)))
            }
            wasmparser::Operator::MemoryFill { mem } => Some((BulkOperator::Fill, mem, None)),
            _ => None,
        })
    }

    /// The register a bulk operation reads: a data segment's instance binding, another memory's
    /// binding, or the fill value.
    fn bulk_source(
        &mut self,
        bulk: BulkOperator,
        source: Option<u32>,
        input: Register,
    ) -> Result<Register, crate::Diagnostic> {
        match (bulk, source) {
            (BulkOperator::Init, Some(index)) => {
                self.instance_binding(usize::from(self.data_slot(index)?))
            }
            (_, Some(index)) => self.memory_binding(index),
            (_, None) => Ok(input),
        }
    }
}

/// The bulk memory operators that share one operand shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BulkOperator {
    Init,
    Transfer,
    Fill,
}

mod direct;
pub use direct::addressable_in_place;
#[cfg(test)]
mod storage_tests {
    use super::SharedMemory as MemoryStorage;
    use std::sync::{Arc, Barrier};

    #[test]
    fn shared_backing_preserves_mutations_and_orders_opposing_copy_locks() {
        let first = Arc::new(MemoryStorage::new(super::LinearBytes::heap(vec![1; 32])));
        let second = Arc::new(MemoryStorage::new(super::LinearBytes::heap(vec![2; 32])));
        let start = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            for (destination, source) in [(&first, &second), (&second, &first)] {
                let start = start.clone();
                scope.spawn(move || {
                    start.wait();
                    for _ in 0..100 {
                        destination.copy_from(source, 0..32, 0..32);
                    }
                });
            }
        });
        assert_eq!(&**first.lock(), &**second.lock());
        let backing = first.clone();
        std::thread::spawn(move || {
            let mut bytes = backing.lock();
            assert!(bytes.grow_to(64));
            bytes[48] = 7;
        })
        .join()
        .unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(first.lock()[48], 7);
        first.copy_from(&first, 48..49, 49..50);
        assert_eq!(first.lock()[49], 7);
    }
}
