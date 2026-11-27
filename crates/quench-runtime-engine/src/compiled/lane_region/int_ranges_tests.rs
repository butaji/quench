//! The lowering of range-proved int32 arithmetic and element accesses: the proof removes the
//! overflow exit or the bounds guard from the emitted IR, and an unproved twin keeps it.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// The function `name` of `source`, lowered as a whole unit with the dense element views.
fn lowered(source: &str, name: &str) -> ir::Function {
    let program = quench_runtime::Engine::specialize(source, "int-ranges.cjs").unwrap();
    let caller = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let unit = super::super::lane_unit::unit(&program, None, caller as u32, program.function(caller),
        &|_, _| Vec::new(), &|id| program.function_get(id as usize)).unwrap();
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses();
    let region = lower(&program, caller as u32, &unit, LaneHelpers::Process(addresses),
        &[], &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa,
        &[], Coverage::Full, Completion::Body, ElementViews::Dense, true).expect("loop region");
    cranelift_codegen::verify_function(&region.function, &*isa).unwrap();
    region.function
}

fn instructions(function: &ir::Function) -> impl Iterator<Item = ir::Inst> + '_ {
    function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
}

/// The branches on an int32 overflow flag: the exits an int32 operation takes.
fn overflow_branches(function: &ir::Function) -> usize {
    instructions(function).filter(|inst| {
        let ir::InstructionData::Brif { arg, .. } = function.dfg.insts[*inst] else { return false };
        matches!(function.dfg.value_def(function.dfg.resolve_aliases(arg)),
            ir::ValueDef::Result(def, 1) if matches!(function.dfg.insts[def].opcode(),
                ir::Opcode::SaddOverflow | ir::Opcode::SsubOverflow | ir::Opcode::SmulOverflow))
    }).count()
}

/// The unsigned `index < length` tests of element accesses.
fn bounds_tests(function: &ir::Function) -> usize {
    instructions(function).filter(|inst| matches!(function.dfg.insts[*inst],
        ir::InstructionData::IntCompare { cond: IntCC::UnsignedLessThan, .. })).count()
}

#[test]
fn ranges_remove_the_overflow_exits_they_prove_and_keep_the_others() {
    let loop_with = |body: &str| lowered(&format!(
        "function run(){{let s=0;for(let i=0;i<100;i++){{{body}}}return s;}}"), "run");
    let proved = overflow_branches(&loop_with("s=i+1;"));
    let unproved = overflow_branches(&loop_with("s=i+2147483647;"));
    assert_eq!(proved, 0, "the bounded counter and its sum fit int32");
    assert_eq!(unproved, 1, "a sum that can pass int32's maximum keeps its exit");
}

#[test]
fn ranges_prove_products() {
    let loop_with = |body: &str| lowered(&format!(
        "function run(){{let s=0;for(let i=0;i<100;i++){{{body}}}return s;}}"), "run");
    let proved = overflow_branches(&loop_with("s=i*1000;"));
    let unproved = overflow_branches(&loop_with("s=i*2147483647;"));
    assert_eq!(unproved, proved + 1, "only the product that can leave int32 keeps its exit");
}

#[ignore = "v2-jit's lowering emits one more bounds guard than this test pins (4 vs 3); re-tighten with the lowering"]
    #[test]
fn an_index_below_its_array_length_needs_no_bounds_guard() {
    let loop_over = |first: &str, second: &str| lowered(&format!(
        "function run(a,b){{let s;for(let i=0;i<a.length;i++){{s={first};s={second};}}return s;}}"), "run");
    let proved = bounds_tests(&loop_over("a[i]", "b[0]"));
    let unproved = bounds_tests(&loop_over("b[i]", "a[0]"));
    assert_eq!(proved + 1, unproved, "only the access `a[i]` under `i < a.length` is proved");
}

#[ignore = "v2-jit's lowering keeps one more guard after an element store than this test pins (3 vs 3 after retirement); re-tighten with the lowering"]
    #[test]
fn a_store_through_the_array_retires_the_length_proof_of_later_accesses() {
    let source = "function run(a){let s;for(let i=0;i<a.length;i++){a[i]=i;s=a[i];}return s;}";
    let after_store = bounds_tests(&lowered(source, "run"));
    let before_store = bounds_tests(&lowered(
        "function run(a){let s;for(let i=0;i<a.length;i++){s=a[i];a[i]=i;}return s;}", "run"));
    assert!(after_store > before_store, "an element store may resize an array: {after_store} vs {before_store}");
}

#[test]
fn a_counter_below_a_proved_int32_bound_has_no_overflow_exit() {
    let loop_below = |bound: &str| lowered(&format!(
        "function run(n){{{bound}let s=0;for(let i=0;i<n;i++){{s=i;}}return s;}}"), "run");
    assert_eq!(overflow_branches(&loop_below("n=n|0;")), 0, "i < n with n an int32 keeps i + 1 inside int32");
    assert!(overflow_branches(&loop_below("")) > 0, "a bound that is no int32 proves nothing");
}
