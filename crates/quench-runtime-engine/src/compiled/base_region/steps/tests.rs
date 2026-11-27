#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::Op;

fn instruction(op: Op) -> WideInstruction {
    WideInstruction::new(op, 1, 2, 3, 0)
}

/// The facts of a program that has recorded nothing: no add, no store in place, no fold.
fn known(program: &ResidualProgram) -> Known<'_> {
    Known { program, stitch: &crate::compiled::tier_policy::STATIC_ROWS.stitch, stores: None, reads: None, adds: None, names: None, invalidation: None, constants: &[], typeof_names: &[], instanceofs: None, presences: None, fields: None, globals: None, classes: &[], shapes: &[] }
}

fn empty_program() -> ResidualProgram {
    quench_runtime::Engine::specialize("0", "known.js").expect("a program")
}

/// Every opcode has a lowering, and the helper steps are exactly the in-place instructions and
/// the call and construction helpers' own.
#[test]
fn every_opcode_is_lowered() {
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    let mut counts = [0usize; 2];
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).expect("an opcode");
        let step = lowering(instruction(op), known(&program), process);
        counts[usize::from(step.runs_helper())] += 1;
        if op.steps_in_lane() && instruction(op).written_registers().count() <= 1 {
            assert!(step.runs_helper(), "{op:?}");
        }
    }
    assert_eq!(counts[0] + counts[1], Op::COUNT);
    assert!(counts[1] > 0 && counts[0] > 0);
}

/// Under `LaneHelpers::Ahead` (an AOT body) an instruction no row takes leaves, as a JIT body's does.
#[test]
fn under_ahead_every_opcode_without_a_row_leaves() {
    let program = empty_program();
    for index in 0..Op::COUNT {
        let op = Op::from_index(index).expect("an opcode");
        assert_eq!(lowering(instruction(op), known(&program), LaneHelpers::Ahead), Step::Leave, "{op:?}");
    }
}

/// The opcodes whose only lowering is the generic helper step, each for the reason beside it:
/// no fast arm a register's word, a shape or a backing the row can read decides, so the helper
/// is their whole operation, not a slow arm.
const RESIDUAL: [Op; 20] = [
    // Reads the frame's `this` binding, a VM-held dynamic binding of the derived constructor's environment.
    Op::SuperCallCheck,
    // The define adds a property: where its site recorded the transition (`Known::adds`) its staged row (`staged::add`) is the fast arm (`adds`) and this helper its slow arm; with none recorded, the helper is the whole operation.
    Op::DefineField,
    // The key is converted (guest code) and the property defined through the shape's slow path.
    Op::DefineComputedField,
    // The private atom is the running environment's home's (`Vm::private_name_atom`), resolved per environment and so no site fact; the add is then a shape transition.
    Op::DefinePrivateField,
    // The brand check resolves the private atom through the environment, then asks the shape (`Vm::has_private_slot`).
    Op::CheckPrivate,
    // The brand test resolves the private atom through the environment, then asks the shape.
    Op::PrivateIn,
    // Brands the object by a shape transition keyed by the home's private atom (`private_slots`); the home's declaration is VM state.
    Op::MarkPrivateName,
    // Names any function the site's recorded edge (`names`) does not: its general naming also records that edge.
    Op::SetFunctionName,
    // Builds the name string from a key and a prefix: allocates.
    Op::SetFunctionNameKey,
    // Edits VM-held function provenance only.
    Op::SetFunctionDebugName,
    // Defines through the sparse-or-dense element store, which allocates and grows the backing.
    Op::DefineArrayElement,
    // A spread iterates (guest code) and elisions set a sparse length; only the one-element append has a fast arm (the capacity word), and this helper is its slow arm, which grows the backing.
    Op::AppendArray,
    // Enumerates the source's keys and runs guest code (getters, proxies).
    Op::CopyDataProperties,
    // Allocates the iterator record and may run guest code.
    Op::GetIterator,
    // Takes the key snapshot: allocates the state, memoized by VM-held for-in state.
    Op::ForInStart,
    // The fast arm (`staged::iteration::for_in_next`) steps a state whose memo proves the snapshot current; this helper is its slow arm, which takes any other step and the exhausted one's jump.
    Op::ForInNext,
    // Calls `return` on the iterator: guest code.
    Op::IteratorClose,
    // Pushes onto the frame's VM-held active-iterator stack.
    Op::IteratorCleanupPush,
    // Pops the frame's VM-held active-iterator stack.
    Op::IteratorCleanupPop,
    // Compares the receiver's shape with a define site's overwrite entry (`Vm::constant_fields_held`): VM-held cache state, read once per construction in place of the run of helper defines it skips.
    Op::ConstantFieldsHeld,
];

