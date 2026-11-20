use super::*;
use crate::bytecode::Instr;

#[test]
fn fixed_transfers_match_the_general_derivation() {
    // Every op with a fixed transfer, over field values in and past the set's width and
    // every result flag combination the A field can carry.
    let values: [u16; 6] = [0, 1, 63, 127, 128, u16::MAX >> 4];
    let mut checked = 0;
    for (index, transfer) in FIXED_TRANSFERS.iter().enumerate() {
        let op = Op::from_index(index).unwrap();
        if transfer.is_none() {
            continue;
        }
        for &a in values.iter().chain(&[crate::bytecode::RETURN_REGISTER, crate::bytecode::NUMERIC_LOCAL_TARGET, 5 | crate::bytecode::RETURN_REGISTER]) {
            for &b in &values {
                for &c in &values {
                    let instruction = WideInstruction::new(op, a, b, c, 0);
                    let no_methods: &[MethodSite] = &[];
                    assert_eq!(uses(instruction, no_methods, &[], &[]), general_uses(instruction, no_methods, &[], &[]), "{op:?} {a} {b} {c}");
                    assert_eq!(definitions(instruction, &[]), general_definitions(instruction, &[]), "{op:?} {a} {b} {c}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 0);
}

#[test]
fn register_ranges_are_their_registers_bits() {
    let width = RegisterSet::BITS as u16;
    for base in (0..width + 4).chain([u16::MAX - 2, u16::MAX]) {
        for count in (0..width + 4).chain([u16::MAX - base]) {
            let expected = (0..count)
                .filter_map(|offset| base.checked_add(offset))
                .fold(RegisterSet::default(), |mask, register| mask | register_bit(register));
            assert_eq!(range(base, count), expected, "base {base} count {count}");
        }
    }
}
use crate::bytecode::Op;

const NO_METHODS: &[MethodSite] = &[];

/// A function of `code` over `registers` registers, with `handlers`, and nothing else.
fn function_of(code: Vec<Instr>, registers: u16, handlers: Vec<crate::bytecode::Handler>) -> Function {
    Function {
        parent: None, name: None, inferred_name: None, is_arrow: false, self_binding_slot: None, source_span: None,
        params: 0, length: 0, parameter_end_pc: 0, parameter_atoms: vec![].into(), rare: None, declared_functions: Default::default(), declared_captures: Default::default(), rest: false, is_async: false,
        is_generator: false, is_class_constructor: false, derived_constructor: false, constant_fields: Default::default(),
        super_home_atom: None, constructible: true, class_initializer: false, parameter_eval_arguments_error: false,
        unaliased_arguments: false, rest_alias: false, prototype_unobservable: false, arguments_slot: None, simple_parameters: true,
        strict: false, locals: 0, local_atoms: vec![].into(), environment_atoms: vec![].into(),
        selective_capture_slots: None, local_registers: Vec::new().into(), inherited_with_scope: false,
        lexical_atoms: vec![].into(), name_bindings: Default::default(),
        binding_sites: vec![].into(), source_positions: Default::default(), environment_clones: vec![].into(), code,
        wide: vec![], registers, dispatch: crate::bytecode::DispatchClass::General, decoded: Default::default(),
        plain_locals: Default::default(), caches: Default::default(),
        lazy_flags: Default::default(), call_plan: Default::default(),
        handlers: handlers.into(), register_roots: Default::default(), undefined_register: None,
        parameter_registers: None, initial_register: Default::default(),
    }
}

#[test]
fn branch_roots_are_derived_from_successor_uses() {
    let function = function_of(vec![
        Instr::new(Op::LoadConst, 0, 0, 0, 0),
        Instr::new(Op::Jump, 0, 0, 0, 3),
        Instr::new(Op::Return, 0, 0, 0, 0),
        Instr::new(Op::Return, 1, 0, 0, 0),
    ], 2, vec![]);
    let roots = analyze(&function, NO_METHODS, &[], &[]).unwrap();
    assert_eq!(roots[3], register_bit(1));
    assert_eq!(roots[1], register_bit(1));
    assert_eq!(roots[1] & register_bit(0), 0);
}

/// A register only a catch block reads is live before every instruction its handler covers,
/// including one that overwrites it: the write is not made when that instruction throws.
#[test]
fn a_register_only_the_catch_reads_is_live_in_the_covered_code() {
    let handler = crate::bytecode::Handler { start: 1, end: 2, target: 3, slot: None, return_target: None, return_slot: None, with_depth: 0 };
    let function = function_of(vec![
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::LoadConst, 1, 0, 0, 0),
        Instr::new(Op::Return, 0, 0, 0, 0),
        Instr::new(Op::Return, 1, 0, 0, 0),
    ], 2, vec![handler]);
    let live = analyze(&function, NO_METHODS, &[], &[]).unwrap();
    assert_ne!(live[1] & register_bit(1), 0, "the covered write does not kill the catch's input");
    assert_eq!(live[0] & register_bit(1), 0, "outside the covered range the write kills it");
}

#[test]
fn operand_windows_root_every_pending_input() {
    for op in [
        Op::WasmStructNew,
        Op::WasmStructNewDesc,
        Op::WasmStructNewDefaultDesc,
        Op::WasmArrayNewFixed,
        Op::WasmArrayNewData,
        Op::WasmArrayNewElem,
        Op::WasmExceptionNew,
        Op::WasmAtomicAccess,
    ] {
        let instruction = Instr::new(op, 0, 1, 3, 0);
        assert_eq!(uses(instruction.as_wide(), NO_METHODS, &[], &[]), register_bit(1) | register_bit(2) | register_bit(3));
        assert_eq!(definitions(instruction.as_wide(), &[]), register_bit(0));
        assert_eq!(uses(Instr::new(op, 0, 0, 0, 0).as_wide(), NO_METHODS, &[], &[]), 0);
    }
}

#[test]
fn optional_registers_read_or_write_by_their_layout() {
    // An appended spread source is read: a collection while it iterates must still see it.
    let append = Instr::new(
        Op::AppendArray,
        1,
        crate::bytecode::encode_optional_register(Some(2)),
        1,
        0,
    );
    assert_eq!(uses(append.as_wide(), NO_METHODS, &[], &[]), register_bit(1) | register_bit(2));
    assert_eq!(definitions(append.as_wide(), &[]), 0);
    // A store's optional register receives a copy of the stored value.
    let store = Instr::new(
        Op::StoreLocalPlain,
        1,
        crate::bytecode::encode_optional_register(Some(3)),
        0,
        0,
    );
    assert_eq!(uses(store.as_wide(), NO_METHODS, &[], &[]), register_bit(1));
    assert_eq!(definitions(store.as_wide(), &[]), register_bit(3));
}

#[test]
fn local_target_does_not_define_a_register() {
    let instruction = Instr::new(
        Op::Binary,
        crate::bytecode::NUMERIC_LOCAL_TARGET | 2,
        0,
        1,
        8,
    );
    assert_eq!(definitions(instruction.as_wide(), &[]), 0);
}
