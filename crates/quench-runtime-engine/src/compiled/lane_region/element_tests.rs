//! The element rows over a loop's hoisted array, read in the IR the lane lowers them to.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::Class;
use cranelift_codegen::{dominator_tree::DominatorTree, flowgraph::ControlFlowGraph, loop_analysis::LoopAnalysis};

pub(crate) const SUM: &str = "function sum(a, n) { var s = 0; for (var i = 0; i < n; i++) { s += a[i]; } return s; }";
const FILL: &str = "function fill(a, n) { for (var i = 0; i < n; i++) { a[i] = i; } }";

/// The loop region of the function `source` declares, over dense views; `observed` types its element
/// reads Numbers (`ObservedRead`), `left` records that its element stores left before (`generic`).
fn region(source: &str, observed: bool, left: bool) -> ir::Function {
    region_over(source, observed, left, ElementViews::Dense)
}

/// `region` over the element `views`.
fn region_over(source: &str, observed: bool, left: bool, views: ElementViews) -> ir::Function {
    let program = quench_runtime::Engine::specialize(source, "element-rows.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let header = quench_runtime::bytecode::back_edges_of(unit.instructions.iter().copied()).map(|(_, header)| header).min().unwrap();
    let at = |class: Class| -> Vec<usize> { unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| instruction.op().class().eq(&class)).map(|(pc, _)| pc).collect() };
    let reads: Vec<_> = at(Class::GetIndex).into_iter().filter(|_| observed)
        .map(|pc| super::super::facts::ObservedRead { pc, ty: Type::NUMBER, kind: None }).collect();
    let generic = if left { at(Class::SetIndex) } else { Vec::new() };
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses();
    lower(&program, 1, &unit, LaneHelpers::Process(addresses), &[], &Default::default(), VmFacts::STATIC,
        &generic, &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &reads, (&[], &[], &[]), &[], &[],
        Coverage::PrefixFrom(header), Completion::Body, views, true).expect("a loop region").function
}

/// The instructions of `function`'s loop blocks.
fn loop_body(function: &ir::Function) -> Vec<ir::Inst> {
    let cfg = ControlFlowGraph::with_function(function);
    let dom = DominatorTree::with_function(function, &cfg);
    let mut loops = LoopAnalysis::new();
    loops.compute(function, &cfg, &dom);
    function.layout.blocks().filter(|&block| loops.innermost_loop(block).is_some())
        .flat_map(|block| function.layout.block_insts(block)).collect()
}

/// Whether `value` is the integer constant `bits`.
fn is_constant(function: &ir::Function, value: ir::Value, bits: u64) -> bool {
    let ir::ValueDef::Result(def, _) = function.dfg.value_def(function.dfg.resolve_aliases(value)) else { return false };
    matches!(function.dfg.insts[def], ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } if imm.bits() as u64 == bits)
}

/// The comparisons in `body` against the hole (`Value::DELETED`).
fn hole_tests(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| match function.dfg.insts[inst] {
        ir::InstructionData::IntCompare { args, .. } => args.iter().any(|&arg| is_constant(function, arg, Value::DELETED.to_bits())),
        _ => false,
    }).count()
}

#[test]
fn a_hoisted_number_read_has_no_hole_test_and_any_other_read_keeps_it() {
    let observed = region(SUM, true, false);
    assert_eq!(hole_tests(&observed, &loop_body(&observed)), 0, "a packed view's read has no hole test: {observed}");
    let unobserved = region(SUM, false, false);
    assert!(hole_tests(&unobserved, &loop_body(&unobserved)) > 0, "a read no Number proof follows tests the hole: {unobserved}");
}

/// The calls to the element store's host helper in `body`.
fn index_set_calls(function: &ir::Function, body: &[ir::Inst]) -> usize {
    let helper = LaneHelper::addresses()[LaneHelper::IndexSet as usize] as usize;
    body.iter().filter(|&&inst| function.dfg.insts[inst].opcode() == ir::Opcode::CallIndirect
        && is_constant(function, function.dfg.inst_args(inst)[0], helper as u64)).count()
}

