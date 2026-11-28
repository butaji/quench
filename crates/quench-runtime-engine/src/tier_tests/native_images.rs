//! Native images and the native API against the interpreter: publication, bootstrap from a snapshot,
//! native cursors and field probes (moved from the core's library tests, whose binary does not link
//! the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::{Engine, Runtime};
use quench_runtime::vm::field_cache::FieldCacheProbe;

#[test]
fn native_loop_calls_return_their_integer_state_with_a_captured_constant() {
    let body = r#"
        const feedback = 0xD008;
        function state(n, seed) {
            n |= 0; let x = seed | 0;
            for (let i = 0; i < n; i = (i + 1) | 0) {
                if ((x & 1) === 0) x = x >> 1;
                else x = (x >> 1) ^ feedback;
            }
            return x | 0;
        }
        for (let warm = 0; warm < 64; warm++) state(1, warm);
        const args = ['-1', '0', '123456789', '1', '17'];
        print(state(Number(args[0]), Number(args[1])));
        print(state(Number(args[1]), Number(args[2])));
        print(state(Number(args[3]), Number(args[2])));
        print(state(Number(args[4]), Number(args[2])));
    "#;
    let method = format!("const sink = {{ log: print }}; {}", body.replace("print(state(", "sink.log(state("));
    for source in [body.to_owned(), format!("(function () {{ {body} }})();"), format!("(function () {{ {method} }})();")] {
        let program = Engine::specialize(&source, "native-loop-result.js").unwrap();
        let mut expected = Runtime::new(quench_runtime::test_host::TestHost::default());
        expected.set_execution_mode(quench_runtime::ExecutionMode::Interpreted);
        expected.execute(&program).unwrap();
        let expected = expected.host_mut().lines().to_vec();
        let native = quench_runtime::NativeJsProgram::publish(program.clone()).unwrap();
        let mut builder = Runtime::new(quench_runtime::test_host::TestHost::default());
        let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
        let mut runtime = Runtime::new(quench_runtime::test_host::TestHost::default());
        runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
        runtime.execute_native_from_snapshot(Rc::new(bytes), &native, |_, _| Ok(())).unwrap();
        assert_eq!(runtime.host_mut().lines().to_vec(), expected, "{source}");
    }
}

