//! Generated whole-region i32 patterns over canonical residual operations.

use crate::ir::{Instruction, Opcode};
use crate::machine::{BaselineEntry, CodeView};
use std::{cell::RefCell, rc::Rc};

const REGION_LEN: usize = 12;
const OPERATIONS: [Opcode; REGION_LEN] = [
    Opcode::LoadLocal,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::Binary,
    Opcode::LoadLocal,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::Binary,
    Opcode::LoadConst,
    Opcode::Binary,
    Opcode::Return,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct I32PatternSelection {
    value_slot: u16,
    shift_slot: u16,
}

pub(crate) struct NativeI32PatternPlan {
    selection: I32PatternSelection,
    owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    image: crate::stencil_region_layout::VerifiedRegionImage,
    cache: crate::stencil_select::RenderedRegionCache,
    installed: Option<crate::stencil_arena::EntryToken<extern "C" fn(i32, i32) -> i32>>,
}

impl NativeI32PatternPlan {
    pub(crate) fn new(
        selection: I32PatternSelection,
        policy: crate::stencil_policy::ExecutionPolicy,
        owner: Rc<RefCell<crate::stencil_arena::SharedStencilSlab>>,
    ) -> Option<Self> {
        policy.local_fusions.numeric().then_some(())?;
        let view = crate::stencil_select::select_physical_for_abi(
            crate::stencil_select::bitwise_shift_mask_return_region_key(),
            crate::stencil_select::RegionAbi::ScalarI32,
        )?;
        view.generated.then_some(())?;
        Some(Self {
            selection,
            owner,
            image: region_image(view),
            cache: crate::stencil_select::RenderedRegionCache::new(),
            installed: None,
        })
    }

    pub(crate) const fn span(&self) -> usize {
        REGION_LEN
    }

    pub(crate) fn route() -> impl Iterator<Item = &'static str> {
        [
            "LoadLocalChecked",
            "LoadLocalChecked",
            "Binary",
            "Binary",
            "Binary",
            "Binary",
            "Return",
        ]
        .into_iter()
    }

    pub(crate) fn execute(&mut self, environment: &crate::environment::Environment) -> Option<f64> {
        let value = to_int32(environment.get_number(self.selection.value_slot)?);
        let shift = to_int32(environment.get_number(self.selection.shift_slot)?);
        let entry = self.entry()?;
        let lease =
            crate::stencil_arena::SharedStencilSlab::acquire_owned(&self.owner, entry).ok()?;
        lease.invoke(|call| call(value, shift)).ok().map(f64::from)
    }

    fn entry(
        &mut self,
    ) -> Option<crate::stencil_arena::EntryToken<extern "C" fn(i32, i32) -> i32>> {
        if let Some(entry) = self.installed {
            if self.owner.borrow().entry_token_is_live(entry) {
                return Some(entry);
            }
            self.installed = None;
        }
        let address = self
            .owner
            .borrow_mut()
            .publish_region_image_or_get(&mut self.cache, &self.image)
            .ok()?;
        let entry = self.owner.borrow().owned_i32_entry(address).ok()?;
        self.installed = Some(entry);
        Some(entry)
    }
}

pub(crate) fn select_i32_pattern(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &crate::stencil_cfg::ControlFlowFacts,
    start: usize,
) -> Option<I32PatternSelection> {
    let instructions = operation_window(entries, start)?;
    cfg.region_control(start, start.checked_add(REGION_LEN)?)?;
    constants_match(code, &instructions)?;
    bindings_match(&instructions)?;
    Some(I32PatternSelection {
        value_slot: instructions[0].b,
        shift_slot: instructions[1].b,
    })
}

fn operation_window(entries: &[BaselineEntry], start: usize) -> Option<[Instruction; REGION_LEN]> {
    let instructions: [Instruction; REGION_LEN] = entries
        .get(start..start.checked_add(REGION_LEN)?)?
        .iter()
        .map(|entry| entry.instruction)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    instructions
        .iter()
        .zip(OPERATIONS)
        .all(|(instruction, opcode)| instruction.opcode == opcode)
        .then_some(instructions)
}

fn constants_match(code: CodeView<'_>, instructions: &[Instruction; REGION_LEN]) -> Option<()> {
    number_constant(code, instructions[2], 31.0)?;
    number_constant(code, instructions[6], 3.0)?;
    number_constant(code, instructions[9], 0.0)
}

fn number_constant(code: CodeView<'_>, instruction: Instruction, expected: f64) -> Option<()> {
    let crate::ops::Constant::Number(value) = code.constant(instruction.b)? else {
        return None;
    };
    (value.to_bits() == expected.to_bits()).then_some(())
}

fn bindings_match(i: &[Instruction; REGION_LEN]) -> Option<()> {
    binary(i[3], crate::ops::BinaryOp::BitwiseAnd, i[1].a, i[2].a)?;
    binary(i[4], crate::ops::BinaryOp::ShiftLeft, i[0].a, i[3].a)?;
    (i[5].b == i[0].b).then_some(())?;
    binary(
        i[7],
        crate::ops::BinaryOp::ShiftRightZeroFill,
        i[5].a,
        i[6].a,
    )?;
    binary(i[8], crate::ops::BinaryOp::BitwiseXor, i[4].a, i[7].a)?;
    binary(i[10], crate::ops::BinaryOp::BitwiseOr, i[8].a, i[9].a)?;
    (i[11].a == i[10].a).then_some(())
}

fn binary(
    instruction: Instruction,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Option<()> {
    (instruction.flags == crate::ir::compact_binary_id(operator)
        && instruction.b == lhs
        && instruction.c == rhs)
        .then_some(())
}

fn to_int32(value: f64) -> i32 {
    crate::intl::tolocale::value::to_int32(value)
}

fn region_image(
    view: crate::stencil_select::PhysicalStencilView,
) -> crate::stencil_region_layout::VerifiedRegionImage {
    let identity = crate::stencil_region_layout::RegionImageIdentity {
        key: view.key,
        cache_signature: fingerprint(view.stencil.bytes),
        abi: view.abi,
    };
    crate::stencil_region_layout::VerifiedRegionImage::from_composed(
        identity,
        view.stencil.bytes.to_vec(),
    )
}

fn fingerprint(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x1000_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}