#[test]
fn a_hoisted_store_calls_no_helper_in_its_loop_until_its_site_has_missed() {
    let unobserved = region(FILL, false, false);
    assert_eq!(index_set_calls(&unobserved, &loop_body(&unobserved)), 0, "a never-missed store exits cold: {unobserved}");
    let missed = region(FILL, false, true);
    assert!(index_set_calls(&missed, &loop_body(&missed)) > 0, "a recorded miss plans the helper arm: {missed}");
}

/// Whether `body` loads a record table base as a read-only, movable word.
fn hoistable_loads(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| matches!(function.dfg.insts[inst], ir::InstructionData::Load { .. })
        && function.dfg.insts[inst].memflags_data(&function.dfg).is_some_and(|flags| flags.readonly() && flags.can_move())).count()
}

#[test]
fn a_numeric_loop_over_an_array_loads_the_record_base_read_only_and_movable() {
    let numeric = region("function sum(a) { var s = 0; for (var i = 0; i < 100; i++) { s += a[i]; } return s; }", true, false);
    assert!(hoistable_loads(&numeric, &loop_body(&numeric)) > 0, "the record base is hoisted out of the loop: {numeric}");
    let storing = region(FILL, false, false);
    assert_eq!(hoistable_loads(&storing, &loop_body(&storing)), 0, "a loop that stores moves arrays: {storing}");
}

/// The comparisons in `body` with the constant `bits`, by `icmp` or `icmp_imm`.
fn compares_with(function: &ir::Function, body: &[ir::Inst], bits: u64) -> usize {
    body.iter().filter(|&&inst| match function.dfg.insts[inst] {
        ir::InstructionData::IntCompare { args, .. } => args.iter().any(|&arg| is_constant(function, arg, bits)),
        _ => false,
    }).count()
}

#[test]
fn a_float64_sum_loop_has_no_kind_dispatch_in_its_body_and_checks_the_kind_once_before_it() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    // Kinds whose discriminants no other comparison of a sum loop names.
    let kinds = [Kind::Int32, Kind::Float32, Kind::Float64].map(|kind| kind as u64);
    let dispatch = |function: &ir::Function| -> usize {
        let body = loop_body(function);
        kinds.iter().map(|&kind| compares_with(function, &body, kind)).sum()
    };
    let folded = region_over(SUM, false, false, ElementViews::Typed(Some(Kind::Float64)));
    assert_eq!(dispatch(&folded), 0, "a site kind the proof compared is a constant of the loop: {folded}");
    let body = loop_body(&folded);
    let before: Vec<ir::Inst> = folded.layout.blocks().flat_map(|block| folded.layout.block_insts(block))
        .filter(|inst| !body.contains(inst)).collect();
    assert!(compares_with(&folded, &before, Kind::Float64 as u64) >= 1, "the kind is checked before the loop: {folded}");
    let dynamic = region_over(SUM, false, false, ElementViews::Typed(None));
    assert!(dispatch(&dynamic) >= kinds.len(), "a view of no site kind dispatches on the record's in the loop: {dynamic}");
}

/// The conversions between a Number and an integer in `body`.
fn conversions(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| matches!(function.dfg.insts[inst].opcode(),
        ir::Opcode::FcvtFromSint | ir::Opcode::FcvtToSint | ir::Opcode::FcvtToSintSat
            | ir::Opcode::FcvtFromUint | ir::Opcode::FcvtToUint | ir::Opcode::FcvtToUintSat)).count()
}

#[test]
fn a_uint8_load_is_an_int32_word_that_no_conversion_touches() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let crc = "function crc(d) { var c = 0; for (var i = 0; i < 100; i++) { c = (c ^ d[i]) & 255; } return c; }";
    let bytes = region_over(crc, false, false, ElementViews::Typed(Some(Kind::Uint8)));
    assert_eq!(conversions(&bytes, &loop_body(&bytes)), 0, "a byte is read as the int32 it is: {bytes}");
}

