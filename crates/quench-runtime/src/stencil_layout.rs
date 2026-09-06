//! Bounded symbolic layout for composing verified stencil fragments.
//!
//! This module only lays out bytes and resolves internal branch fixups. It
//! neither selects JavaScript semantics nor publishes executable memory.

use crate::stencil_fact::{HoleKind, PatchValues, Stencil};
use crate::stencil_patch::{apply_holes, write_branch26, write_rel32, PatchError};

pub(crate) const MAX_LAYOUT_FRAGMENTS: usize = 8;
pub(crate) const MAX_LAYOUT_FIXUPS: usize = 16;
pub(crate) const MAX_LAYOUT_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LabelId(pub(crate) u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Fragment<'a> {
    pub(crate) label: LabelId,
    pub(crate) bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FixupKind {
    X86Rel32,
    Aarch64Branch26,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Fixup {
    pub(crate) fragment: u8,
    pub(crate) offset: u16,
    pub(crate) target: LabelId,
    pub(crate) addend: i32,
    pub(crate) kind: FixupKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LayoutError {
    FragmentBudget,
    FixupBudget,
    ByteBudget,
    DuplicateLabel(LabelId),
    UndefinedLabel(LabelId),
    InvalidFragment(u8),
    FixupOutOfBounds,
    OverlappingFixups,
    TargetOutOfBounds,
    Patch(PatchError),
}

pub(crate) struct StencilLayout<'a> {
    fragments: &'a [Fragment<'a>],
    fixups: &'a [Fixup],
}

impl<'a> StencilLayout<'a> {
    pub(crate) const fn new(fragments: &'a [Fragment<'a>], fixups: &'a [Fixup]) -> Self {
        Self { fragments, fixups }
    }

    /// Resolve into scratch storage and replace `output` only after all checks
    /// and patches succeed.
    pub(crate) fn finalize_into(&self, output: &mut Vec<u8>) -> Result<(), LayoutError> {
        self.validate_budgets()?;
        let (offsets, byte_len) = self.fragment_offsets()?;
        self.validate_labels()?;
        let ranges = self.fixup_ranges(&offsets)?;
        validate_disjoint(&ranges)?;
        let mut scratch = Vec::with_capacity(byte_len);
        for fragment in self.fragments {
            scratch.extend_from_slice(fragment.bytes);
        }
        self.apply_fixups(&mut scratch, &offsets)?;
        *output = scratch;
        Ok(())
    }

    fn validate_budgets(&self) -> Result<(), LayoutError> {
        if self.fragments.len() > MAX_LAYOUT_FRAGMENTS {
            return Err(LayoutError::FragmentBudget);
        }
        if self.fixups.len() > MAX_LAYOUT_FIXUPS {
            return Err(LayoutError::FixupBudget);
        }
        Ok(())
    }

    fn fragment_offsets(&self) -> Result<(Vec<usize>, usize), LayoutError> {
        let mut offsets = Vec::with_capacity(self.fragments.len());
        let mut byte_len = 0usize;
        for fragment in self.fragments {
            offsets.push(byte_len);
            byte_len = byte_len
                .checked_add(fragment.bytes.len())
                .ok_or(LayoutError::ByteBudget)?;
            if byte_len > MAX_LAYOUT_BYTES {
                return Err(LayoutError::ByteBudget);
            }
        }
        Ok((offsets, byte_len))
    }

    fn validate_labels(&self) -> Result<(), LayoutError> {
        for (index, fragment) in self.fragments.iter().enumerate() {
            if self.fragments[..index]
                .iter()
                .any(|prior| prior.label == fragment.label)
            {
                return Err(LayoutError::DuplicateLabel(fragment.label));
            }
        }
        Ok(())
    }

    fn fixup_ranges(&self, offsets: &[usize]) -> Result<Vec<std::ops::Range<usize>>, LayoutError> {
        self.fixups
            .iter()
            .map(|fixup| self.fixup_range(*fixup, offsets))
            .collect()
    }

    fn fixup_range(
        &self,
        fixup: Fixup,
        offsets: &[usize],
    ) -> Result<std::ops::Range<usize>, LayoutError> {
        let fragment_index = usize::from(fixup.fragment);
        let fragment = self
            .fragments
            .get(fragment_index)
            .ok_or(LayoutError::InvalidFragment(fixup.fragment))?;
        let local = usize::from(fixup.offset);
        let local_end = local.checked_add(4).ok_or(LayoutError::FixupOutOfBounds)?;
        if local_end > fragment.bytes.len() {
            return Err(LayoutError::FixupOutOfBounds);
        }
        let start = offsets[fragment_index]
            .checked_add(local)
            .ok_or(LayoutError::FixupOutOfBounds)?;
        Ok(start..start + 4)
    }

    fn apply_fixups(&self, bytes: &mut [u8], offsets: &[usize]) -> Result<(), LayoutError> {
        for fixup in self.fixups {
            let range = self.fixup_range(*fixup, offsets)?;
            let target = self.target_offset(*fixup, offsets, bytes.len())?;
            let base = match fixup.kind {
                FixupKind::X86Rel32 => range.end,
                FixupKind::Aarch64Branch26 => range.start,
            };
            patch_relative(bytes, range.start, target, base, fixup.kind)?;
        }
        Ok(())
    }

    fn target_offset(
        &self,
        fixup: Fixup,
        offsets: &[usize],
        byte_len: usize,
    ) -> Result<usize, LayoutError> {
        let index = self
            .fragments
            .iter()
            .position(|fragment| fragment.label == fixup.target)
            .ok_or(LayoutError::UndefinedLabel(fixup.target))?;
        let target = offsets[index] as i128 + i128::from(fixup.addend);
        if !(0..byte_len as i128).contains(&target) {
            return Err(LayoutError::TargetOutOfBounds);
        }
        usize::try_from(target).map_err(|_| LayoutError::TargetOutOfBounds)
    }
}

