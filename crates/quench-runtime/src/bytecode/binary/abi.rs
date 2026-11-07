//! The identity of the encoded format: a program decodes only under the runtime ABI it was
//! encoded for.
use crate::bytecode::{CALL_PLAN_FLAG_COUNT, DERIVED_FACT_COUNT};

impl crate::bytecode::ResidualProgram {
    pub const FORMAT_VERSION: u8 = 137;
    pub const RUNTIME_ABI_FINGERPRINT: u64 = {
        const ABI_SIGNATURE: u64 = 0x5251_4a00_0000_0000;
        const FORMAT_VERSION_SHIFT: u32 = 16;
        const DERIVED_FACT_COUNT_SHIFT: u32 = u8::BITS;
        assert!(DERIVED_FACT_COUNT <= u8::MAX as u32);
        assert!(CALL_PLAN_FLAG_COUNT <= u8::MAX as u32);
        ABI_SIGNATURE | ((Self::FORMAT_VERSION as u64) << FORMAT_VERSION_SHIFT)
            | ((DERIVED_FACT_COUNT as u64) << DERIVED_FACT_COUNT_SHIFT)
            | CALL_PLAN_FLAG_COUNT as u64
    };
}
