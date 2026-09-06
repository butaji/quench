//! Small, target-specific decoders used to verify published stencil effects.
//!
//! These routines inspect bytes only; semantic admission and ABI facts remain
//! in the generated catalog and `machine` verifier.

pub(crate) fn contains_call(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        return bytes.chunks_exact(4).any(|word| {
            let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
            encoded & 0xFC00_0000 == 0x9400_0000 || encoded & 0xFFFF_FC1F == 0xD63F_0000
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
        return words.windows(3).any(|window| {
            window[0] == 0xF940_1805
                && window[1] == 0x3940_00A6
                && window[2] & 0xFF00_001F == 0x3500_0006
        });
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = bytes;
        false
    }
}

pub(crate) fn simd_clobber_mask(bytes: &[u8]) -> u16 {
    #[cfg(target_arch = "aarch64")]
    {
        return bytes
            .chunks_exact(4)
            .filter_map(|word| {
                let encoded = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
                let fp_load = encoded & 0xFFC0_0000 == 0xFD40_0000;
                let fp_arith = encoded & 0xFF20_FC00 == 0x1E20_2800;
                let fp_move = encoded & 0xFF20_FC00 == 0x1E20_4000;
                (fp_load || fp_arith || fp_move).then_some((encoded & 0x1f) as u16)
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
                let load = encoded & 0xFFC0_0000 == 0xF940_0000;
                let writes_rt = load
                    || encoded & 0xFFE0_0000 == 0x8B00_0000
                    || encoded & 0xFFC0_0000 == 0x9100_0000
                    || encoded & 0xFFE0_0000 == 0x5280_0000
                    || encoded & 0xFFC0_0000 == 0x3940_0000;
                writes_rt.then_some((encoded & 0x1f) as u16)
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
    let (immediate, bits) = if encoded & 0xFC00_0000 == 0x1400_0000 {
        (encoded & 0x03FF_FFFF, 26)
    } else if encoded & 0xFF00_0010 == 0x5400_0000 || encoded & 0x7F00_0000 == 0x3500_0000 {
        ((encoded >> 5) & 0x7_FFFF, 19)
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
    known_aarch64_instruction(encoded)
}

#[cfg(target_arch = "aarch64")]
fn known_aarch64_instruction(encoded: u32) -> bool {
    encoded == 0xD65F_03C0
        || encoded == 0x9E67_03E1
        || encoded & 0xFFC0_0000 == 0xF940_0000
        || encoded & 0xFFC0_0000 == 0xF900_0000
        || encoded & 0xFFC0_0000 == 0xFD40_0000
        || encoded & 0xFFC0_0000 == 0xFD00_0000
        || encoded & 0xFFC0_0000 == 0x3940_0000
        || encoded & 0xFFE0_0000 == 0x8B00_0000
        || encoded & 0xFFC0_0000 == 0x9100_0000
        || encoded & 0xFF80_0000 == 0x5280_0000
        || encoded & 0xFC00_0000 == 0x1400_0000
        || encoded & 0xFF00_0010 == 0x5400_0000
        || encoded & 0x7F00_0000 == 0x3500_0000
        || encoded & 0xFF20_FC00 == 0x1E20_2800
        || encoded & 0xFF20_FC00 == 0x1E20_3800
        || encoded & 0xFF20_FC00 == 0x1E20_4000
        || encoded & 0xFF20_FC00 == 0x1E20_0800
        || encoded & 0xFF20_FC00 == 0x1E20_1800
        || encoded & 0xFF00_0000 == 0x5800_0000
        || encoded & 0xFFFF_FC1F == 0xD61F_0000
        || encoded & 0xFFFF_FC1F == 0xD63F_0000
        || encoded & 0xFF20_FC00 == 0x1E20_2000
        || encoded & 0xFFE0_07E0 == 0x1A80_07E0
        || encoded & 0xFF00_0000 == 0x0A00_0000
        || encoded & 0xFF00_0000 == 0x2A00_0000
        || encoded & 0xFF00_0000 == 0x4A00_0000
        || encoded & 0xFFE0_FC00 == 0x1AC0_2000
        || encoded & 0xFFE0_FC00 == 0x1AC0_2400
        || encoded & 0xFFE0_FC00 == 0x1AC0_2800
        || encoded & 0xFF20_FC00 == 0x1E20_4000
        || encoded & 0xFFE0_FC1F == 0xEB00_001F
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
}
