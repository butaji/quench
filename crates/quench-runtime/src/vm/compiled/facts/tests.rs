use super::*;

#[test]
fn native_call_retires_conditional_shapes_in_cfg_facts() {
    let program = crate::Engine::specialize(
        "function caller(f, value) { f(value); return value; }", "shape-call.js").unwrap();
    let function = program.function(1);
    let call = function.instructions().find(|instruction| instruction.op().is_call()).unwrap();
    let null = Type::language(crate::ValueType::Null);
    for ty in [Type::direct_object(7), Type::direct_object(7).join(null),
        Type::direct_object(7).join(Type::NULLISH), Type::INT32, Type::ANY] {
        let mut types = vec![ty; usize::from(function.registers)];
        assert!(matches!(transfer(&mut types, call, &program.constants,
            &super::super::lane_region::writes, true), Flow::Next));
        for (register, actual) in types.into_iter().enumerate() {
            let expected = if register == usize::from(call.result_register()) { Type::ANY }
                else { ty.without_object_shape() };
            assert_eq!(actual, expected, "{ty:?}, register {register}");
        }
    }
}

/// A construction continues where the region calls natively, as the lowering's does
/// (`Lowering::construct`): a loop that constructs carries its backedge's facts to its
/// header, instead of the analysis stopping at the construction.
#[test]
fn native_construction_continues_like_a_call() {
    let program = crate::Engine::specialize(
        "function make(C) { return new C(1); }", "construct.js").unwrap();
    let function = program.function(1);
    let construct = function.instructions().find(|instruction| instruction.op().is_construct()).unwrap();
    let mut types = vec![Type::direct_object(7); usize::from(function.registers)];
    assert!(matches!(transfer(&mut types, construct, &program.constants,
        &super::super::lane_region::writes, true), Flow::Next));
    assert_eq!(types[usize::from(construct.result_register())], Type::ANY);
    assert!(types.iter().all(|ty| *ty == Type::ANY || *ty == Type::direct_object(7).without_object_shape()));
    assert!(matches!(transfer(&mut types, construct, &program.constants,
        &super::super::lane_region::writes, false), Flow::Leaves));
}

fn infer_inductive(body: &str, names: &[&str]) -> (Vec<Type>, Vec<Register>) {
    let program = crate::Engine::specialize(&format!("function loop(n,flag,seed){{{body}}}"), "inductive.js").unwrap();
    let function = program.function(1);
    let instructions: Vec<_> = function.instructions().collect();
    let leaders = super::super::lane_region::leaders(&instructions);
    let edges: Vec<_> = instructions.iter().enumerate().filter_map(|(pc, instruction)|
        instruction.op().control_flow_layout().jumps()
            .then(|| instruction.jump_target() as usize).filter(|target| *target <= pc).map(|target| (target, pc))).collect();
    let header = edges.iter().map(|(header, _)| *header).min().unwrap();
    let back = edges.iter().filter(|(target, _)| *target == header).map(|(_, pc)| *pc).max().unwrap();
    // These fixtures initialize the tested homes from their third input.
    // Promoted locals need not retain debugger names in the bytecode.
    let homes: Vec<_> = instructions[..header].iter().filter(|instruction|
        instruction.op().is_move() && instruction.register_b() == 2)
        .map(|instruction| instruction.result_register()).take(names.len()).collect();
    assert_eq!(homes.len(), names.len(), "candidate homes {names:?}: {}", program.disassemble());
    let mut initial = vec![Type::ANY; usize::from(function.registers)];
    let mut candidates: CompoundBitSet = CompoundBitSet::with_capacity(initial.len());
    for register in &homes { candidates.insert(usize::from(*register)); }
    inductive_int32(&mut initial, candidates, header..=back, &instructions, &leaders, &program.constants,
        &super::super::lane_region::writes, &|_| None, false, Observed::NONE);
    (initial, homes)
}

#[test]
fn inductive_integer_definitions_preserve_unchanged_arms_and_all_latches() {
    for body in [
        "let acc=seed,i=0;while(i<n){i=(i+1)|0;if(flag)acc=(acc+1)|0;}return acc;",
        "let acc=seed,i=0;while(i<n){i=(i+1)|0;if(flag)continue;acc=(acc+1)|0;}return acc;",
        "let acc=seed,i=0;while(i<n){i=(i+1)|0;if(flag)acc=(acc+1)|0;else acc=(acc-1)|0;}return acc;",
    ] {
        let (initial, homes) = infer_inductive(body, &["acc"]);
        assert_eq!(initial[usize::from(homes[0])], Type::INT32, "{body}");
    }
}

#[test]
fn inductive_integer_definitions_reject_wide_and_unproved_writes() {
    for update in ["acc=0.5", "acc=2147483648", "acc='text'", "acc=1n", "acc=true", "acc=seed"] {
        let body = format!("let acc=seed,i=0;while(i<n){{i=(i+1)|0;if(flag)acc=(acc+1)|0;else {update};}}return acc;");
        let (initial, homes) = infer_inductive(&body, &["acc"]);
        assert_eq!(initial[usize::from(homes[0])], Type::ANY, "{update}");
    }
}

#[test]
fn inductive_integer_definitions_remove_failed_dependencies() {
    let body = "let x=seed,y=seed,i=0;while(i<n){i=(i+1)|0;if(flag){x=0;y=0;}else{x=y;y=0.5;}}return x;";
    let (initial, homes) = infer_inductive(body, &["x", "y"]);
    assert!(homes.iter().all(|home| initial[usize::from(*home)] == Type::ANY));
    // A home without its own candidate proof cannot supply an I32 fact.
    let (initial, homes) = infer_inductive("let x=seed,y=seed,i=0;while(i<n){i=(i+1)|0;if(flag)x=0;else x=y;}return x;", &["x"]);
    assert_eq!(initial[usize::from(homes[0])], Type::ANY);
}

#[test]
fn joins_widen_only_the_reached_block_and_converge_after_storage_growth() {
    let mut facts = Facts { offsets: vec![None; 4], types: Vec::new(), registers: 2, ..Facts::default() };
    assert!(facts.merge(2, &[Type::INT32, Type::direct_object(7)]));
    assert!(facts.merge(0, &[Type::NUMBER, Type::ANY]));
    assert!(facts.merge(2, &[Type::NUMBER, Type::direct_object(8)]));
    let widened = [Type::NUMBER, Type::DIRECT_OBJECT_FAMILY];
    assert_eq!(facts.at(2), Some(widened.as_slice()));
    assert_eq!(facts.at(0), Some([Type::NUMBER, Type::ANY].as_slice()));
    assert!(facts.at(1).is_none());
    assert!(!facts.merge(2, &[Type::INT32, Type::direct_object(7)]));
    assert!(!facts.merge(4, &[Type::ANY, Type::ANY]));
}

#[test]
fn zero_register_blocks_keep_reachability_without_allocating_type_rows() {
    let mut facts = Facts { offsets: vec![None; 3], types: Vec::new(), registers: 0, ..Facts::default() };
    assert!(facts.merge(2, &[]));
    assert!(facts.merge(0, &[]));
    assert!(!facts.merge(2, &[]));
    assert_eq!(facts.at(0), Some([].as_slice()));
    assert!(facts.at(1).is_none());
    assert_eq!(facts.at(2), Some([].as_slice()));
    assert!(facts.types.is_empty());
}