const MATMUL: &str = "function mm(a, b, c, n) { for (var i = 0; i < n; i++) for (var j = 0; j < n; j++) { var s = 0; \
    for (var k = 0; k < n; k++) s += a[i * n + k] * b[k * n + j]; c[i * n + j] = s; } }";

/// The innermost loop of the function `source` declares entered natively at its header as an OSR region
/// is, over typed `views`, every register an int32 of the range `entering` there (`int_entries`).
fn entered_region(source: &str, views: ElementViews, entering: IntRange) -> ir::Function {
    let program = quench_runtime::Engine::specialize(source, "index-math.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let (header, backedge) = unit.instructions.iter().enumerate().find_map(|(pc, instruction)|
        { let mut back = None; quench_runtime::bytecode::for_each_instruction_successor(pc, *instruction, |target| if target <= pc && back.is_none() { back = Some(target) }); back.map(|target| (target, pc)) })
        .expect("the function has a loop");
    let entries: Vec<(Register, IntRange)> = (0..unit.registers).map(|register| (register, entering)).collect();
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses();
    lower(&program, 1, &unit, LaneHelpers::Process(addresses), &[], &Default::default(), VmFacts::STATIC,
        &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], (&entries, &[], &[]), &[], &[],
        Coverage::Loop { header, backedge, poll: false }, Completion::Body, views, true).expect("a loop region").function
}

/// Whether `value` is the sign extension of an int32.
fn is_sign_extension(function: &ir::Function, value: ir::Value) -> bool {
    let ir::ValueDef::Result(def, _) = function.dfg.value_def(function.dfg.resolve_aliases(value)) else { return false };
    function.dfg.insts[def].opcode() == ir::Opcode::Sextend
}

/// Whether `value` is an integer constant (an element's byte scale is one, a program's product has none).
fn is_any_constant(function: &ir::Function, value: ir::Value) -> bool {
    let ir::ValueDef::Result(def, _) = function.dfg.value_def(function.dfg.resolve_aliases(value)) else { return false };
    matches!(function.dfg.insts[def], ir::InstructionData::UnaryImm { .. })
}

/// The instructions of `body` of `opcode` with 64-bit operands of which `accepts` takes the operands' definitions.
fn wide_operations(function: &ir::Function, body: &[ir::Inst], opcode: ir::Opcode,
    accepts: impl Fn(&ir::Function, [ir::Value; 2]) -> bool) -> usize {
    body.iter().filter(|&&inst| function.dfg.insts[inst].opcode() == opcode
        && function.dfg.value_type(function.dfg.first_result(inst)) == types::I64
        && accepts(function, [function.dfg.inst_args(inst)[0], function.dfg.inst_args(inst)[1]])).count()
}

#[test]
fn matmul_indices_are_computed_in_integer_words_with_no_conversion_in_the_loop() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let function = entered_region(MATMUL, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    assert_eq!(conversions(&function, &body), 0, "an index is no Number that converts to an integer and back: {function}");
    let sums = wide_operations(&function, &body, ir::Opcode::Iadd, |function, operands|
        operands.iter().any(|operand| is_sign_extension(function, *operand)));
    assert!(sums >= 2, "each index adds its int32 to a 64-bit product: {function}");
}

#[test]
fn an_index_that_may_be_a_negative_zero_is_still_an_integer_word() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    // `x * y` is `-0` for a zero and a negative factor, which no integer type holds; an element
    // index reads no sign, so the product is the integer word it would be without that case.
    let source = "function f(a, x, y) { var s = 0; for (var i = 0; i < 100; i++) s += a[x * y]; return s; }";
    let function = entered_region(source, ElementViews::Typed(Some(Kind::Float64)), IntRange::FULL);
    let body = loop_body(&function);
    assert_eq!(conversions(&function, &body), 0, "{function}");
    let everything: Vec<ir::Inst> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block)).collect();
    let words = |instructions: &[ir::Inst]| wide_operations(&function, instructions, ir::Opcode::Imul, |function, operands|
        operands.iter().all(|operand| is_sign_extension(function, *operand)));
    assert_eq!(words(&everything), 1, "the product runs in 64-bit words: {function}");
    assert_eq!(words(&body), 0, "and once, on the loop's entering edge, as its operands are invariant: {function}");
}