/// No function of the lane fixtures holds a generic helper step of the lane's in-place steps
/// beyond the listed residual opcodes (the others are `Op::steps_in_body`'s own), and a helper
/// step that jumps names its target (its row branches).
#[test]
fn lane_fixtures_use_the_generic_helper_only_for_residual_opcodes() {
    let process = crate::compiled::process_lane_helpers();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/lanes");
    for entry in std::fs::read_dir(&directory).expect("the lane fixtures") {
        let path = entry.expect("a directory entry").path();
        if !path.extension().is_some_and(|extension| extension == "js") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a fixture");
        let program = quench_runtime::Engine::specialize(&source, "lane-fixture.js").unwrap_or_else(|errors| panic!("{path:?}: {errors:?}"));
        for index in 0..program.functions.len() {
            let function = program.function(index);
            for packed in &function.code {
                let instruction = function.widened(*packed);
                let row = decode_step(0, instruction, known(&program), (process, false));
                if row.step == Some(Step::Helper) {
                    assert!(RESIDUAL.contains(&instruction.op()) || !instruction.op().steps_in_lane(), "{path:?}: {:?} takes the generic helper", instruction.op());
                    assert_eq!(row.target.is_some(), instruction.op().control_flow_layout().jumps(), "{:?}", instruction.op());
                }
            }
        }
    }
}

/// A `throw` is the raising step, which with the catch ends the run; a helper step falls on.
#[test]
fn a_throw_is_the_raising_step_and_ends_the_run() {
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    assert_eq!(lowering(instruction(Op::Throw), known(&program), process), Step::Throw);
    assert_eq!(decode_step(0, instruction(Op::Throw), known(&program), (process, false)).step, Some(Step::Throw));
    assert!(!Step::Throw.falls() && !Step::Catch.falls() && !Step::Leave.falls() && Step::Helper.falls());
}

/// A conditional jump backward is a helper step where a row polls before it (a body's), and a
/// leave where none does (a loop region's).
#[test]
fn a_backward_conditional_jump_is_a_helper_step_where_a_row_polls_before_it() {
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    let back = |polls| decode_step(5, instruction(Op::JumpBinaryFalse), known(&program), (process, polls));
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
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    for op in [Op::CloneEnv, Op::InitializeTdz, Op::StoreEnvLocal, Op::LoadConst, Op::ValidateClassHeritage, Op::LoadName] {
        assert_eq!(lowering(instruction(op), known(&program), process), Step::Helper, "{op:?}");
    }
    let flagged = WideInstruction::new(Op::Binary, 1 | quench_runtime::bytecode::RETURN_REGISTER, 2, 3, 0);
    assert_eq!(lowering(flagged, known(&program), process), Step::Leave);
}

/// A one-element append is a fast row over the backing's capacity word; a spread, an elision count
/// or no element leave the generic helper step alone.
#[test]
fn a_one_element_append_has_a_fast_row_and_a_spread_the_helper_alone() {
    use quench_runtime::bytecode::encode_optional_register;
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    let append = |element: Option<u16>, spread: u16, elisions: u32| {
        lowering(WideInstruction::new(Op::AppendArray, 1, encode_optional_register(element), spread, elisions), known(&program), process)
    };
    assert_eq!(append(Some(2), 0, 0), Step::Fast(Heap::Append));
    assert_eq!(append(Some(2), 1, 0), Step::Helper);
    assert_eq!(append(Some(2), 0, 1), Step::Helper);
    assert_eq!(append(None, 0, 2), Step::Helper);
    let row = decode_step(0, WideInstruction::new(Op::AppendArray, 1, encode_optional_register(Some(2)), 0, 0), known(&program), (process, false));
    assert!(matches!(row.a, Some(Source::Register(2))) && row.base == Some(1) && row.dst.is_none());
}

