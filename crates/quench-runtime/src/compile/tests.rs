use super::*;

#[test]
fn regression_global_var_lowering_checks_instruction_roles_before_slots() {
    let wide_count = (1..=u16::MAX)
        .find(|&count| Instr::try_new(Op::MakeConstArray, 0, count, 0, 0).is_none())
        .unwrap();
    let wide_array = format!("[{}];", "0,".repeat(usize::from(wide_count)));
    for suffix in ["", wide_array.as_str()] {
        let source = format!(
            "var global = 1; let lexical = 2; global = global + lexical; \
             function read() {{ return global; }} read(); {suffix}"
        );
        for specialize in [false, true] {
            let program = if specialize {
                Engine::specialize(&source, "global-lowering.js")
            } else {
                Engine::specialize_unspecialized(&source, "global-lowering.js")
            }
            .unwrap();
            let root = &program.functions[0];
            assert!(
                root.code
                    .iter()
                    .any(|instruction| instruction.op() == Op::StoreEnvLocal)
            );
            assert!(!root.global_lexical_atoms().is_empty());
            assert!(
                root.global_lexical_atoms()
                    .iter()
                    .all(|atom| !root.global_var_atoms().contains(atom))
            );
            assert_eq!(!root.wide.is_empty(), !suffix.is_empty());
            program.validate().unwrap();
        }
    }
}

#[test]
fn scalar_constants_reuse_exact_slots() {
    let mut compiler = Compiler::new(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &NoAtomPrefix,
        FxHashMap::default(),
    );
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    assert_eq!(compiler.constant(Constant::String("x".into())), 1);
    assert_eq!(compiler.constants.len(), 2);
}

#[test]
fn scalar_constants_preserve_signed_zero_bits() {
    let mut compiler = Compiler::new(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &NoAtomPrefix,
        FxHashMap::default(),
    );
    assert_ne!(
        compiler.constant(Constant::Number(0.0)),
        compiler.constant(Constant::Number(-0.0))
    );
}

#[test]
fn register_promotion_admits_statement_level_and_nested_writes() {
    // A nested write keeps the binding in its register too: reads an enclosing operation
    // holds across it take a copy (`clobbered_locals`).
    for body in [
        "value = 2;",
        "value += 2;",
        "value++;",
        "value = other = 2;",
        "for (value of [2]) {}",
        "({ field: value } = { field: 2 });",
        "other = (value = 2);",
        "other = value++;",
    ] {
        let source = format!("function update(value) {{ var other; {body} return value; }}");
        let program = Engine::specialize(&source, "write.js").unwrap();
        program.compile_released();
        assert_eq!(
            program.functions[1].local_registers.len(),
            2,
            "the parameter stays in a register: {body}"
        );
        program.validate().unwrap();
    }
    let program = Engine::specialize(
        "function erase(value) { delete value; return value; }",
        "delete.js",
    )
    .unwrap();
    program.compile_released();
    assert!(program.functions[1].local_registers.is_empty());
    program.validate().unwrap();
}

#[test]
fn statement_level_compound_writes_compile_like_statements() {
    // Each part of a statement-level comma sequence, conditional or logical expression runs
    // after the earlier parts' reads are consumed, so its writes make no other read of the
    // local take a copy (`clobbered_locals`), as separate statements' do not: a minified
    // loop body copies no more locals than its statements would.
    let copies = |body: &str| {
        let source = format!(
            "function f(p) {{ var i, d = 1, q = 3, r = 4; for (i = 0; i < 4; i++) {body} return d + q + r + p; }}"
        );
        let program = Engine::specialize(&source, "sequence-writes.js").unwrap();
        program.compile_released();
        program.validate().unwrap();
        let function = &program.functions[1];
        let promoted: Vec<_> = function.local_registers.iter().map(|local| local.register).collect();
        function
            .code
            .iter()
            .filter(|instruction| instruction.op() == Op::Move && promoted.contains(&instruction.b()))
            .count()
    };
    let statements = copies("{ d = d + (q >>> 3 ^ q << 2); q = r; r = d | 0; }");
    for folded in [
        "d = d + (q >>> 3 ^ q << 2), q = r, r = d | 0;",
        "(d = d + (q >>> 3 ^ q << 2), q = r), r = d | 0;",
        "i < 9 ? (d = d + (q >>> 3 ^ q << 2), q = r, r = d | 0) : 0;",
        "i < 9 && (d = d + (q >>> 3 ^ q << 2), q = r, r = d | 0);",
    ] {
        assert_eq!(copies(folded), statements, "{folded}");
    }
    // A write nested in an operand still makes the held read copy.
    assert!(copies("d = d + (q ^ (q = r) << 2), r = d | 0;") > statements);
}

