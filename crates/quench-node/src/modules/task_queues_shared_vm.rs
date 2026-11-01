//! Microtasks use the ordinary scheduler and AsyncResource lifecycle.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use super::event_constructor_shared_vm::get;
type Context<'a> = NativeContext<'a, NodeHost>;

#[cfg(test)]
mod tests {
    use quench_runtime::{Host, HostFunction, HostFunctionId, NativeContext, RootId, RootedError};
    struct TestHost;
    impl Host for TestHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 { 0.0 }
        fn functions(&self) -> &[HostFunction<Self>] {
            quench_runtime::host_functions![
                global "makeNative" (1) => make,
                method "invoke" (0) => super::invoke::<Self>,
                global "collect" (0) => collect,
            ]
        }
    }
    fn collect(c: &mut NativeContext<'_, TestHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
        c.collect()?;
        Ok(c.undefined())
    }
    fn make(c: &mut NativeContext<'_, TestHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
        c.host_function_with_data(HostFunctionId(1), args[0])
    }
    #[test]
    fn microtask_finally_preserves_errors_receiver_and_collection() {
        let program = quench_runtime::Engine::specialize(r#"
          function makeGuest(resource) { return () => {
            try { resource.callback(); } finally { resource.emitDestroy(); }
          }; }
          function run(make, phase) {
            const log = [], callbackError = {}, destroyError = {};
            const resource = {
              get callback() {
                log.push('callback:get');
                if (phase === 'callback:get') throw callbackError;
                return function() { log.push(['callback', this === resource]); collect();
                  if (phase === 'callback' || phase === 'both') throw callbackError;
                  return 42;
                };
              },
              get emitDestroy() {
                log.push('destroy:get');
                if (phase === 'destroy:get') throw destroyError;
                return function() { log.push(['destroy', this === resource]); collect();
                  if (phase === 'destroy' || phase === 'both') throw destroyError;
                  return 17;
                };
              }
            };
            const invoke = make(resource); collect();
            try { log.push(['result', String(invoke.call(null))]); }
            catch (e) { log.push(['throw', e === callbackError ? 'callback' : e === destroyError ? 'destroy' : 'wrong identity']); }
            return JSON.stringify(log);
          }
          for (const phase of ['', 'callback:get', 'callback', 'destroy:get', 'destroy', 'both']) {
            const expected = run(makeGuest, phase), actual = run(makeNative, phase);
            if (expected !== actual) throw Error(actual + ' != ' + expected);
          }
        "#, "native-microtask-finally.js").unwrap();
        quench_runtime::Runtime::new(TestHost).execute(&program).unwrap();
    }
}

pub(crate) fn module(c: &mut Context<'_>, enqueue: RootId, require: RootId) -> Result<RootId, RootedError> {
    let state = c.null_object_rooted()?;
    c.assign_named_property_rooted(state, "enqueue", enqueue)?;
    c.assign_named_property_rooted(state, "require", require)?;
    let queue = c.host_constructor_named_with_data(crate::host::shared_vm::operation("taskQueueMicrotask"), "queueMicrotask", state)?;
    c.record_rooted(&["queueMicrotask"], &[queue])
}

pub(crate) fn queue(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let callback = args.first().copied().unwrap_or(undefined);
    let state = c.host_function_data()?;
    let enqueue = get(c, state, "enqueue")?;
    if !c.is_callable_rooted(callback)? { return c.call_rooted(enqueue, undefined, &[callback]); }
    let mut constructor = get(c, state, "AsyncResource")?;
    if c.rooted_value(constructor).is_some_and(|value| value.is_null() || value.is_undefined()) {
        let require = get(c, state, "require")?;
        let name = c.string_rooted("async_hooks");
        let module = c.call_rooted(require, undefined, &[name])?;
        constructor = get(c, module, "AsyncResource")?;
        c.assign_named_property_rooted(state, "AsyncResource", constructor)?;
    }
    let name = c.string_rooted("Microtask");
    let options = c.object_rooted()?;
    let yes = c.boolean(true);
    c.assign_named_property_rooted(options, "requireManualDestroy", yes)?;
    let resource = c.construct_rooted(constructor, constructor, &[name, options])?;
    let key = c.string_rooted("callback");
    c.set_property_rooted(resource, key, callback, resource)?;
    let run = c.host_function_named_with_data(crate::host::shared_vm::operation("taskMicrotaskRun"), "", resource)?;
    c.call_rooted(enqueue, undefined, &[run])?;
    Ok(undefined)
}

pub(crate) fn run(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = c.host_function_data()?;
    let scope = get(c, resource, "runInAsyncScope")?;
    let invoke = c.host_function_named_with_data(crate::host::shared_vm::operation("taskMicrotaskInvoke"), "", resource)?;
    c.call_rooted(scope, resource, &[invoke])?;
    Ok(c.undefined())
}

pub(crate) fn invoke<H: quench_runtime::Host>(c: &mut NativeContext<'_, H>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let resource = c.host_function_data()?;
    let result = (|| {
        let callback = c.get_named_property_rooted(resource, "callback")?;
        c.call_rooted(callback, resource, &[])
    })();
    let destroy = c.get_named_property_rooted(resource, "emitDestroy")?;
    c.call_rooted(destroy, resource, &[])?;
    result?;
    Ok(c.undefined())
}
