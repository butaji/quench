//! A VM frame's register file with the frame context in front of register 0 (M2): the words
//! `FRAME_CONTEXT` names sit at `registers[-FRAME_CONTEXT..0]` (receiver, environment, captures, then a plain activation's header), as below every lane activation's window,
//! so a handler reads `this`, the environment and the captures at fixed negative registers
//! whichever kind of frame it runs on. Everything else sees the registers alone: it derefs to
//! the slice past the context, and its length, capacity and pointers are the registers'.

use crate::snapshot::{Reader, Snap, SnapshotResult, Writer};
use crate::value::Value;

/// The words in front of register 0: the frame context `staged::frame::ContextWords` lays out
/// (`CONTEXT_WORDS` of them, `this` at `r[-1]`, then env, capture, call, the caller links, packed,
/// callee and body).
pub const FRAME_CONTEXT: usize = crate::staged::frame::CONTEXT_WORDS;
/// The words a new file holds room for past its context: a typical frame's registers and window plus
/// `crate::vm::engine_types::INLINED_WINDOW_ROOM`.
const SPARE_WORDS: usize = 32 + crate::vm::engine_types::INLINED_WINDOW_ROOM;

#[derive(Clone)]
pub struct RegisterFile {
    /// The context words, then the registers.
    words: Vec<Value>,
}

impl Default for RegisterFile {
    fn default() -> Self { Self::new() }
}

impl RegisterFile {
    /// An empty file with spare room for a small frame's registers, its environment window and the windows
    /// of the callees a loop region inlines past them (`dispatch_fast::frame_holds_window_in_lane`), so that a
    /// region runs over the frame without growing it under the lane's pointers.
    pub(in crate::vm) fn new() -> Self {
        let mut words = Vec::with_capacity(FRAME_CONTEXT + SPARE_WORDS);
        words.resize(FRAME_CONTEXT, Value::UNDEFINED);
        Self { words }
    }

    pub(in crate::vm) fn from_registers(registers: &[Value]) -> Self {
        let mut file = Self::new();
        file.words.extend_from_slice(registers);
        file
    }

    #[quench_macros::test_visible]
    #[inline(always)] pub(in crate::vm) fn clear(&mut self) { self.words.truncate(FRAME_CONTEXT); }
    #[inline(always)] pub(in crate::vm) fn reserve(&mut self, additional: usize) { self.words.reserve(additional); }
    #[inline(always)] pub fn capacity(&self) -> usize { self.words.capacity() - FRAME_CONTEXT }
    #[inline(always)] pub(in crate::vm) fn shrink_to(&mut self, capacity: usize) { self.words.shrink_to(FRAME_CONTEXT + capacity); }
    #[quench_macros::test_visible]
    #[inline(always)] pub(in crate::vm) fn resize(&mut self, len: usize, value: Value) { self.words.resize(FRAME_CONTEXT + len, value); }
    #[inline(always)] pub(in crate::vm) fn extend_from_slice(&mut self, values: &[Value]) { self.words.extend_from_slice(values); }

    /// # Safety
    /// As `Vec::set_len`, for the registers.
    #[inline(always)]
    pub(in crate::vm) unsafe fn set_len(&mut self, len: usize) {
        // SAFETY: as the caller's contract, past the context words, which always hold values.
        unsafe { self.words.set_len(FRAME_CONTEXT + len) }
    }

    /// Register 0's address; the context words sit just below it.
    #[inline(always)]
    pub(in crate::vm) fn as_mut_ptr(&mut self) -> *mut Value {
        // SAFETY: the words always hold the context.
        unsafe { self.words.as_mut_ptr().add(FRAME_CONTEXT) }
    }

    #[inline(always)]
    pub(in crate::vm) fn as_ptr(&self) -> *const Value {
        // SAFETY: as `as_mut_ptr`.
        unsafe { self.words.as_ptr().add(FRAME_CONTEXT) }
    }
}

impl std::ops::Deref for RegisterFile {
    type Target = [Value];
    #[inline(always)]
    fn deref(&self) -> &[Value] {
        // SAFETY: the words always hold the context.
        unsafe { self.words.get_unchecked(FRAME_CONTEXT..) }
    }
}

impl std::ops::DerefMut for RegisterFile {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [Value] {
        // SAFETY: as `deref`.
        unsafe { self.words.get_unchecked_mut(FRAME_CONTEXT..) }
    }
}

/// The registers as a list, as `Vec<Value>` records them: the context words are a running
/// window's, rewritten when one links the file.
impl Snap for RegisterFile {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|value| value.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self::from_registers(&Vec::<Value>::unsnap(reader)?))
    }
}

impl std::fmt::Debug for RegisterFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { (**self).fmt(f) }
}

#[cfg(test)]
impl<const N: usize> PartialEq<[Value; N]> for RegisterFile {
    fn eq(&self, other: &[Value; N]) -> bool { **self == *other }
}
