//! Resolved operands supplied by linked code, independent of packed bytecode.

use crate::bytecode::{Op, WideInstruction};

/// The native semantic-call data layout. Each register field retains the full
/// bytecode operand representation, including flags; the opcode is bound by
/// the selected entry. Both words are initialized and contain no heap pointers.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct NativeJsOperands {
    registers: u64,
    immediate: u64,
}

impl NativeJsOperands {
        const REGISTER_BITS: u32 = u16::BITS;

    pub const fn new(a: u16, b: u16, c: u16, immediate: u32) -> Self {
        Self {
            registers: a as u64
                | ((b as u64) << Self::REGISTER_BITS)
                | ((c as u64) << (Self::REGISTER_BITS + Self::REGISTER_BITS)),
            immediate: immediate as u64,
        }
    }

    /// The operands of `instruction`, as its semantic entry reads them.
    pub(crate) const fn of(instruction: WideInstruction) -> Self {
        Self::new(instruction.a(), instruction.b(), instruction.c(), instruction.imm())
    }

    /// The record's two words, in layout order: what an image stores and a linker places.
    pub(crate) const fn words(&self) -> [u64; 2] {
        [self.registers, self.immediate]
    }

    /// The two-word native-call argument pointer, valid while this value lives.
    pub fn as_ptr(&self) -> *const u64 {
        &self.registers
    }

    /// The host-native object representation; object emission targets this host.
    pub(super) fn append_native_bytes(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.registers.to_ne_bytes());
        bytes.extend_from_slice(&self.immediate.to_ne_bytes());
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
