//! Bounded symbolic layout for composing verified stencil fragments.
//!
//! This module only lays out bytes and resolves internal branch fixups. It
//! neither selects JavaScript semantics nor publishes executable memory.

use crate::stencil_fact::{HoleKind, PatchValues, Stencil};
use crate::stencil_patch::{
    apply_holes, write_branch26, write_cond_branch19, write_rel32, PatchError,
};

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
    Aarch64CondBranch19,
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
                FixupKind::Aarch64Branch26 | FixupKind::Aarch64CondBranch19 => range.start,
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
    let expected = hole_kind(kind).ok_or(LayoutError::Patch(PatchError::UnsupportedOffset))?;
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
    let expected = hole_kind(kind).ok_or(LayoutError::Patch(PatchError::UnsupportedOffset))?;
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
    let Some(expected) = hole_kind(kind) else {
        return Vec::new();
    };
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

const fn hole_kind(kind: FixupKind) -> Option<HoleKind> {
    match kind {
        FixupKind::X86Rel32 => Some(HoleKind::Rel32),
        FixupKind::Aarch64Branch26 => Some(HoleKind::Branch26),
        FixupKind::Aarch64CondBranch19 => None,
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
    if kind == FixupKind::Aarch64CondBranch19 {
        let displacement = i64::try_from(displacement)
            .map_err(|_| LayoutError::TargetOutOfBounds)?;
        return write_cond_branch19(bytes, offset, displacement).map_err(LayoutError::Patch);
    }
    let (encoded_target, encoded_base) = displacement_pair(displacement)?;
    let site = crate::quickening::QuickeningSite::<1>::new(crate::ir::Opcode::Jump);
    let values = crate::stencil_fact::PatchValues::from_site(&site)
        .with_relative_target(encoded_target, encoded_base)
        .ok_or(LayoutError::TargetOutOfBounds)?;
    let offset = u16::try_from(offset).map_err(|_| LayoutError::FixupOutOfBounds)?;
    match kind {
        FixupKind::X86Rel32 => write_rel32(bytes, offset, &values),
        FixupKind::Aarch64Branch26 => write_branch26(bytes, offset, &values),
        FixupKind::Aarch64CondBranch19 => unreachable!("handled before PatchValues"),
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
include!("stencil_layout_tests.rs");
