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
        crate::stencil_region_builder::compose_word_branch(branch, terminal, &control, &values)
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
