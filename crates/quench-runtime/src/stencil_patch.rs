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

pub fn apply_holes<const N: usize>(
    dst: &mut [u8],
    holes: &[Hole],
    values: &PatchValues<'_, N>,
) -> Result<(), PatchError> {
    for hole in holes {
        match hole.kind {
            HoleKind::Imm32 => write_imm32(dst, hole.offset, values)?,
            HoleKind::Disp32 => write_disp32(dst, hole.offset, values)?,
            HoleKind::Rel32 => write_rel32(dst, hole.offset, values)?,
            HoleKind::Ptr64 => write_ptr64(dst, hole.offset, values)?,
        }
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
}
