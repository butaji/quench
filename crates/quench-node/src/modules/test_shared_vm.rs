//! Minimal shared-VM callback surface for `node:test`.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const TEST_FACTORY: &str = quench_js_check::checked_js!(r#"(nativeTest, assert) => {
  const mock = {
    fn(implementation = () => {}) {
      const calls = [];
      const wrapped = function(...args) {
        const call = { arguments: args, this: this, result: undefined };
        calls.push(call);
        try {
          call.result = { type: "return", value: implementation.apply(this, args) };
          return call.result.value;
        } catch (error) {
          call.result = { type: "throw", value: error };
          throw error;
        }
      };
      wrapped.mock = {
        calls,
        get callCount() { return calls.length; },
        resetCalls() { calls.length = 0; },
      };
      return wrapped;
    },
    method(object, name, implementation) {
      const original = object[name];
      const wrapped = this.fn(implementation || original);
      object[name] = wrapped;
      wrapped.mock.restore = () => { object[name] = original; };
      return wrapped;
    },
    reset() {},
    restoreAll() {},
  };

  const context = () => ({ assert, mock });
  const invoke = (args) => {
    const callback = [...args].reverse().find((arg) => typeof arg === "function");
    if (!callback) return undefined;
    const done = (error) => {
      if (error !== undefined) throw error;
    };
    return callback.length >= 2 ? callback(context(), done) : callback(context());
  };
  const run = (...args) => {
    if (args.some((arg) => typeof arg === "function")) return invoke(args);
    return nativeTest(...args);
  };
  run.test = run;
  run.describe = run.suite = (...args) => invoke(args);
  run.it = run;
  run.before = run.after = run.beforeEach = run.afterEach = (...args) => invoke(args);
  run.skip = run.todo = () => undefined;
  run.mock = mock;
  return run;
}"#);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(TEST_FACTORY, "node:test/shared.js")?;
    let test = context.host_function(crate::host::shared_vm::operation("nodeTest"))?;
    let util = crate::modules::util_shared_vm::module(context)?;
    let assert = crate::modules::assert_shared_vm::module(context, util)?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[test, assert])
}

pub(crate) fn run(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let mut callback = None;
    for argument in args {
        if context.is_callable_rooted(*argument)? {
            callback = Some(*argument);
            break;
        }
    }
    let Some(callback) = callback else {
        return missing_callback(context);
    };
    let receiver = context.undefined();
    context.call_rooted(callback, receiver, &[])
}

fn missing_callback(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let error = context.type_error_rooted("The \"fn\" argument must be of type function")?;
    let code_key = context.string_rooted("code");
    let code = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let _ = context.set_property_rooted(error, code_key, code, error)?;
    Err(context.throw(error))
}
