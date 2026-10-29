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
        Engine::specialize_script_with_atom_prefix(source, "host.js", &NoAtomPrefix, super::ReleasedRecords::Dropped).unwrap();
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

#[test]
fn parenthesized_function_expressions_keep_their_records() {
    // A bundle's module functions and a UMD factory are marked as about to run by
    // parentheses; the load keeps their records instead of releasing them to a replay.
    let source = "var modules = { a: (function marked(exports) { return exports; }) };\n\
                  (function (global, factory) { factory(global); })(this, (function factory(exports) {}));\n\
                  var later = function released() { return 1; };\n\
                  function declared() { return 2; }";
    let program = Engine::specialize(source, "parenthesized-functions.js").unwrap();
    let kept = |name: &str| {
        !program
            .functions
            .iter()
            .find(|slot| {
                slot.header()
                    .name
                    .is_some_and(|atom| program.atoms[atom as usize] == *name)
            })
            .unwrap_or_else(|| panic!("{name} is compiled"))
            .is_released()
    };
    assert!(kept("marked"));
    assert!(kept("factory"));
    assert!(!kept("released"));
    assert!(!kept("declared"));
}

#[test]
fn records_lowered_on_first_read_match_records_lowered_at_load() {
    // Each source reaches a lowering pass: numeric dispatch, plain locals promoted to frame
    // registers, plain-local stores, `with`, handlers, generators, `arguments` and captures.
    let sources = [
        "function numeric(n) { let a = 0, b = 1, c = 2, d = 3; for (let i = 0; i < n; i++) { a = a + b * c - d; b = b + a * 2 - c; c = c * d + a - b; d = d - a + b * c; a = a ^ b; b = b | c; } return a + b + c + d; } numeric(10);",
        "function plain(x) { var total = 0; for (var i = 0; i < x.length; i++) { total += x[i]; } return total; } plain([1, 2, 3]);",
        "function scoped(o) { with (o) { return value + 1; } } scoped({ value: 1 });",
        "function guarded(f) { try { return f(); } catch (error) { return error; } finally { f = null; } } guarded(() => 1);",
        "function* counter(n) { for (let i = 0; i < n; i++) yield i; } [...counter(3)];",
        "async function waits(p) { const value = await p; return value + 1; } waits(Promise.resolve(1));",
        "function legacy() { return arguments.length + arguments[0]; } legacy(1, 2);",
        "function outer() { let count = 0; function inner() { return ++count; } return [inner(), inner()]; } outer();",
        "class Point { constructor(x) { this.x = x; } get double() { return this.x * 2; } static of(x) { return new Point(x); } } Point.of(2).double;",
    ];
    for source in sources {
        let lazy = Engine::specialize(source, "lowering.js").unwrap();
        crate::compile::LOWER_AT_LOAD.with(|flag| flag.set(true));
        let eager = Engine::specialize(source, "lowering.js");
        crate::compile::LOWER_AT_LOAD.with(|flag| flag.set(false));
        let eager = eager.unwrap();
        assert_eq!(lazy.functions.len(), eager.functions.len(), "{source}");
        eager.prime_loop_decoding();
        let mut compared = 0;
        for index in 0..lazy.functions.len() {
            if eager.functions[index].is_released() {
                assert!(lazy.functions[index].is_released(), "function {index} of {source}");
                continue;
            }
            // The root lowers at load; every other record waits for its first read.
            assert_eq!(lazy.functions[index].record().is_some(), index == 0, "function {index} of {source}");
            assert_eq!(
                format!("{:?}", lazy.function(index)),
                format!("{:?}", eager.function(index)),
                "function {index} of {source}"
            );
            compared += 1;
        }
        assert!(compared > 0, "{source}");
    }
}

#[test]
fn partition_points_found_from_the_last_match_a_search() {
    let table = [1, 3, 3, 7, 12];
    let last = std::cell::Cell::new(0);
    for bound in (0..15).chain((0..15).rev()).chain([9, 2, 14, 0, 3, 3]) {
        let expected = table.partition_point(|value| *value < bound);
        assert_eq!(super::partition_point_from(&table, &last, |value| *value < bound), expected, "{bound}");
    }
    // A point left past a shorter table is searched again.
    let last = std::cell::Cell::new(10);
    assert_eq!(super::partition_point_from(&table, &last, |value| *value < 4), 3);
}

#[test]
fn source_lines_found_from_the_last_line_match_a_search() {
    for starts in [vec![], vec![0], vec![0, 4, 4, 9, 30], vec![3, 8]] {
        let last = std::cell::Cell::new(0);
        // Offsets forwards, backwards and jumping, as emission visits them.
        let offsets = (0..40).chain((0..40).rev()).chain([35, 2, 9, 9, 4, 0, 31]);
        for offset in offsets {
            let expected = starts.partition_point(|start| *start <= offset).saturating_sub(1);
            assert_eq!(super::source_line_index(&starts, &last, offset), expected, "{starts:?} {offset}");
        }
    }
}

#[test]
fn utf16_excess_scans_match_each_character() {
    let per_character = |text: &str| {
        let mut excess = 0;
        text.char_indices()
            .filter(|(_, character)| !character.is_ascii())
            .map(|(offset, character)| {
                excess += character.len_utf8() - character.len_utf16();
                (offset, excess)
            })
            .collect::<Vec<_>>()
    };
    let pieces = ["", "a", "abcdefgh", "\u{e9}", "\u{1F600}", "\u{4e2d}\u{6587}", "0123456789abcdef"];
    for first in pieces {
        for second in pieces {
            for third in pieces {
                let text = format!("{first}{second}x{third}{first}");
                assert_eq!(super::source_utf16_excess(&text), per_character(&text), "{text:?}");
            }
        }
    }
}

#[test]
fn discarded_sequence_elements_and_branches_release_their_temporaries() {
    // A comma chain's discarded elements, a conditional's test and branches and a logical
    // operator's right operand reuse the registers they freed, so a minified body's long
    // chain stays within what liveness and frame registers cover (`ROOT_MAP_REGISTERS`).
    let chain = (0..80).map(|_| "g(e.a, e.b)").collect::<Vec<_>>().join(",");
    let source = format!("function f(e) {{ {chain}; return e.x ? g(e.y, e.z) : g(e.w, e.v) && g(e.u, e.t) }}");
    let program = Engine::specialize(&source, "sequence-registers.js").unwrap();
    program.compile_released();
    let registers = program.functions[1].registers;
    assert!(registers < 32, "{registers} registers");
    program.validate().unwrap();
}
