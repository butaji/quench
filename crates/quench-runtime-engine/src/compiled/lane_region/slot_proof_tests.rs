//! The tests an online body's field reads make of slots whose type its dependencies hold.
use super::*;
use quench_runtime::bytecode::Class;

/// The lowering of `source`'s `subject`, whose sites read `names` as own slots of one shape and
/// type `ty`, under online facts (the invalidation word at `invalidation`) or the static ones.
fn lowered(source: &str, names: &[&str], ty: Type, invalidation: Option<usize>) -> ir::Function {
    let program = quench_runtime::Engine::specialize(source, "slot-proof.js").unwrap();
    let function = program.function(1);
    let fields = flat_table(function.instructions().filter_map(|instruction| {
        if instruction.op().class().eq(Class::GetField)
            && let Some(FieldLookup::Atom { atom, cache_site, .. }) = instruction.field_lookup()
            && let Some(slot) = names.iter().position(|name| *name == &program.atoms[atom as usize]) {
            Some((usize::from(cache_site), vec![NativeFieldLoad { shape: 0, holder: None, slot: slot as u16, storage_len: names.len(),
                storage: None, ty, inherited: None, function_shape: false, raw_double: false, formed: None, holder_id: None }]))
        } else { None }
    }));
    let atoms = quench_runtime::bitset::BitSet::default();
    let facts = match invalidation {
        Some(invalidation) => VmFacts { online: Some(OnlineFacts { method_cache_atoms: &atoms, invalidation, constants: &[],
            instanceofs: &|_| None, shape_stable: &|_| false, names: None, own_constant: &|_| None, root_constant: &|_| None }), ..VmFacts::STATIC },
        None => VmFacts::STATIC,
    };
    let isa = quench_runtime::vm::jit_backend::target_isa().unwrap();
    let unit = super::super::lane_unit::unit(&program, 1, function, &|_, _| Vec::new(), &|_| None).unwrap();
    lower(&program, 1, &unit, LaneHelpers::Process(&[std::ptr::null(); LaneHelper::COUNT]), &fields, &Default::default(),
        facts, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], (&[], &[], &[]), &[], &[], Coverage::Prefix, Completion::Body, ElementViews::Dense, true)
        .unwrap().function
}

/// The constant `value` is in `native`, where an `iconst` defines it.
fn constant_of(native: &ir::Function, value: ir::Value) -> Option<i64> {
    let definition = native.dfg.value_def(value).inst()?;
    match native.dfg.insts[definition] {
        ir::InstructionData::UnaryImm { imm, .. } => Some(imm.bits()),
        _ => None,
    }
}

/// The shifts of a word by the tag shift, which a tag test makes.
fn tag_tests(native: &ir::Function) -> usize {
    native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
        .filter(|&instruction| matches!(native.dfg.insts[instruction], ir::InstructionData::Binary { opcode: ir::Opcode::Ushr, args }
            if constant_of(native, args[1]) == Some(i64::from(quench_runtime::value::ValueLayout::TAG_SHIFT)))).count()
}

/// The loads of the word at `address`.
fn word_reads(native: &ir::Function, address: usize) -> usize {
    native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
        .filter(|&instruction| matches!(native.dfg.insts[instruction], ir::InstructionData::Load { arg, .. }
            if constant_of(native, arg) == Some(address as i64))).count()
}

#[test]
fn an_online_body_reads_int32_slots_under_its_invalidation_word_alone() {
    let source = "function subject(o) { return o.left + o.right; }";
    let names = ["left", "right"];
    let word = quench_runtime::code_dependencies::Invalidation::default();
    let tested = lowered(source, &names, Type::INT32, None);
    let held = lowered(source, &names, Type::INT32, Some(word.address()));
    assert_eq!(tag_tests(&held) + names.len(), tag_tests(&tested), "no read of a held slot tests a tag:\n{held}");
    assert_eq!(word_reads(&held, word.address()), 1, "both reads share one invalidation point:\n{held}");
    assert_eq!(word_reads(&tested, word.address()), 0);
}

#[test]
fn an_online_body_still_tests_slots_that_are_not_int32() {
    let source = "function subject(o) { return o.left; }";
    let word = quench_runtime::code_dependencies::Invalidation::default();
    let any = lowered(source, &["left"], Type::ANY, Some(word.address()));
    assert_eq!(tag_tests(&any), tag_tests(&lowered(source, &["left"], Type::ANY, None)));
}
