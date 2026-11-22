use super::*;
use quench_runtime::bytecode::Class;
use quench_runtime::typed_ir::IntRange;

#[test]
fn native_call_retires_conditional_shapes_in_cfg_facts() {
    let program = quench_runtime::Engine::specialize(
        "function caller(f, value) { f(value); return value; }", "shape-call.js").unwrap();
    let function = program.function(1);
    let call = function.instructions().find(|instruction| instruction.op().is_call()).unwrap();
    let null = Type::language(quench_runtime::ValueType::Null);
    for ty in [Type::direct_object(7), Type::direct_object(7).join(null),
        Type::direct_object(7).join(Type::NULLISH), Type::INT32, Type::ANY] {
        let mut types = vec![ty; usize::from(function.registers)];
        assert!(matches!(transfer(&mut types, call, Operation::<u64, ()>::of(call).as_ref(), &program.constants,
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
    let program = quench_runtime::Engine::specialize(
        "function make(C) { return new C(1); }", "construct.js").unwrap();
    let function = program.function(1);
    let construct = function.instructions().find(|instruction| instruction.op().is_construct()).unwrap();
    let mut types = vec![Type::direct_object(7); usize::from(function.registers)];
    assert!(matches!(transfer(&mut types, construct, Operation::<u64, ()>::of(construct).as_ref(), &program.constants,
        &super::super::lane_region::writes, true), Flow::Next));
    assert_eq!(types[usize::from(construct.result_register())], Type::ANY);
    assert!(types.iter().all(|ty| *ty == Type::ANY || *ty == Type::direct_object(7).without_object_shape()));
    assert!(matches!(transfer(&mut types, construct, Operation::<u64, ()>::of(construct).as_ref(), &program.constants,
        &super::super::lane_region::writes, false), Flow::Leaves));
}

fn infer_inductive(body: &str, names: &[&str]) -> (Vec<Type>, Vec<Register>) {
    let program = quench_runtime::Engine::specialize(&format!("function loop(n,flag,seed){{{body}}}"), "inductive.js").unwrap();
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
        &super::super::lane_region::writes, &|_| None, false, &[], Observed::NONE);
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

/// The unit PCs of `source`'s first function whose int32 arithmetic or step the run proves
/// overflow-free, and the PCs of all its arithmetic and steps, entered with register 0 typed
/// `input` and guarded to the `entering` ranges.
fn overflow_proofs_entered(source: &str, input: Type, entering: &[(Register, IntRange)]) -> (Vec<usize>, Vec<usize>) {
    use quench_runtime::vm::operations::NumberArithmetic;
    let program = quench_runtime::Engine::specialize(source, "ranges.js").unwrap();
    let function = program.function(1);
    let instructions: Vec<_> = function.instructions().collect();
    let leaders = super::super::lane_region::leaders(&instructions);
    let mut initial = vec![Type::ANY; usize::from(function.registers)];
    initial[0] = input;
    let text = Text { entry: 0, instructions: &instructions, leaders: &leaders, constants: &program.constants,
        written: &super::super::lane_region::writes, own_shape: &|_| None, native_calls: false, raises: &[] };
    let seeds = Seeds { entering, ..Seeds::observing(Observed::NONE) };
    let mut free = Vec::new();
    analyze_text(&text, &initial, seeds).replay(&text, seeds, |pc, point| {
        if point.overflow_free(&program.constants, instructions[pc]) { free.push(pc); }
    });
    let steps = instructions.iter().enumerate().filter(|(_, instruction)| instruction.op().class().eq(Class::IncDec)
        || (instruction.op().js_denotation() == quench_runtime::bytecode::JsDenotation::NumericBinary
            && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()).is_some()))
        .map(|(pc, _)| pc).collect();
    (free, steps)
}

fn overflow_proofs(source: &str) -> (Vec<usize>, Vec<usize>) { overflow_proofs_entered(source, Type::ANY, &[]) }

#[test]
fn a_counted_loops_induction_variable_and_sum_are_overflow_free() {
    let (free, steps) = overflow_proofs("function run(){let s=0;for(let i=0;i<100;i++){s=i+1;}return s;}");
    assert!(steps.len() >= 2, "the step and the sum");
    assert!(steps.iter().all(|pc| free.contains(pc)), "{free:?} of {steps:?}");
}

#[test]
fn an_unbounded_counter_widens_to_a_fixpoint_and_keeps_its_overflow_exit() {
    let (free, steps) = overflow_proofs("function run(n){let i=0;while(n){i=i+1;}return i;}");
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|pc| !free.contains(pc)), "{free:?} of {steps:?}");
}

#[test]
fn a_counter_below_a_proved_int32_bound_never_overflows_and_one_below_a_number_may() {
    let (free, steps) = overflow_proofs("function run(n){n=n|0;let s=0;for(let i=0;i<n;i++){s=i;}return s;}");
    assert!(!steps.is_empty() && steps.iter().all(|pc| free.contains(pc)), "{free:?} of {steps:?}");
    let (free, steps) = overflow_proofs("function run(n){let s=0;for(let i=0;i<n;i++){s=i;}return s;}");
    assert!(!steps.is_empty() && steps.iter().all(|pc| !free.contains(pc)), "{free:?} of {steps:?}");
}

#[test]
fn an_entry_guarded_range_seeds_its_input_and_an_unguarded_int32_proves_nothing() {
    let source = "function run(k){const r=k-1;return r;}";
    let (free, steps) = overflow_proofs_entered(source, Type::INT32, &[(0, IntRange::NON_NEGATIVE)]);
    assert!(!steps.is_empty() && steps.iter().all(|pc| free.contains(pc)), "{free:?} of {steps:?}");
    let (free, _) = overflow_proofs_entered(source, Type::INT32, &[]);
    assert!(free.is_empty(), "{free:?}");
}

#[test]
fn a_seeded_typed_array_load_types_its_register_with_the_kinds_range() {
    use quench_runtime::heap::TypedArrayKind as Kind;
    let program = quench_runtime::Engine::specialize(
        "function run(d){let c=0;for(let i=0;i<100;i++){c=(c^d[i])&255;}return c;}", "typed.js").unwrap();
    let function = program.function(1);
    let instructions: Vec<_> = function.instructions().collect();
    let leaders = super::super::lane_region::leaders(&instructions);
    let byte = quench_runtime::staged::typed_elements::NativeKind::new(Kind::Uint8).unwrap().element_type();
    let reads: Vec<ObservedRead> = instructions.iter().enumerate().filter(|(_, instruction)| instruction.op().is_get_index())
        .map(|(pc, _)| ObservedRead { pc, ty: byte }).collect();
    assert!(!reads.is_empty());
    let initial = vec![Type::ANY; usize::from(function.registers)];
    let text = Text { entry: 0, instructions: &instructions, leaders: &leaders, constants: &program.constants,
        written: &super::super::lane_region::writes, own_shape: &|_| None, native_calls: false, raises: &[] };
    let seeds = Seeds { observed: Observed(&reads), ..Seeds::observing(Observed::NONE) };
    let mut loaded = Vec::new();
    analyze_text(&text, &initial, seeds).replay(&text, seeds, |pc, point| {
        for read in reads.iter().filter(|read| read.pc + 1 == pc) {
            loaded.push(point.types[usize::from(instructions[read.pc].result_register())]);
        }
    });
    assert!(!loaded.is_empty() && loaded.iter().all(|ty| *ty == byte), "{loaded:?}");
    assert_eq!(byte.machine(), Type::INT32, "an int32 representation, not a Float64 one");
}

#[test]
fn a_midpoint_of_a_bounded_sum_is_below_the_bound_and_at_most_the_length_the_bound_is() {
    use super::lengths::Relations;
    use quench_runtime::vm::operations::NumberRelation;
    let (lo, hi, sum, mid, array) = (0, 1, 2, 3, 4);
    let mut at_length = vec![None; 5];
    at_length[usize::from(hi)] = Some(array);
    let fresh = || Relations::of_rows(at_length.clone(), vec![None; 5], vec![None; 5], at_length.clone(), vec![None; 5]);
    let mut relations = fresh();
    relations.bound(NumberRelation::LessThan, [Operand::register(lo), Operand::register(hi)]);
    assert_eq!(relations.atmost[usize::from(hi)], Some(array));
    let non_negative = |_: Operand| Some(quench_runtime::typed_ir::IntRange::NON_NEGATIVE);
    relations.relate_sum_of(sum, lo, hi, &non_negative);
    assert_eq!(relations.twice[usize::from(sum)], Some(hi), "lo + hi is below twice hi");
    relations.relate_shift_of(mid, sum);
    assert_eq!(relations.under[usize::from(mid)], Some(hi), "half of a sum below twice hi is below hi");
    assert_eq!(relations.atmost[usize::from(mid)], Some(array), "and at most the length hi is at most");
    relations.kill(hi);
    assert_eq!(relations.twice[usize::from(sum)], None, "a write of the bound retires the sum");
    let mut signed = fresh();
    signed.bound(NumberRelation::LessThan, [Operand::register(lo), Operand::register(hi)]);
    signed.relate_sum_of(sum, lo, hi, &|_| Some(quench_runtime::typed_ir::IntRange::FULL));
    assert_eq!(signed.twice[usize::from(sum)], None, "a sum of a negative operand is no half of a bound");
}

#[test]
fn a_register_below_another_is_a_relation_that_dies_with_either_write_and_survives_a_call() {
    use super::lengths::Relations;
    use quench_runtime::vm::operations::NumberRelation;
    let (index, bound, copy) = (0, 1, 2);
    let mut relations = Relations::new(3);
    relations.bound(NumberRelation::LessThan, [Operand::register(index), Operand::register(bound)]);
    assert_eq!(relations.under[usize::from(index)], Some(bound));
    relations.copy(copy, index);
    assert_eq!(relations.under[usize::from(copy)], Some(bound), "a copy of an index is below the same bound");
    relations.clear();
    assert_eq!(relations.under[usize::from(index)], Some(bound), "a bound on a value outlives what changes a length");
    relations.kill(bound);
    assert!(relations.under.iter().all(Option::is_none), "the bound's write retires every relation to it");
    relations.bound(NumberRelation::GreaterThan, [Operand::register(bound), Operand::register(index)]);
    assert_eq!(relations.under[usize::from(index)], Some(bound), "`bound > index` bounds the same register");
    relations.kill(index);
    assert!(relations.under[usize::from(index)].is_none());
}

/// A direct call whose callee's summary types its result (`lane_region::call_results`) types the
/// register the call defines after it, where the lowering checks the result at the completion. Without
/// the observed read the result is `ANY`, as every call's result was before.
#[test]
fn a_direct_call_result_takes_the_type_its_callee_summary_observed() {
    let program = quench_runtime::Engine::specialize(
        "function caller(f, value) { let r = f(value); return r; }", "summary-call.js").unwrap();
    let function = program.function(1);
    let instructions: Vec<_> = function.instructions().collect();
    let call_pc = instructions.iter().position(|instruction| instruction.op().is_call()).unwrap();
    let result = instructions[call_pc].result_register();
    let leaders = super::super::lane_region::leaders(&instructions);
    let initial = vec![Type::ANY; usize::from(function.registers)];
    let text = Text { entry: 0, instructions: &instructions, leaders: &leaders, constants: &program.constants,
        written: &super::super::lane_region::writes, own_shape: &|_| None, native_calls: true, raises: &[] };
    let typed_after = |reads: &[ObservedRead]| {
        let seeds = Seeds { observed: Observed(reads), ..Seeds::observing(Observed::NONE) };
        let mut after = None;
        analyze_text(&text, &initial, seeds).replay(&text, seeds, |pc, point| {
            if pc == call_pc + 1 { after = Some(point.types[usize::from(result)]); }
        });
        after
    };
    assert_eq!(typed_after(&[]), Some(Type::ANY), "an untyped call's result is ANY");
    let reads = [ObservedRead { pc: call_pc, ty: Type::NUMBER }];
    assert_eq!(typed_after(&reads), Some(Type::NUMBER), "the summary's result types the call's result after it");
}
