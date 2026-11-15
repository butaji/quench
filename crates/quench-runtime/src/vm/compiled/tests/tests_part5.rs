use super::*;

#[cfg(feature = "aot")]
#[test]
fn native_global_scripts_preserve_lexicals_completion_and_invocation_names() {
    let mut compiler = crate::NativeJsCompiler::new(true);
    let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
    let sources = ["const frozenLexical = 41; frozenLexical + 1;", "frozenLexical;", "/* empty */"];
    let mut counters = Vec::new();
    let artifacts = sources.into_iter().map(|source| {
        let key = crate::NativeSourceKey::global_script(true, source);
        let residual = compiler.compile_source(&key).unwrap();
        let (native, counts, _) = linked_test_residual(residual);
        counters.push(counts);
        (key, native)
    }).collect::<Vec<_>>();
    let entry = compiler.finish(entry).unwrap();
    let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
        Ok(artifacts.iter().find(|(key, _)| key.name() == name && key.parts().eq(parts.iter().copied()))
            .map(|(_, native)| native.clone()))
    });
    let mut vm = Vm::new(host);
    vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
    vm.set_execution_mode(ExecutionMode::NativeOnly);
    let context = vm.embedding_program().unwrap();
    assert_eq!(vm.eval_script_source(&context, sources[0], "first-invocation.js").unwrap(), Value::number(42.0));
    let first_id = super::super::ProgramId::from_raw(u32::try_from(vm.programs.len() - 1).unwrap());
    assert_eq!(vm.programs.get(first_id).unwrap().source_name, "first-invocation.js");
    assert_eq!(vm.eval_script_source(&context, sources[1], "second-invocation.js").unwrap(), Value::number(41.0));
    assert_eq!(counters[0][0].get(), 1);
    assert_eq!(counters[1][0].get(), 1);
    let last = vm.programs.len();
    assert_eq!(vm.eval_script_source(&context, sources[2], "empty.js").unwrap(), Value::UNDEFINED);
    assert_eq!(vm.programs.len(), last);
    let error = vm.eval_script_source(&context, "let unknown = ;", "unknown.js").unwrap_err();
    assert!(error.to_string().contains("source compilation is unavailable"), "{error}");
    assert!(vm.lookup_atom("unknown").is_none());
    // The same linked declaration still performs per-invocation instantiation.
    let error = vm.eval_script_source(&context, sources[0], "duplicate.js").unwrap_err();
    assert!(error.to_string().contains("SyntaxError"), "{error}");
        assert_eq!(counters[0][0].get(), 1);
        let mut context = crate::NativeContext::new(&mut vm);
        let realm = context.create_realm_rooted().unwrap();
        let result = context.evaluate_script_in_realm_rooted(realm, sources[0], "foreign-realm.js").unwrap();
        let expected = context.number(42.0);
        assert!(context.strict_equal_rooted(result, expected).unwrap());
        assert_eq!(counters[0][0].get(), 2);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_global_scripts_defer_syntax_failures_and_reject_module_code() {
        for module in [false, true] {
            let mut compiler = crate::NativeJsCompiler::new(true);
            let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
            let source = "let invalid = ;";
            let native = module.then(|| linked_test_residual(compiler.compile_module("export {};", "module.js").unwrap()).0);
            let entry = compiler.finish(entry).unwrap();
            let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
                assert_eq!(name, crate::GLOBAL_SCRIPT_SOURCE_NAME);
                assert_eq!(parts, ["specialized", crate::GLOBAL_SCRIPT_GOAL, source]);
                match &native {
                    Some(native) => Ok(Some(native.clone())),
                    None => Err(crate::NativeSourceError::Syntax("deferred script syntax".into())),
                }
            });
            let mut vm = Vm::new(host);
            vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let context = vm.embedding_program().unwrap();
            let error = vm.eval_script_source(&context, source, "invocation.js").unwrap_err();
            let expected = if module { "different parse goal" } else { "SyntaxError: deferred script syntax" };
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_scripts_preserve_code_through_preparation_and_compiled_handles() {
        for typescript in [false, true] {
            let mut compiler = crate::NativeJsCompiler::new(true);
            let entry = compiler.compile_script("", "seed.js", crate::HostScriptOptions::default()).unwrap();
            let source = if typescript {
                "function answer(value: unknown): unknown { return (value as number) + 1; } answer('x');"
            } else {
                "function answer(value) { return value + 1; } answer('x');"
            };
            let options = crate::HostScriptOptions { typescript, ..Default::default() };
            let residual = compiler.compile_script(source, "library", options).unwrap();
            let entry = compiler.finish(entry).unwrap();
            let (native, counts, weak) = linked_test_residual(residual);
            let goal = options.cache_goal();
            let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
                assert_eq!(name, "library");
                assert_eq!(parts, ["specialized", &goal, source]);
                Ok(Some(native.clone()))
            });
            let mut vm = Vm::new(host);
            vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
            let prepared = vm.prepare_embedding_specialized_script_retaining(
                source, "library", None, None, false, typescript,
            ).unwrap();
            for _ in 0..2 {
                let result = vm.evaluate_embedding_prepared_script(&prepared).unwrap();
                assert_eq!(vm.heap.string(result).unwrap().to_string(), "x1");
            }
            assert!(counts.iter().all(|count| count.get() > 0));
            assert!(weak.upgrade().is_some());
            if !typescript {
                let compiled = vm.compile_embedding_host_script(source, "library", (0, 0)).unwrap();
                let before = counts[0].get();
                for _ in 0..2 {
                    let result = vm.evaluate_embedding_compiled_script(&compiled).unwrap();
                    assert_eq!(vm.heap.string(result).unwrap().to_string(), "x1");
                }
                assert!(counts[0].get() > before);
                let mut context = crate::NativeContext::new(&mut vm);
                let first = context.create_realm_rooted().unwrap();
                let second = context.create_realm_rooted().unwrap();
                for global in [first, first, second] {
                    let result = context.evaluate_compiled_script_in_realm_rooted(global, &compiled).unwrap();
                    let expected = context.string_rooted("x1");
                    assert!(context.strict_equal_rooted(result, expected).unwrap());
                }

            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_wrappers_and_bootstrap_use_exact_source_keys() {
        for bootstrap in [false, true] {
            let mut compiler = crate::NativeJsCompiler::new(true);
            let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
            let prefix = "(function() {\n";
            let body = "return 42;";
            let suffix = "\n})();";
            let source = "globalThis.bootResult = 42;";
            let residual = if bootstrap {
                compiler.compile_host_script(source, "unit").unwrap()
            } else {
                compiler.compile_library_body(prefix, std::sync::Arc::from(body), suffix, "unit").unwrap()
            };
            let entry = compiler.finish(entry).unwrap();
            let (native, counts, _) = linked_test_residual(residual);
            let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
                assert_eq!(name, "unit");
                if bootstrap { assert_eq!(parts, ["specialized", source]); }
                else { assert_eq!(parts, ["specialized", prefix, body, suffix]); }
                Ok(Some(native.clone()))
            });
            let mut vm = Vm::new(host);
            vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
            if bootstrap {
                let context = vm.embedding_program().unwrap();
                vm.eval_host_script(&context, source, "unit").unwrap();
                let atom = vm.lookup_atom("bootResult").unwrap();
                assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(42.0)));
            } else {
                let prepared = vm.prepare_embedding_library_body_kept(
                    prefix, crate::bytecode::SourceText::Shared(std::sync::Arc::from(body)), suffix, "unit",
                ).unwrap();
                assert_eq!(vm.evaluate_embedding_prepared_script(&prepared).unwrap(), Value::number(42.0));
            }
            assert!(counts.iter().all(|count| count.get() > 0));
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_script_rejects_atom_remapping_before_effects() {
        let (native, counts, _) = linked_test_program("globalThis.nativeEffect = 42;", false);
        let vm = semantic_vm("globalThis.differentNamespace = 0;");
        let error = match vm.prepare_native_script(native) {
            Ok(_) => panic!("incompatible native atoms were accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("atom IDs"));
        assert!(counts.iter().all(|count| count.get() == 0));
        assert!(vm.lookup_atom("nativeEffect").is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_host_script_rejects_specialization_mismatch_and_resolver_errors() {
        let (native, counts, _) = linked_test_program("", false);
        let entry = crate::Engine::specialize_unspecialized("", "unspecialized.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
        let error = match vm.prepare_native_script(native) {
            Ok(_) => panic!("incompatible specialization was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("specialization"));
        assert!(counts.iter().all(|count| count.get() == 0));

        let host = crate::test_host::TestHost::default().with_native_programs(|_, _| Err("broken linked image".into()));
        let mut vm = Vm::new(host);
        let entry = crate::Engine::specialize("", "entry.js").unwrap();
        vm.initialize_shared(&Rc::new(entry), super::super::builtins::RealmScope::Complete).unwrap();
        let error = match vm.prepare_embedding_specialized_script("globalThis.effect = 1;", "unit", None) {
            Ok(_) => panic!("resolver error silently fell back to compilation"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("broken linked image"));
        assert!(vm.lookup_atom("effect").is_none());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn runtime_native_resolver_preserves_host_fallback_and_refuses_snapshot_loss() {
        let mut compiler = crate::NativeJsCompiler::new(true);
        let entry = compiler.compile_script("", "seed.js", Default::default()).unwrap();
        let unit = compiler.compile_script("throw 42;", "unit.js", Default::default()).unwrap();
        let (native, _, _) = linked_test_residual(unit);
        let calls = Rc::new(std::cell::Cell::new(0));
        let observed = calls.clone();
        let host = crate::test_host::TestHost::default().with_native_programs(move |_, _| {
            observed.set(observed.get() + 1);
            Ok(Some(native.clone()))
        });
        let mut vm = Vm::new(host);
        vm.initialize_shared(&Rc::new(compiler.finish(entry).unwrap()), super::super::builtins::RealmScope::Complete).unwrap();
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        vm.set_native_program_resolver(|_, _| Ok(None));
        let prepared = vm.prepare_embedding_specialized_script("throw 42;", "unit.js", None).unwrap();
        let error = vm.evaluate_embedding_prepared_script(&prepared).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
        assert_eq!(calls.get(), 1);
        assert!(vm.write_snapshot(|_, _| Ok(())).unwrap_err().to_string().contains("native source resolver"));
        vm.set_native_program_resolver(|_, _| Err(crate::NativeSourceError::InvalidImage("incompatible runtime catalog".into())));
        let error = match vm.prepare_embedding_specialized_script("throw 42;", "unit.js", None) {
            Ok(_) => panic!("resolver error fell back to the host"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("incompatible runtime catalog"));
        assert_eq!(calls.get(), 1);
    }

    #[cfg(feature = "native-execution")]
    #[test]
    fn native_only_rejects_unlinked_roots_before_guest_effects() {
        let program = crate::Engine::specialize("globalThis.effect = 1;", "unlinked.js").unwrap();
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute(&program).unwrap_err();
        assert!(error.to_string().contains("native guest code required"));
        let atom = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), None);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_only_runs_linked_calls_but_rejects_dynamic_function_bodies() {
        let (native, counts, _) = linked_test_program("function answer(x) { return x + 1; } throw answer(41);", false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
        assert!(counts.iter().all(|count| count.get() > 0));

        let (native, _, _) = linked_test_program("globalThis.effect = 0; (new Function(globalThis.body))();", false);
        let host = crate::test_host::TestHost::default().with_install(|context| {
            let global = context.global_root()?;
            let key = context.string_rooted("body");
            let body = context.string_rooted("globalThis.effect = 1;");
            context.set_property_rooted(global, key, body, global)?;
            Ok(())
        });
        let mut vm = Vm::new(host);
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert!(error.to_string().contains("native guest code required"));
        let atom = vm.lookup_atom("effect").unwrap();
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(0.0)));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_function_artifacts_preserve_identity_kinds_and_conversion_order() {
        use crate::{DynamicFunctionKind as Kind, NativeSourceCompilation as Compilation, NativeSourceKey};
        let source = r#"
            globalThis.result = 40;
            function make() { let result = 100; return Function('x', 'return result + x;'); }
            const first = make();
            const second = new Function('x', 'return result + x;');
            if (first === second || first.prototype === second.prototype) throw 'identity';
            if (first.name !== 'anonymous' || first.length !== 1 || first(2) !== 42) throw 'scope';
            if (first.toString() !== 'function anonymous(x\n) {\nreturn result + x;\n}') throw 'source';
            const commented = Function('a/*', '#!comment*/', 'return a;');
            if (commented(42) !== 42 || commented.length !== 1) throw 'parameter comment';
            const generator = (function*(){}).constructor('x', 'yield x;');
            if (generator(42).next().value !== 42) throw 'generator';
            const async = (async function(){}).constructor('x', 'return x;');
            if (!(async(42) instanceof Promise)) throw 'async';
            const asyncGenerator = (async function*(){}).constructor('x', 'yield x;');
            if (!(asyncGenerator(42).next() instanceof Promise)) throw 'async generator';
            let trace = '';
            try { Function({toString() { trace += 'p'; return 'a) {}); globalThis.injected = 1; (function('; }},
                           {toString() { trace += 'b'; return 'return 1;'; }}); throw 'accepted'; }
            catch (error) { if (!(error instanceof SyntaxError) || trace !== 'pb') throw 'syntax/order'; }
            if ('injected' in globalThis) throw 'parameter injection';
            trace = '';
            try { Function({toString() { trace += 'p'; return '#!bad'; }},
                           {toString() { trace += 'b'; return 'return 1;'; }}); }
            catch (error) { if (!(error instanceof SyntaxError) || trace !== 'pb') throw 'hashbang order'; }

            const cause = {};
            try { Function({toString() { throw cause; }}, 'return 1;'); }
            catch (error) { if (error !== cause) throw 'coercion cause'; }
            if (false) Function('let invalid = ;');
            throw 42;
        "#;
    let mut compiler = crate::NativeJsCompiler::new(true);
    let entry = compiler.compile_script(source, "entry.js", Default::default()).unwrap();
    let requests = [
        (Kind::Ordinary, "x", "return result + x;"),
        (Kind::Generator, "x", "yield x;"),
        (Kind::Async, "x", "return x;"),
        (Kind::AsyncGenerator, "x", "yield x;"),
        (Kind::Ordinary, "a) {}); globalThis.injected = 1; (function(", "return 1;"),
        (Kind::Ordinary, "", "let invalid = ;"),
        (Kind::Ordinary, "#!bad", "return 1;"),
        (Kind::Ordinary, "a/*,#!comment*/", "return a;"),
    ];
    let artifacts = requests.into_iter().map(|(kind, parameters, body)| {
        let key = NativeSourceKey::function_constructor(true, kind, parameters, body);
        let artifact = match compiler.compile_function_source(&key).unwrap() {
            Compilation::Program(program) => Ok(linked_test_residual(program).0),
            Compilation::SyntaxError(message) => Err(crate::NativeSourceError::Syntax(message)),
        };
        (key, artifact)
    }).collect::<Vec<_>>();
    let entry = linked_test_residual(compiler.finish(entry).unwrap()).0;
    let host = crate::test_host::TestHost::default().with_native_programs(move |name, parts| {
        Ok(match artifacts.iter().find(|(key, _)| key.name() == name && key.parts().eq(parts.iter().copied())) {
            Some((_, artifact)) => Some(artifact.clone()?),
            None => None,
        })
    });
    let mut vm = Vm::new(host);
    vm.set_execution_mode(ExecutionMode::NativeOnly);
    let error = vm.execute_native_program(&entry, true).unwrap_err();
    assert_eq!(error.thrown_value(), Some(Value::number(42.0)), "{error}");
}

#[cfg(feature = "aot")]
#[test]
fn native_only_rejects_source_before_parsing_and_preserves_function_argument_coercion() {
    for source in [
        "eval('let compilationOnly = ;');",
        "(0, eval)('let compilationOnly = ;');",
        "new Function({toString() { globalThis.converted = 1; return 'let compilationOnly = ;'; }});",
    ] {
        let (native, _, _) = linked_test_program(source, false);
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(ExecutionMode::NativeOnly);
        let error = vm.execute_native_program(&native, true).unwrap_err();
        assert!(error.to_string().contains("source compilation is unavailable"), "{source}: {error}");
        assert!(vm.lookup_atom("compilationOnly").is_none());
        if source.starts_with("new Function") {
            let atom = vm.lookup_atom("converted").unwrap();
            assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(1.0)));
        }
    }
    let (native, _, _) = linked_test_program("throw eval(42);", false);
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(ExecutionMode::NativeOnly);
    assert_eq!(vm.execute_native_program(&native, true).unwrap_err().thrown_value(), Some(Value::number(42.0)));
}

#[cfg(feature = "aot")]
#[test]
fn linked_native_frames_call_return_and_unwind_without_replaying_effects() {
    for typescript in [false, true] {
        let (native, counts, _) = linked_test_program(
            "let trace = ''; function fail() { trace += 't'; throw 7; } function recover() { try { fail(); } catch (e) { trace += 'c'; return e + 1; } finally { trace += 'f'; } } if (recover() !== 8) throw 'bad return'; throw trace;",
            typescript,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected trace throw") };
        let value = error.thrown_value().unwrap();
        assert_eq!(vm.heap.string(value).unwrap().to_string(), "tcf");
        assert!(counts.iter().all(|count| count.get() > 0), "every called function enters native code");
    }
}

#[cfg(feature = "aot")]
#[test]
fn linked_native_calls_share_the_dispatch_frame_transition() {
    for typescript in [false, true] {
        let (native, _, _) = linked_test_program(
            "function target(value) { return value + 1; } function caller() { const a = target(20); const b = target(20, ...[]); return a + b; } throw caller();",
            typescript,
        );
        #[cfg(not(quench_profiled))]
        let program = native.residual();
        #[cfg(not(quench_profiled))]
        let caller = program.functions.iter().position(|body| {
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "caller")
            }).unwrap() as u32;
            let mut vm = Vm::new(crate::test_host::TestHost::default());
            vm.initialize_shared(&Rc::new(native.residual().clone()), super::super::builtins::RealmScope::Complete).unwrap();
            let prepared = vm.prepare_native_script(native).unwrap();
            vm.programs.attach_native_js(ProgramId::MAIN, prepared.native.unwrap()).unwrap();
            #[cfg(not(quench_profiled))]
            {
                vm.programs.enable_js_jit_diagnostics();
                // Linked code skips JIT admission, which normally creates this row.
                vm.programs.record_js_jit_entry(ProgramId::MAIN, caller, false, 0);
            }
            vm.set_execution_mode(ExecutionMode::NativeOnly);
            let error = vm.run_main(&prepared.program, true).unwrap_err();
            assert_eq!(error.thrown_value(), Some(Value::number(42.0)));
            #[cfg(not(quench_profiled))]
            {
                let sites = vm.programs.js_jit_call_site_counts(ProgramId::MAIN, caller).unwrap();
                for opcode in [Op::Call, Op::CallArray] {
                    assert!(sites.iter().any(|(_, op, _, outcomes, ..)| {
                        *op == opcode as u16
                            && outcomes[super::super::program_store::JsJitCallOutcome::PushFrame as usize] > 0
                    }), "linked {opcode:?} must use the shared frame transition");
                }
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn linked_native_generator_stops_after_parameters_and_resumes_yield() {
        let (native, counts, _) = linked_test_program(
            "let trace = ''; function* values(x = (trace += 'p', 1)) { trace += 'b'; yield x; trace += 'r'; return 2; } let g = values(); if (trace !== 'p') throw 'body ran during parameters'; let first = g.next(); let last = g.next(); if (first.value !== 1 || first.done || last.value !== 2 || !last.done) throw 'bad yield'; throw trace;",
            false,
        );
        let mut vm = Vm::new(crate::test_host::TestHost::default());
        let Err(error) = vm.execute_native_program(&native, true) else { panic!("expected trace throw") };
        let value = error.thrown_value().unwrap();
        assert_eq!(vm.heap.string(value).unwrap().to_string(), "pbr");
        assert!(counts.iter().all(|count| count.get() > 0));
    }
