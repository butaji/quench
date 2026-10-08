//! Memory layout and scalar access rules. Heap ownership stays in the VM.

use super::{WasmTrap, WasmType, WasmValue};
use std::ops::Range;

/// One backing store for linear memory. Imports retain the original cell, and
/// agents sharing the backing store serialize byte access and growth here.
#[derive(Debug)]
pub struct MemoryStorage(std::sync::Mutex<MemoryState>);

#[derive(Debug)]
pub(super) struct MemoryState {
    pub(super) bytes: Vec<u8>,
    pub(super) waiters: Vec<super::wait::Waiter>,
}

pub(crate) struct MemoryGuard<'a>(pub(super) std::sync::MutexGuard<'a, MemoryState>);

impl std::ops::Deref for MemoryGuard<'_> {
    type Target = Vec<u8>;
    fn deref(&self) -> &Vec<u8> {
        &self.0.bytes
    }
}
impl std::ops::DerefMut for MemoryGuard<'_> {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0.bytes
    }
}

impl MemoryStorage {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(std::sync::Mutex::new(MemoryState {
            bytes,
            waiters: Vec::new(),
        }))
    }

    pub(crate) fn lock(&self) -> MemoryGuard<'_> {
        // Guest byte accesses are checked before mutation; poisoning records a
        // Rust unwind, not corruption of the backing Vec's storage.
        MemoryGuard(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    pub(crate) fn len(&self) -> usize {
        let _order = super::atomic::memory_order();
        self.lock().len()
    }
    #[cfg(any(feature = "profile-aggregate", feature = "profile-memory"))]
    pub(crate) fn capacity(&self) -> usize {
        let state = self.lock();
        state.capacity() + state.0.waiters.capacity() * std::mem::size_of::<super::wait::Waiter>()
    }

    pub(crate) fn copy_from(&self, source: &Self, input: Range<usize>, output: Range<usize>) {
        if std::ptr::eq(self, source) {
            self.lock().copy_within(input, output.start);
            return;
        }
        // Every two-memory operation acquires locks in backing-store order.
        // This order is independent of guest addresses or import wrapper identity.
        if std::ptr::from_ref(self) < std::ptr::from_ref(source) {
            let mut destination = self.lock();
            let source = source.lock();
            destination[output].copy_from_slice(&source[input]);
        } else {
            let source = source.lock();
            let mut destination = self.lock();
            destination[output].copy_from_slice(&source[input]);
        }
    }
}

/// Immutable data segments and locked linear memory share a byte-read view.
pub(crate) enum MemoryRead<'a> {
    Segment(&'a [u8]),
    Memory(MemoryGuard<'a>),
}

impl std::ops::Deref for MemoryRead<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Segment(bytes) => bytes,
            Self::Memory(bytes) => bytes,
        }
    }
}

pub const WASM_PAGE_BITS: u32 = 16;
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

macro_rules! loads {
    ($value:ident; $($name:ident, $word:ty => $result:expr;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum MemoryLoad { $($name,)+ }
        impl MemoryLoad {
            const ALL: &'static [Self] = &[$(Self::$name,)+];
            pub(crate) fn from_tag(tag: u32) -> Option<Self> { Self::ALL.get(tag as usize).copied() }
            pub(super) fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, wasmparser::MemArg)> {
                match op { $(wasmparser::Operator::$name { memarg } => Some((Self::$name, *memarg)),)+ _ => None }
            }
            pub(crate) fn width(self) -> usize { match self { $(Self::$name => std::mem::size_of::<$word>(),)+ } }
            pub(crate) fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
                match self { $(Self::$name => {
                    let range = checked_range(address, std::mem::size_of::<$word>(), bytes.len())?;
                    let $value = <$word>::from_le_bytes(bytes[range].try_into().expect("checked memory width"));
                    Ok($result)
                },)+ }
            }
        }
    }
}
loads! { word;
    I32Load, i32 => WasmValue::I32(word as i32);
    I64Load, i64 => WasmValue::I64(word as i64);
    F32Load, u32 => WasmValue::F32(word as u32);
    F64Load, u64 => WasmValue::F64(word as u64);
    I32Load8S, i8 => WasmValue::I32(word as i32);
    I32Load8U, u8 => WasmValue::I32(word as i32);
    I32Load16S, i16 => WasmValue::I32(word as i32);
    I32Load16U, u16 => WasmValue::I32(word as i32);
    I64Load8S, i8 => WasmValue::I64(word as i64);
    I64Load8U, u8 => WasmValue::I64(word as i64);
    I64Load16S, i16 => WasmValue::I64(word as i64);
    I64Load16U, u16 => WasmValue::I64(word as i64);
    I64Load32S, i32 => WasmValue::I64(word as i64);
    I64Load32U, u32 => WasmValue::I64(word as i64);
    V128Load, u128 => WasmValue::V128(word as u128);
    V128Load8x8S, u64 => super::simd::SimdOperator::I16x8ExtendLowI8x16S.apply(WasmValue::V128(u128::from(word)), None, 0);
    V128Load8x8U, u64 => super::simd::SimdOperator::I16x8ExtendLowI8x16U.apply(WasmValue::V128(u128::from(word)), None, 0);
    V128Load16x4S, u64 => super::simd::SimdOperator::I32x4ExtendLowI16x8S.apply(WasmValue::V128(u128::from(word)), None, 0);
    V128Load16x4U, u64 => super::simd::SimdOperator::I32x4ExtendLowI16x8U.apply(WasmValue::V128(u128::from(word)), None, 0);
    V128Load32x2S, u64 => super::simd::SimdOperator::I64x2ExtendLowI32x4S.apply(WasmValue::V128(u128::from(word)), None, 0);
    V128Load32x2U, u64 => super::simd::SimdOperator::I64x2ExtendLowI32x4U.apply(WasmValue::V128(u128::from(word)), None, 0);
    V128Load8Splat, u8 => super::simd::SimdOperator::I8x16Splat.apply(WasmValue::I32(word as i32), None, 0);
    V128Load16Splat, u16 => super::simd::SimdOperator::I16x8Splat.apply(WasmValue::I32(word as i32), None, 0);
    V128Load32Splat, u32 => super::simd::SimdOperator::I32x4Splat.apply(WasmValue::I32(word as i32), None, 0);
    V128Load64Splat, u64 => super::simd::SimdOperator::I64x2Splat.apply(WasmValue::I64(word as i64), None, 0);
    V128Load32Zero, u32 => WasmValue::V128(u128::from(word));
    V128Load64Zero, u64 => WasmValue::V128(u128::from(word));
}

