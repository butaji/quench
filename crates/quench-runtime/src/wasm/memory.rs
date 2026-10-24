//! Memory layout and scalar access rules. Heap ownership stays in the VM.

use super::{WasmTrap, WasmType, WasmValue};
use std::ops::Range;

/// One linear memory's backing store. The memory type decides the variant:
/// an unshared memory belongs to one agent, so a borrow flag serializes its
/// accesses; agents sharing a backing store synchronize through its lock.
/// Imports retain the original cell, so both variants have one owner cell.
#[derive(Debug)]
pub enum MemoryStorage {
    Unshared(std::cell::RefCell<MemoryState>),
    Shared(std::sync::Arc<SharedMemory>),
}

/// A backing store that several agents may access concurrently.
#[derive(Debug)]
pub struct SharedMemory(std::sync::Mutex<MemoryState>);

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
    /// `alias`, where the host has one, is the descriptor of the shared
    /// pages the reservation maps twice: at its base and again
    /// `WRAP_ALIAS` bytes above, so an i32 sum formed in 64 bits that
    /// passed 2^32 reads the bytes its wrapped value names.
    Reserved {
        base: *mut u8,
        len: usize,
        alias: Option<std::os::fd::OwnedFd>,
    },
}

/// Where the reservation repeats the memory: the i32 address space, so a
/// sum of two i32 operands formed in 64 bits addresses its wrapped value.
pub(crate) const WRAP_ALIAS: usize = u32::MAX as usize + 1;

/// The largest accessible prefix of a reservation: one Wasm page below the
/// alias, so an access that crosses 2^32 without wrapping meets the gap
/// and faults (`memory.grow` may refuse growth beyond it).
pub(crate) const RESERVED_LIMIT: usize = WRAP_ALIAS - WASM_PAGE;

/// A Wasm page.
pub(crate) const WASM_PAGE: usize = 1 << 16;

/// The page granularity of the reservation's accessible prefix.
pub(crate) const HOST_PAGE: usize = 4096;

