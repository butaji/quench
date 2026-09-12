//! Word-valued control fragments sharing one internal `x0` continuation ABI.

use crate::stencil_fact::{PatchValues, RegionId, RegionKey};
use crate::stencil_layout::LayoutError;
use crate::stencil_region_layout::{
    compose_planned_region, PlannedFragment, RegionImageIdentity, RegionPoint, VerifiedRegionImage,
};
use crate::stencil_select::PhysicalStencilView;
use std::cell::RefCell;
use std::rc::Rc;

const BRANCH_ID: RegionId = RegionId(0x5143_0002);
const CONSTANT_BRANCH_ID: RegionId = RegionId(0x5143_0003);
const RETURN_ID: RegionId = RegionId(0x5143_0004);
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

type WordEntry = extern "C" fn(u64) -> u64;

/// A generated tagged-word return boundary. The copied stencil owns only the
/// ABI-preserving word transfer; `RegisterFile::own_tagged_bits` remains the
/// Rust ownership boundary that materializes the completion value.
pub(crate) struct NativeWordReturnPlan {
    image: VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<WordEntry>,
    #[cfg(test)]
    entries: u64,
}

impl NativeWordReturnPlan {
    pub(crate) fn new(owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>) -> Option<Self> {
        let terminal = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::return_word_region_key(),
            crate::stencil_select::RegionAbi::ScalarWordBool,
        )?;
        (terminal.generated
            && terminal.record.operations == [crate::ir::Opcode::Return]
            && terminal.links.is_empty()
            && compatible_word_views(&[terminal]))
        .then_some(())?;
        let control = crate::stencil_cfg::RegionControlPlan::linear(0, 1)?;
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Return);
        let values = PatchValues::from_site(&site);
        let fragments = [fragment(0, terminal, values)];
        let mut bytes = Vec::new();
        compose_planned_region(
            &control,
            &[crate::ir::Opcode::Return],
            &fragments,
            &[],
            &mut bytes,
        )
        .ok()?;
        Some(Self {
            image: image(
                RETURN_ID,
                &[crate::ir::Opcode::Return],
                &[terminal],
                &[values],
                bytes,
            ),
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            #[cfg(test)]
            entries: 0,
        })
    }

    pub(crate) fn execute(&mut self, bits: u64) -> Option<u64> {
        let entry = self.entry()?;
        let result = self.physical.invoke(entry, |call| call(bits)).ok()?;
        #[cfg(test)]
        {
            self.entries = self.entries.saturating_add(1);
        }
        Some(result)
    }

    fn entry(&mut self) -> Option<crate::stencil_arena::EntryToken<WordEntry>> {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_word_bool_entry(address),
            )
            .ok()
    }

    #[cfg(test)]
    pub(crate) const fn native_entry_count(&self) -> u64 {
        self.entries
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeWordBranchContinuation {
    Return { next_pc: usize },
    Store { destination: u16, next_pc: usize },
    Jump { truthy_pc: usize, falsy_pc: usize },
}

#[derive(Clone, Copy)]
pub(crate) struct WordConstantArm {
    pub(crate) register: crate::ir::Register,
    pub(crate) bits: u64,
    pub(crate) next: usize,
}

pub(crate) struct NativeWordConstantBranchPlan {
    image: VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<WordEntry>,
    condition: u16,
    truthy: WordConstantArm,
    falsy: WordConstantArm,
    #[cfg(test)]
    entries: u64,
}

/// Composed boolean branch whose arms select existing tagged registers. The
/// selected word either completes at a Return arm or is committed once at a
/// verified Move-to-join continuation.
///
/// The physical image validates the boolean transfer and reaches the shared
/// return boundary; register ownership and selected-value materialization stay
/// in the VM boundary. This keeps the branch leaf useful for arbitrary tagged
/// values without giving the copied stencil a second value representation.
pub(crate) struct NativeWordBranchPlan {
    image: VerifiedRegionImage,
    physical: crate::stencil_installation::SharedPhysicalEntry<WordEntry>,
    condition: u16,
    truthy: u16,
    falsy: u16,
    continuation: NativeWordBranchContinuation,
    #[cfg(test)]
    entries: u64,
}

impl NativeWordBranchPlan {
    pub(crate) fn new(
        entries: &[crate::machine::BaselineEntry],
        branch_pc: usize,
        true_pc: usize,
        false_pc: usize,
        control: crate::stencil_cfg::RegionControlPlan,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        validate_source_branch(entries, branch_pc, true_pc, false_pc)?;
        validate_branch_control(&control, branch_pc, &BRANCH_OPS)?;
        let truthy = return_arm(entries, true_pc)?;
        let falsy = return_arm(entries, false_pc)?;
        let (branch, terminal) = generated_word_branch_views()?;
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let image = compose_word_branch(branch, terminal, &control, &values).ok()?;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            condition: entries.get(branch_pc)?.instruction.a,
            truthy,
            falsy,
            continuation: NativeWordBranchContinuation::Return {
                next_pc: branch_pc.checked_add(BRANCH_OPS.len())?,
            },
            #[cfg(test)]
            entries: 0,
        })
    }

    /// Build the physical branch/return image for a source CFG whose two arms
    /// move values into one joined destination before a canonical Return. The
    /// source moves are validated here; the copied image selects the tagged
    /// word and the VM performs the single ownership-aware destination store.
    pub(crate) fn new_move_join(
        entries: &[crate::machine::BaselineEntry],
        branch_pc: usize,
        true_pc: usize,
        false_pc: usize,
        join_pc: usize,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let (truthy, falsy, destination) =
            move_join_operands(entries, branch_pc, true_pc, false_pc, join_pc)?;
        let (branch, terminal) = generated_word_branch_views()?;
        let control = crate::stencil_cfg::RegionControlPlan::from_relative_edges(
            BRANCH_OPS.len(),
            &[(0, 1), (0, 2)],
        )?;
        validate_branch_control(&control, 0, &BRANCH_OPS)?;
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let image = compose_word_branch(branch, terminal, &control, &values).ok()?;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            condition: entries.get(branch_pc)?.instruction.a,
            truthy,
            falsy,
            continuation: NativeWordBranchContinuation::Store {
                destination,
                next_pc: join_pc,
            },
            #[cfg(test)]
            entries: 0,
        })
    }

    /// Build a branch-only image for a proven boolean condition.  The physical
    /// branch still uses the shared word/return ABI; the VM consumes the
    /// returned boolean to select the canonical successor PC.  No arm values
    /// are materialized or owned by this cover.
    pub(crate) fn new_jump(
        entries: &[crate::machine::BaselineEntry],
        branch_pc: usize,
        true_pc: usize,
        false_pc: usize,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let branch = entries.get(branch_pc)?.instruction;
        Self::new_jump_from_instruction(branch, branch_pc, true_pc, false_pc, owner)
    }

    /// Build the branch image from an already decoded canonical instruction.
    /// Generic Bridge execution uses this form so a resident branch does not
    /// allocate a full baseline-entry snapshot to validate one source word.
    pub(crate) fn new_jump_from_instruction(
        branch: crate::ir::Instruction,
        branch_pc: usize,
        true_pc: usize,
        false_pc: usize,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        validate_source_branch_instruction(branch, branch_pc, true_pc, false_pc)?;
        (true_pc != false_pc).then_some(())?;
        let control = crate::stencil_cfg::RegionControlPlan::from_relative_edges(
            BRANCH_OPS.len(),
            &[(0, 1), (0, 2)],
        )?;
        validate_branch_control(&control, 0, &BRANCH_OPS)?;
        let (branch_view, terminal) = generated_word_branch_views()?;
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site);
        let image = compose_word_branch(branch_view, terminal, &control, &values).ok()?;
        let condition = branch.a;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            condition,
            truthy: condition,
            falsy: condition,
            continuation: NativeWordBranchContinuation::Jump {
                truthy_pc: true_pc,
                falsy_pc: false_pc,
            },
            #[cfg(test)]
            entries: 0,
        })
    }

    /// Build an unconditional transfer image from the same audited word-branch
    /// ABI. The condition is supplied as a constant Boolean at execution, so
    /// the copied bytes still own only the physical transfer; the canonical
    /// `Jump` target and CFG edge remain the source of truth.
    pub(crate) fn new_unconditional(
        entries: &[crate::machine::BaselineEntry],
        jump_pc: usize,
        target_pc: usize,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        let jump = entries.get(jump_pc)?;
        Self::new_unconditional_from_instruction(jump.instruction, target_pc, owner)
    }

    /// Build an unconditional transfer image from an already decoded Jump.
    /// This keeps dynamic Bridge setup free of a full baseline-entry snapshot.
    pub(crate) fn new_unconditional_from_instruction(
        jump: crate::ir::Instruction,
        target_pc: usize,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        (jump.opcode == crate::ir::Opcode::Jump && usize::from(jump.a) == target_pc)
            .then_some(())?;
        let control = crate::stencil_cfg::RegionControlPlan::from_relative_edges(
            BRANCH_OPS.len(),
            &[(0, 1), (0, 2)],
        )?;
        validate_branch_control(&control, 0, &BRANCH_OPS)?;
        let (branch, terminal) = generated_word_branch_views()?;
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::Jump);
        let values = PatchValues::from_site(&site);
        let image = compose_word_branch(branch, terminal, &control, &values).ok()?;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            condition: 0,
            truthy: 0,
            falsy: 0,
            continuation: NativeWordBranchContinuation::Jump {
                truthy_pc: target_pc,
                falsy_pc: target_pc,
            },
            #[cfg(test)]
            entries: 0,
        })
    }

    pub(crate) fn execute(
        &mut self,
        condition_bits: u64,
        truthy_bits: u64,
        falsy_bits: u64,
    ) -> Option<u64> {
        let selected =
            match crate::native_core::value_word::TaggedValue::from_bits(condition_bits).decode() {
                crate::native_core::value_word::DecodedValue::Bool(true) => truthy_bits,
                crate::native_core::value_word::DecodedValue::Bool(false) => falsy_bits,
                _ => return None,
            };
        self.invoke(condition_bits)?;
        #[cfg(test)]
        {
            self.entries = self.entries.saturating_add(1);
        }
        Some(selected)
    }

    pub(crate) const fn condition(&self) -> u16 {
        self.condition
    }

    pub(crate) const fn truthy(&self) -> u16 {
        self.truthy
    }

    pub(crate) const fn falsy(&self) -> u16 {
        self.falsy
    }

    pub(crate) const fn continuation(&self) -> NativeWordBranchContinuation {
        self.continuation
    }

    fn invoke(&mut self, bits: u64) -> Option<u64> {
        let entry = self.entry()?;
        self.physical.invoke(entry, |call| call(bits)).ok()
    }

    fn entry(&mut self) -> Option<crate::stencil_arena::EntryToken<WordEntry>> {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_word_bool_entry(address),
            )
            .ok()
    }

    #[cfg(test)]
    pub(crate) const fn native_entry_count(&self) -> u64 {
        self.entries
    }
}

