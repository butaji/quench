//! Lane bodies against the interpreter: each program runs compiled and interpreted, and the compiled run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use quench_runtime::vm::tier_events::Event;
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use crate::test_oracle::{assert_tier_agrees, Expect, Tier};

#[test]
#[cfg(not(quench_profiled))]
fn typed_lane_recovery_preserves_observable_parameter_updates() {
    let program = Engine::specialize(r#"
      function exposed(value, done) { value=123; if (done) return 0; return globalThis.probe.value; }
      globalThis.probe={ get value() { return exposed.arguments[0]; } };
    "#, "typed-lane-reflected-parameter.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "exposed")).unwrap();
    let body = program.function(id);
    let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).expect("native prefix");
    let parameter = usize::from(body.promoted_register(0).expect("promoted parameter"));
    let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
    registers[parameter] = Value::integer(111);
    let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
        &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
    assert!(pc < body.code.len());
    assert_eq!(registers[parameter], Value::integer(123),
        "f.arguments observes a current parameter even when bytecode no longer reads it");
    let atom = vm.intern_atom("exposed");
    let function = vm.own_property(vm.realm.globals, atom).unwrap();
    assert_eq!(vm.call_value(&program, function, Value::UNDEFINED, &[Value::integer(111)]).unwrap(), Value::integer(123));
}

