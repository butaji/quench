//! Small, target-specific decoders used to verify published stencil effects.
//!
//! These routines inspect bytes only; semantic admission and ABI facts remain
//! in the generated catalog and `machine` verifier.

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
struct Aarch64Pattern {
    mask: u32,
    value: u32,
}

#[cfg(target_arch = "aarch64")]
impl Aarch64Pattern {
    const fn new(mask: u32, value: u32) -> Self {
        Self { mask, value }
    }
    const fn matches(self, word: u32) -> bool {
        word & self.mask == self.value
    }
}

#[cfg(target_arch = "aarch64")]
mod aarch64 {
    use super::Aarch64Pattern as P;

    pub(super) const RETURN: u32 = 0xD65F_03C0;
    pub(super) const FMOV_D1_XZR: u32 = 0x9E67_03E1;
    pub(super) const DIRECT_BRANCH: P = P::new(0xFC00_0000, 0x1400_0000);
    pub(super) const DIRECT_CALL: P = P::new(0xFC00_0000, 0x9400_0000);
    pub(super) const INDIRECT_BRANCH: P = P::new(0xFFFF_FC1F, 0xD61F_0000);
    pub(super) const INDIRECT_CALL: P = P::new(0xFFFF_FC1F, 0xD63F_0000);
    pub(super) const CONDITIONAL_BRANCH: P = P::new(0xFF00_0010, 0x5400_0000);
    pub(super) const COMPARE_BRANCH_ZERO: P = P::new(0x7F00_0000, 0x3400_0000);
    pub(super) const COMPARE_BRANCH_NONZERO: P = P::new(0x7F00_0000, 0x3500_0000);
    pub(super) const LOAD_X: P = P::new(0xFFC0_0000, 0xF940_0000);
    pub(super) const LOAD_W: P = P::new(0xFFC0_0000, 0xB940_0000);
    pub(super) const STORE_X: P = P::new(0xFFC0_0000, 0xF900_0000);
    pub(super) const STORE_W: P = P::new(0xFFC0_0000, 0xB900_0000);
    pub(super) const STORE_W_REGISTER_OFFSET: P = P::new(0xFFE0_FC00, 0xB820_7800);
    pub(super) const LOAD_D: P = P::new(0xFFC0_0000, 0xFD40_0000);
    pub(super) const STORE_D: P = P::new(0xFFC0_0000, 0xFD00_0000);
    pub(super) const LOAD_BYTE: P = P::new(0xFFC0_0000, 0x3940_0000);
    pub(super) const ADD_X_SHIFTED: P = P::new(0xFFE0_0000, 0x8B00_0000);
    pub(super) const SUB_X_SHIFTED: P = P::new(0xFFE0_0000, 0xCB00_0000);
    pub(super) const ADD_W_SHIFTED: P = P::new(0x7FE0_0000, 0x0B00_0000);
    pub(super) const SUB_W_SHIFTED: P = P::new(0x7FE0_0000, 0x4B00_0000);
    pub(super) const MUL_W: P = P::new(0xFFE0_FC00, 0x1B00_7C00);
    pub(super) const SIGNED_DIVIDE_W: P = P::new(0xFFE0_FC00, 0x1AC0_0C00);
    pub(super) const MULTIPLY_SUBTRACT_W: P = P::new(0xFFE0_8000, 0x1B00_8000);
    pub(super) const MULTIPLY_ADD_W: P = P::new(0xFFE0_8000, 0x1B00_0000);
    pub(super) const ADD_X_IMMEDIATE: P = P::new(0xFFC0_0000, 0x9100_0000);
    pub(super) const ADD_W_IMMEDIATE: P = P::new(0x7FC0_0000, 0x1100_0000);
    pub(super) const MOVE_W_IMMEDIATE: P = P::new(0xFF80_0000, 0x5280_0000);
    pub(super) const FP_ADD: P = P::new(0xFF20_FC00, 0x1E20_2800);
    pub(super) const FP_SUB: P = P::new(0xFF20_FC00, 0x1E20_3800);
    pub(super) const FP_MOVE: P = P::new(0xFF20_FC00, 0x1E20_4000);
    pub(super) const FP_MUL: P = P::new(0xFF20_FC00, 0x1E20_0800);
    pub(super) const FP_DIV: P = P::new(0xFF20_FC00, 0x1E20_1800);
    pub(super) const FP_IMMEDIATE: P = P::new(0xFF20_1FE0, 0x1E20_1000);
    pub(super) const FP_COMPARE: P = P::new(0xFF20_FC00, 0x1E20_2000);
    pub(super) const FP_TO_UNSIGNED_X: P = P::new(0xFFFF_FC00, 0x9E79_0000);
    pub(super) const UNSIGNED_W_TO_FP: P = P::new(0xFFFF_FC00, 0x1E63_0000);
    pub(super) const SIGNED_W_TO_FP: P = P::new(0xFFFF_FC00, 0x1E62_0000);
    pub(super) const LOAD_LITERAL_X: P = P::new(0xFF00_0000, 0x5800_0000);
    pub(super) const CONDITIONAL_SELECT: P = P::new(0xFFE0_07E0, 0x1A80_07E0);
    pub(super) const CONDITIONAL_INCREMENT: P = P::new(0x7FE0_0C00, 0x1A80_0400);
    pub(super) const AND_W: P = P::new(0xFF00_0000, 0x0A00_0000);
    pub(super) const AND_W_IMMEDIATE: P = P::new(0x7F80_0000, 0x1200_0000);
    pub(super) const OR_W: P = P::new(0xFF00_0000, 0x2A00_0000);
    pub(super) const XOR_W: P = P::new(0xFF00_0000, 0x4A00_0000);
    pub(super) const SHIFT_LEFT_W: P = P::new(0xFFE0_FC00, 0x1AC0_2000);
    pub(super) const SHIFT_RIGHT_W: P = P::new(0xFFE0_FC00, 0x1AC0_2400);
    pub(super) const SHIFT_RIGHT_UNSIGNED_W: P = P::new(0xFFE0_FC00, 0x1AC0_2800);
    pub(super) const COMPARE_X: P = P::new(0xFFE0_FC1F, 0xEB00_001F);
    pub(super) const COMPARE_W: P = P::new(0xFFE0_FC1F, 0x6B00_001F);
    pub(super) const COMPARE_W_IMMEDIATE: P = P::new(0xFFC0_001F, 0x7100_001F);
    pub(super) const SIGN_EXTEND_W_TO_X: P = P::new(0xFFFF_FC00, 0x9340_7C00);
    pub(super) const MULTIPLY_ADD_X: P = P::new(0xFFE0_8000, 0x9B00_0000);
}