pub(crate) fn compose_fallthrough<const N: usize>(
    head: &Stencil,
    tail: &Stencil,
    values: &PatchValues<'_, N>,
    branch_offset: u16,
    kind: FixupKind,
    output: &mut Vec<u8>,
) -> Result<(), LayoutError> {
    validate_chain(head, tail, branch_offset, kind)?;
    let head_bytes = patch_non_branch_holes(head, values, kind)?;
    let tail_bytes = patch_non_branch_holes(tail, values, kind)?;
    let fragments = [
        Fragment {
            label: LabelId(0),
            bytes: &head_bytes,
        },
        Fragment {
            label: LabelId(1),
            bytes: &tail_bytes,
        },
    ];
    let fixups = chain_fixups(head, kind);
    StencilLayout::new(&fragments, &fixups).finalize_into(output)
}

fn validate_chain(
    head: &Stencil,
    tail: &Stencil,
    branch_offset: u16,
    kind: FixupKind,
) -> Result<(), LayoutError> {
    if !head.validate() || !tail.validate() || has_relative_hole(tail) {
        return Err(LayoutError::Patch(PatchError::UnsupportedOffset));
    }
    let expected = hole_kind(kind);
    if !head
        .holes
        .iter()
        .any(|hole| hole.offset == branch_offset && hole.kind == expected)
    {
        return Err(LayoutError::Patch(PatchError::UnsupportedOffset));
    }
    Ok(())
}

fn patch_non_branch_holes<const N: usize>(
    stencil: &Stencil,
    values: &PatchValues<'_, N>,
    kind: FixupKind,
) -> Result<Vec<u8>, LayoutError> {
    let expected = hole_kind(kind);
    if stencil
        .holes
        .iter()
        .any(|hole| is_relative(hole.kind) && hole.kind != expected)
    {
        return Err(LayoutError::Patch(PatchError::UnsupportedOffset));
    }
    let holes = stencil
        .holes
        .iter()
        .copied()
        .filter(|hole| !is_relative(hole.kind))
        .collect::<Vec<_>>();
    let mut bytes = stencil.bytes.to_vec();
    apply_holes(&mut bytes, &holes, values).map_err(LayoutError::Patch)?;
    Ok(bytes)
}