impl NativeWordConstantBranchPlan {
    pub(crate) fn new(
        code: crate::machine::CodeView<'_>,
        entries: &[crate::machine::BaselineEntry],
        branch_pc: usize,
        true_pc: usize,
        false_pc: usize,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        validate_source_branch(entries, branch_pc, true_pc, false_pc)?;
        let truthy = constant_arm(code, entries, true_pc)?;
        let falsy = constant_arm(code, entries, false_pc)?;
        // The constant-arm image is a virtual physical recipe: its two
        // materializers are copied beside the branch even when the canonical
        // arms are separated by unrelated residual instructions. Source CFG
        // legality is checked by the caller; this relative plan describes only
        // the generated image being invoked here.
        let control = projected_constant_branch_control()?;
        let views = generated_truthy_branch_views()?;
        let site = crate::quickening::QuickeningSite::<2>::new(crate::ir::Opcode::JumpIfFalse);
        let values = PatchValues::from_site(&site)
            .with_constant_bits(crate::native_core::value_word::TaggedValue::bool(true).bits());
        let image =
            compose_word_constant_branch(views, &control, &values, truthy.bits, falsy.bits).ok()?;
        Some(Self {
            image,
            physical: crate::stencil_installation::SharedPhysicalEntry::new(owner),
            condition: entries.get(branch_pc)?.instruction.a,
            truthy,
            falsy,
            #[cfg(test)]
            entries: 0,
        })
    }

