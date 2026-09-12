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

#[test]
fn generated_boolean_control_selects_move_join_values() {
    let instructions = [
        crate::ir::Instruction::jump_if_false(0, 3),
        crate::ir::Instruction::move_(4, 1),
        crate::ir::Instruction::jump(4),
        crate::ir::Instruction::move_(4, 2),
        crate::ir::Instruction::ret(4),
    ];
    let entries = instructions.map(|instruction| crate::machine::BaselineEntry {
        instruction,
        control: instruction.opcode.control_operands(instruction),
    });
    let owner = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("branch slab"),
    ));
    let mut plan = crate::stencil_word_composition::NativeWordBranchPlan::new_move_join(
        &entries, 0, 1, 3, 4, owner,
    )
    .expect("move-join branch plan");
    assert_eq!(
        plan.continuation(),
        crate::stencil_word_composition::NativeWordBranchContinuation::Store {
            destination: 4,
            next_pc: 4,
        }
    );
    let true_bits = crate::native_core::value_word::TaggedValue::bool(true).bits();
    let false_bits = crate::native_core::value_word::TaggedValue::bool(false).bits();
    assert_eq!(plan.execute(true_bits, 11, 22), Some(11));
    assert_eq!(plan.execute(false_bits, 11, 22), Some(22));
}

#[test]
fn generated_boolean_control_accepts_noncontiguous_move_arms() {
    let instructions = [
        crate::ir::Instruction::jump_if_false(0, 4),
        crate::ir::Instruction::move_(4, 1),
        crate::ir::Instruction::jump(6),
        crate::ir::Instruction::move_(7, 7), // unreachable padding
        crate::ir::Instruction::move_(4, 2),
        crate::ir::Instruction::jump(6),
        crate::ir::Instruction::ret(4),
    ];
    let entries = instructions.map(|instruction| crate::machine::BaselineEntry {
        instruction,
        control: instruction.opcode.control_operands(instruction),
    });
    let owner = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("branch slab"),
    ));
    let mut plan = crate::stencil_word_composition::NativeWordBranchPlan::new_move_join(
        &entries, 0, 1, 4, 6, owner,
    )
    .expect("non-contiguous move-join branch plan");
    let true_bits = crate::native_core::value_word::TaggedValue::bool(true).bits();
    let false_bits = crate::native_core::value_word::TaggedValue::bool(false).bits();
    assert_eq!(plan.execute(true_bits, 11, 22), Some(11));
    assert_eq!(plan.execute(false_bits, 11, 22), Some(22));
}

#[test]
fn generated_boolean_control_jumps_to_canonical_successors() {
    let instructions = [
        crate::ir::Instruction::jump_if_false(0, 4),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::ret(1),
        crate::ir::Instruction::ret(2),
    ];
    let entries = instructions.map(|instruction| crate::machine::BaselineEntry {
        instruction,
        control: instruction.opcode.control_operands(instruction),
    });
    let owner = std::rc::Rc::new(std::cell::RefCell::new(
        crate::stencil_arena::SharedStencilSlab::new(4096).expect("branch slab"),
    ));
    let mut plan =
        crate::stencil_word_composition::NativeWordBranchPlan::new_jump(&entries, 0, 1, 4, owner)
            .expect("branch-only plan");
    assert_eq!(
        plan.continuation(),
        crate::stencil_word_composition::NativeWordBranchContinuation::Jump {
            truthy_pc: 1,
            falsy_pc: 4,
        }
    );
    let true_bits = crate::native_core::value_word::TaggedValue::bool(true).bits();
    let false_bits = crate::native_core::value_word::TaggedValue::bool(false).bits();
    assert_eq!(
        plan.execute(true_bits, true_bits, true_bits),
        Some(true_bits)
    );
    assert_eq!(
        plan.execute(false_bits, false_bits, false_bits),
        Some(false_bits)
    );
}

#[test]
fn baseline_admits_generated_move_join_and_returns_at_join() {
    let executable = crate::machine::ExecutableCode::from_ops(vec![
        crate::ops::Op::Branch {
            condition: 0,
            then_ops: crate::machine::FunctionCode::pending(vec![crate::ops::Op::Move {
                dst: 4,
                src: 1,
            }]),
            else_ops: crate::machine::FunctionCode::pending(vec![crate::ops::Op::Move {
                dst: 4,
                src: 2,
            }]),
        },
        crate::ops::Op::Return { src: 4 },
    ]);
    let code = executable.code();
    let plan = crate::machine::BaselinePlan::compile_for_test(
        code,
        crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test(),
    );
    let native = plan.native_word_branch_at(0).expect("move-join admission");
    let context = crate::vm::current_context_or_default();
    for (condition, expected) in [
        (crate::value::Value::Boolean(true), 11.0),
        (crate::value::Value::Boolean(false), 22.0),
        (crate::value::Value::Number(3.0), 11.0),
        (crate::value::Value::Number(0.0), 22.0),
    ] {
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            condition,
            crate::value::Value::Number(11.0),
            crate::value::Value::Number(22.0),
            crate::value::Value::Undefined,
            crate::value::Value::Undefined,
        ]);
        let (completion, next) = crate::vm::execute_baseline_code_from(
            code,
            &plan,
            0,
            &mut registers,
            &context,
            crate::environment::Environment::new(),
        )
        .expect("execute generated move-join branch");
        assert!(matches!(
            completion,
            crate::completion::Completion::Return(crate::value::Value::Number(value))
                if value == expected
        ));
        assert_eq!(next, 5);
    }
    assert_eq!(native.borrow().native_entry_count(), 2);
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