fn chain_fixups(head: &Stencil, kind: FixupKind) -> Vec<Fixup> {
    let expected = hole_kind(kind);
    head.holes
        .iter()
        .filter(|hole| hole.kind == expected)
        .map(|hole| Fixup {
            fragment: 0,
            offset: hole.offset,
            target: LabelId(1),
            addend: 0,
            kind,
        })
        .collect()
}

const fn hole_kind(kind: FixupKind) -> HoleKind {
    match kind {
        FixupKind::X86Rel32 => HoleKind::Rel32,
        FixupKind::Aarch64Branch26 => HoleKind::Branch26,
    }
}

const fn is_relative(kind: HoleKind) -> bool {
    matches!(kind, HoleKind::Rel32 | HoleKind::Branch26)
}

fn has_relative_hole(stencil: &Stencil) -> bool {
    stencil.holes.iter().any(|hole| is_relative(hole.kind))
}

fn validate_disjoint(ranges: &[std::ops::Range<usize>]) -> Result<(), LayoutError> {
    for (index, range) in ranges.iter().enumerate() {
        if ranges[..index]
            .iter()
            .any(|prior| range.start < prior.end && prior.start < range.end)
        {
            return Err(LayoutError::OverlappingFixups);
        }
    }
    Ok(())
}

fn patch_relative(
    bytes: &mut [u8],
    offset: usize,
    target: usize,
    base: usize,
    kind: FixupKind,
) -> Result<(), LayoutError> {
    let displacement = target as i128 - base as i128;
    let (encoded_target, encoded_base) = displacement_pair(displacement)?;
    let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
    let values = crate::stencil_fact::PatchValues::from_site(&site)
        .with_relative_target(encoded_target, encoded_base)
        .ok_or(LayoutError::TargetOutOfBounds)?;
    let offset = u16::try_from(offset).map_err(|_| LayoutError::FixupOutOfBounds)?;
    match kind {
        FixupKind::X86Rel32 => write_rel32(bytes, offset, &values),
        FixupKind::Aarch64Branch26 => write_branch26(bytes, offset, &values),
    }
    .map_err(LayoutError::Patch)
}

