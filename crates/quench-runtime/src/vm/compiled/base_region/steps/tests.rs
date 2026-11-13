use super::*;
use crate::bytecode::Traits;

fn instruction(op: Op) -> WideInstruction {
    WideInstruction::new(op, 1, 2, 3, 0)
}

/// Every opcode has a lowering, and the helper steps are exactly the in-place instructions and
/// the call and construction helpers' own.
#[test]
fn every_opcode_is_lowered() {
    let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
    let mut counts = [0usize; 2];
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).expect("an opcode");
        let step = lowering(instruction(op), process);
        counts[usize::from(step.runs_helper())] += 1;
        if op.traits().has(Traits::LANE_IN_PLACE) && instruction(op).written_registers().count() <= 1 {
            assert!(step.runs_helper(), "{op:?}");
        }
    }
    assert_eq!(counts[0] + counts[1], Op::COUNT);
    assert!(counts[1] > 0 && counts[0] > 0);
}

/// Without a helper table that has addresses (a linked or symbolic one) an instruction leaves:
/// still lowered, never refused.
#[test]
fn without_helper_addresses_every_opcode_leaves() {
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).expect("an opcode");
        assert_eq!(lowering(instruction(op), LaneHelpers::Ahead), Step::Leave, "{op:?}");
    }
}

/// The opcodes whose only lowering is the generic helper step, each for the reason beside it:
/// no fast arm a register's word, a shape or a backing the row can read decides, so the helper
/// is their whole operation, not a slow arm.
const RESIDUAL: [Op; 18] = [
    // The define adds a property: a shape transition and a storage append the object's barrier and shape readers observe (`Object::append_plain`).
    Op::DefineField,
    // The key is converted (guest code) and the property defined through the shape's slow path.
    Op::DefineComputedField,
    // The brand is the home's VM-held private-name table, not a shape or a register fact.
    Op::DefinePrivateField,
    // The brand check reads the home's VM-held private-name table.
    Op::CheckPrivate,
    // The brand test reads the home's VM-held private-name table.
    Op::PrivateIn,
    // Writes the VM-held private-name table.
    Op::MarkPrivateName,
    // The inferred-name slot write also records the function's name in VM-held provenance (`capture_function_name_for_atom`).
    Op::SetFunctionName,
    // Builds the name string from a key and a prefix: allocates.
    Op::SetFunctionNameKey,
    // Edits VM-held function provenance only.
    Op::SetFunctionDebugName,
    // Defines through the sparse-or-dense element store, which allocates and grows the backing.
    Op::DefineArrayElement,
    // The room past the length is proved only by a VM reserve (`LaneHelper::AppendRoom`): the backing header holds no capacity.
    Op::AppendArray,
    // Enumerates the source's keys and runs guest code (getters, proxies).
    Op::CopyDataProperties,
    // Allocates the iterator record and may run guest code.
    Op::GetIterator,
    // Takes the key snapshot: allocates the state, memoized by VM-held for-in state.
    Op::ForInStart,
    // The step's fast arm reads VM-held memo state (the snapshot's epoch and shape), not a layout fact.
    Op::ForInNext,
    // Calls `return` on the iterator: guest code.
    Op::IteratorClose,
    // Pushes onto the frame's VM-held active-iterator stack.
    Op::IteratorCleanupPush,
    // Pops the frame's VM-held active-iterator stack.
    Op::IteratorCleanupPop,
];

/// No function of the lane fixtures holds a generic helper step of the lane's in-place steps
/// beyond the listed residual opcodes (the others are `Op::steps_in_body`'s own), and a helper
/// step that jumps names its target (its row branches).
#[test]
fn lane_fixtures_use_the_generic_helper_only_for_residual_opcodes() {
    let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/lanes");
    for entry in std::fs::read_dir(&directory).expect("the lane fixtures") {
        let path = entry.expect("a directory entry").path();
        if !path.extension().is_some_and(|extension| extension == "js") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a fixture");
        let program = crate::Engine::specialize(&source, "lane-fixture.js").unwrap_or_else(|errors| panic!("{path:?}: {errors:?}"));
        for index in 0..program.functions.len() {
            let function = program.function(index);
            for packed in &function.code {
                let instruction = crate::vm::dispatch_fast::decoded_instruction(function, *packed);
                let row = decode_step(0, instruction, (process, false));
                if row.step == Some(Step::Helper) {
                    assert!(RESIDUAL.contains(&instruction.op()) || !instruction.op().traits().has(Traits::LANE_IN_PLACE), "{path:?}: {:?} takes the generic helper", instruction.op());
                    assert_eq!(row.target.is_some(), instruction.op().control_flow_layout().jumps(), "{:?}", instruction.op());
                }
            }
        }
    }
}