/// The branches in `body` on an int32 overflow flag: the exits an int32 operation takes.
fn overflow_exits(function: &ir::Function, body: &[ir::Inst]) -> usize {
    body.iter().filter(|&&inst| {
        let ir::InstructionData::Brif { arg, .. } = function.dfg.insts[inst] else { return false };
        matches!(function.dfg.value_def(function.dfg.resolve_aliases(arg)),
            ir::ValueDef::Result(def, 1) if matches!(function.dfg.insts[def].opcode(),
                ir::Opcode::SaddOverflow | ir::Opcode::SsubOverflow | ir::Opcode::SmulOverflow))
    }).count()
}

#[test]
fn matmul_indices_have_no_int32_overflow_exit_in_the_loop() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let function = entered_region(MATMUL, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    assert_eq!(overflow_exits(&function, &body), 0, "the counter stays below its int32 bound and the indices are words: {function}");
}

/// The unsigned `index < length` tests of element accesses among `instructions`.
fn bounds_compares(function: &ir::Function, instructions: &[ir::Inst]) -> usize {
    instructions.iter().filter(|&&inst| matches!(function.dfg.insts[inst],
        ir::InstructionData::IntCompare { cond: IntCC::UnsignedLessThan, .. })).count()
}

/// The signed `bound <= length` compares among `instructions`: the comparison of a loop's bound with
/// its array's length (`loop_versions`).
fn version_compares(function: &ir::Function, instructions: &[ir::Inst]) -> usize {
    instructions.iter().filter(|&&inst| matches!(function.dfg.insts[inst],
        ir::InstructionData::IntCompare { cond: IntCC::SignedLessThanOrEqual, .. })).count()
}

#[test]
fn a_counted_loop_compares_its_bound_with_the_array_length_once_and_not_per_access() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let function = entered_region(SUM, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    let before: Vec<ir::Inst> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
        .filter(|inst| !body.contains(inst)).collect();
    assert_eq!(bounds_compares(&function, &body), 0, "the loop's accesses carry their proof: {function}");
    assert!(version_compares(&function, &before) >= 1, "the entry compares the bound with the length: {function}");
    // A loop that writes its bound keeps the per-access test and has nothing to compare at its entry.
    let shrinking = "function sum(a, n) { var s = 0; for (var i = 0; i < n; i++) { s += a[i]; if (s > 1e9) n--; } return s; }";
    let function = entered_region(shrinking, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    assert!(bounds_compares(&function, &body) >= 1, "a bound the loop writes proves nothing: {function}");
}

/// The region of the innermost loop of `source` over `views`, with the feedback a previous body left:
/// `left_header` records that the loop's header is one a previous body left at (`generic`), and
/// `entry_bounds` is false once its entries were declined (`LoopTier::missed_entry`): either way the
/// loop's entry proves no index.
fn region_with_feedback(source: &str, views: ElementViews, left_header: bool, entry_bounds: bool) -> ir::Function {
    let program = quench_runtime::Engine::specialize(source, "element-rows.js").unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let header = quench_runtime::bytecode::back_edges_of(unit.instructions.iter().copied()).map(|(_, header)| header).min().unwrap();
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let addresses = LaneHelper::addresses();
    let generic = if left_header { vec![header] } else { Vec::new() };
    lower(&program, 1, &unit, LaneHelpers::Process(addresses), &[], &Default::default(), VmFacts::STATIC,
        &generic, &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], (&[], &[], &[]), &[], &[],
        Coverage::PrefixFrom(header), Completion::Body, views, entry_bounds).expect("a loop region").function
}