fn displacement_pair(displacement: i128) -> Result<(usize, usize), LayoutError> {
    let displacement = i64::try_from(displacement).map_err(|_| LayoutError::TargetOutOfBounds)?;
    if displacement >= 0 {
        Ok((displacement as usize, 0))
    } else {
        let magnitude = displacement
            .checked_neg()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(LayoutError::TargetOutOfBounds)?;
        Ok((0, magnitude))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: LabelId = LabelId(1);
    const MIDDLE: LabelId = LabelId(2);
    const END: LabelId = LabelId(3);

    fn x86_jump() -> [u8; 5] {
        [0xE9, 0, 0, 0, 0]
    }

    fn x86_fixup(fragment: u8, target: LabelId) -> Fixup {
        Fixup {
            fragment,
            offset: 1,
            target,
            addend: 0,
            kind: FixupKind::X86Rel32,
        }
    }

    #[test]
    fn resolves_distinct_forward_and_backward_labels() {
        let jump = x86_jump();
        let fragments = [
            Fragment {
                label: START,
                bytes: &jump,
            },
            Fragment {
                label: MIDDLE,
                bytes: &jump,
            },
            Fragment {
                label: END,
                bytes: &[0xC3],
            },
        ];
        let fixups = [x86_fixup(0, END), x86_fixup(1, START)];
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &fixups)
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(i32::from_le_bytes(output[1..5].try_into().unwrap()), 5);
        assert_eq!(i32::from_le_bytes(output[6..10].try_into().unwrap()), -10);
    }

    #[test]
    fn fixup_addend_targets_inside_labeled_fragment() {
        let jump = x86_jump();
        let fragments = [
            Fragment {
                label: START,
                bytes: &jump,
            },
            Fragment {
                label: END,
                bytes: &[0x90, 0xC3],
            },
        ];
        let fixup = Fixup {
            addend: 1,
            ..x86_fixup(0, END)
        };
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &[fixup])
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(i32::from_le_bytes(output[1..5].try_into().unwrap()), 1);
    }

    #[test]
    fn rejects_duplicate_and_undefined_labels_transactionally() {
        let duplicate = [
            Fragment {
                label: START,
                bytes: &[0x90],
            },
            Fragment {
                label: START,
                bytes: &[0xC3],
            },
        ];
        let mut output = vec![0xA5];
        assert_eq!(
            StencilLayout::new(&duplicate, &[]).finalize_into(&mut output),
            Err(LayoutError::DuplicateLabel(START))
        );
        assert_eq!(output, [0xA5]);

        let jump = x86_jump();
        let fragments = [Fragment {
            label: START,
            bytes: &jump,
        }];
        assert_eq!(
            StencilLayout::new(&fragments, &[x86_fixup(0, END)]).finalize_into(&mut output),
            Err(LayoutError::UndefinedLabel(END))
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn rejects_overlap_and_out_of_bounds_transactionally() {
        let bytes = [0xE9, 0, 0, 0, 0, 0xC3];
        let fragments = [Fragment {
            label: START,
            bytes: &bytes,
        }];
        let overlap = [
            x86_fixup(0, START),
            Fixup {
                offset: 2,
                ..x86_fixup(0, START)
            },
        ];
        let mut output = vec![0xA5];
        assert_eq!(
            StencilLayout::new(&fragments, &overlap).finalize_into(&mut output),
            Err(LayoutError::OverlappingFixups)
        );
        let outside = [Fixup {
            offset: 3,
            ..x86_fixup(0, START)
        }];
        assert_eq!(
            StencilLayout::new(&fragments, &outside).finalize_into(&mut output),
            Err(LayoutError::FixupOutOfBounds)
        );
        assert_eq!(output, [0xA5]);
    }

    #[test]
    fn enforces_fragment_fixup_and_byte_budgets() {
        let fragment = Fragment {
            label: START,
            bytes: &[],
        };
        let fragments = [fragment; MAX_LAYOUT_FRAGMENTS + 1];
        assert_eq!(
            StencilLayout::new(&fragments, &[]).finalize_into(&mut Vec::new()),
            Err(LayoutError::FragmentBudget)
        );
        let fixup = x86_fixup(0, START);
        let fixups = [fixup; MAX_LAYOUT_FIXUPS + 1];
        assert_eq!(
            StencilLayout::new(&[fragment], &fixups).finalize_into(&mut Vec::new()),
            Err(LayoutError::FixupBudget)
        );
        let oversized = [0u8; MAX_LAYOUT_BYTES + 1];
        let fragments = [Fragment {
            label: START,
            bytes: &oversized,
        }];
        assert_eq!(
            StencilLayout::new(&fragments, &[]).finalize_into(&mut Vec::new()),
            Err(LayoutError::ByteBudget)
        );
    }

    #[test]
    fn aarch64_branch26_uses_instruction_pc_and_preserves_opcode() {
        let branch = 0x1400_0000u32.to_le_bytes();
        let fragments = [
            Fragment {
                label: START,
                bytes: &branch,
            },
            Fragment {
                label: END,
                bytes: &0xD65F_03C0u32.to_le_bytes(),
            },
        ];
        let fixup = Fixup {
            fragment: 0,
            offset: 0,
            target: END,
            addend: 0,
            kind: FixupKind::Aarch64Branch26,
        };
        let mut output = Vec::new();
        StencilLayout::new(&fragments, &[fixup])
            .finalize_into(&mut output)
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(output[..4].try_into().unwrap()),
            0x1400_0001
        );
    }

    #[test]
    fn malformed_aarch64_branch_keeps_output_unchanged() {
        let not_branch = 0x5400_0000u32.to_le_bytes();
        let fragments = [Fragment {
            label: START,
            bytes: &not_branch,
        }];
        let fixup = Fixup {
            fragment: 0,
            offset: 0,
            target: START,
            addend: 0,
            kind: FixupKind::Aarch64Branch26,
        };
        let mut output = vec![0xA5];
        assert_eq!(
            StencilLayout::new(&fragments, &[fixup]).finalize_into(&mut output),
            Err(LayoutError::Patch(PatchError::UnsupportedOffset))
        );
        assert_eq!(output, [0xA5]);
    }
}