    pub(crate) fn execute(&mut self, bits: u64) -> Option<WordConstantArm> {
        let arm = match crate::native_core::value_word::TaggedValue::from_bits(bits).decode() {
            crate::native_core::value_word::DecodedValue::Bool(true) => self.truthy,
            crate::native_core::value_word::DecodedValue::Bool(false) => self.falsy,
            _ => return None,
        };
        (self.invoke(bits)? == arm.bits).then_some(())?;
        #[cfg(test)]
        {
            self.entries = self.entries.saturating_add(1);
        }
        Some(arm)
    }

    pub(crate) const fn condition(&self) -> u16 {
        self.condition
    }

    fn invoke(&mut self, bits: u64) -> Option<u64> {
        let entry = self.entry()?;
        self.physical.invoke(entry, |call| call(bits)).ok()
    }

    fn entry(&mut self) -> Option<crate::stencil_arena::EntryToken<WordEntry>> {
        self.physical
            .entry(
                |owner, cache| {
                    owner
                        .borrow_mut()
                        .publish_region_image_or_get(cache, &self.image)
                },
                |pool, address| pool.owned_word_bool_entry(address),
            )
            .ok()
    }

    #[cfg(test)]
    pub(crate) const fn native_entry_count(&self) -> u64 {
        self.entries
    }

