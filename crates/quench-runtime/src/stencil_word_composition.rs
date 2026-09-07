//! Word-valued control fragments sharing one internal `x0` continuation ABI.

use crate::stencil_fact::{PatchValues, RegionId, RegionKey};
use crate::stencil_layout::LayoutError;
use crate::stencil_region_layout::{
    compose_planned_region, PlannedFragment, RegionImageIdentity, RegionPoint, VerifiedRegionImage,
};
use crate::stencil_select::PhysicalStencilView;

const BRANCH_ID: RegionId = RegionId(0x5143_0002);
const CONSTANT_BRANCH_ID: RegionId = RegionId(0x5143_0003);
const BRANCH_OPS: [crate::ir::Opcode; 3] = [
    crate::ir::Opcode::JumpIfFalse,
    crate::ir::Opcode::Return,
    crate::ir::Opcode::Return,
];
const CONSTANT_BRANCH_OPS: [crate::ir::Opcode; 5] = [
    crate::ir::Opcode::JumpIfFalse,
    crate::ir::Opcode::LoadConst,
    crate::ir::Opcode::Return,
    crate::ir::Opcode::LoadConst,
    crate::ir::Opcode::Return,
];

pub(crate) fn compose_word_branch<const N: usize>(
    branch: PhysicalStencilView,
    terminal: PhysicalStencilView,
    control: &crate::stencil_cfg::RegionControlPlan,
    values: &PatchValues<'_, N>,
) -> Result<VerifiedRegionImage, LayoutError> {
    validate_word_branch(branch, terminal)?;
    let fragments = branch_fragments(branch, terminal, *values);
    let transfers = branch_transfers(branch, 1, 2)?;
    let mut bytes = Vec::new();
    compose_planned_region(control, &BRANCH_OPS, &fragments, &transfers, &mut bytes)?;
    let views = [branch, terminal, terminal];
    Ok(image(BRANCH_ID, &BRANCH_OPS, &views, &[*values; 3], bytes))
}

pub(crate) fn compose_word_constant_branch<const N: usize>(
    views: [PhysicalStencilView; 3],
    control: &crate::stencil_cfg::RegionControlPlan,
    values: &PatchValues<'_, N>,
    true_bits: u64,
    false_bits: u64,
) -> Result<VerifiedRegionImage, LayoutError> {
    validate_constant_branch(views)?;
    let truthy = (*values).with_constant_bits(true_bits);
    let falsy = (*values).with_constant_bits(false_bits);
    let patches = [*values, truthy, *values, falsy, *values];
    let fragments = constant_fragments(views, patches);
    let transfers = constant_transfers(views)?;
    let mut bytes = Vec::new();
    compose_planned_region(
        control,
        &CONSTANT_BRANCH_OPS,
        &fragments,
        &transfers,
        &mut bytes,
    )?;
    let sequence = [views[0], views[1], views[2], views[1], views[2]];
    Ok(image(
        CONSTANT_BRANCH_ID,
        &CONSTANT_BRANCH_OPS,
        &sequence,
        &patches,
        bytes,
    ))
}

fn validate_word_branch(
    branch: PhysicalStencilView,
    terminal: PhysicalStencilView,
) -> Result<(), LayoutError> {
    let valid = compatible_word_views(&[branch, terminal])
        && branch.record.operations == [crate::ir::Opcode::JumpIfFalse]
        && terminal.record.operations == [crate::ir::Opcode::Return]
        && branch.links.len() == 2
        && terminal.links.is_empty();
    valid.then_some(()).ok_or(LayoutError::RelocationContract)
}

fn validate_constant_branch(views: [PhysicalStencilView; 3]) -> Result<(), LayoutError> {
    let valid = compatible_word_views(&views)
        && views[0].record.operations == [crate::ir::Opcode::JumpIfFalse]
        && views[1].record.operations == [crate::ir::Opcode::LoadConst]
        && views[2].record.operations == [crate::ir::Opcode::Return]
        && [
            views[0].links.len(),
            views[1].links.len(),
            views[2].links.len(),
        ] == [2, 1, 0];
    valid.then_some(()).ok_or(LayoutError::RelocationContract)
}

