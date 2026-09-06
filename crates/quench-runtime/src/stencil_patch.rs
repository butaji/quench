//! Straight-line typed hole writers.
//!
//! Hole-kind dispatch is centralized in `apply_holes`; each writer handles one
//! closed relocation shape and cannot silently interpret another kind.

use crate::stencil_fact::{Hole, HoleKind, PatchValues};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchError {
    OutOfBounds,
    UnsupportedOffset,
}

/// The four writers differ only in their declared hole kind and width.  A
/// macro keeps that fact table canonical while still expanding to four plain,
/// straight-line functions (no dynamic dispatch in a writer).
macro_rules! typed_writer {
    ($name:ident, $kind:ident, $width:literal, $value_type:ty) => {
        pub fn $name<const N: usize>(
            dst: &mut [u8],
            offset: u16,
            values: &PatchValues<'_, N>,
        ) -> Result<(), PatchError> {
            let start = usize::from(offset);
            let bytes = <$value_type>::try_from(values.value_for(HoleKind::$kind))
                .map_err(|_| PatchError::UnsupportedOffset)?
                .to_le_bytes();
            let slot = dst
                .get_mut(start..start + $width)
                .ok_or(PatchError::OutOfBounds)?;
            slot.copy_from_slice(&bytes);
            Ok(())
        }
    };
}

typed_writer!(write_imm32, Imm32, 4, u32);
typed_writer!(write_disp32, Disp32, 4, u32);
typed_writer!(write_literal64, Literal64, 8, u64);
typed_writer!(write_ptr64, Ptr64, 8, u64);

/// Rel32 carries a signed displacement encoded through the shared `u64` value
/// view.  Validate the signed range before converting to bytes so a malformed
/// target cannot silently wrap into a different branch destination.
pub fn write_rel32<const N: usize>(
    dst: &mut [u8],
    offset: u16,
    values: &PatchValues<'_, N>,
) -> Result<(), PatchError> {
    let start = usize::from(offset);
    let displacement = values.value_for(HoleKind::Rel32) as i64;
    let bytes = i32::try_from(displacement)
        .map_err(|_| PatchError::UnsupportedOffset)?
        .to_le_bytes();
    let slot = dst
        .get_mut(start..start + 4)
        .ok_or(PatchError::OutOfBounds)?;
    slot.copy_from_slice(&bytes);
    Ok(())
}

/// Patch an AArch64 `B` immediate while preserving its opcode bits. The
/// displacement is byte-based in `PatchValues`; the ISA stores a signed,
/// four-byte-aligned word offset in bits 25:0 (±128 MiB).
pub fn write_branch26<const N: usize>(
    dst: &mut [u8],
    offset: u16,
    values: &PatchValues<'_, N>,
) -> Result<(), PatchError> {
    if dst
        .get(usize::from(offset)..usize::from(offset).saturating_add(4))
        .is_none()
    {
        return Err(PatchError::OutOfBounds);
    }
    validate_branch26(dst, offset, values)?;
    let start = usize::from(offset);
    let slot = &mut dst[start..start + 4];
    let words = (values.value_for(HoleKind::Branch26) as i64) / 4;
    let mut instruction = u32::from_le_bytes(slot.try_into().expect("validated width"));
    // A Branch26 hole is only valid in an AArch64 unconditional `B` word.
    // Refuse to patch arbitrary data or a conditional/register branch: keeping
    // the high bits would otherwise turn a malformed template into callable
    // code with an unrelated control-flow encoding.
    if instruction & 0x7c00_0000 != 0x1400_0000 {
        return Err(PatchError::UnsupportedOffset);
    }
    instruction = (instruction & 0xfc00_0000) | (words as u32 & 0x03ff_ffff);
    slot.copy_from_slice(&instruction.to_le_bytes());
    Ok(())
}