/// The reservation a guarded memory32 needs: every index plus offset plus
/// the widest scalar access, rounded up to a host page.
pub(crate) const GUARDED_RESERVATION: usize = (u32::MAX as usize + 1) * 2 + HOST_PAGE;

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
    /// With `wraps`, the reservation also repeats the memory above 2^32
    /// where the host can (see `LinearRepr::Reserved`).
    pub(crate) fn reserved(len: usize, wraps: bool) -> Option<Self> {
        if len > RESERVED_LIMIT {
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
                alias: wraps.then(wrap_alias_pages).flatten(),
            },
        };
        bytes.grow_to(len).then_some(bytes)
    }

    /// Whether the reservation repeats the memory at `WRAP_ALIAS`.
    pub(crate) fn wraps(&self) -> bool {
        matches!(self.repr, LinearRepr::Reserved { alias: Some(_), .. })
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
            LinearRepr::Reserved { base, len, alias } => {
                if new_len > RESERVED_LIMIT {
                    return false;
                }
                let committed = len.next_multiple_of(HOST_PAGE);
                let wanted = new_len.next_multiple_of(HOST_PAGE);
                if wanted > committed {
                    let committed_ok = match alias {
                        // SAFETY: page-aligned ranges inside the reservation,
                        // at the base and at the alias, of one descriptor
                        // whose new pages read as zero.
                        Some(fd) => unsafe { commit_aliased(*base, fd, committed, wanted) },
                        // SAFETY: a page-aligned range inside the
                        // reservation; fresh anonymous pages read as zero.
                        None => unsafe {
                            libc::mprotect(
                                base.add(committed).cast(),
                                wanted - committed,
                                libc::PROT_READ | libc::PROT_WRITE,
                            ) == 0
                        },
                    };
                    if !committed_ok {
                        return false;
                    }
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

/// The descriptor of the shared pages a wrapping reservation maps twice;
/// `None` where the host has no anonymous shared files, leaving the
/// reservation unaliased.
#[cfg(target_os = "linux")]
fn wrap_alias_pages() -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    // SAFETY: a fresh descriptor this function owns on success.
    let fd = unsafe { libc::memfd_create(c"quench-linear-memory".as_ptr(), libc::MFD_CLOEXEC) };
    // SAFETY: as above.
    (fd >= 0).then(|| unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}

#[cfg(not(target_os = "linux"))]
fn wrap_alias_pages() -> Option<std::os::fd::OwnedFd> {
    None
}

/// Extend the shared pages behind `fd` to `wanted` bytes and map
/// `committed..wanted` of them at `base` and at `base + WRAP_ALIAS`.
///
/// # Safety
/// `base` must start a `GUARDED_RESERVATION` this memory owns, and both
/// offsets must be page aligned.
unsafe fn commit_aliased(
    base: *mut u8,
    fd: &std::os::fd::OwnedFd,
    committed: usize,
    wanted: usize,
) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(size) = libc::off_t::try_from(wanted) else {
        return false;
    };
    // SAFETY: the descriptor this memory owns.
    if unsafe { libc::ftruncate(fd.as_raw_fd(), size) } != 0 {
        return false;
    }
    [0, WRAP_ALIAS].into_iter().all(|at| {
        // SAFETY: a page-aligned range inside the reservation, replaced by
        // the descriptor's pages at the same offset.
        let mapped = unsafe {
            libc::mmap(
                base.add(at + committed).cast(),
                wanted - committed,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_FIXED,
                fd.as_raw_fd(),
                committed as libc::off_t,
            )
        };
        mapped != libc::MAP_FAILED
    })
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

// SAFETY: the reservation is owned memory like a Vec's buffer; shared
// memories synchronize their accesses through their lock.
unsafe impl Send for LinearBytes {}
unsafe impl Sync for LinearBytes {}

#[derive(Debug)]
pub struct MemoryState {
    pub(crate) bytes: LinearBytes,
    pub(super) waiters: Vec<super::wait::Waiter>,
}

impl MemoryState {
    fn new(bytes: LinearBytes) -> Self {
        Self {
            bytes,
            waiters: Vec::new(),
        }
    }
}

pub(crate) enum MemoryGuard<'a> {
    Unshared(std::cell::RefMut<'a, MemoryState>),
    Shared(std::sync::MutexGuard<'a, MemoryState>),
}

impl MemoryGuard<'_> {
    pub(super) fn state(&mut self) -> &mut MemoryState {
        match self {
            Self::Unshared(state) => state,
            Self::Shared(state) => state,
        }
    }
}

impl std::ops::Deref for MemoryGuard<'_> {
    type Target = LinearBytes;
    fn deref(&self) -> &LinearBytes {
        match self {
            Self::Unshared(state) => &state.bytes,
            Self::Shared(state) => &state.bytes,
        }
    }
}
impl std::ops::DerefMut for MemoryGuard<'_> {
    fn deref_mut(&mut self) -> &mut LinearBytes {
        &mut self.state().bytes
    }
}

impl MemoryStorage {
    pub(crate) fn new(bytes: LinearBytes, shared: bool) -> Self {
        if shared {
            Self::Shared(std::sync::Arc::new(SharedMemory::new(bytes)))
        } else {
            Self::Unshared(std::cell::RefCell::new(MemoryState::new(bytes)))
        }
    }

    pub(crate) fn lock(&self) -> MemoryGuard<'_> {
        match self {
            // An unshared memory's accessors never nest, as with the shared lock.
            Self::Unshared(state) => MemoryGuard::Unshared(state.borrow_mut()),
            Self::Shared(memory) => memory.lock(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Unshared(state) => state.borrow().bytes.len(),
            Self::Shared(memory) => memory.len(),
        }
    }

    pub(crate) fn capacity(&self) -> usize {
        let mut guard = self.lock();
        let state = guard.state();
        state.bytes.capacity()
            + state.waiters.capacity() * std::mem::size_of::<super::wait::Waiter>()
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
        Self(std::sync::Mutex::new(MemoryState::new(bytes)))
    }

    pub(crate) fn lock(&self) -> MemoryGuard<'_> {
        MemoryGuard::Shared(self.state())
    }

    pub(super) fn state(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        // Guest byte accesses are checked before mutation; poisoning records a
        // Rust unwind, not corruption of the backing Vec's storage.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn len(&self) -> usize {
        let _order = super::atomic::memory_order();
        self.lock().len()
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
            pub(crate) const fn width(self) -> usize { match self { $(Self::$name => std::mem::size_of::<$word>(),)+ } }
            #[inline(always)]
            pub(crate) fn read(self, bytes: &[u8], address: u64) -> Result<WasmValue, WasmTrap> {
                let range = checked_range(address, self.width(), bytes.len())?;
                // SAFETY: the range is `width` bytes inside `bytes`.
                Ok(unsafe { self.read_at(bytes.as_ptr().add(range.start)) })
            }
            /// The value at `at`, unchecked.
            ///
            /// # Safety
            /// `width` bytes from `at` must be readable.
            #[inline(always)]
            pub(crate) unsafe fn read_at(self, at: *const u8) -> WasmValue {
                match self { $(Self::$name => {
                    // SAFETY: the caller keeps the word's bytes readable.
                    let $value = <$word>::from_le_bytes(unsafe { at.cast::<[u8; std::mem::size_of::<$word>()]>().read() });
                    $result
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
            pub(crate) const fn width(self) -> usize { match self { $(Self::$name => std::mem::size_of::<$word>(),)+ } }
            #[inline(always)]
            pub(crate) fn write(self, bytes: &mut [u8], address: u64, value: WasmValue) -> Result<(), WasmTrap> {
                let range = checked_range(address, self.width(), bytes.len())?;
                // SAFETY: the range is `width` bytes inside `bytes`.
                unsafe { self.write_at(bytes.as_mut_ptr().add(range.start), value) };
                Ok(())
            }
            /// Write `value` at `at`, unchecked.
            ///
            /// # Safety
            /// `width` bytes from `at` must be writable.
            #[inline(always)]
            pub(crate) unsafe fn write_at(self, at: *mut u8, value: WasmValue) {
                match self { $(Self::$name => {
                    let WasmValue::$variant(word) = value else { unreachable!("decoded store type") };
                    // SAFETY: the caller keeps the word's bytes writable.
                    unsafe { at.cast::<[u8; std::mem::size_of::<$word>()]>().write((word as $word).to_le_bytes()) };
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

// Scalar memory32 accesses with a static offset have one opcode each. The
// access semantics remain the MemoryLoad/MemoryStore rules above; the opcode
// only names the operator so dispatch needs no selector decode.
macro_rules! direct_accesses {
    ($kind:ident; $($name:ident => $op:ident;)+) => {
        impl $kind {
            pub(crate) const fn direct_op(self) -> Option<crate::bytecode::Op> {
                match self { $(Self::$name => Some(crate::bytecode::Op::$op),)+ #[allow(unreachable_patterns)] _ => None }
            }
            pub(crate) const fn from_direct_op(op: crate::bytecode::Op) -> Option<Self> {
                match op { $(crate::bytecode::Op::$op => Some(Self::$name),)+ _ => None }
            }
        }
    }
}
direct_accesses! { MemoryLoad;
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
direct_accesses! { MemoryStore;
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

/// The direct opcode for a scalar access, when the memory has a prologue
/// binding register and the access can use the direct memory32 form.
pub(super) fn direct_access(
    operator: &wasmparser::Operator<'_>,
) -> Option<(crate::bytecode::Op, wasmparser::MemArg)> {
    MemoryLoad::from_wasm(operator)
        .and_then(|(load, memarg)| Some((load.direct_op()?, memarg)))
        .or_else(|| {
            MemoryStore::from_wasm(operator)
                .and_then(|(store, memarg)| Some((store.direct_op()?, memarg)))
        })
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

/// Memories whose scalar accesses use a prologue binding register: memory32
/// memories accessed directly at least once by this function body.
pub(super) fn direct_access_memories(
    memories: &[WasmMemory],
    operators: &[wasmparser::Operator<'_>],
) -> Vec<bool> {
    let mut used = vec![false; memories.len()];
    for (_, memarg) in operators.iter().filter_map(direct_access) {
        if let Some(memory) = memories.get(memarg.memory as usize)
            && !memory.ty.memory64
        {
            used[memarg.memory as usize] = true;
        }
    }
    used
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
            crate::bytecode::Op::WasmInstanceBinding,
            register,
            0,
            0,
            slot as u32,
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
        if let Some((op, memarg)) = direct_access(operator)
            && let Some(&Some(memory)) = self.memory_registers.get(memarg.memory as usize)
        {
            if self.path == super::Reachability::Dead {
                return Ok(true);
            }
            let offset = u32::try_from(memarg.offset).map_err(|_| {
                crate::Diagnostic::unsupported(self.name, "Wasm memory32 offset out of range")
            })?;
            if MemoryStore::from_direct_op(op).is_some() {
                let value = self.pop()?;
                let address = self.pop()?;
                self.emit(op, value, memory, address, offset)?;
            } else {
                let address = self.pop()?;
                let result = self.push()?;
                self.emit(op, result, memory, address, offset)?;
                self.produced(result);
            }
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
