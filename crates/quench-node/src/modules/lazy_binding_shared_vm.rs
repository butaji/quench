//! Replaceable lazy lexical bindings shared by native globals.
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn install(c: &mut NativeContext<'_, crate::NodeHost>, name: &str, load: RootId, enumerable: bool, names: [&str; 2]) -> Result<(), RootedError> {
    let state = c.null_object_rooted()?;
    c.assign_named_property_rooted(state, "load", load)?;
    let get = c.host_function_named_with_data(crate::host::shared_vm::operation("lazyBindingGet"), names[0], state)?;
    let set = c.host_function_named_with_data(crate::host::shared_vm::operation("lazyBindingSet"), names[1], state)?;
    let descriptor = c.null_object_rooted()?;
    c.assign_named_property_rooted(descriptor, "get", get)?;
    c.assign_named_property_rooted(descriptor, "set", set)?;
    let yes = c.boolean(true);
    let enumerable = c.boolean(enumerable);
    c.assign_named_property_rooted(descriptor, "configurable", yes)?;
    c.assign_named_property_rooted(descriptor, "enumerable", enumerable)?;
    let global = c.global_root()?;
    let key = c.string_rooted(name);
    c.define_property_descriptor_rooted(global, key, descriptor)?;
    Ok(())
}

pub(crate) fn global_get<H: quench_runtime::Host>(c: &mut NativeContext<'_, H>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    if c.has_own_property_rooted(state, "value")? { return c.get_named_property_rooted(state, "value"); }
    let load = c.get_named_property_rooted(state, "load")?;
    let undefined = c.undefined();
    let value = c.call_rooted(load, undefined, &[])?;
    c.assign_named_property_rooted(state, "value", value)?;
    Ok(value)
}

pub(crate) fn global_set<H: quench_runtime::Host>(c: &mut NativeContext<'_, H>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let undefined = c.undefined();
    c.assign_named_property_rooted(state, "value", args.first().copied().unwrap_or(undefined))?;
    Ok(undefined)
}

#[cfg(test)]
mod global_tests {
    use quench_runtime::{Host, HostFunction, HostFunctionId, NativeContext, RootId, RootedError};
    struct TestHost;
    impl Host for TestHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 { 0.0 }
        fn functions(&self) -> &[HostFunction<Self>] {
            quench_runtime::host_functions![
                global "makeNative" (1) => make,
                method "get" (0) => super::global_get::<Self>,
                method "set" (1) => super::global_set::<Self>,
                global "collect" (0) => collect,
            ]
        }
    }
    fn collect(c: &mut NativeContext<'_, TestHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
        c.collect()?;
        Ok(c.undefined())
    }
    fn make(c: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
        let state = c.null_object_rooted()?;
        c.assign_named_property_rooted(state, "load", args[0])?;
        let get = c.host_function_named_with_data(HostFunctionId(1), "get", state)?;
        let set = c.host_function_named_with_data(HostFunctionId(2), "set", state)?;
        c.record_rooted(&["get", "set"], &[get, set])
    }
    #[test]
    fn buffer_binding_preserves_undefined_retry_reentry_and_gc() {
        let program = quench_runtime::Engine::specialize(r#"
          function makeGuest(load) {
            let value, bound = false;
            return {get() { if (!bound) { value = load(); bound = true; } return value; },
              set(v) { bound = true; value = v; }};
          }
          function run(make) {
            const trace = []; let calls = 0;
            const a = make(() => { collect(); trace.push('load'); calls++; return 42; });
            trace.push(a.get(), a.get(), a.get.name, a.get.length, a.set.name, a.set.length);
            trace.push(String(a.set(undefined)), String(a.get()), calls);
            const b = make(() => { throw Error('must stay unloaded'); });
            b.set(undefined); trace.push(String(b.get()));
            let attempts = 0, c;
            c = make(() => { collect(); if (++attempts === 1) throw Error('retry'); c.set('during load'); return 'loaded'; });
            try { c.get(); } catch (e) { trace.push(e.message); }
            trace.push(c.get(), c.get(), attempts);
            collect(); a.set('after collection'); trace.push(a.get());
            return JSON.stringify(trace);
          }
          const expected = run(makeGuest), actual = run(makeNative);
          if (actual !== expected) throw Error(actual + ' != ' + expected);
        "#, "buffer-global-binding.js").unwrap();
        quench_runtime::Runtime::new(TestHost).execute(&program).unwrap();
    }
}