/// An empty object is the allocation step, whose destination is the row's only hole; a literal array of
/// elements is the helper alone.
#[test]
fn an_empty_object_is_the_allocation_step_and_a_literal_the_helper_alone() {
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    let lower = |instruction| lowering(instruction, known(&program), process);
    assert_eq!(lower(WideInstruction::new(Op::MakeObject, 1, 0, 0, 0)), Step::Allocate);
    assert_eq!(lower(WideInstruction::new(Op::MakeArray, 1, 0, 0, 2)), Step::Helper);
    let row = decode_step(0, WideInstruction::new(Op::MakeObject, 1, 0, 0, 0), known(&program), (process, false));
    assert!(row.dst == Some(1) && row.a.is_none() && row.b.is_none());
}

/// Where a region's heap rows read snapshots a helper's effects would not reach, a fast arm stays
/// and its slow arm becomes the leave, while a helper step leaves; elsewhere both are kept.
#[test]
fn a_guard_reads_the_payload_and_runs_no_helper() {
    assert!(Step::Guard(Heap::Append).reads_payload() && !Step::Guard(Heap::Append).runs_helper());
    assert!(Step::Helper.runs_helper() && Step::Fast(Heap::Append).runs_helper());
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
    let program = empty_program();
    let row = decode_step(7, instruction(Op::Nop), known(&program), (LaneHelpers::Ahead, false));
    assert_eq!((row.kind, row.target, row.step.is_none()), (Kind::Jump, Some(8), true));
}

/// The planner refuses no instruction of the lane fixtures: each decodes to a step (or the
/// jump a `Nop` is), whatever the program holds.
#[test]
fn nothing_in_the_lane_fixtures_is_refused() {
    let process = crate::compiled::process_lane_helpers();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/lanes");
    let (mut fixtures, mut instructions) = (0, 0);
    for entry in std::fs::read_dir(&directory).expect("the lane fixtures") {
        let path = entry.expect("a directory entry").path();
        if !path.extension().is_some_and(|extension| extension == "js") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a fixture");
        let program = quench_runtime::Engine::specialize(&source, "lane-fixture.js").unwrap_or_else(|errors| panic!("{path:?}: {errors:?}"));
        fixtures += 1;
        for index in 0..program.functions.len() {
            let function = program.function(index);
            for packed in &function.code {
                let instruction = function.widened(*packed);
                let row = decode_step(0, instruction, known(&program), (process, false));
                assert!(row.step.is_some() || row.target.is_some(), "{path:?}: {:?}", instruction.op());
                instructions += 1;
            }
        }
    }
    assert!(fixtures > 0 && instructions > 0);
}

/// An empty object and an object literal of a site's shape lower to the allocation step where the helper table
/// addresses it; a literal that writes a flag does not.
#[test]
fn fixed_shape_objects_lower_to_the_allocation_step() {
    let process = crate::compiled::process_lane_helpers();
    let program = empty_program();
    assert_eq!(lowering(WideInstruction::new(Op::MakeObject, 1, 0, 0, 0), known(&program), process), Step::Allocate);
    assert_eq!(lowering(WideInstruction::new(Op::MakeObjectLiteral, 4, 1, 3, 9), known(&program), process), Step::Allocate);
    let flagged = WideInstruction::new(Op::MakeObject, 1 | quench_runtime::bytecode::RETURN_REGISTER, 0, 0, 0);
    assert_ne!(lowering(flagged, known(&program), process), Step::Allocate);
}
