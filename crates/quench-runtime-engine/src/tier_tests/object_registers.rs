//! Object register promotion: each promoted program runs interpreted, compiled and as a published
//! image (moved from the core's `compile/object_registers` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;

fn check(source: &str, promoted: bool) {
    let program = quench_runtime::Engine::specialize(source, "object-registers.js").unwrap();
    let id = program.functions.iter().position(|f| f.header().name
        .is_some_and(|name| &program.atoms[name as usize] == "subject")).unwrap();
    let body = program.function(id);
    assert_eq!(body.instructions().any(|i| matches!(i.op(), Op::MakeObject2 | Op::MakeObjectLiteral)), !promoted,
        "{}", program.disassemble());
    program.validate().unwrap();
    for mode in [quench_runtime::ExecutionMode::Interpreted, quench_runtime::ExecutionMode::Compiled] {
        let mut runtime = quench_runtime::Runtime::new(quench_runtime::SystemHost);
        runtime.set_execution_mode(mode);
        runtime.execute(&program).unwrap();
    }
    {
        let dynamic = (0..program.functions.len())
            .any(|id| program.function(id).has_dynamic_scope_access());
        let native = quench_runtime::NativeJsProgram::publish(program).unwrap();
        let mut runtime = quench_runtime::Runtime::new(quench_runtime::SystemHost);
        runtime.set_execution_mode(if dynamic { quench_runtime::ExecutionMode::Compiled }
            else { quench_runtime::ExecutionMode::NativeOnly });
        runtime.execute_native(&native).unwrap();
    }
}

#[test]
fn promotes_own_fields_through_loop_aliases_and_joins() {
    check(r#"
        function subject(n, seed) {
            const s = { count: n | 0, control: seed | 0 };
            while (s.count > 0) {
                s.count = (s.count - 1) | 0;
                if ((s.control & 1) === 0) s.control = s.control >> 1;
                else s.control = (s.control >> 1) ^ 0xD008;
            }
            return s.control | 0;
        }
        function reference(n, x) {
            n |= 0; x |= 0;
            while (n-- > 0) x = (x >> 1) ^ ((x & 1) ? 0xD008 : 0);
            return x;
        }
        for (let n of [-1,0,1,2,17,257]) for (let seed of [0,1,-1,2147483647,-2147483648]) {
            if (subject(n,seed) !== reference(n,seed)) throw Error('field state');
        }
    "#, true);
}

#[test]
fn promotes_an_allocation_each_loop_iteration_repeats() {
    check(r#"
        function subject(n) {
            let s = 0;
            for (let i = 0; i < n; i++) { const p = { x: i, y: 1 }; p.y = (p.y + i) | 0; s = (s + p.x + p.y) | 0; }
            return s;
        }
        for (const n of [0, 1, 2, 100]) {
            let r = 0; for (let i = 0; i < n; i++) r = (r + i + 1 + i) | 0;
            if (subject(n) !== r) throw Error('loop allocation ' + n);
        }
    "#, true);
}

#[test]
fn promotes_wider_literals() {
    check(r#"
        function subject(n) {
            let s = 0;
            for (let i = 0; i < n; i++) {
                const p = { x: i, y: 1, z: 2, w: i };
                p.z = (p.z + p.w) | 0;
                s = (s + p.x + p.y + p.z) | 0;
            }
            return s;
        }
        for (const n of [0, 1, 3, 50]) {
            let r = 0; for (let i = 0; i < n; i++) r = (r + i + 1 + 2 + i) | 0;
            if (subject(n) !== r) throw Error('wide literal ' + n);
        }
    "#, true);
}

#[test]
fn keeps_a_literal_read_through_its_prototype() {
    check(r#"
        function subject(n) {
            let s = '';
            for (let i = 0; i < n; i++) { const p = { x: i, y: 1, z: 2 }; s = typeof p.toString; }
            return s;
        }
        if (subject(2) !== 'function') throw Error('prototype read');
    "#, false);
}

#[test]
fn keeps_an_allocation_whose_earlier_instance_stays_live() {
    check(r#"
        function subject(n) {
            let s = 0, previous = { x: -1, y: -1 };
            for (let i = 0; i < n; i++) {
                const p = { x: i, y: i };
                s = (s + previous.x) | 0;
                previous = p;
            }
            return s;
        }
        if (subject(4) !== (-1 + 0 + 1 + 2)) throw Error('earlier instance');
    "#, false);
}

#[test]
fn preserves_initializer_and_field_value_effect_order() {
    check(r#"
        let events = [];
        function subject(value) {
            'use strict';
            const s = { a: (events.push('a'), value), b: (events.push('b'), 2) };
            s.b = s.a + s.b;
            return s.b | 0;
        }
        const value = { valueOf() { events.push('coerce'); return 5; } };
        if (subject(value) !== 7 || events.join(',') !== 'a,b,coerce') throw Error('order');
    "#, true);
}

#[test]
fn retains_escaping_or_observable_objects() {
    for (body, assertion) in [
        ("return s;", "if (subject().a !== 1) throw Error('return');"),
        ("return (() => s)();", "if (subject().a !== 1) throw Error('capture');"),
        ("eval('s.a = 9'); return s.a | 0;", "if (subject() !== 9) throw Error('eval');"),
        ("Object.defineProperty(s, 'a', {get(){return 9}}); return s.a | 0;", "if (subject() !== 9) throw Error('getter');"),
        ("delete s.a; return s.a;", "if (subject() !== undefined) throw Error('delete');"),
        ("s.a = s; return s.a === s;", "if (!subject()) throw Error('identity');"),
    ] {
        check(&format!("function subject() {{ const s = {{a:1,b:2}}; {body} }} {assertion}"), false);
    }
}

#[test]
fn retains_ambiguous_aliases_at_control_flow_joins() {
    check(r#"
        function subject(flag) {
            const s = {a:1,b:2};
            let alias = s;
            if (flag) alias = {a:7,b:8};
            alias.a = 11;
            return (s.a + alias.b) | 0;
        }
        if (subject(false) !== 13 || subject(true) !== 9) throw Error('alias join');
    "#, false);
}