    #[cfg(test)]
    pub(crate) const fn identity(&self) -> RegionImageIdentity {
        self.image.identity()
    }
}

fn validate_source_branch(
    entries: &[crate::machine::BaselineEntry],
    branch_pc: usize,
    true_pc: usize,
    false_pc: usize,
) -> Option<()> {
    validate_source_branch_instruction(
        entries.get(branch_pc)?.instruction,
        branch_pc,
        true_pc,
        false_pc,
    )
}

fn validate_source_branch_instruction(
    branch: crate::ir::Instruction,
    branch_pc: usize,
    true_pc: usize,
    false_pc: usize,
) -> Option<()> {
    (branch.opcode == crate::ir::Opcode::JumpIfFalse
        && true_pc == branch_pc.checked_add(1)?
        && usize::from(branch.b) == false_pc)
        .then_some(())
}

fn validate_branch_control(
    control: &crate::stencil_cfg::RegionControlPlan,
    branch_pc: usize,
    operations: &[crate::ir::Opcode],
) -> Option<()> {
    (control.start() == branch_pc
        && control.span_len() == operations.len()
        && control.matches_operations(operations))
    .then_some(())
}

fn projected_constant_branch_control() -> Option<crate::stencil_cfg::RegionControlPlan> {
    crate::stencil_cfg::RegionControlPlan::from_relative_edges(5, &[(0, 1), (0, 3)])
}

fn return_arm(entries: &[crate::machine::BaselineEntry], pc: usize) -> Option<u16> {
    let instruction = entries.get(pc)?.instruction;
    (instruction.opcode == crate::ir::Opcode::Return
        && instruction.flags == 0
        && instruction
            .opcode
            .operands_are_canonical([instruction.a, instruction.b, instruction.c]))
    .then_some(instruction.a)
}

