//! Compatibility surface for Node's `vm` module.
//!
//! The shared runtime currently executes one JavaScript realm. This adapter
//! provides the common script and sandbox APIs in that realm, while keeping
//! context objects distinct and routing identifier reads through their
//! sandbox objects.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = quench_js_check::checked_js!(r#"(() => {
  const contexts = new WeakSet();
  const Script = class Script {
    constructor(code, options = {}) {
      if (typeof code !== 'string') code = String(code);
      this.code = code;
      this.filename = options.filename || 'evalmachine.<anonymous>';
      this.lineOffset = options.lineOffset || 0;
      this.columnOffset = options.columnOffset || 0;
    }
    runInThisContext(options) { return runInThisContext(this.code, options); }
    runInContext(context, options) { return runInContext(this.code, context, options); }
    runInNewContext(context, options) {
      return runInNewContext(this.code, context, options);
    }
  };
  function createContext(context = {}, options = {}) {
    if (context === null || (typeof context !== 'object' && typeof context !== 'function')) {
      throw new TypeError('The "contextObject" argument must be an object.');
    }
    contexts.add(context);
    if (!Object.prototype.hasOwnProperty.call(context, 'global')) {
      Object.defineProperty(context, 'global', { configurable: true, get() { return context; } });
    }
    if (!Object.prototype.hasOwnProperty.call(context, 'globalThis')) {
      Object.defineProperty(context, 'globalThis', { configurable: true, get() { return context; } });
    }
    return context;
  }
  function isContext(context) { return context !== null && contexts.has(context); }
  function runInThisContext(code, options) {
    try { return (0, eval)(String(code)); }
    catch (error) { addFilename(error, options); throw error; }
  }
  function addFilename(error, options) {
    if (error && typeof error === 'object' && typeof error.stack === 'string') {
      const filename = typeof options === 'string' ? options : options?.filename;
      if (filename && !error.stack.startsWith(`${filename}:`)) {
        error.stack = `${filename}:1\n${error.stack}`;
      }
    }
  }
  function runInContext(code, context, options = {}) {
    if (context === null || (typeof context !== 'object' && typeof context !== 'function')) {
      throw new TypeError('The "contextifiedObject" argument must be a vm.Context.');
    }
    if (!contexts.has(context)) {
      const error = new TypeError('The "contextifiedObject" argument must be a vm.Context.');
      error.code = 'ERR_INVALID_ARG_TYPE';
      throw error;
    }
    if (typeof code === 'object' && code !== null && typeof code.code === 'string') code = code.code;
    const source = String(code);
    // `with` gives scripts access to the context's own global properties. A
    // fresh function provides script-local declarations while assignments to
    // existing sandbox properties remain visible to the caller.
    const scope = new Proxy(context, {
      has(target, key) {
        return key !== Symbol.unscopables && key !== 'eval' && key !== 'sandbox' && key !== 'source';
      },
      get(target, key) {
        if (key === Symbol.unscopables) return undefined;
        if (key === 'globalThis' || key === 'global') return scope;
        if (key === 'process' || key === 'require') return Reflect.has(target, key) ? target[key] : undefined;
        if (key === 'gc' && !Reflect.has(target, key) && typeof globalThis.gc !== 'function') return () => {};
        return Reflect.has(target, key) ? Reflect.get(target, key, target) : globalThis[key];
      },
      set(target, key, value) { return Reflect.set(target, key, value, target); },
    });
    try {
      return Function('sandbox', 'source', 'with (sandbox) { return eval(source); }')
        .call(scope, scope, source);
    } catch (error) { addFilename(error, options); throw error; }
  }
  function runInNewContext(code, context = {}, options = {}) {
    return runInContext(code, createContext(context, options), options);
  }
  function compileFunction(code, params = [], options = {}) {
    if (!Array.isArray(params)) throw new TypeError('The "params" argument must be an array.');
    const fn = Function(...params.map(String), String(code));
    return fn;
  }
  const constants = Object.freeze({ USE_MAIN_CONTEXT_DEFAULT_LOADER: -1 });
  return {
    Script, createScript: (code, options) => new Script(code, options),
    createContext, isContext, runInThisContext, runInContext, runInNewContext,
    compileFunction, constants,
    measureMemory() { return Promise.reject(new Error('vm.measureMemory is unavailable')); },
  };
})()"#);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(MODULE, "node:vm/shared.js")
}