#[test]
fn a_masked_index_compares_its_mask_with_the_array_length_once_and_not_per_access() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    // `i & 255` lies in 0..=255 wherever it is read: the entry compares 255 with the length once.
    let masked = "function sum(a, n) { var s = 0; for (var i = 0; i < n; i++) { s += a[i & 255]; } return s; }";
    let function = entered_region(masked, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    let before: Vec<ir::Inst> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block))
        .filter(|inst| !body.contains(inst)).collect();
    assert_eq!(bounds_compares(&function, &body), 0, "the masked accesses carry their proof: {function}");
    assert!(bounds_compares(&function, &before) >= 1, "the entry compares the mask's ceiling with the length: {function}");
    // A loop a previous body left at its header, or whose entries were declined, keeps every access's own test.
    let views = || ElementViews::Typed(Some(Kind::Float64));
    for (left_header, entry_bounds) in [(true, true), (false, false)] {
        let function = region_with_feedback(masked, views(), left_header, entry_bounds);
        let body = loop_body(&function);
        assert!(bounds_compares(&function, &body) >= 1, "left {left_header}, entry bounds {entry_bounds}: no index is proved: {function}");
    }
}

#[test]
fn a_loop_nest_proves_the_views_of_its_arrays_once_where_their_bases_are_invariant() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let function = region_over(MATMUL, false, false, ElementViews::Typed(Some(Kind::Float64)));
    let kind = Kind::Float64 as u64;
    let everything: Vec<ir::Inst> = function.layout.blocks().flat_map(|block| function.layout.block_insts(block)).collect();
    assert!(compares_with(&function, &everything, kind) >= 3, "each of `a`, `b` and `c` is proved: {function}");
    assert_eq!(compares_with(&function, &loop_body(&function), kind), 0,
        "no loop of the nest proves a view again per iteration of the one around it: {function}");
}

#[test]
fn an_index_that_adds_an_invariant_base_to_a_counter_compares_base_and_bound_once() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let source = "function row(a, r, n) { var s = 0; for (var k = 0; k < n; k++) s += a[r + k]; return s; }";
    let function = entered_region(source, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    assert_eq!(bounds_compares(&function, &body), 0, "{function}");
}

/// The comparison of the entering edge: instructions of `function` outside its loops.
fn before_loops(function: &ir::Function) -> Vec<ir::Inst> {
    let body = loop_body(function);
    function.layout.blocks().flat_map(|block| function.layout.block_insts(block)).filter(|inst| !body.contains(inst)).collect()
}

#[test]
fn matmul_k_loop_multiplies_nothing_and_compares_no_index_with_a_length() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let function = entered_region(MATMUL, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let body = loop_body(&function);
    let program_product = |function: &ir::Function, operands: [ir::Value; 2]| !operands.iter().any(|operand| is_any_constant(function, *operand));
    let products = wide_operations(&function, &body, ir::Opcode::Imul, program_product);
    assert_eq!(products, 0, "`i * n` is evaluated on the entering edge and `k * n` advances by `n`: {function}");
    assert_eq!(bounds_compares(&function, &body), 0, "both accesses carry the entry's proof: {function}");
    let entry = before_loops(&function);
    assert!(wide_operations(&function, &entry, ir::Opcode::Imul, program_product) >= 2, "the entry holds `i * n` and the start of `k * n`: {function}");
    assert!(version_compares(&function, &entry) >= 2, "the entry compares the reach of each access with its length: {function}");
}

#[test]
fn a_loop_invariant_product_is_one_word_the_entry_evaluates_once() {
    use quench_runtime::tier_surface::TypedArrayKind as Kind;
    let source = "function f(a, x, y, n) { var s = 0; for (var k = 0; k < n; k++) s += a[x * y + k]; return s; }";
    let function = entered_region(source, ElementViews::Typed(Some(Kind::Float64)), IntRange::entering(0));
    let products = wide_operations(&function, &loop_body(&function), ir::Opcode::Imul, |function, operands|
        !operands.iter().any(|operand| is_any_constant(function, *operand)));
    assert_eq!(products, 0, "`x * y` does not change with `k`: {function}");
}
