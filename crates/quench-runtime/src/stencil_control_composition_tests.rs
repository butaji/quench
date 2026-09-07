#![cfg(all(target_arch = "aarch64", quench_generated_stencil_artifacts))]

use crate::quickening::QuickeningSite;
use crate::stencil_fact::PatchValues;
use crate::stencil_select::{PhysicalStencilView, RegionAbi};

#[test]
fn generated_boolean_control_executes_both_native_successors() {
    let control = word_branch_control();
    let (branch, terminal) = word_branch_views();
    let site = QuickeningSite::<2>::new(crate::ir::Opcode::JumpIfFalse);
    let values = PatchValues::from_site(&site);
    let image =
        crate::stencil_word_composition::compose_word_branch(branch, terminal, &control, &values)
            .expect("compose boolean control");
    let mut arena = crate::stencil_arena::StencilArena::new(4096).unwrap();
    let mut cache = crate::stencil_select::RenderedRegionCache::new();
    let address = arena
        .publish_region_image_or_get(&mut cache, &image)
        .expect("publish boolean control");
    let entry = arena.word_bool_entry(address).expect("typed branch entry");
    assert_eq!(entry(0), 0);
    assert_eq!(entry(7), 7);
}

#[test]
fn generated_boolean_control_patches_distinct_constant_arms() {
    let control = constant_branch_control();
    let views = constant_branch_views();
    let site = QuickeningSite::<2>::new(crate::ir::Opcode::JumpIfFalse);
    let values = PatchValues::from_site(&site);
    let image = crate::stencil_word_composition::compose_word_constant_branch(
        views,
        &control,
        &values,
        0x1122_3344_5566_7788,
        0x8877_6655_4433_2211,
    )
    .expect("compose constant branch");
    assert_eq!(execute_word_branch(&image, 7), 0x1122_3344_5566_7788);
    assert_eq!(execute_word_branch(&image, 0), 0x8877_6655_4433_2211);
}

fn execute_word_branch(
    image: &crate::stencil_region_layout::VerifiedRegionImage,
    input: u64,
) -> u64 {
    let mut arena = crate::stencil_arena::StencilArena::new(4096).unwrap();
    let mut cache = crate::stencil_select::RenderedRegionCache::new();
    let address = arena
        .publish_region_image_or_get(&mut cache, image)
        .expect("publish constant branch");
    let entry = arena.word_bool_entry(address).expect("typed branch entry");
    entry(input)
}

fn constant_branch_control() -> crate::stencil_cfg::RegionControlPlan {
    let instructions = [
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::load_const(0, 0),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::load_const(0, 1),
        crate::ir::Instruction::ret(0),
    ];
    control_for(instructions)
}

fn control_for<const N: usize>(
    instructions: [crate::ir::Instruction; N],
) -> crate::stencil_cfg::RegionControlPlan {
    let entries = instructions.map(|instruction| crate::machine::BaselineEntry {
        instruction,
        handler: instruction.opcode.handler(),
        control: instruction.opcode.control_operands(instruction),
    });
    let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &vec![None; N]);
    facts.region_control(0, N).expect("branch control")
}

fn constant_branch_views() -> [PhysicalStencilView; 3] {
    let select = |key| {
        crate::stencil_select::select_physical_for_abi(key, RegionAbi::ScalarWordBool)
            .expect("generated word fragment")
    };
    let views = [
        select(crate::stencil_select::bool_branch_region_key()),
        select(crate::stencil_select::word_const_fragment_region_key()),
        select(crate::stencil_select::return_word_region_key()),
    ];
    assert!(views.iter().all(|view| view.generated));
    views
}

fn word_branch_control() -> crate::stencil_cfg::RegionControlPlan {
    let instructions = [
        crate::ir::Instruction::jump_if_false(0, 2),
        crate::ir::Instruction::ret(0),
        crate::ir::Instruction::ret(0),
    ];
    let entries = instructions.map(|instruction| crate::machine::BaselineEntry {
        instruction,
        handler: instruction.opcode.handler(),
        control: instruction.opcode.control_operands(instruction),
    });
    let facts = crate::stencil_cfg::ControlFlowFacts::new(&entries, &[None; 3]);
    facts.region_control(0, 3).expect("branched control")
}

fn word_branch_views() -> (PhysicalStencilView, PhysicalStencilView) {
    let branch = crate::stencil_select::select_physical_for_abi(
        crate::stencil_select::bool_branch_region_key(),
        RegionAbi::ScalarWordBool,
    )
    .expect("generated boolean branch");
    let terminal = crate::stencil_select::select_physical_for_abi(
        crate::stencil_select::return_word_region_key(),
        RegionAbi::ScalarWordBool,
    )
    .expect("generated return fragment");
    assert!(branch.generated && terminal.generated);
    (branch, terminal)
}
