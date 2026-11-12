//! The lowering of range-proved int32 arithmetic and element accesses: the proof removes the
//! overflow exit or the bounds guard from the emitted IR, and an unproved twin keeps it.

use super::*;

/// The function `name` of `source`, lowered as a whole unit with the dense element views.
fn lowered(source: &str, name: &str) -> ir::Function {
    let program = crate::Engine::specialize(source, "int-ranges.cjs").unwrap();
    let caller = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let unit = super::super::lane_unit::unit(&program, caller as u32, program.function(caller),
        &|_, _| Vec::new(), &|id| program.function_get(id as usize)).unwrap();
    let isa = super::super::super::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses::<crate::test_host::TestHost>();
    let region = lower(&program, caller as u32, &unit, LaneHelpers::Process(addresses),
        &BTreeMap::new(), &Default::default(), VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS,
        &[], &[], &[], &[], Coverage::Full, Completion::StatusAndPc, ElementViews::Dense).expect("loop region");
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

#[test]
fn an_index_below_its_array_length_needs_no_bounds_guard() {
    let loop_over = |first: &str, second: &str| lowered(&format!(
        "function run(a,b){{let s;for(let i=0;i<a.length;i++){{s={first};s={second};}}return s;}}"), "run");
    let proved = bounds_tests(&loop_over("a[i]", "b[0]"));
    let unproved = bounds_tests(&loop_over("b[i]", "a[0]"));
    assert_eq!(proved + 1, unproved, "only the access `a[i]` under `i < a.length` is proved");
}

#[test]
fn a_store_through_the_array_retires_the_length_proof_of_later_accesses() {
    let source = "function run(a){let s;for(let i=0;i<a.length;i++){a[i]=i;s=a[i];}return s;}";
    let after_store = bounds_tests(&lowered(source, "run"));
    let before_store = bounds_tests(&lowered(
        "function run(a){let s;for(let i=0;i<a.length;i++){s=a[i];a[i]=i;}return s;}", "run"));
    assert!(after_store > before_store, "an element store may resize an array: {after_store} vs {before_store}");
}