#[test]
#[cfg(not(quench_profiled))]
fn typed_shape_edges_preserve_joins_reassignment_and_layout_fallback() {
    use quench_runtime::bytecode::Op;
    let source = r#"
        var packed={left:3,right:5}, arena={left:3.5,right:5.5};
        var other={right:11,padding:99,left:4}, root={left:3,right:5,next:null};
        root.next=root;
        function read(o, flag) { let n=o.left; if(flag) n+=1; return n+o.right; }
        function joined(o, other, flag) { let n=o.left; if(flag) o=other; return n+o.right; }
        function reload(o, flag) { let n=o.left; if(flag) o=o.next; return n+o.right; }
        function update(o, flag, value) { let n=o.left; if(flag) o.right=value; return n+o.right; }
        function setNext(value) { root.next=value; }
        read(packed,false); read(arena,true);
        joined(packed,packed,false); joined(packed,packed,true);
        reload(root,false); reload(root,true);
        update(packed,false,6); update(packed,true,6);
    "#;
    let typescript = source.replace("function read(o, flag)",
        "function read(o: {left:number,right:number}, flag:boolean): number");
    for program in [Engine::specialize(source, "typed-shape-edges.js"),
        Engine::specialize_typescript(&typescript, "typed-shape-edges.ts")] {
        let program = program.unwrap();
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let global = |vm: &mut Vm, name| {
            let atom = vm.intern_atom(name);
            vm.own_property(vm.realm.globals, atom).unwrap()
        };
        let packed = global(&mut vm, "packed");
        let arena = global(&mut vm, "arena");
        let other = global(&mut vm, "other");
        let root = global(&mut vm, "root");
        for name in ["read", "joined", "reload", "update"] {
            let id = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            let body = program.function(id);
            let instructions: Vec<_> = body.instructions().collect();
            let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new())
                .expect("typed shape artifact");
            let function = global(&mut vm, name);
            let run = |vm: &mut Vm, arguments: &[Value]| {
                let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
                for (index, value) in arguments.iter().copied().enumerate() {
                    let register = body.parameter_registers.map(|base| base + index as u16)
                        .or_else(|| body.promoted_register(index as u16)).unwrap();
                    registers[usize::from(register)] = value;
                }
                let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                    &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut())
                    .settled(&mut registers, &instructions).expect("caller exit");
                (pc, registers)
            };
            if name == "update" {
                let right = vm.intern_atom("right");
                for value in [Value::integer(7), Value::number(7.5)] {
                    let before = vm.own_property(packed, right).unwrap();
                    let (pc, registers) = run(&mut vm, &[packed, Value::TRUE, value]);
                    if value.as_int().is_some() {
                        assert_eq!(instructions[pc].op(), Op::Return);
                        assert_eq!(registers[usize::from(instructions[pc].register_a())], Value::integer(10));
                        assert_eq!(vm.own_property(packed, right), Some(value));
                    } else {
                        assert_eq!(instructions[pc].op(), Op::SetField);
                        assert_eq!(vm.own_property(packed, right), Some(before), "exit before storage migration");
                    }
                }
                continue;
            }
            let cases = match name {
                "read" => vec![(vec![packed, Value::FALSE], true), (vec![arena, Value::TRUE], true),
                    (vec![Value::integer(5), Value::FALSE], false)],
                "joined" => vec![(vec![packed, packed, Value::TRUE], true),
                    (vec![packed, other, Value::FALSE], true), (vec![packed, other, Value::TRUE], false)],
                "reload" => {
                    let (pc, registers) = run(&mut vm, &[root, Value::TRUE]);
                    assert_eq!(instructions[pc].op(), Op::Return);
                    assert_eq!(registers[usize::from(instructions[pc].register_a())], Value::integer(8));
                    let setter = global(&mut vm, "setNext");
                    vm.call_value(&program, setter, Value::UNDEFINED, &[other]).unwrap();
                    vec![(vec![root, Value::FALSE], true), (vec![root, Value::TRUE], false)]
                }
                _ => unreachable!(),
            };
            for (arguments, completes) in cases {
                let expected = vm.call_value(&program, function, Value::UNDEFINED, &arguments).unwrap();
                let (pc, registers) = run(&mut vm, &arguments);
                if completes {
                    assert_eq!(instructions[pc].op(), Op::Return, "{name} at {pc}");
                    assert_eq!(registers[usize::from(instructions[pc].register_a())], expected);
                } else {
                    assert_eq!(instructions[pc].op(), Op::GetField, "{name} at {pc}");
                    let Some(quench_runtime::bytecode::FieldLookup::Atom { base, .. }) = instructions[pc].field_lookup()
                        else { panic!("own-field recovery") };
                    let receiver = registers[usize::from(base.register_index().unwrap())];
                    assert_eq!(receiver, if name == "read" { Value::integer(5) } else { other });
                }
            }
        }
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_own_slot_proofs_preserve_alias_stores_and_receiver_changes() {
    use quench_runtime::bytecode::Op;
    let source = include_str!("../../../quench-runtime/tests/fixtures/own-slot-proofs.js");
    let typescript = source
        .replace("function readPair(node)", "function readPair(node: {left: number, right: number}): number")
        .replace("function update(value)", "function update(value: number): number");
    for program in [Engine::specialize(source, "own-slot-proofs.js"),
        Engine::specialize_typescript(&typescript, "own-slot-proofs.ts")] {
        let program = program.unwrap();
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let global = |vm: &mut Vm, name| {
            let atom = vm.intern_atom(name);
            vm.own_property(vm.realm.globals, atom).unwrap()
        };
        let packed = global(&mut vm, "packed");
        let arena = global(&mut vm, "arena");
        let wider = global(&mut vm, "wider");
        for name in ["readPair", "reassigned", "joined", "update"] {
            let id = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            let body = program.function(id);
            let instructions: Vec<_> = body.instructions().collect();
            let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).expect("own-slot native artifact");
            let function = global(&mut vm, name);
            let run = |vm: &mut Vm, receiver, arguments: &[Value]| {
                let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
                for (index, value) in arguments.iter().copied().enumerate() {
                    let register = body.parameter_registers.map(|base| base + index as u16)
                        .or_else(|| body.promoted_register(index as u16)).expect("register parameter");
                    registers[usize::from(register)] = value;
                }
                let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                    &program, registers.as_mut_ptr(), receiver, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
                (pc, registers)
            };
            if name != "update" {
                for object in [packed, arena] {
                    for flag in [Value::FALSE, Value::TRUE] {
                        let arguments = [object, if name == "reassigned" { wider } else { arena }, flag];
                        let expected = vm.call_value(&program, function, Value::UNDEFINED, &arguments).unwrap();
                        let (pc, registers) = run(&mut vm, Value::UNDEFINED, &arguments[..usize::from(body.params)]);
                        assert_eq!(instructions[pc].op(), Op::Return, "{name} left at {pc}: {instructions:?}");
                        assert_eq!(registers[usize::from(instructions[pc].register_a())], expected);
                    }
                }
                if name == "readPair" {
                    let effects = vm.intern_atom("effects");
                    for (name, expected, count) in [("getter", 23, 1), ("proxy", 27, 2), ("extras", 10, 0)] {
                        let object = global(&mut vm, name);
                        let before = vm.own_property(vm.realm.globals, effects).unwrap().as_int().unwrap();
                        let (pc, _) = run(&mut vm, Value::UNDEFINED, &[object]);
                        assert_eq!(instructions[pc].op(), Op::GetField, "{name} leaves before the first read");
                        assert_eq!(vm.own_property(vm.realm.globals, effects), Some(Value::integer(before)));
                        assert_eq!(vm.call_value(&program, function, Value::UNDEFINED, &[object]).unwrap(), Value::integer(expected));
                        assert_eq!(vm.own_property(vm.realm.globals, effects), Some(Value::integer(before + count)));
                    }
                }
                continue;
            }
            let left = vm.intern_atom("left");
            let right = vm.intern_atom("right");
            for object in [packed, arena] {
                for value in [Value::integer(5), Value::integer(7)] {
                    let before = vm.own_property(object, left).unwrap().as_number().unwrap();
                    let other = vm.own_property(object, right).unwrap().as_number().unwrap();
                    let expected = Value::number(before + value.as_number().unwrap() + other);
                    let (pc, registers) = run(&mut vm, object, &[value]);
                    assert_eq!(instructions[pc].op(), Op::Return, "alias store left at {pc}");
                    assert_eq!(registers[usize::from(instructions[pc].register_a())], expected,
                        "a layout proof must not snapshot writable packed contents");
                    assert_eq!(vm.own_property(object, left), Some(value));
                }
            }
            let (pc, _) = run(&mut vm, packed, &[Value::number(0.5)]);
            assert!(matches!(instructions[pc].op(), Op::SetThisField | Op::SetThisFieldStrict),
                "leave before packed storage widens, after proving its first read");
            assert_eq!(vm.own_property(packed, left), Some(Value::integer(7)));
            assert_eq!(vm.call_value(&program, function, packed, &[Value::number(0.5)]).unwrap(), Value::number(9.5));
            let (pc, registers) = run(&mut vm, packed, &[Value::number(1.5)]);
            assert_eq!(instructions[pc].op(), Op::Return, "the same artifact admits the new arena layout");
            assert_eq!(registers[usize::from(instructions[pc].register_a())], Value::number(4.0));
        }
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn native_own_stores_share_barriers_and_exact_fallback() {
    use quench_runtime::bytecode::Op;
    let source = include_str!("../../../quench-runtime/tests/fixtures/own-store-proof.js");
    let typescript = source
        .replace("function write(object, value)", "function write(object: {left: number, right: number}, value: number): number")
        .replace("function strictWrite(object, value)", "function strictWrite(object: {left: number, right: number}, value: number): number");
    for program in [Engine::specialize(source, "own-store-proof.js"),
        Engine::specialize_typescript(&typescript, "own-store-proof.ts")] {
        let program = program.unwrap();
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let global = |vm: &mut Vm, name| {
            let atom = vm.intern_atom(name);
            vm.own_property(vm.realm.globals, atom).unwrap()
        };
        let left = vm.intern_atom("left");
        let effects = vm.intern_atom("effects");
        for name in ["updateAlias", "write", "strictWrite"] {
            let id = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            let body = program.function(id);
            let instructions: Vec<_> = body.instructions().collect();
            let function = global(&mut vm, name);
            let run = |vm: &mut Vm, body: &quench_runtime::bytecode::Function, method: &quench_runtime::vm::engine_types::LaneMethod, object, value| {
                let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
                for (index, value) in [object, value].into_iter().enumerate() {
                    let register = body.parameter_registers.map(|base| base + index as u16)
                        .or_else(|| body.promoted_register(index as u16)).unwrap();
                    registers[usize::from(register)] = value;
                }
                let pc = vm.run_lane_method(method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                    &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
                (pc, registers)
            };
            let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new())
                .unwrap_or_else(|| panic!("native {name}: {instructions:?}"));
            for object_name in ["packed", "arena"] {
                let object = global(&mut vm, object_name);
                let before = vm.own_property(object, left).unwrap().as_number().unwrap();
                let right = vm.intern_atom("right");
                let other = vm.own_property(object, right).unwrap().as_number().unwrap();
                let (pc, registers) = run(&mut vm, body, &method, object, Value::integer(7));
                assert_eq!(instructions[pc].op(), Op::Return, "{name}({object_name}) left at {pc}");
                assert_eq!(vm.own_property(object, left), Some(Value::integer(7)));
                let expected = if name == "updateAlias" { Value::number(before + 7.0 + other) } else { Value::integer(7) };
                assert_eq!(registers[usize::from(instructions[pc].register_a())], expected);
            }
            if name == "updateAlias" { continue; }
            for object_name in ["accessor", "readonly", "proxy"] {
                let object = global(&mut vm, object_name);
                let before = vm.own_property(vm.realm.globals, effects).unwrap();
                let (pc, _) = run(&mut vm, body, &method, object, Value::integer(3));
                assert!(matches!(instructions[pc].op(), Op::SetField | Op::SetFieldStrict), "{object_name} leaves before writing");
                assert_eq!(vm.own_property(vm.realm.globals, effects), Some(before));
                let result = vm.call_value(&program, function, Value::UNDEFINED, &[object, Value::integer(3)]);
                assert_eq!(result.is_err(), name == "strictWrite" && object_name == "readonly");
                let increment = match object_name { "accessor" => 3, "proxy" => 1, _ => 0 };
                assert_eq!(vm.own_property(vm.realm.globals, effects), Some(Value::integer(before.as_int().unwrap() + increment)));
            }
            // Recompile after GC may renumber shapes, then create a young value with no independent root.
            vm.collect_now(&program);
            vm.collect_now(&program);
            let object = global(&mut vm, "arena");
            vm.call_value(&program, function, Value::UNDEFINED, &[object, Value::integer(7)]).unwrap();
            let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).unwrap();
            let numeric = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "writeNumber")).unwrap();
            let numeric_function = global(&mut vm, "writeNumber");
            vm.call_value(&program, numeric_function, Value::UNDEFINED, &[object, Value::number(1.5)]).unwrap();
            let numeric_body = program.function(numeric);
            let numeric_method = vm.compile_lane_method(&program, numeric as u32, &|_| (0, 0), Vec::new()).unwrap();
            for value in [Value::integer(7), Value::number(1.5), Value::number(-0.0),
                Value::number(f64::NAN), Value::number(f64::INFINITY)] {
                let (pc, _) = run(&mut vm, numeric_body, &numeric_method, object, value);
                assert_eq!(numeric_body.instructions().nth(pc).unwrap().op(), Op::Return,
                    "verified Number store must remain native");
                let stored = vm.own_property(object, left).unwrap().as_number().unwrap();
                let expected = value.as_number().unwrap();
                assert!(stored.to_bits() == expected.to_bits() || (stored.is_nan() && expected.is_nan()));
            }
            let child = vm.heap.alloc_object(quench_runtime::heap::Object::new(Value::NULL));
            let (pc, registers) = run(&mut vm, body, &method, object, child);
            assert_eq!(instructions[pc].op(), Op::Return, "a heap write remains native with the logged helper");
            assert_eq!(registers[usize::from(instructions[pc].register_a())], child);
            assert_eq!(vm.own_property(object, left), Some(child));
            vm.collect_slow(&program, false);
            assert!(vm.heap.is_live(child), "the stored young value survives collection");
            // Restore the Number fixture for the next function's oracle.
            vm.call_value(&program, function, Value::UNDEFINED, &[object, Value::integer(7)]).unwrap();
        }
        let object = global(&mut vm, "methodOwner");
        let invoke = global(&mut vm, "invoke");
        assert_eq!(vm.call_value(&program, invoke, Value::UNDEFINED, &[object]).unwrap(), Value::integer(1));
        // This frontend's Call site uses its lane cache. Install a valid legacy
        // method entry explicitly to exercise the shared atom-admission policy.
        let original = vm.own_property(object, left).unwrap();
        let shape = vm.heap.object(object).unwrap().shape();
        let target = vm.call_target(original).unwrap();
        let slot = vm.shape_slot(shape, left).unwrap() as u16;
        let site = vm.method_caches.len();
        vm.method_caches.push([quench_runtime::vm::EMPTY_METHOD_CACHE; 2]);
        vm.record_method_cache(site, MethodCache { shape, atom: left, proto: object,
            callee: original, guard: quench_runtime::vm::MethodGuard { owner: object, owner_shape: shape, slot, depth: 0 },
            target: Some(target) });
        assert!(vm.method_caches_may_name(left));
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "write")).unwrap();
        let body = program.function(id);
        let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).unwrap();
        let replacement = global(&mut vm, "replacement");
        let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
        for (index, value) in [object, replacement].into_iter().enumerate() {
            let register = body.parameter_registers.map(|base| base + index as u16)
                .or_else(|| body.promoted_register(index as u16)).unwrap();
            registers[usize::from(register)] = value;
        }
        let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
            &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        assert_eq!(body.instructions().nth(pc).unwrap().op(), Op::SetField);
        assert_eq!(vm.own_property(object, left), Some(original), "method invalidation happens before the write");
        let write = global(&mut vm, "write");
        vm.call_value(&program, write, Value::UNDEFINED, &[object, replacement]).unwrap();
        assert_eq!(vm.call_value(&program, invoke, Value::UNDEFINED, &[object]).unwrap(), Value::integer(2));
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn native_chain_append_completes_without_a_store_handoff() {
    use quench_runtime::bytecode::Op;
    let program = Engine::specialize(include_str!("../../../quench-runtime/tests/fixtures/own-store-proof.js"),
        "own-store-chain.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "append")).unwrap();
    let body = program.function(id);
    let instructions: Vec<_> = body.instructions().collect();
    assert!(instructions.iter().any(|instruction| instruction.op() == Op::SetField));
    let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).unwrap();
    let left = vm.intern_atom("left");
    for (head_name, item_name) in [("head", "item"), ("arenaHead", "arenaItem")] {
        let atom = vm.intern_atom(head_name);
        let head = vm.own_property(vm.realm.globals, atom).unwrap();
        let tail = vm.own_property(head, left).unwrap();
        let atom = vm.intern_atom(item_name);
        let item = vm.own_property(vm.realm.globals, atom).unwrap();
        let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
        let parameter = body.parameter_registers.or_else(|| body.promoted_register(0)).unwrap();
        registers[usize::from(parameter)] = head;
        let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
            &program, registers.as_mut_ptr(), item, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        assert_eq!(instructions[pc].op(), Op::Return, "append leaves at {pc}: {instructions:?}");
        assert_eq!(registers[usize::from(instructions[pc].register_a())], head);
        assert_eq!(vm.own_property(tail, left), Some(item));
        assert_eq!(vm.own_property(item, left), Some(Value::NULL));
    }
}