impl MemoryLoad {
    pub(crate) fn read_atomic(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
        if address % self.width() as u64 != 0 {
            return Err(WasmTrap::UnalignedAtomic);
        }
        self.read(bytes, address)
    }
}

macro_rules! stores {
    ($($name:ident, $word:ty, $variant:ident;)+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub(crate) enum MemoryStore { $($name,)+ }
        impl MemoryStore {
            const ALL: &'static [Self] = &[$(Self::$name,)+];
            pub(crate) fn from_tag(tag: u32) -> Option<Self> { Self::ALL.get(tag as usize).copied() }
            pub(super) fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, wasmparser::MemArg)> {
                match op { $(wasmparser::Operator::$name { memarg } => Some((Self::$name, *memarg)),)+ _ => None }
            }
            pub(crate) fn value_type(self) -> WasmType {
                match self { $(Self::$name => WasmType::$variant,)+ }
            }
            pub(crate) fn write(self, bytes: &mut [u8], address: u64, value: WasmValue) -> Result<(), WasmTrap> {
                match self { $(Self::$name => {
                    let WasmValue::$variant(word) = value else { unreachable!("decoded store type") };
                    let range = checked_range(address, std::mem::size_of::<$word>(), bytes.len())?;
                    bytes[range].copy_from_slice(&(word as $word).to_le_bytes());
                    Ok(())
                },)+ }
            }
        }
    }
}
stores! {
    I32Store, i32, I32;
    I64Store, i64, I64;
    F32Store, u32, F32;
    F64Store, u64, F64;
    I32Store8, i8, I32;
    I32Store16, i16, I32;
    I64Store8, i8, I64;
    I64Store16, i16, I64;
    I64Store32, i32, I64;
    V128Store, u128, V128;
}

// Lane accesses compose existing scalar memory access with SIMD lane rules.
macro_rules! lane_accesses {
    ($($wasm:ident, $op:ident, $access:path, $lane_op:ident;)+) => {
        fn lane_access(op: &wasmparser::Operator<'_>) -> Option<(crate::bytecode::Op, u32, wasmparser::MemArg, super::simd::SimdOperator, u8)> {
            use crate::bytecode::Op;
            match op {
                $(wasmparser::Operator::$wasm { memarg, lane } => Some((Op::$op, $access as u32, *memarg, super::simd::SimdOperator::$lane_op, *lane)),)+
                _ => None,
            }
        }
    }
}
lane_accesses! {
    V128Load8Lane, WasmMemoryLoad, MemoryLoad::I32Load8U, I8x16ReplaceLane;
    V128Store8Lane, WasmMemoryStore, MemoryStore::I32Store8, I8x16ExtractLaneU;
    V128Load16Lane, WasmMemoryLoad, MemoryLoad::I32Load16U, I16x8ReplaceLane;
    V128Store16Lane, WasmMemoryStore, MemoryStore::I32Store16, I16x8ExtractLaneU;
    V128Load32Lane, WasmMemoryLoad, MemoryLoad::I32Load, I32x4ReplaceLane;
    V128Store32Lane, WasmMemoryStore, MemoryStore::I32Store, I32x4ExtractLane;
    V128Load64Lane, WasmMemoryLoad, MemoryLoad::I64Load, I64x2ReplaceLane;
    V128Store64Lane, WasmMemoryStore, MemoryStore::I64Store, I64x2ExtractLane;
}