pub(crate) fn contains_call(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        return bytes.chunks_exact(4).any(|word| {
            let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            aarch64::DIRECT_CALL.matches(encoded) || aarch64::INDIRECT_CALL.matches(encoded)
        });
    }
    #[cfg(target_arch = "x86_64")]
    {
        // This is deliberately conservative: without a full x86 decoder,
        // every plausible CALL opcode is treated as a helper effect.  False
        // positives reject a template; false negatives would misdeclare its
        // ABI.  The scan still distinguishes FF /2 CALL from FF /4 JMP.
        return bytes.windows(5).any(|window| window[0] == 0xE8)
            || bytes.windows(2).any(|window| {
                // FF /2 is CALL r/m; /4 is JMP and must not be treated as a
                // helper call merely because it shares the FF opcode.
                window[0] == 0xFF && window[1] & 0x38 == 0x10
            });
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = bytes;
        false
    }
}

pub(crate) fn contains_interrupt_checkpoint(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        return words.windows(3).any(aarch64_interrupt_poll);
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = bytes;
        false
    }
}

#[cfg(target_arch = "aarch64")]
fn aarch64_interrupt_poll(window: &[u32]) -> bool {
    let pointer = window[0];
    let flag = window[1];
    let branch = window[2];
    let pointer_register = pointer & 0x1f;
    let flag_register = flag & 0x1f;
    const LOAD_BASE_AND_TARGET_MASK: u32 = 0xFFC0_03E0;
    const BRANCH_OPCODE_AND_TARGET_MASK: u32 = 0xFF00_001F;
    pointer & LOAD_BASE_AND_TARGET_MASK == aarch64::LOAD_X.value
        && flag & LOAD_BASE_AND_TARGET_MASK == (aarch64::LOAD_BYTE.value | (pointer_register << 5))
        && branch & BRANCH_OPCODE_AND_TARGET_MASK
            == (aarch64::COMPARE_BRANCH_NONZERO.value | flag_register)
}

pub(crate) fn simd_clobber_mask(bytes: &[u8]) -> u16 {
    #[cfg(target_arch = "aarch64")]
    {
        return bytes
            .chunks_exact(4)
            .filter_map(|word| {
                let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
                let fp_load = aarch64::LOAD_D.matches(encoded);
                let fp_arith = aarch64::FP_ADD.matches(encoded)
                    || aarch64::FP_SUB.matches(encoded)
                    || aarch64::FP_MUL.matches(encoded)
                    || aarch64::FP_DIV.matches(encoded);
                let fp_move = aarch64::FP_MOVE.matches(encoded);
                let fp_immediate = aarch64::FP_IMMEDIATE.matches(encoded);
                let integer_to_fp = aarch64::UNSIGNED_W_TO_FP.matches(encoded)
                    || aarch64::SIGNED_W_TO_FP.matches(encoded);
                (fp_load || fp_arith || fp_move || fp_immediate || integer_to_fp)
                    .then_some((encoded & 0x1f) as u16)
            })
            .fold(0u16, |mask, register| {
                if register < 16 {
                    mask | (1u16 << register)
                } else {
                    u16::MAX
                }
            });
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = bytes;
        0
    }
}

