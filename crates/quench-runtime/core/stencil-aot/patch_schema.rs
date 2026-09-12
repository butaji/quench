use crate::operand_holes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchEncoding {
    Branch26,
    LoadStoreUnsigned12,
    AddImmediate12,
    MovWide16,
    RawWord32,
    Pointer64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchBinding {
    Next,
    Slow,
    Taken,
    Operand(operand_holes::OperandHoleKind),
    SiteAdvanceBytes,
    RawValue { id: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchSite {
    pub offset: usize,
    pub encoding: PatchEncoding,
    pub binding: PatchBinding,
}

const AARCH64_LOAD_STORE_UNSIGNED_OPCODE_MASK: u32 = 0xffc0_0000;
const AARCH64_LOAD_X_UNSIGNED_OPCODE: u32 = 0xf940_0000;
const AARCH64_STORE_X_UNSIGNED_OPCODE: u32 = 0xf900_0000;
const AARCH64_LOAD_F64_UNSIGNED_OPCODE: u32 = 0xfd40_0000;
const AARCH64_STORE_F64_UNSIGNED_OPCODE: u32 = 0xfd00_0000;
const AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT: u32 = 10;
const AARCH64_UNSIGNED_OFFSET_FIELD_MASK: u32 =
    (1_u32 << operand_holes::AARCH64_UNSIGNED_OFFSET_FIELD_BITS) - 1;
const AARCH64_ADD_IMMEDIATE_OPCODE_MASK: u32 = 0xffc0_0000;
const AARCH64_ADD_X_IMMEDIATE_OPCODE: u32 = 0x9100_0000;
const AARCH64_ADD_IMMEDIATE_FIELD_SHIFT: u32 = 10;
const AARCH64_ADD_IMMEDIATE_FIELD_MASK: u32 = 0x0fff;
const AARCH64_MOV_WIDE_OPCODE_MASK: u32 = 0xff80_0000;
const AARCH64_MOV_ZERO_X_OPCODE: u32 = 0xd280_0000;
const AARCH64_MOV_KEEP_X_OPCODE: u32 = 0xf280_0000;
const AARCH64_MOV_ZERO_W_OPCODE: u32 = 0x5280_0000;
const AARCH64_MOV_KEEP_W_OPCODE: u32 = 0x7280_0000;
const AARCH64_MOV_WIDE_LANE_SHIFT: u32 = 21;
const AARCH64_MOV_WIDE_LANE_MASK: u32 = 0b11;
const AARCH64_MOV_WIDE_IMMEDIATE_SHIFT: u32 = 5;
const AARCH64_MOV_WIDE_IMMEDIATE_MASK: u32 = 0xffff;
const AARCH64_BRANCH_OPCODE_MASK: u32 = 0xfc00_0000;
const AARCH64_TAIL_BRANCH_OPCODE: u32 = 0x1400_0000;
const AARCH64_BRANCH_IMMEDIATE_MASK: u32 = 0x03ff_ffff;
const AARCH64_INSTRUCTION_BYTES: usize = core::mem::size_of::<u32>();
const POINTER_BYTES: usize = core::mem::size_of::<usize>();

impl PatchEncoding {
    pub const fn changed_mask(self) -> Option<u32> {
        match self {
            Self::Branch26 => Some(AARCH64_BRANCH_IMMEDIATE_MASK),
            Self::LoadStoreUnsigned12 => {
                Some(AARCH64_UNSIGNED_OFFSET_FIELD_MASK << AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT)
            }
            Self::AddImmediate12 => {
                Some(AARCH64_ADD_IMMEDIATE_FIELD_MASK << AARCH64_ADD_IMMEDIATE_FIELD_SHIFT)
            }
            Self::MovWide16 => {
                Some(AARCH64_MOV_WIDE_IMMEDIATE_MASK << AARCH64_MOV_WIDE_IMMEDIATE_SHIFT)
            }
            Self::RawWord32 | Self::Pointer64 => None,
        }
    }

    pub fn apply(self, bytes: &mut [u8], offset: usize, value: u64) -> Option<()> {
        match self {
            Self::Pointer64 => {
                let destination = bytes.get_mut(offset..offset.checked_add(POINTER_BYTES)?)?;
                destination.copy_from_slice(&(value as usize).to_le_bytes());
                Some(())
            }
            Self::RawWord32 => {
                let value = u32::try_from(value).ok()?;
                write_instruction(bytes, offset, value)
            }
            encoding => {
                let instruction = read_instruction(bytes, offset)?;
                let instruction = encoding.patch_instruction(instruction, value)?;
                write_instruction(bytes, offset, instruction)
            }
        }
    }

    fn patch_instruction(self, mut instruction: u32, value: u64) -> Option<u32> {
        match self {
            Self::Branch26 => {
                if instruction & AARCH64_BRANCH_OPCODE_MASK != AARCH64_TAIL_BRANCH_OPCODE {
                    return None;
                }
                let immediate = u32::try_from(value).ok()?;
                if immediate & !AARCH64_BRANCH_IMMEDIATE_MASK != 0 {
                    return None;
                }
                instruction &= !AARCH64_BRANCH_IMMEDIATE_MASK;
                instruction |= immediate;
            }
            Self::LoadStoreUnsigned12 => {
                let opcode = instruction & AARCH64_LOAD_STORE_UNSIGNED_OPCODE_MASK;
                if !matches!(
                    opcode,
                    AARCH64_LOAD_X_UNSIGNED_OPCODE
                        | AARCH64_STORE_X_UNSIGNED_OPCODE
                        | AARCH64_LOAD_F64_UNSIGNED_OPCODE
                        | AARCH64_STORE_F64_UNSIGNED_OPCODE
                ) {
                    return None;
                }
                let byte_offset = usize::try_from(value).ok()?;
                if byte_offset % operand_holes::VALUE_BYTE_WIDTH != 0 {
                    return None;
                }
                let scaled = byte_offset / operand_holes::VALUE_BYTE_WIDTH;
                if scaled > operand_holes::AARCH64_UNSIGNED_OFFSET_MAX_SCALED {
                    return None;
                }
                instruction &=
                    !(AARCH64_UNSIGNED_OFFSET_FIELD_MASK << AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT);
                instruction |= (scaled as u32) << AARCH64_UNSIGNED_OFFSET_FIELD_SHIFT;
            }
            Self::AddImmediate12 => {
                if instruction & AARCH64_ADD_IMMEDIATE_OPCODE_MASK != AARCH64_ADD_X_IMMEDIATE_OPCODE
                    || value > u64::from(AARCH64_ADD_IMMEDIATE_FIELD_MASK)
                {
                    return None;
                }
                instruction &=
                    !(AARCH64_ADD_IMMEDIATE_FIELD_MASK << AARCH64_ADD_IMMEDIATE_FIELD_SHIFT);
                instruction |= (value as u32) << AARCH64_ADD_IMMEDIATE_FIELD_SHIFT;
            }
            Self::MovWide16 => {
                let opcode = instruction & AARCH64_MOV_WIDE_OPCODE_MASK;
                if !matches!(
                    opcode,
                    AARCH64_MOV_ZERO_X_OPCODE
                        | AARCH64_MOV_KEEP_X_OPCODE
                        | AARCH64_MOV_ZERO_W_OPCODE
                        | AARCH64_MOV_KEEP_W_OPCODE
                ) {
                    return None;
                }
                let lane =
                    (instruction >> AARCH64_MOV_WIDE_LANE_SHIFT) & AARCH64_MOV_WIDE_LANE_MASK;
                let lane_shift = lane * u16::BITS;
                let immediate = ((value >> lane_shift) as u32) & AARCH64_MOV_WIDE_IMMEDIATE_MASK;
                instruction &=
                    !(AARCH64_MOV_WIDE_IMMEDIATE_MASK << AARCH64_MOV_WIDE_IMMEDIATE_SHIFT);
                instruction |= immediate << AARCH64_MOV_WIDE_IMMEDIATE_SHIFT;
            }
            Self::RawWord32 | Self::Pointer64 => return None,
        }
        Some(instruction)
    }
}

fn read_instruction(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(AARCH64_INSTRUCTION_BYTES)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn write_instruction(bytes: &mut [u8], offset: usize, instruction: u32) -> Option<()> {
    let end = offset.checked_add(AARCH64_INSTRUCTION_BYTES)?;
    bytes
        .get_mut(offset..end)?
        .copy_from_slice(&instruction.to_le_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mov_wide_patches_each_lane_from_one_raw_value() {
        const VALUE: u64 = 0x0123_4567_89ab_cdef;
        let mut instructions = [
            AARCH64_MOV_ZERO_X_OPCODE,
            AARCH64_MOV_KEEP_X_OPCODE | (1 << AARCH64_MOV_WIDE_LANE_SHIFT),
            AARCH64_MOV_KEEP_X_OPCODE | (2 << AARCH64_MOV_WIDE_LANE_SHIFT),
            AARCH64_MOV_KEEP_X_OPCODE | (3 << AARCH64_MOV_WIDE_LANE_SHIFT),
        ];
        let mut bytes = instructions
            .iter()
            .flat_map(|instruction| instruction.to_le_bytes())
            .collect::<Vec<_>>();
        for offset in (0..bytes.len()).step_by(AARCH64_INSTRUCTION_BYTES) {
            PatchEncoding::MovWide16
                .apply(&mut bytes, offset, VALUE)
                .expect("mov-wide instruction accepts raw value");
        }
        for (index, instruction) in instructions.iter_mut().enumerate() {
            *instruction = read_instruction(&bytes, index * AARCH64_INSTRUCTION_BYTES).unwrap();
            let immediate = (*instruction >> AARCH64_MOV_WIDE_IMMEDIATE_SHIFT)
                & AARCH64_MOV_WIDE_IMMEDIATE_MASK;
            assert_eq!(
                immediate,
                ((VALUE >> (index as u32 * u16::BITS)) & 0xffff) as u32
            );
        }
    }

    #[test]
    fn mov_wide_patches_word32_literal_lanes() {
        const WORD: u64 = 0x89ab_cdef;
        let instructions = [
            AARCH64_MOV_ZERO_W_OPCODE,
            AARCH64_MOV_KEEP_W_OPCODE | (1 << AARCH64_MOV_WIDE_LANE_SHIFT),
        ];
        let mut bytes = instructions
            .iter()
            .flat_map(|instruction| instruction.to_le_bytes())
            .collect::<Vec<_>>();
        for offset in (0..bytes.len()).step_by(AARCH64_INSTRUCTION_BYTES) {
            PatchEncoding::MovWide16
                .apply(&mut bytes, offset, WORD)
                .expect("Word32 mov-wide accepts a literal patch");
        }
        let low = read_instruction(&bytes, 0).unwrap();
        let high = read_instruction(&bytes, AARCH64_INSTRUCTION_BYTES).unwrap();
        assert_eq!(
            (low >> AARCH64_MOV_WIDE_IMMEDIATE_SHIFT) & AARCH64_MOV_WIDE_IMMEDIATE_MASK,
            WORD as u32 & AARCH64_MOV_WIDE_IMMEDIATE_MASK,
        );
        assert_eq!(
            (high >> AARCH64_MOV_WIDE_IMMEDIATE_SHIFT) & AARCH64_MOV_WIDE_IMMEDIATE_MASK,
            (WORD >> u16::BITS) as u32 & AARCH64_MOV_WIDE_IMMEDIATE_MASK,
        );
    }
}