impl super::Lowering<'_> {
    pub(super) fn memory_binding(
        &mut self,
        index: u32,
    ) -> Result<crate::bytecode::Register, crate::Diagnostic> {
        self.memories.get(index as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
        })?;
        self.instance_binding(self.globals.len() + index as usize)
    }

    pub(super) fn instance_binding(
        &mut self,
        slot: usize,
    ) -> Result<crate::bytecode::Register, crate::Diagnostic> {
        let register = self.push()?;
        self.emit(
            crate::bytecode::Op::LoadCapture,
            register,
            0,
            0,
            crate::bytecode::ImmediateLayout::capture_immediate(0, slot as u16),
        )?;
        Ok(register)
    }

    pub(super) fn memory_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        use crate::bytecode::Op;
        if self.atomic_operator(operator)? || self.bulk_memory_operator(operator)? {
            return Ok(true);
        }
        let access = MemoryLoad::from_wasm(operator)
            .map(|(op, arg)| (Op::WasmMemoryLoad, op as u32, arg, None))
            .or_else(|| {
                MemoryStore::from_wasm(operator)
                    .map(|(op, arg)| (Op::WasmMemoryStore, op as u32, arg, None))
            })
            .or_else(|| {
                lane_access(operator).map(|(op, selector, arg, lane_op, lane)| {
                    (op, selector, arg, Some((lane_op, lane)))
                })
            });
        if let Some((op, selector, arg, lane_op)) = access {
            self.memories.get(arg.memory as usize).ok_or_else(|| {
                crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
            })?;
            if self.path == super::Reachability::Dead {
                return Ok(true);
            }
            let value = if op == Op::WasmMemoryStore || lane_op.is_some() {
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
            self.emit(Op::WasmMemoryAddress, effective, address, memory, offset)?;
            if let Some((lane_op, lane)) = lane_op {
                let lane_selector = lane_op.selector(lane);
                if super::simd::SimdOperator::from_selector(lane_selector).is_none() {
                    return Err(crate::Diagnostic::unsupported(
                        self.name,
                        "invalid SIMD memory lane",
                    ));
                }
                let scalar = self.push()?;
                let vector = value.expect("lane access vector operand");
                if op == Op::WasmMemoryLoad {
                    self.emit(op, scalar, memory, effective, selector)?;
                    self.emit(Op::WasmSimd, address, vector, scalar, lane_selector)?;
                } else {
                    self.emit(Op::WasmSimd, scalar, vector, 0, lane_selector)?;
                    self.emit(op, scalar, memory, effective, selector)?;
                }
            } else {
                self.emit(op, value.unwrap_or(address), memory, effective, selector)?;
            }
            self.depth = depth;
            if op == Op::WasmMemoryLoad {
                self.push()?;
            }
            return Ok(true);
        }
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
        self.emit(
            if grow {
                Op::WasmMemoryGrow
            } else {
                Op::WasmMemorySize
            },
            result,
            memory,
            delta.unwrap_or(0),
            0,
        )?;
        self.depth = depth;
        Ok(true)
    }
}

impl super::Lowering<'_> {
    pub(super) fn data_slot(&self, index: u32) -> Result<u16, crate::Diagnostic> {
        self.data.get(index as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm data segment out of bounds")
        })?;
        Ok((self.globals.len() + self.memories.len() + index as usize) as u16)
    }

    fn bulk_memory_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        use crate::bytecode::{ImmediateLayout, Op};
        let (op, destination, source) = match *operator {
            wasmparser::Operator::DataDrop { data_index } => {
                let slot = self.data_slot(data_index)?;
                self.drop_instance_binding(slot)?;
                return Ok(true);
            }
            wasmparser::Operator::MemoryInit { data_index, mem } => {
                self.data_slot(data_index)?;
                (Op::WasmMemoryInit, mem, Some(data_index))
            }
            wasmparser::Operator::MemoryCopy { dst_mem, src_mem } => {
                self.memories.get(src_mem as usize).ok_or_else(|| {
                    crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
                })?;
                (Op::WasmMemoryCopy, dst_mem, Some(src_mem))
            }
            wasmparser::Operator::MemoryFill { mem } => (Op::WasmMemoryFill, mem, None),
            _ => return Ok(false),
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
        let source = match source {
            Some(index) if op == Op::WasmMemoryInit => {
                self.instance_binding(usize::from(self.data_slot(index)?))?
            }
            Some(index) => self.memory_binding(index)?,
            None => input,
        };
        let addresses = ImmediateLayout::register_pair_immediate(
            output,
            if op == Op::WasmMemoryFill {
                output
            } else {
                input
            },
        );
        self.emit(op, memory, source, length, addresses)?;
        self.depth = depth;
        Ok(true)
    }
}

#[cfg(test)]
mod storage_tests {
    use super::MemoryStorage;
    use std::sync::{Arc, Barrier};

    #[test]
    fn shared_backing_preserves_mutations_and_orders_opposing_copy_locks() {
        let first = Arc::new(MemoryStorage::new(vec![1; 32]));
        let second = Arc::new(MemoryStorage::new(vec![2; 32]));
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
        assert_eq!(*first.lock(), *second.lock());
        let backing = first.clone();
        std::thread::spawn(move || {
            let mut bytes = backing.lock();
            bytes.resize(64, 0);
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