pub(crate) fn gpr_clobber_mask(bytes: &[u8]) -> u16 {
    #[cfg(target_arch = "aarch64")]
    {
        return bytes
            .chunks_exact(4)
            .filter_map(|word| {
                let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
                let load = aarch64::LOAD_X.matches(encoded) || aarch64::LOAD_W.matches(encoded);
                let writes_rt = load
                    || aarch64::ADD_X_SHIFTED.matches(encoded)
                    || aarch64::SUB_X_SHIFTED.matches(encoded)
                    || aarch64::ADD_W_SHIFTED.matches(encoded)
                    || aarch64::SUB_W_SHIFTED.matches(encoded)
                    || aarch64::MUL_W.matches(encoded)
                    || aarch64::SIGNED_DIVIDE_W.matches(encoded)
                    || aarch64::MULTIPLY_SUBTRACT_W.matches(encoded)
                    || aarch64::MULTIPLY_ADD_W.matches(encoded)
                    || aarch64::MULTIPLY_ADD_X.matches(encoded)
                    || aarch64::ADD_X_IMMEDIATE.matches(encoded)
                    || aarch64::ADD_W_IMMEDIATE.matches(encoded)
                    || aarch64::MOVE_W_IMMEDIATE.matches(encoded)
                    || aarch64::LOAD_BYTE.matches(encoded);
                let writes_rt = writes_rt
                    || aarch64::FP_TO_UNSIGNED_X.matches(encoded)
                    || aarch64::SIGN_EXTEND_W_TO_X.matches(encoded);
                let writes_rt = writes_rt || aarch64::AND_W_IMMEDIATE.matches(encoded);
                let writes_rt = writes_rt
                    || aarch64::AND_W.matches(encoded)
                    || aarch64::OR_W.matches(encoded)
                    || aarch64::XOR_W.matches(encoded);
                let conditional_select = aarch64::CONDITIONAL_INCREMENT.matches(encoded);
                (writes_rt || conditional_select).then_some((encoded & 0x1f) as u16)
            })
            .fold(0u16, |mask, register| {
                if register < 16 {
                    mask | (1u16 << register)
                } else {
                    u16::MAX
                }
            });
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = bytes;
        0
    }
}

