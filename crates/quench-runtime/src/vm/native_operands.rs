//! Resolved operands supplied by linked code, independent of packed bytecode.

use crate::bytecode::{Op, WideInstruction};

/// The native semantic-call data layout. Each register field retains the full
/// bytecode operand representation, including flags; the opcode is bound by
/// the selected entry. Both words are initialized and contain no heap pointers.
#[repr(C)]
pub struct NativeJsOperands {
    registers: u64,
    immediate: u64,
}

impl NativeJsOperands {
    const REGISTER_BITS: u32 = u16::BITS;

    #[cfg(feature = "aot")]
    pub const fn new(a: u16, b: u16, c: u16, immediate: u32) -> Self {
        Self {
            registers: a as u64
                | ((b as u64) << Self::REGISTER_BITS)
                | ((c as u64) << (Self::REGISTER_BITS + Self::REGISTER_BITS)),
            immediate: immediate as u64,
        }
    }

    /// The two-word native-call argument pointer, valid while this value lives.
    #[cfg(feature = "aot")]
    pub fn as_ptr(&self) -> *const u64 {
        &self.registers
    }

    pub(super) fn instruction(&self, opcode: Op) -> WideInstruction {
        WideInstruction::new(
            opcode,
            self.registers as u16,
            (self.registers >> Self::REGISTER_BITS) as u16,
            (self.registers >> (Self::REGISTER_BITS + Self::REGISTER_BITS)) as u16,
            self.immediate as u32,
        )
    }
}