fn compatible_word_views(views: &[PhysicalStencilView]) -> bool {
    views.iter().all(|view| {
        view.abi == crate::stencil_select::RegionAbi::ScalarWordBool
            && view.continuation_abi == crate::stencil_select::ContinuationAbi::WordX0
            && view.stencil.validate()
    })
}

fn branch_fragments<'values, const N: usize>(
    branch: PhysicalStencilView,
    terminal: PhysicalStencilView,
    values: PatchValues<'values, N>,
) -> [PlannedFragment<'static, 'values, N>; 3] {
    [
        fragment(0, branch, values),
        fragment(1, terminal, values),
        fragment(2, terminal, values),
    ]
}

fn constant_fragments<'values, const N: usize>(
    views: [PhysicalStencilView; 3],
    values: [PatchValues<'values, N>; 5],
) -> [PlannedFragment<'static, 'values, N>; 5] {
    [
        fragment(0, views[0], values[0]),
        fragment(1, views[1], values[1]),
        fragment(2, views[2], values[2]),
        fragment(3, views[1], values[3]),
        fragment(4, views[2], values[4]),
    ]
}

fn fragment<'values, const N: usize>(
    operation: u8,
    view: PhysicalStencilView,
    values: PatchValues<'values, N>,
) -> PlannedFragment<'static, 'values, N> {
    PlannedFragment {
        point: RegionPoint::Operation(operation),
        stencil: view.stencil,
        values,
    }
}

fn constant_transfers(
    views: [PhysicalStencilView; 3],
) -> Result<Vec<crate::stencil_region_layout::PlannedTransfer>, LayoutError> {
    let mut transfers = branch_transfers(views[0], 1, 3)?;
    transfers.extend(next_transfer(views[1], 1, 2)?);
    transfers.extend(next_transfer(views[1], 3, 4)?);
    Ok(transfers)
}

fn branch_transfers(
    view: PhysicalStencilView,
    truthy: u8,
    falsy: u8,
) -> Result<Vec<crate::stencil_region_layout::PlannedTransfer>, LayoutError> {
    let successors = [
        successor(crate::stencil_select::SuccessorRole::False, falsy),
        successor(crate::stencil_select::SuccessorRole::True, truthy),
    ];
    crate::stencil_region_links::selected_transfers_by_role(
        view,
        RegionPoint::Operation(0),
        &successors,
    )
}

fn next_transfer(
    view: PhysicalStencilView,
    source: u8,
    target: u8,
) -> Result<Vec<crate::stencil_region_layout::PlannedTransfer>, LayoutError> {
    crate::stencil_region_links::selected_transfers_by_role(
        view,
        RegionPoint::Operation(source),
        &[successor(
            crate::stencil_select::SuccessorRole::Next,
            target,
        )],
    )
}

fn successor(
    role: crate::stencil_select::SuccessorRole,
    target: u8,
) -> crate::stencil_region_links::SuccessorPlacement {
    crate::stencil_region_links::SuccessorPlacement {
        role,
        target: RegionPoint::Operation(target),
    }
}

fn image<const N: usize>(
    id: RegionId,
    operations: &[crate::ir::Opcode],
    views: &[PhysicalStencilView],
    values: &[PatchValues<'_, N>],
    bytes: Vec<u8>,
) -> VerifiedRegionImage {
    let signature = views.iter().zip(values).fold(0u64, |hash, (view, value)| {
        hash.rotate_left(7) ^ view.cache_signature(value)
    });
    let identity = RegionImageIdentity {
        key: RegionKey::from_opcodes(id, operations),
        cache_signature: signature,
        abi: views[0].abi,
    };
    VerifiedRegionImage::from_composed(identity, bytes)
}