#[test]
fn native_bootstrap_preserves_static_namespace_and_retains_code_for_jobs() {
    let program = Engine::specialize(r#"
        function fold(n, x) { for (let i = 0; i < n; i++) x = (x ^ (x << 3)) | 0; return x; }
        print(fold(17, 7));
        Promise.resolve(9).then(value => print(value));
    "#, "native-bootstrap.js").unwrap();
    let mut builder = Runtime::new(quench_runtime::test_host::TestHost::default());
    let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
    let mut expected = Runtime::new(quench_runtime::test_host::TestHost::default());
    expected.execute(&program).unwrap();
    let expected = expected.host_mut().lines().to_vec();
    for mode in [quench_runtime::ExecutionMode::Compiled, quench_runtime::ExecutionMode::NativeOnly] {
        let native = quench_runtime::NativeJsProgram::publish(program.clone()).unwrap();
        let mut runtime = Runtime::new(quench_runtime::test_host::TestHost::default());
        runtime.set_execution_mode(mode);
        runtime.execute_native_from_snapshot(Rc::new(bytes.clone()), &native, |_, _| Ok(())).unwrap();
        let program = native.residual().clone();
        drop(native);
        runtime.run_host_jobs(&program).unwrap();
        assert_eq!(&*runtime.host_mut().lines(), &expected);
        assert_eq!(runtime.native_execution_rejections(), 0);
    }
}

#[test]
fn native_bootstrap_rejects_namespace_or_specialization_mismatch_before_entry() {
    let program = Engine::specialize("function original() { print('effect'); } original();", "native-bootstrap.js").unwrap();
    let mut builder = Runtime::new(quench_runtime::test_host::TestHost::default());
    let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
    for (foreign, message) in [
        (Engine::specialize("function foreign() { print('effect'); } foreign();", "foreign.js").unwrap(), "atom IDs"),
        (Engine::specialize("function foreign(argument) { print(extra); } foreign();", "foreign.js").unwrap(), "atom IDs"),
    ] {
        let native = quench_runtime::NativeJsProgram::publish(foreign).unwrap();
        let mut runtime = Runtime::new(quench_runtime::test_host::TestHost::default());
        runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
        let error = runtime.execute_native_from_snapshot(Rc::new(bytes.clone()), &native, |_, _| Ok(())).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert!(runtime.host_mut().lines().is_empty());
    }
}

#[test]
fn native_only_field_probes_use_semantic_specialization_permission() {
    let program = quench_runtime::Engine::specialize(r#"
        var own={value:7};
        function Static(){}
        Static.value=8;
        var inherited=Object.create({value:9});
        var getterCalls=0;
        var accessor={get value(){getterCalls++;return 11;}};
        function readOwn(object){return object.value;}
        function readCell(object){return object.value;}
        function readInherited(object){return object.value;}
        for(let i=0;i<4;i++){
            if(readOwn(own)!==7||readCell(Static)!==8||readInherited(inherited)!==9)throw 'data';
        }
    "#, "native-field-permission.js").unwrap();
    let native = quench_runtime::NativeJsProgram::publish(program).unwrap();
    let program = native.residual();
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    vm.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
    vm.execute_native_program(&native, true).unwrap();
    assert!(!vm.admits_in_lane(quench_runtime::vm::ProgramId::MAIN, program), "native semantics do not admit interpreter lane dispatch");
    let site = |name: &str| {
        let function = program.functions.iter().position(|body| body.header().name
            .is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        program.function(function).instructions().find_map(|instruction| match instruction.field_lookup() {
            Some(quench_runtime::bytecode::FieldLookup::Atom { atom, cache_site, .. })
                if &program.atoms[atom as usize] == "value" => Some(cache_site),
            _ => None,
        }).unwrap()
    };
    let mut global = |name| {
        let atom = vm.intern_atom(name);
        vm.own_property(vm.realm.globals, atom).unwrap()
    };
    let own = global("own");
    let function = global("Static");
    let inherited = global("inherited");
    let accessor = global("accessor");
    let own_site = site("readOwn");
    let cell_site = site("readCell");
    let inherited_site = site("readInherited");
    assert_eq!(vm.cell_own_field_hit(own, own_site), Some(Value::integer(7)));
    assert_eq!(vm.cell_own_field_hit(function, cell_site), Some(Value::integer(8)));
    assert!(matches!(vm.cached_field_hit(inherited, inherited_site), FieldCacheProbe::Hit(value)
        if value == Value::integer(9)));
    assert!(matches!(vm.cached_field_hit(function, cell_site), FieldCacheProbe::Hit(value)
        if value == Value::integer(8)));
    assert_eq!(vm.cell_own_field_hit(accessor, own_site), None,
        "a data probe must leave accessor semantics untouched");
    let value = vm.intern_atom("value");
    assert_eq!(vm.get_field_cached(program, accessor, value, own_site).unwrap(), Value::integer(11));
    let calls = vm.intern_atom("getterCalls");
    assert_eq!(vm.own_property(vm.realm.globals, calls), Some(Value::integer(1)));
}

#[test]
fn native_cursors_leave_interpreter_decoding_cold() {
    let program = quench_runtime::Engine::specialize(
        "var n = 3; while (n > 0) n--; throw n;", "native-cursor.js",
    ).unwrap();
    let native = quench_runtime::NativeJsProgram::publish(program.clone()).unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    vm.set_execution_mode(ExecutionMode::NativeOnly);
    let error = vm.execute_native_program(&native, true).unwrap_err();
    assert_eq!(error.thrown_value(), Some(Value::number(0.0)));
    vm.set_execution_mode(ExecutionMode::NativeOnly);
    let cursor = vm.general_cursor(ProgramId::MAIN, native.residual(), 0, true);
    assert!(cursor.decoded.is_null(), "linked code must not expand interpreter instructions");
    for mode in [ExecutionMode::NativeOnly, ExecutionMode::Compiled] {
        vm.set_execution_mode(mode);
        let cursor = vm.general_cursor(ProgramId::MAIN, native.residual(), 0, true);
        assert!(cursor.lane.is_null() && !cursor.back_edges,
            "linked code must not prepare an interpreter lane");
    }

    let mut interpreter = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    interpreter.initialize_for_snapshot(&program).unwrap();
    let cursor = interpreter.general_cursor(ProgramId::MAIN, &program, 0, false);
    assert!(!cursor.decoded.is_null(), "a looping function expands its dispatch cache on its first entry");
    let straight = quench_runtime::Engine::specialize("var a = 1; throw a + 1;", "straight-cursor.js").unwrap();
    let mut once = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    once.initialize_for_snapshot(&straight).unwrap();
    let cursor = once.general_cursor(ProgramId::MAIN, &straight, 0, false);
    assert!(cursor.decoded.is_null(), "a loop-free function entered once runs from packed code");
    let error = quench_runtime::test_support::execute(&mut interpreter, &program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(Value::number(0.0)));
}

#[test]
fn conditional_local_field_copy_keeps_valid_native_metadata() {
    let cases = [
        ("value", "const factory = () => ({model:42}); function Copy() { const result = factory(); if (result === null) {} this.model = result.model; } if (new Copy().model !== 42) throw Error('field copy failed');"),
        ("effects", r#"
            const events = [], poison = {};
            let fail = false;
            const factory = () => ({get model() { events.push('get'); if (fail) throw poison; return 42; }});
            function Copy() { const result = factory(); if (result === null) {} this.model = result.model; }
            Object.defineProperty(Copy.prototype, 'model', {set(value) { events.push('set:' + value); }});
            new Copy(); fail = true;
            try { new Copy(); throw Error('missing getter error'); } catch (error) { if (error !== poison) throw error; }
            if (events.join('|') !== 'get|set:42|get') throw Error('field copy reordered effects');
        "#),
    ];
    for (name, source) in cases {
        let program = quench_runtime::Engine::specialize(source, "local-field-copy.js").unwrap();
        assert!(program.validate().is_ok(), "{:?}", program.field_sites);
        let path = std::env::temp_dir().join(format!("quench-local-field-copy-{}-{name}", std::process::id()));
        program.write_binary(&path).unwrap();
        let program = quench_runtime::ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        for mode in [quench_runtime::ExecutionMode::Interpreted, quench_runtime::ExecutionMode::Compiled] {
            let mut runtime = quench_runtime::Runtime::new(quench_runtime::SystemHost);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).unwrap();
        }
        {
            let native = quench_runtime::NativeJsProgram::publish(program).unwrap();
            let mut runtime = quench_runtime::Runtime::new(quench_runtime::SystemHost);
            runtime.set_execution_mode(quench_runtime::ExecutionMode::NativeOnly);
            runtime.execute_native(&native).unwrap();
            assert_eq!(runtime.native_execution_rejections(), 0);
        }
    }
}

#[test]
fn a_restored_runtime_adopts_the_process_tier_enrollment_again() {
    let program = Engine::specialize("1;", "restore-tier.js").unwrap();
    let mut builder = Runtime::new(TestHost::default());
    let snapshot = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
    let mut restored = Runtime::new(TestHost::default());
    restored.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    assert!(restored.vm.methods_enrolled_in_lane(), "the mode enrolls the lane-body tier");
    restored.restore_snapshot(std::rc::Rc::new(snapshot), &program, |_, _| Ok(())).unwrap();
    assert_eq!(restored.execution_mode(), quench_runtime::ExecutionMode::Compiled);
    assert!(restored.vm.methods_enrolled_in_lane(), "a restore carries the realm, not the tier enrollment");
}