/// A `throw` is the raising step, which with the catch ends the run; a helper step falls on.
#[test]
fn a_throw_is_the_raising_step_and_ends_the_run() {
    let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
    assert_eq!(lowering(instruction(Op::Throw), process), Step::Throw);
    assert_eq!(decode_step(0, instruction(Op::Throw), (process, false)).step, Some(Step::Throw));
    assert!(!Step::Throw.falls() && !Step::Catch.falls() && !Step::Leave.falls() && Step::Helper.falls());
}

/// A conditional jump backward is a helper step where a row polls before it (a body's), and a
/// leave where none does (a loop region's).
#[test]
fn a_backward_conditional_jump_is_a_helper_step_where_a_row_polls_before_it() {
    let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
    let back = |polls| decode_step(5, instruction(Op::JumpBinaryFalse), (process, polls));
    assert_eq!(back(false).step, Some(Step::Leave));
    let polled = back(true);
    assert_eq!((polled.step, polled.target), (Some(Step::Helper), Some(0)));
}

/// The opcode table, not a list here, says which operation raises: the throw alone.
#[test]
fn only_the_throw_raises() {
    let raising: Vec<Op> = (0..Op::COUNT).filter_map(Op::from_index).filter(|op| op.raises()).collect();
    assert_eq!(raising, [Op::Throw]);
}

/// The scope, operator, constant and class steps `Op::steps_in_body` names are helper steps of
/// the instruction that has no flags, and leave where it has them.
#[test]
fn body_steps_are_helper_steps_without_result_flags() {
    let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
    for op in [Op::CloneEnv, Op::InitializeTdz, Op::StoreEnvLocal, Op::LoadConst, Op::ValidateClassHeritage, Op::LoadName] {
        assert_eq!(lowering(instruction(op), process), Step::Helper, "{op:?}");
    }
    let flagged = WideInstruction::new(Op::Binary, 1 | crate::bytecode::RETURN_REGISTER, 2, 3, 0);
    assert_eq!(lowering(flagged, process), Step::Leave);
}

/// A function holding a Wasm opcode is no b-jit body; the catalog's other opcodes are covered.
#[test]
fn wasm_opcodes_are_excluded_by_the_table() {
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).expect("an opcode");
        assert_eq!(!covers(&[instruction(op)]), Op::NAMES[index].starts_with("Wasm"), "{op:?}");
    }
}

#[test]
fn a_nop_is_the_jump_to_the_next_instruction() {
    let row = decode_step(7, instruction(Op::Nop), (LaneHelpers::Ahead, false));
    assert_eq!((row.kind, row.target, row.step.is_none()), (Kind::Jump, Some(8), true));
}

/// The planner refuses no instruction of the lane fixtures: each decodes to a step (or the
/// jump a `Nop` is), whatever the program holds.
#[test]
fn nothing_in_the_lane_fixtures_is_refused() {
    let process = crate::vm::compiled::process_lane_helpers::<crate::test_host::TestHost>();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/lanes");
    let (mut fixtures, mut instructions) = (0, 0);
    for entry in std::fs::read_dir(&directory).expect("the lane fixtures") {
        let path = entry.expect("a directory entry").path();
        if !path.extension().is_some_and(|extension| extension == "js") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a fixture");
        let program = crate::Engine::specialize(&source, "lane-fixture.js").unwrap_or_else(|errors| panic!("{path:?}: {errors:?}"));
        fixtures += 1;
        for index in 0..program.functions.len() {
            let function = program.function(index);
            for packed in &function.code {
                let instruction = crate::vm::dispatch_fast::decoded_instruction(function, *packed);
                let row = decode_step(0, instruction, (process, false));
                assert!(row.step.is_some() || row.target.is_some(), "{path:?}: {:?}", instruction.op());
                instructions += 1;
            }
        }
    }
    assert!(fixtures > 0 && instructions > 0);
}