/// A hot callee's body is entered over and over, and a run binds the dispatch payload once:
/// entries of one lane run find its program, residual and interrupt standing, so the runs bound
/// are a small fraction of the entries.
#[test]
fn interpreter_calls_cross_into_bodies_with_one_payload_per_lane_run() {
    let source = r#"(function () {
      function step(a, b) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + a * i + b) | 0; } return s; }
      let total = 0;
      for (let round = 0; round < 20000; round++) { total = (total + step(round & 7, round & 3)) & 1048575; }
      globalThis.answer = total;
    })();"#;
    let events = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED).events;
    // The caller's loop is compiled too and enters the callee's body by its entry, a nested
    // lane entry (`Vm::run_nested_lane_method`), so the callee's cell is not crossed here.
    let (entries, binds) = (events.count(Event::Entered), events.count(Event::RunBound));
    assert!(entries > 1000, "the hot callee runs as a body, entered {entries} times");
    assert!(binds * 200 < entries, "{binds} payload binds for {entries} entries");
}

/// Runs `source` interpreted and compiled and requires the same answer, which the interpreter
/// leaves in `globalThis.answer`.
pub(super) fn assert_cell_crossings_match(source: &str) {
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    assert!(run.outcome.answer.is_some(), "the source leaves its answer");
}

/// Asserts that the lane body of `drive`, run in `vm`, never left at a method call.
#[cfg(not(quench_profiled))]
pub(super) fn assert_drive_leaves_no_method_call(vm: &Vm, program: &quench_runtime::ResidualProgram, drive: usize) {
    let view = vm.programs.views_in_lane(quench_runtime::vm::program_store::ProgramId::MAIN).unwrap().get(drive)
        .and_then(quench_runtime::vm::program_store::derived_lane_view).unwrap();
    let op_at = |function: u32, pc: u32| program.function(function as usize).instructions().nth(pc as usize)
        .map(|instruction| format!("{:?}", instruction.op()));
    if let Some(report) = view.tier().body_stats.report(0, view.body_state_in_lane(), &op_at) {
        assert!(!report.contains("CallMethod") && !report.contains("CallThisMethod"), "no leave at a method call: {report}");
    }
}