fn move_join_operands(
    entries: &[crate::machine::BaselineEntry],
    branch_pc: usize,
    true_pc: usize,
    false_pc: usize,
    join_pc: usize,
) -> Option<(u16, u16, u16)> {
    validate_source_branch(entries, branch_pc, true_pc, false_pc)?;
    (true_pc == branch_pc.checked_add(1)?
        && false_pc > branch_pc.checked_add(2)?
        && join_pc > false_pc)
        .then_some(())?;
    let true_move = entries.get(true_pc)?.instruction;
    let true_jump = entries.get(true_pc.checked_add(1)?)?.instruction;
    let false_move = entries.get(false_pc)?.instruction;
    let false_tail = entries.get(false_pc.checked_add(1)?)?.instruction;
    let joined_return = entries.get(join_pc)?.instruction;
    (true_move.opcode == crate::ir::Opcode::Move
        && false_move.opcode == crate::ir::Opcode::Move
        && true_move.flags == 0
        && false_move.flags == 0
        && true_move
            .opcode
            .operands_are_canonical([true_move.a, true_move.b, true_move.c])
        && false_move
            .opcode
            .operands_are_canonical([false_move.a, false_move.b, false_move.c])
        && true_move.a == false_move.a
        && true_jump.opcode == crate::ir::Opcode::Jump
        && true_jump.flags == 0
        && true_jump
            .opcode
            .operands_are_canonical([true_jump.a, true_jump.b, true_jump.c])
        && usize::from(true_jump.a) == join_pc
        && (if false_tail.opcode == crate::ir::Opcode::Jump {
            false_tail.flags == 0
                && false_tail.opcode.operands_are_canonical([
                    false_tail.a,
                    false_tail.b,
                    false_tail.c,
                ])
                && usize::from(false_tail.a) == join_pc
        } else {
            false_tail.opcode == crate::ir::Opcode::Return
                && join_pc == false_pc.checked_add(1).unwrap_or(usize::MAX)
                && false_tail.flags == 0
                && false_tail.opcode.operands_are_canonical([
                    false_tail.a,
                    false_tail.b,
                    false_tail.c,
                ])
        })
        && joined_return.opcode == crate::ir::Opcode::Return
        && joined_return.flags == 0
        && joined_return.opcode.operands_are_canonical([
            joined_return.a,
            joined_return.b,
            joined_return.c,
        ])
        && joined_return.a == true_move.a)
        .then_some((true_move.b, false_move.b, true_move.a))
}

fn generated_word_branch_views() -> Option<(PhysicalStencilView, PhysicalStencilView)> {
    let select = |key| {
        crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::ScalarWordBool,
        )
    };
    let branch = select(crate::stencil_select::bool_branch_region_key())?;
    let terminal = select(crate::stencil_select::return_word_region_key())?;
    (branch.generated && terminal.generated).then_some((branch, terminal))
}

fn constant_arm(
    code: crate::machine::CodeView<'_>,
    entries: &[crate::machine::BaselineEntry],
    pc: usize,
) -> Option<WordConstantArm> {
    let (register, constant) = code.constant_at(pc)?;
    let ret = entries.get(pc.checked_add(1)?)?.instruction;
    (ret.opcode == crate::ir::Opcode::Return && ret.a == register).then_some(())?;
    Some(WordConstantArm {
        register,
        bits: crate::machine::constant_word_bits(constant)?,
        next: pc + 1,
    })
}

fn generated_truthy_branch_views() -> Option<[PhysicalStencilView; 3]> {
    let select = |key| {
        crate::stencil_select::select_physical_for_abi(
            key,
            crate::stencil_select::RegionAbi::ScalarWordBool,
        )
    };
    let views = [
        select(crate::stencil_select::truthy_bool_branch_region_key())?,
        select(crate::stencil_select::word_const_fragment_region_key())?,
        select(crate::stencil_select::return_word_region_key())?,
    ];
    views.iter().all(|view| view.generated).then_some(views)
}

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