/// Patch an AArch64 `B.cond` immediate from a byte displacement. The
/// condition and opcode remain part of the verified template; only signed
/// imm19 bits [23:5] are replaced (four-byte aligned, ±1 MiB).
pub(crate) fn write_cond_branch19(
    dst: &mut [u8],
    offset: usize,
    displacement: i64,
) -> Result<(), PatchError> {
    let slot = dst
        .get_mut(offset..offset.saturating_add(4))
        .ok_or(PatchError::OutOfBounds)?;
    if offset % 4 != 0 || displacement % 4 != 0 {
        return Err(PatchError::UnsupportedOffset);
    }
    let words = displacement / 4;
    if !(-(1_i64 << 18)..(1_i64 << 18)).contains(&words) {
        return Err(PatchError::UnsupportedOffset);
    }
    let mut instruction = u32::from_le_bytes(slot.try_into().expect("validated width"));
    if instruction & 0xff00_0010 != 0x5400_0000 {
        return Err(PatchError::UnsupportedOffset);
    }
    instruction = (instruction & !0x00ff_ffe0) | ((words as u32 & 0x7_ffff) << 5);
    slot.copy_from_slice(&instruction.to_le_bytes());
    Ok(())
}

pub fn apply_holes<const N: usize>(
    dst: &mut [u8],
    holes: &[Hole],
    values: &PatchValues<'_, N>,
) -> Result<(), PatchError> {
    for hole in holes {
        validate_hole(dst, *hole, values)?;
    }
    for hole in holes {
        match hole.kind {
            HoleKind::Imm32 => write_imm32(dst, hole.offset, values)?,
            HoleKind::Disp32 => write_disp32(dst, hole.offset, values)?,
            HoleKind::Rel32 => write_rel32(dst, hole.offset, values)?,
            HoleKind::Branch26 => write_branch26(dst, hole.offset, values)?,
            HoleKind::Literal64 => write_literal64(dst, hole.offset, values)?,
            HoleKind::Ptr64 => write_ptr64(dst, hole.offset, values)?,
        }
    }
    Ok(())
}

fn validate_hole<const N: usize>(
    dst: &[u8],
    hole: Hole,
    values: &PatchValues<'_, N>,
) -> Result<(), PatchError> {
    let width = match hole.kind {
        HoleKind::Imm32 | HoleKind::Disp32 | HoleKind::Rel32 | HoleKind::Branch26 => 4,
        HoleKind::Literal64 | HoleKind::Ptr64 => 8,
    };
    let start = usize::from(hole.offset);
    if dst.get(start..start + width).is_none() {
        return Err(PatchError::OutOfBounds);
    }
    if matches!(hole.kind, HoleKind::Branch26) {
        validate_branch26(dst, hole.offset, values)?;
    }
    Ok(())
}