#[test]
fn validator_rejects_a_promoted_binding_read_through_its_local_slot() {
    let mut program = Engine::specialize(
        "function read(value) { return value + 1; }",
        "promoted-register-validation.js",
    )
    .unwrap();
    program.compile_released();
    let function = &mut program.functions[1];
    assert_eq!(function.local_registers.len(), 1);
    let local = function.local_registers[0].local;
    let binary = function
        .code
        .iter_mut()
        .find(|instruction| instruction.op() == Op::Binary)
        .unwrap();
    *binary = crate::bytecode::Instr::new(Op::LoadLocal, 1, 0, 0, u32::from(local));
    assert!(program.validate().is_err());
}

#[test]
fn constant_runs_remain_fresh_and_contiguous() {
    let mut compiler = Compiler::new(
        "test.js",
        "",
        SpecializationMode::Enabled,
        &NoAtomPrefix,
        FxHashMap::default(),
    );
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
    let start = compiler.constant_run(vec![Constant::Number(1.0), Constant::Number(1.0)]);
    assert_eq!(start, 1);
    assert_eq!(compiler.constants.len(), 3);
    assert_eq!(compiler.constant(Constant::Number(1.0)), 0);
}

#[test]
fn packed_domain_overflow_uses_the_wide_side_table() {
    let source = format!("[{}];", "0,".repeat(4097));
    let program = Engine::specialize(&source, "packed-overflow.js").unwrap();
    assert!(
        program
            .functions
            .iter()
            .any(|function| !function.wide.is_empty())
    );
}

#[test]
fn regexp_literals_lower_to_validated_site_metadata() {
    let program = Engine::specialize(
        "function make() { return function inner() { return /a/g; }; } make()();",
        "regexp-site.js",
    )
    .unwrap();
    program.compile_released();
    assert_eq!(program.regexp_literal_sites.len(), 1);
    let site = &program.regexp_literal_sites[0];
    assert!(matches!(
        program.constants.get(site.pattern_constant as usize),
        Some(Constant::String(pattern)) if &**pattern == "a"
    ));
    assert!(matches!(
        program.constants.get(site.flags_constant as usize),
        Some(Constant::String(flags)) if &**flags == "g"
    ));
    assert!(program.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::CreateRegExpLiteral)
    }));
    assert!(
        program
            .functions
            .iter()
            .flat_map(|function| &function.code)
            .all(|instruction| instruction.op() != Op::Construct)
    );
    program.validate().unwrap();

    let path = std::env::temp_dir().join(format!("quench-regexp-site-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    assert_eq!(decoded.regexp_literal_sites, program.regexp_literal_sites);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn constant_computed_property_uses_field_cache_site() {
    let program = Engine::specialize(
        "var object = { answer: 42 }; print(object['answer']);",
        "computed.js",
    )
    .unwrap();
    assert!(
        program.functions[0]
            .code
            .iter()
            .any(|instruction| instruction.op() == Op::GetField)
    );
    assert!(program.field_sites.is_empty());
    assert!(program.cache_sites > 0);
}

#[test]
fn unspecialized_entry_keeps_generic_dispatch_class() {
    let program = Engine::specialize_unspecialized("print(40 + 2);", "generic.js").unwrap();
    assert!(!program.specialized);
    assert!(
        program
            .functions
            .iter()
            .all(|function| function.dispatch == DispatchClass::General)
    );
}

#[test]
fn constructor_source_anchor_survives_argument_move_coalescing() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let program = compile(
            "\nthrow new Error('source snapshot');",
            "constructor-position.js",
        )
        .unwrap();
        let (pc, _) = program.functions[0]
            .instructions()
            .enumerate()
            .find(|(_, instruction)| instruction.op() == Op::Construct)
            .unwrap();
        let position = program.source_position(0, pc as u32).unwrap();
        assert_eq!((position.line, position.column), (2, 7));
        program.validate().unwrap();
    }
}

#[test]
fn completion_capturing_host_scripts_keep_script_declarations() {
    // A strict host script (vm.runInContext, $262.evalScript) declares global bindings; only
    // eval code keeps a strict body's declarations local.
    let source = "'use strict'; function f() {} var v = 1;";
    let script =
        Engine::specialize_script_with_atom_prefix(source, "host.js", &NoAtomPrefix).unwrap();
    assert_eq!(script.kind, crate::bytecode::ProgramKind::Script);
    assert!(!script.root_variables_are_local());
    let eval = Engine::specialize_eval_with_context(
        source,
        "eval.js",
        &NoAtomPrefix,
        false,
        EvalContext::default(),
        &[],
        &[],
    )
    .unwrap();
    assert_eq!(eval.kind, crate::bytecode::ProgramKind::Eval);
    assert!(eval.root_variables_are_local());
}
