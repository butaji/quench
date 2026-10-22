use crate::{Engine, Host, NativeContext, RootId, RootedError, Runtime};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct StackHost {
    formatter: Option<RootId>,
    output: Rc<RefCell<Vec<String>>>,
}
impl Host for StackHost {
    fn write_line(&mut self, text: &str) {
        self.output.borrow_mut().push(text.into());
    }
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
    fn error_stack_formatter(&self) -> Option<RootId> {
        self.formatter
    }
    fn initialize_shadow_realm(
        context: &mut NativeContext<'_, Self>,
        global: RootId,
    ) -> Result<Option<RootId>, RootedError> {
        context.collect()?;
        context
            .evaluate_script_in_realm_rooted(
                global,
                r#"(global, error, sites) => {
            const prepare = global.Error.prepareStackTrace;
            if (typeof prepare === 'function') return prepare(error, sites);
            return [global === globalThis, error.message, sites[0].getFunctionName()].join(':');
        }"#,
                "host-shadow-stack.js",
            )
            .map(Some)
    }
    fn functions(&self) -> &[crate::HostFunction<Self>] {
        crate::host_functions![method "collect" (0) => collect]
    }
    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        let formatter = context.evaluate_script_rooted(
            r#"(global, error, sites) => {
            const prepare = global.Error.prepareStackTrace;
            if (typeof prepare === 'function') return prepare(error, sites);
            global.stackCalls = (global.stackCalls || 0) + 1;
            if (error.fail) throw error.reason;
            return {sameGlobal: global === globalThis, error, message: error.message,
                    caller: sites[0].getFunctionName()};
        }"#,
            "host-stack-formatter.js",
        )?;
        let global = context.global_root()?;
        let key = context.string_rooted("collect");
        let collect = context.host_function(crate::HostFunctionId(0))?;
        context.define_data_property_rooted(global, key, collect, true, true, true)?;
        let formatter = context.retain(formatter)?;
        context.host_mut().formatter = Some(formatter);
        Ok(())
    }
}
fn collect(
    context: &mut NativeContext<'_, StackHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.collect()?;
    Ok(context.undefined())
}
#[test]
fn host_default_stack_policy_roots_callbacks_and_preserves_override_cache_and_throw() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let host = StackHost::default();
        let output = host.output.clone();
        let mut runtime = Runtime::new(host);
        let create = r#"
            function capture() { return new Error('one'); }
            globalThis.saved = capture();
            globalThis.reason = {};
            globalThis.bad = new Error('bad');
            bad.fail = true; bad.reason = reason;
        "#;
        let read = r#"
            const first = saved.stack;
            print([first.sameGlobal, first.error === saved, first.message,
                   first.caller, first === saved.stack, stackCalls].join(':'));
            try { bad.stack; } catch (error) { print(error === reason); }
            bad.fail = false;
            print([bad.stack.error === bad, stackCalls].join(':'));
            Error.prepareStackTrace = () => 42;
            print([new Error().stack, stackCalls].join(':'));
            Error.prepareStackTrace = false;
            print([new Error('last').stack.message, stackCalls].join(':'));
        "#;
        let program = compile(&format!("{create}\ncollect();\n{read}"), "stack-policy.js").unwrap();
        runtime.execute(&program).unwrap();
        assert_eq!(
            &*output.borrow(),
            &[
                "true:true:one:capture:true:1",
                "true",
                "true:3",
                "42:3",
                "last:4"
            ]
        );
    }
}

#[test]
fn per_realm_host_stack_policy_is_rooted_and_keeps_shadow_hooks_isolated() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let host = StackHost::default();
        let output = host.output.clone();
        let mut runtime = Runtime::new(host);
        let program = compile(r#"
            let calls = 0;
            Error.prepareStackTrace = () => { calls++; return 'principal'; };
            const target = new Proxy(function Target() {}, {
                get(target, key, receiver) {
                    return key === 'prototype' ? {tag: 42} : Reflect.get(target, key, receiver);
                }
            });
            print(Reflect.construct(ShadowRealm, [], target).tag);
            const realm = new ShadowRealm();
            collect();
            const ordinary = realm.evaluate("function inner() { return new Error('inside').stack; } inner();");
            print([ordinary, calls].join('|'));
            const custom = realm.evaluate("Error.prepareStackTrace = () => 29; new Error().stack;");
            print([custom, calls].join('|'));
            print([new Error().stack, calls].join('|'));
        "#, "shadow-stack-policy.js").unwrap();
        runtime.execute(&program).unwrap();
        assert_eq!(
            &*output.borrow(),
            &["42", "true:inside:inner|0", "29|0", "principal|1"]
        );
    }
}