fn validate_branch26<const N: usize>(
    dst: &[u8],
    offset: u16,
    values: &PatchValues<'_, N>,
) -> Result<(), PatchError> {
    let start = usize::from(offset);
    if start % 4 != 0 {
        return Err(PatchError::UnsupportedOffset);
    }
    let displacement = values.value_for(HoleKind::Branch26) as i64;
    let words = displacement / 4;
    if displacement % 4 != 0 || !(-(1_i64 << 25)..(1_i64 << 25)).contains(&words) {
        return Err(PatchError::UnsupportedOffset);
    }
    let instruction = u32::from_le_bytes(
        dst[start..start + 4]
            .try_into()
            .map_err(|_| PatchError::OutOfBounds)?,
    );
    if instruction & 0x7c00_0000 != 0x1400_0000 {
        return Err(PatchError::UnsupportedOffset);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Opcode;
    use crate::quickening::QuickeningSite;

    #[test]
    fn typed_writers_patch_only_their_declared_width() {
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site);
        let mut bytes = [0u8; 24];
        apply_holes(
            &mut bytes,
            &[
                Hole {
                    offset: 0,
                    kind: HoleKind::Imm32,
                },
                Hole {
                    offset: 4,
                    kind: HoleKind::Disp32,
                },
                Hole {
                    offset: 8,
                    kind: HoleKind::Rel32,
                },
                Hole {
                    offset: 12,
                    kind: HoleKind::Ptr64,
                },
            ],
            &values,
        )
        .unwrap();
        assert_eq!(&bytes[0..4], &[0, 0, 0, 0]);
        assert_eq!(&bytes[12..20], &(Opcode::GetProperty as u64).to_le_bytes());
    }

    #[test]
    fn literal64_uses_constant_domain_without_accepting_pointer_bits() {
        let site = QuickeningSite::<2>::new(Opcode::AddConst);
        let values = PatchValues::from_site(&site)
            .with_constant_bits(0x3ff0_0000_0000_0000)
            .with_pointer_bits(0xfeed_beef);
        let mut bytes = [0u8; 8];
        write_literal64(&mut bytes, 0, &values).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), 0x3ff0_0000_0000_0000);
    }

    #[test]
    fn relative_holes_accept_signed_displacements_but_reject_overflow() {
        let site = QuickeningSite::<2>::new(Opcode::GetProperty);
        let values = PatchValues::from_site(&site)
            .with_relative_target(0, i32::MAX as usize + 1)
            .expect("negative rel32 displacement");
        let mut bytes = [0u8; 4];
        assert!(write_rel32(&mut bytes, 0, &values).is_ok());
        assert!(PatchValues::from_site(&site)
            .with_relative_target(usize::MAX, 0)
            .is_none());
    }

    #[test]
    fn aarch64_branch26_preserves_opcode_and_patches_word_displacement() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site)
            .with_relative_target(0, 8)
            .expect("aligned in-range branch");
        let mut bytes = 0x1400_0000u32.to_le_bytes();
        write_branch26(&mut bytes, 0, &values).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), 0x17ff_fffe);

        let mut unaligned = 0x1400_0000u32.to_le_bytes();
        let values = PatchValues::from_site(&site)
            .with_relative_target(1, 8)
            .expect("signed displacement is representable");
        assert_eq!(
            write_branch26(&mut unaligned, 0, &values),
            Err(PatchError::UnsupportedOffset)
        );
    }

    #[test]
    fn aarch64_branch26_rejects_non_branch_instruction() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site)
            .with_relative_target(16, 0)
            .expect("aligned branch target");
        let mut bytes = 0x5400_0000u32.to_le_bytes();
        assert_eq!(
            write_branch26(&mut bytes, 0, &values),
            Err(PatchError::UnsupportedOffset)
        );
    }

    #[test]
    fn aarch64_cond_branch19_checks_opcode_alignment_and_signed_limits() {
        let mut forward = [0xa5; 12];
        forward[4..8].copy_from_slice(&0x5400_0001u32.to_le_bytes());
        write_cond_branch19(&mut forward, 4, 4).unwrap();
        assert_eq!(u32::from_le_bytes(forward[4..8].try_into().unwrap()), 0x5400_0021);
        assert_eq!(&forward[..4], &[0xa5; 4]);
        assert_eq!(&forward[8..], &[0xa5; 4]);

        let mut boundary = 0x5400_0000u32.to_le_bytes();
        assert!(write_cond_branch19(&mut boundary, 0, -(1_i64 << 20)).is_ok());
        assert!(write_cond_branch19(&mut boundary, 0, (1_i64 << 20) - 4).is_ok());
        assert_eq!(
            write_cond_branch19(&mut boundary, 0, 1_i64 << 20),
            Err(PatchError::UnsupportedOffset)
        );
        assert_eq!(
            write_cond_branch19(&mut boundary, 0, -(1_i64 << 20) - 4),
            Err(PatchError::UnsupportedOffset)
        );
        assert_eq!(
            write_cond_branch19(&mut boundary, 0, 2),
            Err(PatchError::UnsupportedOffset)
        );
        let mut wrong = 0x1400_0000u32.to_le_bytes();
        assert_eq!(
            write_cond_branch19(&mut wrong, 0, 4),
            Err(PatchError::UnsupportedOffset)
        );
    }

    #[test]
    fn aarch64_branch26_patches_nonzero_offset_without_touching_neighbors() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site)
            .with_relative_target(12, 4)
            .expect("aligned branch target");
        let mut bytes = [0xA5u8; 12];
        bytes[4..8].copy_from_slice(&0x1400_0000u32.to_le_bytes());
        let original_prefix = bytes[..4].to_owned();
        let original_suffix = bytes[8..].to_owned();
        write_branch26(&mut bytes, 4, &values).expect("branch at offset four");
        assert_eq!(&bytes[..4], original_prefix.as_slice());
        assert_eq!(&bytes[8..], original_suffix.as_slice());
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            0x1400_0002
        );
    }

    #[test]
    fn aarch64_branch26_rejects_out_of_range_target() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site)
            .with_relative_target(1 << 27, 0)
            .expect("signed displacement is representable");
        let mut bytes = 0x1400_0000u32.to_le_bytes();
        assert_eq!(
            write_branch26(&mut bytes, 0, &values),
            Err(PatchError::UnsupportedOffset)
        );
    }

    #[test]
    fn aarch64_branch26_accepts_exact_signed_byte_limits() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let cases = [(0usize, 1usize << 27), ((1usize << 27) - 4, 0)];
        for (target, next) in cases {
            let values = PatchValues::from_site(&site)
                .with_relative_target(target, next)
                .expect("branch displacement fits signed rel32");
            let mut bytes = [0xA5u8; 9];
            bytes[4..8].copy_from_slice(&0x1400_0000u32.to_le_bytes());
            write_branch26(&mut bytes, 4, &values).expect("exact branch limit is encodable");
            assert_eq!(bytes[0], 0xA5);
            assert_eq!(bytes[8], 0xA5);
        }
    }

    #[test]
    fn aarch64_branch26_rejects_each_limit_plus_four() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let cases = [(1usize << 27, 0usize), (0usize, (1usize << 27) + 4)];
        for (target, next) in cases {
            let values = PatchValues::from_site(&site)
                .with_relative_target(target, next)
                .expect("rel32 still represents the out-of-range branch");
            let mut bytes = 0x1400_0000u32.to_le_bytes();
            assert_eq!(
                write_branch26(&mut bytes, 0, &values),
                Err(PatchError::UnsupportedOffset)
            );
        }
    }

    #[test]
    fn branch26_rejects_short_buffers_without_partial_write() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site)
            .with_relative_target(16, 0)
            .expect("branch displacement");
        let mut bytes = [0xCCu8; 3];
        assert_eq!(
            write_branch26(&mut bytes, 0, &values),
            Err(PatchError::OutOfBounds)
        );
        assert_eq!(bytes, [0xCC; 3]);
        assert!(PatchValues::from_site(&site)
            .with_relative_target(usize::MAX, 0)
            .is_none());
    }

    #[test]
    fn multi_hole_failure_is_transactional_before_publication() {
        let site = QuickeningSite::<2>::new(Opcode::Add);
        let values = PatchValues::from_site(&site)
            .with_relative_target(8, 0)
            .expect("branch displacement");
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&0x1400_0000u32.to_le_bytes());
        bytes[4..].copy_from_slice(&0x5400_0000u32.to_le_bytes());
        let original = bytes;
        let result = apply_holes(
            &mut bytes,
            &[
                Hole {
                    offset: 0,
                    kind: HoleKind::Branch26,
                },
                Hole {
                    offset: 4,
                    kind: HoleKind::Branch26,
                },
            ],
            &values,
        );
        assert_eq!(result, Err(PatchError::UnsupportedOffset));
        assert_eq!(bytes, original);
    }
}
