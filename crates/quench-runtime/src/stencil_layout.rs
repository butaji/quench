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
pub(crate) const MAX_LAYOUT_HOLES: usize = 32;
pub(crate) const MAX_LAYOUT_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LabelId(pub(crate) u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Fragment<'a> {
    pub(crate) label: LabelId,
    pub(crate) bytes: &'a [u8],
}

#[derive(Clone, Copy)]
pub(crate) struct StencilFragment<'a, 'values, const N: usize> {
    pub(crate) label: LabelId,
    pub(crate) stencil: &'a Stencil,
    pub(crate) values: PatchValues<'values, N>,
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
    HoleBudget,
    ByteBudget,
    DuplicateLabel(LabelId),
    UndefinedLabel(LabelId),
    InvalidFragment(u8),
    FixupOutOfBounds,
    OverlappingFixups,
    TargetOutOfBounds,
    InvalidStencil(u8),
    MissingFixup(u8, u16),
    DuplicateFixup(u8, u16),
    UnexpectedFixup(u8, u16),
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
            eliminate_fallthrough_branch(bytes, range, target, fixup.kind);
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

fn eliminate_fallthrough_branch(
    bytes: &mut [u8],
    range: std::ops::Range<usize>,
    target: usize,
    kind: FixupKind,
) {
    const AARCH64_NOP: [u8; 4] = 0xD503_201Fu32.to_le_bytes();
    if kind == FixupKind::Aarch64Branch26 && target == range.end {
        bytes[range].copy_from_slice(&AARCH64_NOP);
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
    let fragments = [
        StencilFragment {
            label: LabelId(0),
            stencil: head,
            values: *values,
        },
        StencilFragment {
            label: LabelId(1),
            stencil: tail,
            values: *values,
        },
    ];
    let fixups = chain_fixups(head, kind);
    if !has_fixup_at(&fixups, branch_offset) {
        return Err(LayoutError::MissingFixup(0, branch_offset));
    }
    compose_region(&fragments, &fixups, output)
}

pub(crate) fn compose_region<const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
    fixups: &[Fixup],
    output: &mut Vec<u8>,
) -> Result<(), LayoutError> {
    validate_composition_budget(fragments, fixups)?;
    validate_stencil_fixups(fragments, fixups)?;
    let patched = patch_stencil_fragments(fragments)?;
    let physical = fragments_from_bytes(fragments, &patched);
    StencilLayout::new(&physical, fixups).finalize_into(output)
}

fn validate_composition_budget<const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
    fixups: &[Fixup],
) -> Result<(), LayoutError> {
    if fragments.len() > MAX_LAYOUT_FRAGMENTS {
        return Err(LayoutError::FragmentBudget);
    }
    if fixups.len() > MAX_LAYOUT_FIXUPS {
        return Err(LayoutError::FixupBudget);
    }
    let holes = fragments
        .iter()
        .map(|item| item.stencil.holes.len())
        .sum::<usize>();
    if holes > MAX_LAYOUT_HOLES {
        return Err(LayoutError::HoleBudget);
    }
    validate_composition_bytes(fragments)
}

fn validate_composition_bytes<const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
) -> Result<(), LayoutError> {
    let mut byte_len = 0usize;
    for fragment in fragments {
        byte_len = byte_len
            .checked_add(fragment.stencil.bytes.len())
            .ok_or(LayoutError::ByteBudget)?;
        if byte_len > MAX_LAYOUT_BYTES {
            return Err(LayoutError::ByteBudget);
        }
    }
    Ok(())
}

fn validate_stencil_fixups<const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
    fixups: &[Fixup],
) -> Result<(), LayoutError> {
    for (index, fragment) in fragments.iter().enumerate() {
        let index = u8::try_from(index).map_err(|_| LayoutError::FragmentBudget)?;
        if !fragment.stencil.validate() {
            return Err(LayoutError::InvalidStencil(index));
        }
        validate_declared_holes(index, fragment.stencil, fixups)?;
    }
    for fixup in fixups {
        validate_fixup_declaration(fragments, *fixup)?;
    }
    Ok(())
}

fn validate_declared_holes(
    fragment: u8,
    stencil: &Stencil,
    fixups: &[Fixup],
) -> Result<(), LayoutError> {
    for hole in stencil.holes.iter().filter(|hole| is_relative(hole.kind)) {
        let count = fixups
            .iter()
            .filter(|fixup| fixup_matches_hole(**fixup, fragment, *hole))
            .count();
        match count {
            0 => return Err(LayoutError::MissingFixup(fragment, hole.offset)),
            1 => {}
            _ => return Err(LayoutError::DuplicateFixup(fragment, hole.offset)),
        }
    }
    Ok(())
}

fn validate_fixup_declaration<const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
    fixup: Fixup,
) -> Result<(), LayoutError> {
    let stencil = fragments
        .get(usize::from(fixup.fragment))
        .ok_or(LayoutError::InvalidFragment(fixup.fragment))?
        .stencil;
    if stencil
        .holes
        .iter()
        .any(|hole| fixup_matches_hole(fixup, fixup.fragment, *hole))
    {
        Ok(())
    } else {
        Err(LayoutError::UnexpectedFixup(fixup.fragment, fixup.offset))
    }
}

fn fixup_matches_hole(fixup: Fixup, fragment: u8, hole: crate::stencil_fact::Hole) -> bool {
    fixup.fragment == fragment
        && fixup.offset == hole.offset
        && hole_kind(fixup.kind) == Some(hole.kind)
}

fn patch_stencil_fragments<const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
) -> Result<Vec<Vec<u8>>, LayoutError> {
    fragments
        .iter()
        .map(|fragment| patch_non_relative(fragment.stencil, &fragment.values))
        .collect()
}

fn patch_non_relative<const N: usize>(
    stencil: &Stencil,
    values: &PatchValues<'_, N>,
) -> Result<Vec<u8>, LayoutError> {
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

fn fragments_from_bytes<'a, const N: usize>(
    fragments: &[StencilFragment<'_, '_, N>],
    bytes: &'a [Vec<u8>],
) -> Vec<Fragment<'a>> {
    fragments
        .iter()
        .zip(bytes)
        .map(|(fragment, bytes)| Fragment {
            label: fragment.label,
            bytes,
        })
        .collect()
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

fn has_fixup_at(fixups: &[Fixup], offset: u16) -> bool {
    fixups.iter().any(|fixup| fixup.offset == offset)
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
        let displacement =
            i64::try_from(displacement).map_err(|_| LayoutError::TargetOutOfBounds)?;
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