/// Fail-closed validation for the restricted AArch64 raw-kernel vocabulary.
/// A byte-pattern hit is not an instruction proof: every word must be one of
/// the declared load/store, arithmetic, branch, compare, move, or return
/// forms before ABI effects are trusted.
pub(crate) fn validate_raw_instruction_stream(bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_arch = "aarch64")]
    {
        if bytes.len() % 4 != 0 {
            return Err("raw stencil is not instruction aligned".into());
        }
        for (index, word) in bytes.chunks_exact(4).enumerate() {
            let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            if !known_aarch64_raw_instruction(encoded) {
                return Err(format!(
                    "raw stencil contains unknown instruction {encoded:08x} at {index}"
                ));
            }
            if !branch_target_is_local(encoded, index, bytes.len()) {
                return Err(format!("raw stencil branch leaves region at {index}"));
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    let _ = bytes;
    Ok(())
}

#[cfg(target_arch = "aarch64")]
fn branch_target_is_local(encoded: u32, index: usize, length: usize) -> bool {
    const BRANCH26_IMMEDIATE_MASK: u32 = 0x03FF_FFFF;
    const BRANCH19_IMMEDIATE_MASK: u32 = 0x7_FFFF;
    let (immediate, bits) = if aarch64::DIRECT_BRANCH.matches(encoded) {
        (encoded & BRANCH26_IMMEDIATE_MASK, 26)
    } else if aarch64::CONDITIONAL_BRANCH.matches(encoded)
        || aarch64::COMPARE_BRANCH_ZERO.matches(encoded)
        || aarch64::COMPARE_BRANCH_NONZERO.matches(encoded)
    {
        ((encoded >> 5) & BRANCH19_IMMEDIATE_MASK, 19)
    } else {
        return true;
    };
    let sign_bit = 1_i64 << (bits - 1);
    let signed = i64::from(immediate);
    let signed = if signed & sign_bit != 0 {
        signed - (1_i64 << bits)
    } else {
        signed
    };
    let target = index as i64 * 4 + signed * 4;
    target >= 0 && target < length as i64
}

/// Validate a hole-free AArch64 template before trusting its declared effect.
/// Literal-pool templates are checked by their relocation/data contract and
/// intentionally do not enter this instruction-only path.
pub(crate) fn validate_aarch64_instruction_stream(bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_arch = "aarch64")]
    {
        if bytes.len() % 4 != 0 {
            return Err("AArch64 stencil is not instruction aligned".into());
        }
        for (index, word) in bytes.chunks_exact(4).enumerate() {
            let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            if !known_aarch64_instruction(encoded) {
                return Err(format!(
                    "AArch64 stencil contains unknown instruction {encoded:08x} at {index}"
                ));
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    let _ = bytes;
    Ok(())
}

#[cfg(target_arch = "aarch64")]
fn known_aarch64_raw_instruction(encoded: u32) -> bool {
    let indirect_branch = aarch64::INDIRECT_BRANCH.matches(encoded);
    let indirect_call = aarch64::INDIRECT_CALL.matches(encoded);
    known_aarch64_instruction(encoded) && !indirect_branch && !indirect_call
}

#[cfg(target_arch = "aarch64")]
fn known_aarch64_instruction(encoded: u32) -> bool {
    const PATTERNS: &[Aarch64Pattern] = &[
        aarch64::LOAD_X,
        aarch64::LOAD_W,
        aarch64::STORE_X,
        aarch64::STORE_W,
        aarch64::STORE_W_REGISTER_OFFSET,
        aarch64::LOAD_D,
        aarch64::STORE_D,
        aarch64::LOAD_BYTE,
        aarch64::ADD_X_SHIFTED,
        aarch64::SUB_X_SHIFTED,
        aarch64::ADD_W_SHIFTED,
        aarch64::SUB_W_SHIFTED,
        aarch64::MUL_W,
        aarch64::SIGNED_DIVIDE_W,
        aarch64::MULTIPLY_SUBTRACT_W,
        aarch64::MULTIPLY_ADD_W,
        aarch64::ADD_X_IMMEDIATE,
        aarch64::ADD_W_IMMEDIATE,
        aarch64::MOVE_W_IMMEDIATE,
        aarch64::DIRECT_BRANCH,
        aarch64::CONDITIONAL_BRANCH,
        aarch64::COMPARE_BRANCH_ZERO,
        aarch64::COMPARE_BRANCH_NONZERO,
        aarch64::FP_ADD,
        aarch64::FP_SUB,
        aarch64::FP_MOVE,
        aarch64::FP_MUL,
        aarch64::FP_DIV,
        aarch64::FP_IMMEDIATE,
        aarch64::LOAD_LITERAL_X,
        aarch64::INDIRECT_BRANCH,
        aarch64::INDIRECT_CALL,
        aarch64::FP_COMPARE,
        aarch64::FP_TO_UNSIGNED_X,
        aarch64::UNSIGNED_W_TO_FP,
        aarch64::SIGNED_W_TO_FP,
        aarch64::CONDITIONAL_SELECT,
        aarch64::CONDITIONAL_INCREMENT,
        aarch64::AND_W,
        aarch64::AND_W_IMMEDIATE,
        aarch64::OR_W,
        aarch64::XOR_W,
        aarch64::SHIFT_LEFT_W,
        aarch64::SHIFT_RIGHT_W,
        aarch64::SHIFT_RIGHT_UNSIGNED_W,
        aarch64::COMPARE_X,
        aarch64::COMPARE_W,
        aarch64::COMPARE_W_IMMEDIATE,
        aarch64::SIGN_EXTEND_W_TO_X,
        aarch64::MULTIPLY_ADD_X,
    ];
    encoded == aarch64::RETURN
        || encoded == aarch64::FMOV_D1_XZR
        || PATTERNS.iter().any(|pattern| pattern.matches(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn raw_validator_rejects_unknown_register_writer() {
        let orr_x0_x0_x0 = 0xAA00_0000u32.to_le_bytes();
        assert!(validate_raw_instruction_stream(&orr_x0_x0_x0).is_err());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn raw_validator_rejects_direct_branch_outside_region() {
        let branch_past_end = 0x1400_0001u32.to_le_bytes();
        assert!(validate_raw_instruction_stream(&branch_past_end).is_err());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn raw_validator_rejects_unprovable_indirect_control() {
        let branch_x8 = 0xD61F_0100u32.to_le_bytes();
        let call_x8 = 0xD63F_0100u32.to_le_bytes();
        assert!(validate_raw_instruction_stream(&branch_x8).is_err());
        assert!(validate_raw_instruction_stream(&call_x8).is_err());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn interrupt_checkpoint_accepts_declared_register_roles() {
        let bytes = [0xF940_1006u32, 0x3940_00C7, 0x3500_00A7]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(contains_interrupt_checkpoint(&bytes));
    }
}
